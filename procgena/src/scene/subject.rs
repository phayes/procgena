//! The subject the scene shows, the camera, and command handling.
//!
//! Each kind of subject (a tree, a rock, an Ecotope, …) is a [`Subject`]
//! in a module of its own, added to the scene with [`AddSubject`]. The host
//! shows one with [`ViewerCommand::Show`], built by [`Show::new`].

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;

use super::environment::{EnvironmentSettings, sky_fog};
use super::fly;
use super::orbit::{OrbitCamera, OrbitLimits};
use super::protocol::{ViewerCommand, ViewerEvent};
use super::terrain;
use super::viewport::{ViewportActivity, ViewportCamera};

/// The scene camera's far clipping distance, in metres: beyond the far side
/// of the largest grove seen from the grove camera's farthest distance, and
/// beyond the terrain's view distance seen from high above.
const FAR_CLIP: f32 = 2.0 * terrain::VIEW_DISTANCE;

/// One kind of thing the scene can show, such as a tree or an Ecotope.
///
/// Add it to the scene with [`AddSubject::add_subject`], and show one with
/// [`ViewerCommand::Show`]. The scene keeps only the settings of the
/// subject it shows; showing another subject drops them.
pub trait Subject: Send + Sync + 'static {
    /// The settings a subject of this kind is built from.
    type Settings: Clone + PartialEq + fmt::Debug + Send + Sync + 'static;

    /// A short name for messages, such as `"tree"`.
    const NAME: &'static str;

    /// Whether wind is off while this subject is shown, whatever the
    /// environment says.
    const STILL_AIR: bool = false;

    /// Whether this subject draws ground of its own, so the flat ground
    /// plane is hidden while it is shown.
    const OWN_GROUND: bool = false;

    /// Spawns a subject built from `settings` and returns its entity.
    fn spawn(world: &mut World, settings: &Self::Settings) -> Entity;

    /// Shows `new` in place of `old` on the shown subject. Called only when
    /// they differ. By default the subject is replaced at once.
    fn update(world: &mut World, shown: &mut Shown, old: &Self::Settings, new: &Self::Settings) {
        let _ = old;
        let entity = Self::spawn(world, new);
        shown.replace_with(world, entity);
    }

    /// The camera view that fits a subject built from `settings`.
    fn view(world: &World, settings: &Self::Settings) -> OrbitCamera;

    /// Called after the environment changed from `old` while this subject
    /// is shown with `settings`, for subjects sized by it.
    fn environment_changed(
        world: &mut World,
        shown: &mut Shown,
        settings: &Self::Settings,
        old: &EnvironmentSettings,
    ) {
        let _ = (world, shown, settings, old);
    }

    /// The detail this subject is drawn at when `chosen` is asked for.
    /// By default, the one asked for.
    fn detail(chosen: Detail) -> Detail {
        chosen
    }
}

/// Which detail levels to draw, for subjects with levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Detail {
    /// Choose each level by camera distance, but never one more detailed
    /// than level `finest`: it is drawn wherever a finer level would be.
    ByDistance {
        /// The most detailed level drawn. 0 draws every level.
        finest: usize,
    },
    /// Draw only level `n` (0 is the most detailed), at every distance.
    Level(usize),
}

impl Default for Detail {
    /// Every level, chosen by camera distance.
    fn default() -> Self {
        Self::ByDistance { finest: 0 }
    }
}

impl Detail {
    /// The level drawn when the camera distance alone would choose `level`.
    pub fn level_for(self, level: usize) -> usize {
        match self {
            Self::ByDistance { finest } => level.max(finest),
            Self::Level(n) => n,
        }
    }
}

/// A subject to show: its kind, which document it shows, and its settings.
///
/// Build it with [`Show::new`] and send it with [`ViewerCommand::Show`].
#[derive(Clone)]
pub struct Show {
    subject: TypeId,
    name: &'static str,
    document: u64,
    settings: Arc<dyn Any + Send + Sync>,
}

impl Show {
    /// Shows subject `S` built from `settings`, for the host's document
    /// `document`.
    ///
    /// Showing the same document again updates the shown subject in place,
    /// keeping the old one in view while a replacement builds where the
    /// subject supports it. Any other document replaces the subject at once
    /// and fits the camera to it.
    pub fn new<S: Subject>(document: u64, settings: S::Settings) -> Self {
        Self {
            subject: TypeId::of::<S>(),
            name: S::NAME,
            document,
            settings: Arc::new(settings),
        }
    }

    /// The settings, if this shows subject `S`.
    pub fn settings<S: Subject>(&self) -> Option<&S::Settings> {
        self.settings.downcast_ref()
    }

    /// The document shown.
    pub fn document(&self) -> u64 {
        self.document
    }
}

impl fmt::Debug for Show {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Show")
            .field("subject", &self.name)
            .field("document", &self.document)
            .finish_non_exhaustive()
    }
}

/// The subject on screen.
pub struct Shown {
    subject: TypeId,
    document: u64,
    settings: Arc<dyn Any + Send + Sync>,
    /// The entity showing the subject.
    pub entity: Entity,
    /// A replacement being built for `entity`. It becomes the subject once
    /// it is finished.
    pub pending: Option<Entity>,
    /// When the latest build of the subject was asked for.
    pub requested: Instant,
    /// Counts subjects shown, so systems can tell a new one from an update.
    generation: u64,
}

impl Shown {
    /// Notes that the subject started building again, now, and reports it
    /// with [`ViewerEvent::Building`].
    pub fn start_build(&mut self, world: &mut World) {
        self.requested = Instant::now();
        world.write_message(ViewerEvent::Building);
    }

    /// Replaces the subject with `entity` at once, dropping any replacement
    /// being built.
    pub fn replace_with(&mut self, world: &mut World, entity: Entity) {
        world.despawn(self.entity);
        if let Some(pending) = self.pending.take() {
            world.despawn(pending);
        }
        self.entity = entity;
        self.start_build(world);
    }

    /// Where to build a replacement for the subject, measured from now:
    /// the replacement already being built, the subject itself while it is
    /// still being built (a `G` marks that), or else a new empty entity, so
    /// the subject stays in view while its replacement builds.
    pub fn replacement<G: Component>(&mut self, world: &mut World) -> Entity {
        self.start_build(world);
        match self.pending {
            Some(pending) => pending,
            None if world.get::<G>(self.entity).is_some() => self.entity,
            None => *self.pending.insert(world.spawn_empty().id()),
        }
    }

    /// Makes `finished`, a replacement or the subject itself, the subject
    /// once it has finished building. A successful replacement takes the
    /// old subject's place; a failed one is removed, leaving the old
    /// subject in view.
    ///
    /// Returns whether `finished` was the subject or its replacement, so
    /// its build is worth reporting.
    pub fn finish(&mut self, commands: &mut Commands, finished: Entity, ok: bool) -> bool {
        if self.pending != Some(finished) {
            return finished == self.entity;
        }
        self.pending = None;
        if ok {
            commands.entity(self.entity).despawn();
            self.entity = finished;
        } else {
            commands.entity(finished).despawn();
        }
        true
    }
}

/// What the scene shows.
#[derive(Resource, Default)]
pub struct SceneState {
    shown: Option<Shown>,
    generations: u64,
}

impl SceneState {
    /// Whether a subject of kind `S` is shown.
    pub fn shows<S: Subject>(&self) -> bool {
        self.shown
            .as_ref()
            .is_some_and(|shown| shown.subject == TypeId::of::<S>())
    }

    /// The shown subject of kind `S` and its settings, if one is shown.
    pub fn get<S: Subject>(&self) -> Option<(&Shown, &S::Settings)> {
        let shown = self.shown.as_ref()?;
        Some((shown, shown.settings.downcast_ref()?))
    }

    /// The shown subject of kind `S`, for changing which entity shows it.
    pub fn get_mut<S: Subject>(&mut self) -> Option<&mut Shown> {
        self.shown
            .as_mut()
            .filter(|shown| shown.subject == TypeId::of::<S>())
    }

    /// A number that changes whenever another subject or document is shown.
    pub fn generation(&self) -> u64 {
        self.shown.as_ref().map_or(0, |shown| shown.generation)
    }
}

/// The detail asked for with [`ViewerCommand::SetDetail`].
#[derive(Resource, Debug, Default, Clone, Copy)]
pub(crate) struct ChosenDetail(pub Detail);

/// What the scene does with each kind of subject, without its type.
#[derive(Clone, Copy)]
struct SubjectFns {
    spawn: fn(&mut World, &(dyn Any + Send + Sync)) -> Entity,
    update: fn(&mut World, &mut Shown, &(dyn Any + Send + Sync)) -> bool,
    view: fn(&World, &(dyn Any + Send + Sync)) -> OrbitCamera,
    environment_changed: fn(&mut World, &mut Shown, &EnvironmentSettings),
    detail: fn(Detail) -> Detail,
    still_air: bool,
    own_ground: bool,
}

impl SubjectFns {
    /// The functions for subject `S`.
    fn of<S: Subject>() -> Self {
        Self {
            spawn: |world, settings| S::spawn(world, downcast::<S>(settings)),
            update: |world, shown, new| {
                let new = downcast::<S>(new);
                let old = shown.settings.clone();
                let old = downcast::<S>(&*old);
                if old == new {
                    return false;
                }
                S::update(world, shown, old, new);
                true
            },
            view: |world, settings| S::view(world, downcast::<S>(settings)),
            environment_changed: |world, shown, old| {
                let settings = shown.settings.clone();
                S::environment_changed(world, shown, downcast::<S>(&*settings), old);
            },
            detail: S::detail,
            still_air: S::STILL_AIR,
            own_ground: S::OWN_GROUND,
        }
    }
}

/// `settings` as subject `S`'s settings.
///
/// # Panics
///
/// Panics if they are another subject's; the scene keeps them apart by type.
fn downcast<S: Subject>(settings: &(dyn Any + Send + Sync)) -> &S::Settings {
    settings
        .downcast_ref()
        .unwrap_or_else(|| panic!("settings of another subject given to {}", S::NAME))
}

/// The subjects added to the scene.
#[derive(Resource, Default)]
struct Subjects(HashMap<TypeId, SubjectFns>);

/// Adds kinds of subject to the scene.
pub trait AddSubject {
    /// Lets the scene show subject `S`.
    fn add_subject<S: Subject>(&mut self) -> &mut Self;
}

impl AddSubject for App {
    fn add_subject<S: Subject>(&mut self) -> &mut Self {
        self.init_resource::<Subjects>()
            .world_mut()
            .resource_mut::<Subjects>()
            .0
            .insert(TypeId::of::<S>(), SubjectFns::of::<S>());
        self
    }
}

/// How the shown subject asks to be drawn: wind, ground and detail.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct SubjectLook {
    /// Wind is off whatever the environment says.
    pub still_air: bool,
    /// The flat ground plane is hidden.
    pub own_ground: bool,
    /// The detail the subject is drawn at.
    pub detail: Detail,
}

/// Spawns the scene camera, looking at where subjects stand.
pub(crate) fn setup_scene(mut commands: Commands) {
    let view = OrbitCamera {
        focus: Vec3::new(0.0, 1.0, 0.0),
        yaw: 0.6,
        pitch: 0.2,
        distance: 10.0,
        limits: OrbitLimits {
            distance: (0.5, 60.0),
            pitch: (-0.05, 1.5),
        },
    };
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            far: FAR_CLIP,
            ..default()
        }),
        ViewportCamera,
        procgena_terrain_bevy::TerrainViewer,
        view,
        view.transform(),
        sky_fog(),
    ));
}

/// The commands received since the last frame.
fn read_commands(mut received: MessageReader<ViewerCommand>) -> Vec<ViewerCommand> {
    received.read().cloned().collect()
}

/// Applies every [`ViewerCommand`] received this frame, in order, and renders
/// enough frames to show the result.
pub(crate) fn apply_commands(world: &mut World) {
    let received = world.run_system_cached(read_commands).unwrap_or_default();
    if received.is_empty() {
        return;
    }
    world
        .resource_mut::<ViewportActivity>()
        .request_frames(ViewportActivity::SETTLE_FRAMES);
    world.resource_scope(|world, mut state: Mut<SceneState>| {
        for command in received {
            apply_command(world, &mut state, command);
        }
    });
    update_look(world);
}

/// Applies one command.
fn apply_command(world: &mut World, state: &mut SceneState, command: ViewerCommand) {
    match command {
        ViewerCommand::Show(show) => {
            let Some(fns) = world.resource::<Subjects>().0.get(&show.subject).copied() else {
                error!("the scene cannot show a {}", show.name);
                return;
            };
            // The same document updates in place.
            if let Some(shown) = state.shown.as_mut()
                && shown.subject == show.subject
                && shown.document == show.document
            {
                if (fns.update)(world, shown, &*show.settings) {
                    shown.settings = show.settings;
                }
                return;
            }

            // Anything else replaces the subject and fits the camera to it.
            if let Some(old) = state.shown.take() {
                world.despawn(old.entity);
                if let Some(pending) = old.pending {
                    world.despawn(pending);
                }
            }
            let entity = (fns.spawn)(world, &*show.settings);
            let view = (fns.view)(world, &*show.settings);
            fly::set_view(world, view);
            world.write_message(ViewerEvent::Building);
            state.generations += 1;
            state.shown = Some(Shown {
                subject: show.subject,
                document: show.document,
                settings: show.settings,
                entity,
                pending: None,
                requested: Instant::now(),
                generation: state.generations,
            });
        }
        ViewerCommand::Clear => {
            if let Some(old) = state.shown.take() {
                world.despawn(old.entity);
                if let Some(pending) = old.pending {
                    world.despawn(pending);
                }
            }
        }
        ViewerCommand::SetEnvironment(settings) => {
            let old = *world.resource::<EnvironmentSettings>();
            world
                .resource_mut::<EnvironmentSettings>()
                .set_if_neq(settings);
            if let Some(shown) = state.shown.as_mut()
                && let Some(fns) = world.resource::<Subjects>().0.get(&shown.subject).copied()
            {
                (fns.environment_changed)(world, shown, &old);
            }
        }
        ViewerCommand::SetDetail(detail) => {
            world.resource_mut::<ChosenDetail>().0 = detail;
        }
        ViewerCommand::ResetCamera => {
            if let Some(shown) = &state.shown
                && let Some(fns) = world.resource::<Subjects>().0.get(&shown.subject).copied()
            {
                let view = (fns.view)(world, &*shown.settings);
                fly::set_view(world, view);
            }
        }
        ViewerCommand::SetCamera(view) => fly::set_view(world, view),
        ViewerCommand::SetCameraMode(mode) => fly::set_mode(world, mode),
    }
}

/// Sets [`SubjectLook`] from the shown subject and the chosen detail.
fn update_look(world: &mut World) {
    let chosen = world.resource::<ChosenDetail>().0;
    let fns = world
        .resource::<SceneState>()
        .shown
        .as_ref()
        .and_then(|shown| world.resource::<Subjects>().0.get(&shown.subject).copied());
    let look = match fns {
        Some(fns) => SubjectLook {
            still_air: fns.still_air,
            own_ground: fns.own_ground,
            detail: (fns.detail)(chosen),
        },
        None => SubjectLook {
            detail: chosen,
            ..default()
        },
    };
    world.resource_mut::<SubjectLook>().set_if_neq(look);
}

/// A part's camera-distance range, set aside while another is in its place.
#[derive(Component)]
pub(crate) struct SavedRange(pub VisibilityRange);

/// A part with its visibility and its distance range, current or set aside.
pub(crate) type LevelPart<'a, P> = (
    Entity,
    &'a P,
    Option<&'a mut Visibility>,
    Option<&'a VisibilityRange>,
    Option<&'a SavedRange>,
);

/// A distance range that shows a part at every camera distance.
///
/// It still has a fade at its far end, far beyond any camera. Bevy keeps
/// drawing a part with the shader it first picked for it, and a part first
/// drawn with a fading range uses a fading shader; that shader needs a
/// fading range to draw the part at all.
pub(crate) fn every_distance() -> VisibilityRange {
    VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: f32::MAX / 2.0..f32::MAX,
        use_aabb: false,
    }
}

/// The range a part is drawn with in place of its own range.
type StandIn = fn(&VisibilityRange) -> VisibilityRange;

/// `range` reaching all the way in to the camera.
fn from_camera(range: &VisibilityRange) -> VisibilityRange {
    VisibilityRange {
        start_margin: 0.0..0.0,
        ..range.clone()
    }
}

/// Shows or hides the part `entity` of detail level `level` as `detail`
/// asks, setting its own distance range aside while another stands in for
/// it, and putting it back when not. `ranges` holds the part's current
/// range and the one set aside, if any.
///
/// With [`Detail::Level`] the chosen level shows at every distance and the
/// others are hidden. With [`Detail::ByDistance`] finer levels than the
/// finest are hidden and the finest reaches in to the camera.
pub(crate) fn show_level(
    commands: &mut Commands,
    entity: Entity,
    level: usize,
    detail: Detail,
    visibility: Option<Mut<Visibility>>,
    ranges: (Option<&VisibilityRange>, Option<&SavedRange>),
) {
    let (shown, stand_in): (bool, Option<StandIn>) = match detail {
        Detail::Level(n) => (level == n, (level == n).then_some(|_| every_distance())),
        Detail::ByDistance { finest } => (
            level >= finest,
            (finest > 0 && level == finest).then_some(from_camera),
        ),
    };

    // The part's own range is set aside while a stand-in range is used. A
    // part whose range its plugin changed since (as when its tree's
    // impostor is ready) has the new range set aside instead.
    let (range, saved) = ranges;
    let mut entity = commands.entity(entity);
    match (stand_in, range, saved) {
        (Some(stand_in), Some(range), None) => {
            entity.insert((SavedRange(range.clone()), stand_in(range)));
        }
        (Some(stand_in), Some(range), Some(saved)) if *range != stand_in(&saved.0) => {
            entity.insert((SavedRange(range.clone()), stand_in(range)));
        }
        (None, _, Some(SavedRange(range))) => {
            entity.remove::<SavedRange>().insert(range.clone());
        }
        _ => {}
    }
    let wanted = if shown {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };

    // Touch the visibility only when it changes, so nothing is redrawn needlessly.
    match visibility {
        Some(mut visibility) => {
            visibility.set_if_neq(wanted);
        }
        None if wanted != Visibility::Inherited => {
            entity.insert(wanted);
        }
        None => {}
    }
}

/// The level, from `(level, range)` pairs, whose distance range covers
/// `distance`, counting each blend between levels as switching halfway
/// through. `None` if no range covers it.
pub(crate) fn level_in_range(distance: f32, ranges: &[(usize, &VisibilityRange)]) -> Option<usize> {
    let middle = |margin: &std::ops::Range<f32>| (margin.start + margin.end) / 2.0;
    ranges
        .iter()
        .find(|(_, range)| {
            middle(&range.start_margin) <= distance && distance < middle(&range.end_margin)
        })
        .map(|(level, _)| *level)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_distance_picks_the_level_from_its_ranges() {
        // Levels blending over 1.4–1.6 m, and fading out over 20–30 m.
        let blend = |start: std::ops::Range<f32>, end: std::ops::Range<f32>| VisibilityRange {
            start_margin: start,
            end_margin: end,
            use_aabb: false,
        };
        let near = blend(0.0..0.0, 1.4..1.6);
        let far = blend(1.4..1.6, 20.0..30.0);
        let ranges = [(0, &near), (1, &far)];
        assert_eq!(level_in_range(0.0, &ranges), Some(0));
        assert_eq!(level_in_range(1.49, &ranges), Some(0));
        assert_eq!(level_in_range(1.5, &ranges), Some(1));
        assert_eq!(level_in_range(24.9, &ranges), Some(1));
        assert_eq!(level_in_range(25.0, &ranges), None);
    }

    #[test]
    fn the_finest_level_stands_in_for_finer_ones() {
        assert_eq!(Detail::ByDistance { finest: 1 }.level_for(0), 1);
        assert_eq!(Detail::ByDistance { finest: 1 }.level_for(2), 2);
        assert_eq!(Detail::ByDistance { finest: 0 }.level_for(0), 0);
        assert_eq!(Detail::Level(3).level_for(0), 3);
    }
}
