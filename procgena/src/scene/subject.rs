//! The subject being shown (a tree, a grass field, a rock, a mushroom, a
//! grove or a piece of dead wood), the camera, and command handling.

use std::sync::Arc;
use std::time::Instant;

use super::environment::{EnvironmentSettings, sky_fog};
use super::orbit::{OrbitCamera, OrbitLimits};
use super::protocol::{
    BuildStats, ClumpBuildError, ClumpSettings, ClumpSubject, DeadwoodSettings, DeadwoodView,
    FieldSettings, GroveProgress, GroveStats, MeshSize, SceneMode, SceneStart, ViewerCommand,
    ViewerEvent,
};
use super::viewport::{ViewportActivity, ViewportCamera};
use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use procgena_core::LodLevel;
use procgena_deadwood::{DeadwoodOptions, PieceKind, ScatterOptions, default_lod, size_factor};
use procgena_deadwood_bevy::{
    DeadwoodFinished, DeadwoodGenerating, DeadwoodMaterial, DeadwoodParams, DeadwoodPart,
    DeadwoodScatterParams, ScatterGround,
};
use procgena_impostor::{BakedImpostor, ImpostorRequest};
use procgena_mushroom::{Mushroom, MushroomOptions};
use procgena_mushroom_bevy::{
    MushroomFinished, MushroomGenerating, MushroomMaterial, MushroomParams, MushroomPart,
};
use procgena_plant::prelude::{PlantField, PlantGround, PlantSettings};
use procgena_stone::StoneOptions;
use procgena_stone_bevy::{
    StoneEmbedDepth, StoneFinished, StoneGenerating, StoneMaterial, StoneParams, StonePart,
};
use procgena_tree::{TreeGrove, TreeOptions};
use procgena_tree_bevy::{
    TreeFinished, TreeGenerating, TreeGroveAssets, TreeGroveFinished, TreeGroveGenerating,
    TreeGroveParams, TreeGroveStats, TreeLevel, TreeMaterial, TreeParams, TreePart, TreePartKind,
    default_tree_lod,
};

/// Camera distances at which each tree detail level starts, nearest first.
const LOD_DISTANCES: [f32; 4] = [0.0, 175.0, 275.0, 500.0];

/// Camera distance in metres beyond which trees, alone or in a grove, are
/// drawn as impostors: flat pictures baked from the tree. It is beyond the
/// farthest mesh level, which starts at 500 m.
pub(crate) const IMPOSTOR_DISTANCE: f32 = 800.0;

/// The farthest the grove camera can always back away, in metres: far enough
/// to see the trees drawn as impostors.
const GROVE_FARTHEST: f32 = 1.25 * IMPOSTOR_DISTANCE;

/// The scene camera's far clipping distance, in metres: beyond the far side
/// of the largest grove seen from the grove camera's farthest distance.
const FAR_CLIP: f32 = 2500.0;

/// The tree detail levels the scene shows, most detailed first: procgena_tree_bevy's
/// default levels, switched at greater camera distances so that full detail
/// lasts further out. The lowest level starts at 500 m.
pub fn lod_levels() -> Vec<LodLevel<TreeLevel>> {
    let mut levels = default_tree_lod().levels;
    for (level, distance) in levels.iter_mut().zip(LOD_DISTANCES) {
        level.distance = distance;
    }
    levels
}

/// What is shown, the settings for each mode, and each mode's saved camera view.
#[derive(Resource)]
pub(crate) struct SceneState {
    mode: SceneMode,
    /// The entity showing the current mode's subject.
    subject: Entity,
    /// A tree being built to replace `subject`. It has no parts until it is
    /// finished, when it becomes the subject in the same frame the old tree is removed.
    pending: Option<Entity>,
    tree: Arc<TreeOptions>,
    /// Start time of the latest build request for the active tree or rock.
    requested: Instant,
    /// Plant preview settings, retained while another scene mode is active.
    field: Arc<FieldSettings>,
    /// Rock settings retained when another mode is displayed.
    stone: Arc<StoneOptions>,
    tree_view: OrbitCamera,
    plants_view: OrbitCamera,
    /// Camera view restored when returning to stone mode.
    stone_view: OrbitCamera,
    /// Clump subject and settings, retained while another mode is shown.
    clump: Arc<ClumpSettings>,
    /// Camera view restored when returning to clump mode.
    clump_view: OrbitCamera,
    /// Mushroom settings, retained while another mode is shown.
    mushroom: Arc<MushroomOptions>,
    /// Camera view restored when returning to mushroom mode.
    mushroom_view: OrbitCamera,
    /// The grove, retained while another mode is shown.
    grove: Arc<TreeGrove>,
    /// Camera view restored when returning to grove mode.
    grove_view: OrbitCamera,
    /// Dead wood settings, retained while another mode is shown.
    deadwood: Arc<DeadwoodSettings>,
    /// Camera view restored when returning to dead wood mode.
    deadwood_view: OrbitCamera,
    /// The tree detail level last reported with [`ViewerEvent::LodShown`].
    lod_reported: Option<usize>,
    /// The mushroom detail level last reported with
    /// [`ViewerEvent::MushroomLodShown`], or `None` before the first report.
    mushroom_lod_reported: Option<Option<usize>>,
    /// The dead wood detail level last reported with
    /// [`ViewerEvent::DeadwoodLodShown`], or `None` before the first report.
    deadwood_lod_reported: Option<Option<usize>>,
    /// The grove progress last reported with [`ViewerEvent::GroveProgress`],
    /// or `None` before the first report.
    grove_progress_reported: Option<GroveProgress>,
}

impl SceneState {
    /// The mode shown.
    pub(crate) fn mode(&self) -> SceneMode {
        self.mode
    }

    /// The entity showing the current mode's subject.
    pub(crate) fn subject(&self) -> Entity {
        self.subject
    }

    /// The clump mode's subject and settings.
    pub(crate) fn clump(&self) -> &ClumpSettings {
        &self.clump
    }

    /// What dead wood mode shows.
    pub(crate) fn deadwood(&self) -> &DeadwoodSettings {
        &self.deadwood
    }
}

/// Matches tree, rock, mushroom, grove or dead wood entities whose meshes
/// are still being built.
type Generating = Or<(
    With<TreeGenerating>,
    With<StoneGenerating>,
    With<MushroomGenerating>,
    With<TreeGroveGenerating>,
    With<DeadwoodGenerating>,
)>;

/// Matches requests for impostors that are not baked yet.
type PendingImpostor = (With<ImpostorRequest>, Without<BakedImpostor>);

/// The tree detail level to show at every distance, or `None` to switch by
/// camera distance.
#[derive(Resource, Default)]
pub(crate) struct LodChoice(pub Option<usize>);

/// The mushroom detail level to show at every distance, or `None` to switch
/// by camera distance.
#[derive(Resource, Default)]
pub(crate) struct MushroomLodChoice(pub Option<usize>);

/// The dead wood detail level to show at every distance, or `None` to
/// switch by camera distance.
#[derive(Resource, Default)]
pub(crate) struct DeadwoodLodChoice(pub Option<usize>);

/// The grass and plant detail level to show at every distance, or `None` to switch
/// by camera distance.
#[derive(Resource, Default)]
pub(crate) struct PlantLodChoice(pub Option<u32>);

/// Draws grass and plants at the [`PlantLodChoice`] when it changes.
pub(crate) fn apply_plant_lod(
    choice: Res<PlantLodChoice>,
    settings: Option<ResMut<PlantSettings>>,
) {
    let Some(mut settings) = settings else {
        return;
    };
    if choice.is_changed() {
        settings.forced_lod = choice.0.map(|level| level.min(2));
    }
}

/// A tree or mushroom part's camera-distance range, set aside while one
/// detail level is shown.
#[derive(Component)]
pub(crate) struct SavedRange(VisibilityRange);

/// The settings that decide each mode's starting camera view.
struct ViewSubjects<'a> {
    stone: &'a StoneOptions,
    clump: &'a ClumpSettings,
    mushroom: &'a MushroomOptions,
    /// Width of the grove's square of ground, in metres.
    grove_size: f32,
    deadwood: &'a DeadwoodSettings,
}

/// Returns the starting camera view for a mode. In stone, mushroom and grove
/// mode, the focus and distance follow the subject's size. Clump mode uses
/// the tree or stone view for its subject.
fn default_view(mode: SceneMode, subjects: &ViewSubjects) -> OrbitCamera {
    match mode {
        SceneMode::Clump => match &subjects.clump.subject {
            ClumpSubject::Tree(_) => default_view(SceneMode::Tree, subjects),
            ClumpSubject::Stone(rock) => default_view(
                SceneMode::Stone,
                &ViewSubjects {
                    stone: rock,
                    ..*subjects
                },
            ),
        },
        SceneMode::Mushroom => mushroom_view(subjects.mushroom),
        SceneMode::Deadwood => deadwood_view(subjects.deadwood),
        // Look down over the whole square of ground from beyond its corner.
        SceneMode::Grove => OrbitCamera {
            focus: Vec3::new(0.0, 3.0, 0.0),
            yaw: 0.6,
            pitch: 0.45,
            distance: 1.4 * subjects.grove_size.max(10.0),
            limits: OrbitLimits {
                // Far enough out to reach the impostors, and to see the
                // whole grove.
                distance: (2.0, (2.0 * subjects.grove_size).max(GROVE_FARTHEST)),
                pitch: (-0.05, 1.5),
            },
        },
        SceneMode::Tree => OrbitCamera {
            focus: Vec3::new(0.0, 2.5, 0.0),
            yaw: 0.6,
            pitch: 0.05,
            distance: 12.5,
            limits: OrbitLimits {
                // Far enough out to reach the lowest detail level.
                distance: (0.5, 60.0),
                pitch: (-0.15, 1.3),
            },
        },
        SceneMode::Plants => OrbitCamera {
            focus: Vec3::new(0.0, 0.4, 0.0),
            yaw: 0.0,
            pitch: 0.25,
            distance: 6.0,
            limits: OrbitLimits {
                distance: (0.5, 60.0),
                pitch: (-0.05, 1.4),
            },
        },
        // Aim above the ground and scale the viewing distance with the rock size.
        SceneMode::Stone => OrbitCamera {
            focus: Vec3::new(0.0, 0.3 * subjects.stone.scale, 0.0),
            yaw: 0.6,
            pitch: 0.3,
            distance: 2.2 * subjects.stone.scale,
            limits: OrbitLimits {
                distance: (0.1, 150.0),
                pitch: (-0.05, 1.5),
            },
        },
    }
}

/// The camera's nearest allowed distance in mushroom mode, kept clear of the
/// camera's 0.1 m near clipping plane.
const MUSHROOM_NEAREST: f32 = 0.15;

/// Returns the starting view for the mushroom built from `options`, or the
/// whole cluster when it has several stems: aimed at the middle of its
/// width and half its height, and far enough out to show all of it.
/// Settings that can't be built fall back to a view sized from the cap
/// width alone.
fn mushroom_view(options: &MushroomOptions) -> OrbitCamera {
    let (min, max) = Mushroom::new(options.clone())
        .map(|mushroom| mushroom.bounds())
        .unwrap_or((Vec3::ZERO, Vec3::new(0.0, 2.0 * options.size, 0.0)));
    let height = max.y.max(0.0);
    let width = (max.x - min.x).max(max.z - min.z);
    let extent = height.max(width).max(options.size);
    OrbitCamera {
        focus: Vec3::new(0.5 * (min.x + max.x), 0.5 * height, 0.5 * (min.z + max.z)),
        yaw: 0.6,
        pitch: 0.2,
        distance: (2.5 * extent).max(MUSHROOM_NEAREST),
        limits: OrbitLimits {
            distance: (MUSHROOM_NEAREST, (60.0 * extent).max(2.0)),
            pitch: (-0.05, 1.5),
        },
    }
}

/// The camera's nearest allowed distance in dead wood mode, kept clear of
/// the camera's 0.1 m near clipping plane.
const DEADWOOD_NEAREST: f32 = 0.15;

/// Returns the starting view for dead wood mode: over a scatter's whole
/// square of ground, or for a piece, [`piece_view`].
fn deadwood_view(settings: &DeadwoodSettings) -> OrbitCamera {
    let Some((_, size)) = scatter_options(settings.view) else {
        return piece_view(&settings.piece);
    };
    OrbitCamera {
        focus: Vec3::ZERO,
        yaw: 0.6,
        pitch: 0.5,
        distance: 1.2 * size,
        limits: OrbitLimits {
            distance: (DEADWOOD_NEAREST, 10.0 * size),
            pitch: (-0.05, 1.5),
        },
    }
}

/// The scatter that `view` shows and the width of its square of ground in
/// metres, or `None` for a single piece.
fn scatter_options(view: DeadwoodView) -> Option<(ScatterOptions, f32)> {
    match view {
        DeadwoodView::Piece => None,
        DeadwoodView::Kindling => Some((ScatterOptions::kindling(), 6.0)),
        DeadwoodView::TwigLitter => Some((ScatterOptions::twig_litter(), 4.0)),
    }
}

/// Returns the starting view for the dead wood piece built from `options`:
/// aimed at the middle of the piece and far enough out to show all of it,
/// and allowed out to where it stops being drawn.
fn piece_view(options: &DeadwoodOptions) -> OrbitCamera {
    // A stump stands its length tall on a base widened by its flare; logs
    // and branches lie on the ground.
    let stump = options.kind == PieceKind::Stump;
    let (height, width) = if stump {
        (
            options.length,
            options.diameter * (1.0 + options.roots.flare),
        )
    } else {
        (options.diameter, options.diameter)
    };
    let extent = options.length.max(width);
    let lod = default_lod();
    let cull = lod.cull_distance.expect("the default levels cull") * size_factor(&lod, options);
    OrbitCamera {
        focus: Vec3::new(0.0, 0.5 * height, 0.0),
        yaw: 0.6,
        pitch: 0.35,
        distance: (1.6 * extent).max(DEADWOOD_NEAREST),
        limits: OrbitLimits {
            distance: (DEADWOOD_NEAREST, (1.2 * cull).max(2.0)),
            pitch: (-0.05, 1.5),
        },
    }
}

/// The settings the scene starts with, read once by [`setup_scene`].
#[derive(Resource)]
pub(crate) struct Start(pub SceneStart);

/// Spawns the camera and the starting subject, and records the starting state.
pub(crate) fn setup_scene(mut commands: Commands, start: Res<Start>) {
    let start = &start.0;
    let subjects = ViewSubjects {
        stone: &start.stone,
        clump: &start.clump,
        mushroom: &start.mushroom,
        grove_size: start.environment.grove_size,
        deadwood: &start.deadwood,
    };
    let view_for = |mode| default_view(mode, &subjects);
    let state = SceneState {
        mode: start.mode,
        subject: Entity::PLACEHOLDER,
        pending: None,
        tree: start.tree.clone(),
        requested: Instant::now(),
        field: start.field.clone(),
        tree_view: view_for(SceneMode::Tree),
        plants_view: view_for(SceneMode::Plants),
        stone_view: view_for(SceneMode::Stone),
        clump_view: view_for(SceneMode::Clump),
        mushroom_view: view_for(SceneMode::Mushroom),
        grove_view: view_for(SceneMode::Grove),
        deadwood_view: view_for(SceneMode::Deadwood),
        stone: start.stone.clone(),
        clump: start.clump.clone(),
        mushroom: start.mushroom.clone(),
        grove: start.grove.clone(),
        deadwood: start.deadwood.clone(),
        lod_reported: None,
        mushroom_lod_reported: None,
        deadwood_lod_reported: None,
        grove_progress_reported: None,
    };

    let view = view_for(start.mode);
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            far: FAR_CLIP,
            ..default()
        }),
        ViewportCamera,
        view,
        view.transform(),
        sky_fog(),
    ));

    let subject = spawn_subject(&mut commands, &state, &start.environment);
    commands.insert_resource(SceneState { subject, ..state });
    commands.remove_resource::<Start>();
}

/// Adds the tree, plant field, stone, mushroom or grove for the selected
/// scene mode, or in clump mode, the clump's subject. Plant fields and
/// groves are square, sized by `environment`.
fn spawn_subject(
    commands: &mut Commands,
    state: &SceneState,
    environment: &EnvironmentSettings,
) -> Entity {
    match state.mode {
        SceneMode::Tree => commands.spawn(TreeParams::Options(state.tree.clone())).id(),
        SceneMode::Grove => commands
            .spawn(grove_params(&state.grove, environment.grove_size))
            .id(),
        SceneMode::Plants => commands
            .spawn(grass_field(state, environment.field_size))
            .id(),
        SceneMode::Stone => commands
            .spawn(StoneParams::Options(state.stone.clone()))
            .id(),
        SceneMode::Mushroom => commands
            .spawn(MushroomParams::Options(state.mushroom.clone()))
            .id(),
        SceneMode::Deadwood => spawn_deadwood(commands, &state.deadwood),
        SceneMode::Clump => match &state.clump.subject {
            ClumpSubject::Tree(options) => commands.spawn(TreeParams::Options(options.clone())),
            ClumpSubject::Stone(options) => commands.spawn(StoneParams::Options(options.clone())),
        }
        .id(),
    }
}

/// Creates the preview with the current plants and appearance.
/// The field is a flat square with sides of `size` metres and no density map.
fn grass_field(state: &SceneState, size: f32) -> PlantField {
    PlantField {
        size: Vec2::splat(size),
        ground: PlantGround::default(),
        density_map: None,
        ..state.field.field.clone()
    }
}

/// Spawns what dead wood mode shows for `settings`: the piece, or a
/// scatter of sticks on flat ground around the origin.
fn spawn_deadwood(commands: &mut Commands, settings: &DeadwoodSettings) -> Entity {
    match scatter_options(settings.view) {
        None => commands.spawn(DeadwoodParams::new(settings.piece.clone())),
        Some((scatter, size)) => commands.spawn(DeadwoodScatterParams {
            scatter,
            size: Vec2::splat(size),
            ground: ScatterGround::Flat,
        }),
    }
    .id()
}

/// The grove component for `grove` on a square of flat ground `size` metres
/// across, centred on the origin.
fn grove_params(grove: &TreeGrove, size: f32) -> TreeGroveParams {
    TreeGroveParams::new(grove.clone(), Vec2::splat(size))
}

/// Applies every [`ViewerCommand`] received this frame, in order, and renders
/// enough frames to show the result.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_commands(
    mut received: MessageReader<ViewerCommand>,
    mut commands: Commands,
    mut state: ResMut<SceneState>,
    mut environment: ResMut<EnvironmentSettings>,
    mut lod: ResMut<LodChoice>,
    mut plant_lod: ResMut<PlantLodChoice>,
    mut mushroom_lod: ResMut<MushroomLodChoice>,
    mut deadwood_lod: ResMut<DeadwoodLodChoice>,
    mut camera: Query<&mut OrbitCamera, With<ViewportCamera>>,
    generating: Query<(), Generating>,
    mut activity: ResMut<ViewportActivity>,
) {
    let state = &mut *state;
    if !received.is_empty() {
        activity.request_frames(ViewportActivity::SETTLE_FRAMES);
    }
    for command in received.read() {
        match command {
            ViewerCommand::SetMode(mode) => {
                if *mode == state.mode {
                    continue;
                }
                // Swap camera views and subjects.
                if let Ok(mut orbit) = camera.single_mut() {
                    *view_slot(state, state.mode) = *orbit;
                    *orbit = *view_slot(state, *mode);
                }
                commands.entity(state.subject).despawn();
                if let Some(pending) = state.pending.take() {
                    commands.entity(pending).despawn();
                }
                state.mode = *mode;
                state.requested = Instant::now();
                // The newly shown mode reports its detail level and
                // progress again.
                state.lod_reported = None;
                state.mushroom_lod_reported = None;
                state.deadwood_lod_reported = None;
                state.grove_progress_reported = None;
                state.subject = spawn_subject(&mut commands, state, &environment);
            }
            ViewerCommand::SetTree(options) => {
                state.tree = options.clone();
                if state.mode == SceneMode::Tree {
                    let target = replacement(&mut commands, state, &generating);
                    commands
                        .entity(target)
                        .insert(TreeParams::Options(options.clone()));
                }
            }
            ViewerCommand::SetStone(options) => {
                // Retain settings in every mode; rebuild only when rocks are displayed.
                state.stone = options.clone();
                if state.mode == SceneMode::Stone {
                    let target = replacement(&mut commands, state, &generating);
                    commands
                        .entity(target)
                        .insert(StoneParams::Options(options.clone()));
                }
            }
            ViewerCommand::SetMushroom(options) => {
                // Keep the settings in every mode; rebuild only when the mushroom is shown.
                state.mushroom = options.clone();
                if state.mode == SceneMode::Mushroom {
                    let target = replacement(&mut commands, state, &generating);
                    commands
                        .entity(target)
                        .insert(MushroomParams::Options(options.clone()));
                }
            }
            ViewerCommand::SetDeadwood(settings) => {
                // Keep the settings in every mode; rebuild only when dead wood is shown.
                let old = std::mem::replace(&mut state.deadwood, settings.clone());
                if state.mode != SceneMode::Deadwood {
                    continue;
                }
                if old.view != settings.view {
                    // A piece replaces a scatter, or one scatter another, at once.
                    commands.entity(state.subject).despawn();
                    if let Some(pending) = state.pending.take() {
                        commands.entity(pending).despawn();
                    }
                    state.requested = Instant::now();
                    state.deadwood_lod_reported = None;
                    state.subject = spawn_deadwood(&mut commands, settings);
                } else if settings.view == DeadwoodView::Piece && old.piece != settings.piece {
                    // Keep the old piece shown while the new one builds. A
                    // changed cover alone is regrown by `show_deadwood_cover`.
                    let target = replacement(&mut commands, state, &generating);
                    commands
                        .entity(target)
                        .insert(DeadwoodParams::new(settings.piece.clone()));
                }
            }
            ViewerCommand::SetGrove(grove) => {
                // Keep the grove in every mode; the shown grove updates in place.
                state.grove = grove.clone();
                if state.mode == SceneMode::Grove {
                    state.requested = Instant::now();
                    commands
                        .entity(state.subject)
                        .insert(grove_params(grove, environment.grove_size));
                }
            }
            ViewerCommand::SetClump(settings) => {
                let old = std::mem::replace(&mut state.clump, settings.clone());

                // Rebuild the subject when it changed; the clump follows once it is built.
                if state.mode != SceneMode::Clump || old.subject == settings.subject {
                    continue;
                }
                match &settings.subject {
                    // Keep the old tree or rock shown while a new one of the same kind builds.
                    ClumpSubject::Tree(options) if matches!(old.subject, ClumpSubject::Tree(_)) => {
                        let target = replacement(&mut commands, state, &generating);
                        commands
                            .entity(target)
                            .insert(TreeParams::Options(options.clone()));
                    }
                    ClumpSubject::Stone(options)
                        if matches!(old.subject, ClumpSubject::Stone(_)) =>
                    {
                        let target = replacement(&mut commands, state, &generating);
                        commands
                            .entity(target)
                            .insert(StoneParams::Options(options.clone()));
                    }
                    // A tree replaces a rock, or a rock a tree, at once.
                    _ => {
                        commands.entity(state.subject).despawn();
                        if let Some(pending) = state.pending.take() {
                            commands.entity(pending).despawn();
                        }
                        state.requested = Instant::now();
                        state.subject = spawn_subject(&mut commands, state, &environment);
                    }
                }
            }
            ViewerCommand::SetField(field) => {
                state.field = field.clone();

                // Keep the settings in every mode; update the visible field only in Plants mode.
                if state.mode == SceneMode::Plants {
                    commands
                        .entity(state.subject)
                        .insert(grass_field(state, environment.field_size));
                }
            }
            ViewerCommand::SetEnvironment(settings) => {
                let resized = settings.field_size != environment.field_size;
                let regrown = settings.grove_size != environment.grove_size;
                environment.set_if_neq(*settings);

                // Resize the plant field or grove, including one created
                // earlier in this frame.
                if resized && state.mode == SceneMode::Plants {
                    commands
                        .entity(state.subject)
                        .insert(grass_field(state, settings.field_size));
                }
                if regrown && state.mode == SceneMode::Grove {
                    state.requested = Instant::now();
                    commands
                        .entity(state.subject)
                        .insert(grove_params(&state.grove, settings.grove_size));
                }
            }
            ViewerCommand::SetLod(choice) => {
                lod.0 = *choice;
            }
            ViewerCommand::SetPlantLod(choice) => {
                plant_lod.0 = *choice;
            }
            ViewerCommand::SetMushroomLod(choice) => {
                mushroom_lod.0 = *choice;
            }
            ViewerCommand::SetDeadwoodLod(choice) => {
                deadwood_lod.0 = *choice;
            }
            ViewerCommand::ResetCamera => {
                if let Ok(mut orbit) = camera.single_mut() {
                    let subjects = ViewSubjects {
                        stone: &state.stone,
                        clump: &state.clump,
                        mushroom: &state.mushroom,
                        grove_size: environment.grove_size,
                        deadwood: &state.deadwood,
                    };
                    *orbit = default_view(state.mode, &subjects);
                }
            }
        }
    }
}

/// Chooses where to build the replacement and starts its elapsed-time measurement.
/// Reuses a pending replacement or a subject still generating; otherwise creates
/// a separate entity so the existing tree or rock remains visible during the build.
fn replacement(
    commands: &mut Commands,
    state: &mut SceneState,
    generating: &Query<(), Generating>,
) -> Entity {
    state.requested = Instant::now();
    match state.pending {
        Some(pending) => pending,
        None if generating.contains(state.subject) => state.subject,
        None => *state.pending.insert(commands.spawn_empty().id()),
    }
}

fn view_slot(state: &mut SceneState, mode: SceneMode) -> &mut OrbitCamera {
    match mode {
        SceneMode::Tree => &mut state.tree_view,
        SceneMode::Plants => &mut state.plants_view,
        SceneMode::Stone => &mut state.stone_view,
        SceneMode::Clump => &mut state.clump_view,
        SceneMode::Mushroom => &mut state.mushroom_view,
        SceneMode::Grove => &mut state.grove_view,
        SceneMode::Deadwood => &mut state.deadwood_view,
    }
}

/// Requests more frames while tree, rock, mushroom or dead wood meshes or
/// textures are loading, or tree impostors are baking, so an idle viewport
/// still shows the completed result. Impostors bake only in frames that are
/// drawn.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_while_loading(
    generating: Query<(), Generating>,
    baking: Query<(), PendingImpostor>,
    parts: Query<&MeshMaterial3d<TreeMaterial>, With<TreePart>>,
    groves: Query<&TreeGroveAssets>,
    materials: Res<Assets<TreeMaterial>>,
    stone_parts: Query<&MeshMaterial3d<StoneMaterial>, With<StonePart>>,
    stone_materials: Res<Assets<StoneMaterial>>,
    mushroom_parts: Query<&MeshMaterial3d<MushroomMaterial>, With<MushroomPart>>,
    mushroom_materials: Res<Assets<MushroomMaterial>>,
    deadwood_parts: Query<&MeshMaterial3d<DeadwoodMaterial>, With<DeadwoodPart>>,
    deadwood_materials: Res<Assets<DeadwoodMaterial>>,
    server: Res<AssetServer>,
    mut activity: ResMut<ViewportActivity>,
) {
    let loading = |texture: &Option<Handle<Image>>| {
        texture.as_ref().is_some_and(|handle| {
            server
                .get_load_state(handle)
                .is_some_and(|s| s.is_loading())
        })
    };
    // Tree parts, and the materials groves draw their trees with.
    let grove_materials = groves
        .iter()
        .flat_map(|grove| grove.variants().iter().flat_map(|v| v.parts()))
        .map(|part| part.material.id());
    let textures_loading = parts
        .iter()
        .map(|part| part.0.id())
        .chain(grove_materials)
        .filter_map(|id| materials.get(id))
        .any(|material| {
            let base = &material.base;
            loading(&base.base_color_texture)
                || loading(&base.normal_map_texture)
                || loading(&base.metallic_roughness_texture)
                || loading(&base.occlusion_texture)
        });
    let stone_textures_loading = stone_parts
        .iter()
        .filter_map(|part| stone_materials.get(&part.0))
        .any(|material| {
            let textures = &material.extension;
            loading(&textures.color) || loading(&textures.normal) || loading(&textures.orh)
        });
    let mushroom_textures_loading = mushroom_parts
        .iter()
        .filter_map(|part| mushroom_materials.get(&part.0))
        .any(|material| {
            let cap = &material.extension;
            loading(&cap.cap_color) || loading(&cap.cap_normal) || loading(&cap.cap_orh)
        });
    let deadwood_textures_loading = deadwood_parts
        .iter()
        .filter_map(|part| deadwood_materials.get(&part.0))
        .any(|material| {
            let wood = &material.extension;
            loading(&wood.bark_color)
                || loading(&wood.bark_normal)
                || loading(&wood.bark_roughness)
                || loading(&wood.end_color)
                || loading(&wood.end_normal)
        });
    if !generating.is_empty()
        || !baking.is_empty()
        || textures_loading
        || stone_textures_loading
        || mushroom_textures_loading
        || deadwood_textures_loading
    {
        activity.request_frames(ViewportActivity::SETTLE_FRAMES);
    }
}

/// A tree part with its visibility and its distance range, current or set aside.
type LodPart<'a> = (
    Entity,
    &'a TreePart,
    Option<&'a mut Visibility>,
    Option<&'a VisibilityRange>,
    Option<&'a SavedRange>,
);

/// Shows only the chosen detail level's tree parts, or restores switching by
/// camera distance. Trellis wires show at every level. Clump mode always
/// shows level 0, the level its clump grows on, and grove mode always
/// switches by distance, so each tree shows the level that suits it.
pub(crate) fn apply_lod(
    mut commands: Commands,
    choice: Res<LodChoice>,
    state: Option<Res<SceneState>>,
    mut parts: Query<LodPart>,
) {
    let choice = match state.map(|state| state.mode) {
        Some(SceneMode::Clump) => Some(0),
        Some(SceneMode::Grove) => None,
        _ => choice.0,
    };
    for (entity, part, visibility, range, saved) in &mut parts {
        if part.kind == TreePartKind::Trellis {
            continue;
        }
        let ranges = (range, saved);
        show_level(&mut commands, entity, part.lod, choice, visibility, ranges);
    }
}

/// A mushroom part with its visibility and its distance range, current or set aside.
type MushroomLodPart<'a> = (
    Entity,
    &'a MushroomPart,
    Option<&'a mut Visibility>,
    Option<&'a VisibilityRange>,
    Option<&'a SavedRange>,
);

/// Shows only the chosen detail level's mushroom parts, or restores
/// switching by camera distance.
pub(crate) fn apply_mushroom_lod(
    mut commands: Commands,
    choice: Res<MushroomLodChoice>,
    mut parts: Query<MushroomLodPart>,
) {
    for (entity, part, visibility, range, saved) in &mut parts {
        let ranges = (range, saved);
        show_level(
            &mut commands,
            entity,
            part.lod,
            choice.0,
            visibility,
            ranges,
        );
    }
}

/// A dead wood part with its visibility and its distance range, current or
/// set aside.
type DeadwoodLodPart<'a> = (
    Entity,
    &'a DeadwoodPart,
    Option<&'a mut Visibility>,
    Option<&'a VisibilityRange>,
    Option<&'a SavedRange>,
);

/// Shows only the chosen detail level's dead wood parts, or restores
/// switching by camera distance.
pub(crate) fn apply_deadwood_lod(
    mut commands: Commands,
    choice: Res<DeadwoodLodChoice>,
    mut parts: Query<DeadwoodLodPart>,
) {
    for (entity, part, visibility, range, saved) in &mut parts {
        let ranges = (range, saved);
        show_level(
            &mut commands,
            entity,
            part.lod,
            choice.0,
            visibility,
            ranges,
        );
    }
}

/// A distance range that shows a part at every camera distance.
///
/// It still has a fade at its far end, far beyond any camera. Bevy keeps
/// drawing a part with the shader it first picked for it, and a part first
/// drawn with a fading range uses a fading shader; that shader needs a
/// fading range to draw the part at all.
fn every_distance() -> VisibilityRange {
    VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: f32::MAX / 2.0..f32::MAX,
        use_aabb: false,
    }
}

/// Shows the part `entity` of detail level `level` only if `choice` is that
/// level, at every distance, setting its own distance range aside; with no
/// choice, puts each part's range back and leaves the ranges to decide.
/// `ranges` holds the part's current range and the one set aside, if any.
fn show_level(
    commands: &mut Commands,
    entity: Entity,
    level: usize,
    choice: Option<usize>,
    visibility: Option<Mut<Visibility>>,
    ranges: (Option<&VisibilityRange>, Option<&SavedRange>),
) {
    // The chosen level's range is set aside and replaced while it is
    // chosen; every other part has its own range. A chosen part whose range
    // its plugin changed since (as when its tree's impostor is ready) has the
    // new range set aside instead.
    let (range, saved) = ranges;
    let mut entity = commands.entity(entity);
    let chosen = choice == Some(level);
    match (chosen, range, saved) {
        (true, Some(range), None) => {
            entity.insert((SavedRange(range.clone()), every_distance()));
        }
        (true, Some(range), Some(_)) if *range != every_distance() => {
            entity.insert((SavedRange(range.clone()), every_distance()));
        }
        (false, _, Some(SavedRange(range))) => {
            entity.remove::<SavedRange>().insert(range.clone());
        }
        _ => {}
    }
    let wanted = if choice.is_none() || chosen {
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

/// The detail level that the camera distance `distance` selects.
fn lod_at(distance: f32) -> usize {
    lod_levels()
        .iter()
        .rposition(|level| distance >= level.distance)
        .unwrap_or(0)
}

/// Reports the tree detail level on screen whenever it changes.
pub(crate) fn report_lod(
    mut state: ResMut<SceneState>,
    choice: Res<LodChoice>,
    camera: Query<&Transform, With<ViewportCamera>>,
    mut events: MessageWriter<ViewerEvent>,
) {
    if state.mode != SceneMode::Tree {
        return;
    }
    let Ok(camera) = camera.single() else {
        return;
    };
    // The tree stands at the origin.
    let shown = choice
        .0
        .unwrap_or_else(|| lod_at(camera.translation.length()));
    if state.lod_reported != Some(shown) {
        state.lod_reported = Some(shown);
        events.write(ViewerEvent::LodShown(shown));
    }
}

/// Reports the mushroom detail level on screen whenever it changes.
pub(crate) fn report_mushroom_lod(
    mut state: ResMut<SceneState>,
    choice: Res<MushroomLodChoice>,
    camera: Query<&Transform, With<ViewportCamera>>,
    children: Query<&Children>,
    parts: Query<(&MushroomPart, Option<&VisibilityRange>, Option<&SavedRange>)>,
    mut events: MessageWriter<ViewerEvent>,
) {
    if state.mode != SceneMode::Mushroom {
        return;
    }
    let Ok(camera) = camera.single() else {
        return;
    };

    // Each level's distance range, from the shown mushroom's parts. Their
    // ranges grow with the mushroom's size.
    let kids = children.get(state.subject).into_iter().flatten();
    let ranges: Vec<(usize, &VisibilityRange)> = parts
        .iter_many(kids)
        .flatten()
        .filter_map(|(part, range, saved)| Some((part.lod, saved.map(|s| &s.0).or(range)?)))
        .collect();
    if ranges.is_empty() {
        return;
    }

    // The mushroom stands at the origin.
    let shown = match choice.0 {
        Some(level) => Some(level),
        None => level_in_range(camera.translation.length(), &ranges),
    };
    if state.mushroom_lod_reported != Some(shown) {
        state.mushroom_lod_reported = Some(shown);
        events.write(ViewerEvent::MushroomLodShown(shown));
    }
}

/// Reports the dead wood detail level on screen whenever it changes.
pub(crate) fn report_deadwood_lod(
    mut state: ResMut<SceneState>,
    choice: Res<DeadwoodLodChoice>,
    camera: Query<&Transform, With<ViewportCamera>>,
    children: Query<&Children>,
    parts: Query<(&DeadwoodPart, Option<&VisibilityRange>, Option<&SavedRange>)>,
    mut events: MessageWriter<ViewerEvent>,
) {
    if state.mode != SceneMode::Deadwood {
        return;
    }
    let Ok(camera) = camera.single() else {
        return;
    };

    // Each level's distance range, from the shown piece's parts. Their
    // ranges grow with the piece's size.
    let kids = children.get(state.subject).into_iter().flatten();
    let ranges: Vec<(usize, &VisibilityRange)> = parts
        .iter_many(kids)
        .flatten()
        .filter_map(|(part, range, saved)| Some((part.lod, saved.map(|s| &s.0).or(range)?)))
        .collect();
    if ranges.is_empty() {
        return;
    }

    // The piece lies at the origin.
    let shown = match choice.0 {
        Some(level) => Some(level),
        None => level_in_range(camera.translation.length(), &ranges),
    };
    if state.deadwood_lod_reported != Some(shown) {
        state.deadwood_lod_reported = Some(shown);
        events.write(ViewerEvent::DeadwoodLodShown(shown));
    }
}

/// Reports how far the shown grove has got whenever that changes: its tree
/// shapes generating, then their impostors baking.
pub(crate) fn report_grove_progress(
    mut state: ResMut<SceneState>,
    groves: Query<(Option<&TreeGroveAssets>, Has<TreeGroveGenerating>)>,
    baking: Query<(), PendingImpostor>,
    mut events: MessageWriter<ViewerEvent>,
) {
    if state.mode != SceneMode::Grove {
        return;
    }
    let Ok((assets, generating)) = groves.get(state.subject) else {
        return;
    };
    let variants = assets.map(TreeGroveAssets::variants).unwrap_or_default();
    let generated = variants.iter().filter(|v| v.is_ready()).count();

    // A generated shape that has no impostor yet is waiting for its bake,
    // unless no bake is pending: then it will never have one.
    let progress = if generating {
        GroveProgress::Generating {
            done: generated,
            total: variants.len(),
        }
    } else {
        let baked = variants.iter().filter(|v| v.has_impostor()).count();
        if baked < generated && !baking.is_empty() {
            GroveProgress::Baking {
                done: baked,
                total: generated,
            }
        } else {
            GroveProgress::Idle
        }
    };
    if state.grove_progress_reported != Some(progress) {
        state.grove_progress_reported = Some(progress);
        events.write(ViewerEvent::GroveProgress(progress));
    }
}

/// The level, from `(level, range)` pairs, whose distance range covers
/// `distance`, counting each blend between levels as switching halfway
/// through. `None` if no range covers it.
fn level_in_range(distance: f32, ranges: &[(usize, &VisibilityRange)]) -> Option<usize> {
    let middle = |margin: &std::ops::Range<f32>| (margin.start + margin.end) / 2.0;
    ranges
        .iter()
        .find(|(_, range)| {
            middle(&range.start_margin) <= distance && distance < middle(&range.end_margin)
        })
        .map(|(level, _)| *level)
}

/// Swaps in a finished replacement tree, and reports the shown tree's size
/// once it finishes generating.
pub(crate) fn report_tree(
    event: On<TreeFinished>,
    mut commands: Commands,
    mut state: ResMut<SceneState>,
    mut events: MessageWriter<ViewerEvent>,
    children: Query<&Children>,
    parts: Query<(&TreePart, &Mesh3d)>,
    meshes: Res<Assets<Mesh>>,
) {
    if state.pending == Some(event.entity) {
        // The new parts already exist, so removing the old tree in this same
        // frame never leaves the view empty. A rejected tree leaves the old one shown.
        state.pending = None;
        if event.result.is_ok() {
            commands.entity(state.subject).despawn();
            state.subject = event.entity;
        } else {
            commands.entity(event.entity).despawn();
        }
    } else if event.entity != state.subject {
        return;
    }
    let result = event.result.clone().map(|()| {
        let mut levels = vec![MeshSize::default(); lod_levels().len()];
        let kids = children.get(event.entity).into_iter().flatten();
        add_tree_parts(&mut levels, parts.iter_many(kids).flatten(), &meshes);
        BuildStats {
            levels,
            build_time: state.requested.elapsed(),
        }
    });
    // In clump mode, a rejected subject also means no clump.
    if state.mode == SceneMode::Clump
        && let Err(error) = &result
    {
        events.write(ViewerEvent::ClumpBuilt(Err(ClumpBuildError::Tree(
            error.clone(),
        ))));
    }
    events.write(ViewerEvent::TreeBuilt(result));
}

/// Adds the size of each tree part in `parts` to its detail level's entry in
/// `levels`. Trellis wires show at every level, so they count toward each.
fn add_tree_parts<'a>(
    levels: &mut [MeshSize],
    parts: impl Iterator<Item = (&'a TreePart, &'a Mesh3d)>,
    meshes: &Assets<Mesh>,
) {
    for (part, mesh) in parts {
        let Some(mesh) = meshes.get(&mesh.0) else {
            continue;
        };
        let vertices = mesh.count_vertices();
        let triangles = mesh.indices().map_or(0, |i| i.len() / 3);
        let sizes = if part.kind == TreePartKind::Trellis {
            &mut levels[..]
        } else {
            levels.get_mut(part.lod..=part.lod).unwrap_or_default()
        };
        for size in sizes {
            size.vertices += vertices;
            size.triangles += triangles;
        }
    }
}

/// Reports the shown grove's tree count and size once a build ends, or why
/// it could not be built.
pub(crate) fn report_grove(
    event: On<TreeGroveFinished>,
    state: Res<SceneState>,
    mut events: MessageWriter<ViewerEvent>,
    groves: Query<&TreeGroveStats>,
) {
    if state.mode != SceneMode::Grove || event.entity != state.subject {
        return;
    }
    let result = event.result.clone().map(|()| {
        let stats = groves.get(event.entity).cloned().unwrap_or_default();
        let mut levels = vec![MeshSize::default(); lod_levels().len()];
        for (size, level) in levels.iter_mut().zip(&stats.levels) {
            size.vertices = level.vertices;
            size.triangles = level.triangles;
        }
        GroveStats {
            trees: stats.trees,
            build: BuildStats {
                levels,
                build_time: state.requested.elapsed(),
            },
        }
    });
    events.write(ViewerEvent::GroveBuilt(result));
}

/// Displays a successfully built rock sunk into the ground by its suggested
/// embed depth, and reports its mesh counts and build time. Reports rejected
/// settings without replacing the old rock.
pub(crate) fn report_stone(
    event: On<StoneFinished>,
    mut commands: Commands,
    mut state: ResMut<SceneState>,
    mut events: MessageWriter<ViewerEvent>,
    rocks: Query<(&StoneParams, Option<&Children>, Option<&StoneEmbedDepth>)>,
    parts: Query<(&StonePart, &Mesh3d)>,
    meshes: Res<Assets<Mesh>>,
) {
    // Remove the old rock only after its replacement has built successfully.
    if state.pending == Some(event.entity) {
        state.pending = None;
        if event.result.is_ok() {
            commands.entity(state.subject).despawn();
            state.subject = event.entity;
        } else {
            commands.entity(event.entity).despawn();
        }
    } else if event.entity != state.subject {
        return;
    }

    let result = event.result.clone().map(|()| {
        // Count vertices and triangles per level; levels without a mesh stay at zero.
        let Ok((stone, kids, embed)) = rocks.get(event.entity) else {
            return BuildStats {
                levels: Vec::new(),
                build_time: state.requested.elapsed(),
            };
        };
        let level_count = match stone {
            StoneParams::Options(options) => options.mesh.detail.levels().len(),
            StoneParams::Asset(_) => 0,
        };
        let mut levels = vec![MeshSize::default(); level_count];
        let kids = kids.into_iter().flatten();
        for (part, mesh) in parts.iter_many(kids).flatten() {
            let (Some(mesh), Some(size)) = (meshes.get(&mesh.0), levels.get_mut(part.lod)) else {
                continue;
            };
            size.vertices = mesh.count_vertices();
            size.triangles = mesh.indices().map_or(0, |i| i.len() / 3);
        }

        // The rock already rests on the ground plane; sink it by its embed depth.
        let embed = embed.map_or(0.0, |depth| depth.0);
        commands
            .entity(event.entity)
            .insert(Transform::from_xyz(0.0, -embed, 0.0));
        BuildStats {
            levels,
            build_time: state.requested.elapsed(),
        }
    });
    // In clump mode, a rejected subject also means no clump.
    if state.mode == SceneMode::Clump
        && let Err(error) = &result
    {
        events.write(ViewerEvent::ClumpBuilt(Err(ClumpBuildError::Stone(
            error.clone(),
        ))));
    }
    events.write(ViewerEvent::StoneBuilt(result));
}

/// Displays a successfully built mushroom in place of the previous one, and
/// reports its mesh counts per detail level and build time. Reports rejected
/// settings without replacing the old mushroom.
pub(crate) fn report_mushroom(
    event: On<MushroomFinished>,
    mut commands: Commands,
    mut state: ResMut<SceneState>,
    mut events: MessageWriter<ViewerEvent>,
    children: Query<&Children>,
    parts: Query<(&MushroomPart, &Mesh3d)>,
    meshes: Res<Assets<Mesh>>,
) {
    // Remove the old mushroom only after its replacement has built successfully.
    if state.pending == Some(event.entity) {
        state.pending = None;
        if event.result.is_ok() {
            commands.entity(state.subject).despawn();
            state.subject = event.entity;
        } else {
            commands.entity(event.entity).despawn();
        }
    } else if event.entity != state.subject {
        return;
    }

    // Add up each level's parts: a flat level has a cap and a stem strip.
    let result = event.result.clone().map(|()| {
        let mut levels: Vec<MeshSize> = Vec::new();
        let kids = children.get(event.entity).into_iter().flatten();
        for (part, mesh) in parts.iter_many(kids).flatten() {
            let Some(mesh) = meshes.get(&mesh.0) else {
                continue;
            };
            if levels.len() <= part.lod {
                levels.resize(part.lod + 1, MeshSize::default());
            }
            levels[part.lod].vertices += mesh.count_vertices();
            levels[part.lod].triangles += mesh.indices().map_or(0, |i| i.len() / 3);
        }
        BuildStats {
            levels,
            build_time: state.requested.elapsed(),
        }
    });
    events.write(ViewerEvent::MushroomBuilt(result));
}

/// Displays a successfully built dead wood piece in place of the previous
/// one, and reports its mesh counts per detail level and build time.
/// Reports rejected settings without replacing the old piece.
pub(crate) fn report_deadwood(
    event: On<DeadwoodFinished>,
    mut commands: Commands,
    mut state: ResMut<SceneState>,
    mut events: MessageWriter<ViewerEvent>,
    children: Query<&Children>,
    parts: Query<(&DeadwoodPart, &Mesh3d)>,
    meshes: Res<Assets<Mesh>>,
) {
    // Remove the old piece only after its replacement has built successfully.
    if state.pending == Some(event.entity) {
        state.pending = None;
        if event.result.is_ok() {
            commands.entity(state.subject).despawn();
            state.subject = event.entity;
        } else {
            commands.entity(event.entity).despawn();
        }
    } else if event.entity != state.subject {
        return;
    }

    // Add up each level's parts: a level can have round stems and strips.
    let result = event.result.clone().map(|()| {
        let mut levels: Vec<MeshSize> = Vec::new();
        let kids = children.get(event.entity).into_iter().flatten();
        for (part, mesh) in parts.iter_many(kids).flatten() {
            let Some(mesh) = meshes.get(&mesh.0) else {
                continue;
            };
            if levels.len() <= part.lod {
                levels.resize(part.lod + 1, MeshSize::default());
            }
            levels[part.lod].vertices += mesh.count_vertices();
            levels[part.lod].triangles += mesh.indices().map_or(0, |i| i.len() / 3);
        }
        BuildStats {
            levels,
            build_time: state.requested.elapsed(),
        }
    });
    events.write(ViewerEvent::DeadwoodBuilt(result));
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;

    use super::*;

    fn range(start: f32, end: f32) -> VisibilityRange {
        VisibilityRange {
            start_margin: start..start,
            end_margin: end..end,
            use_aabb: false,
        }
    }

    /// Runs [`apply_lod`] with `choice`, and returns each part's visibility
    /// and distance range.
    fn apply(
        world: &mut World,
        choice: Option<usize>,
    ) -> Vec<(TreePart, Visibility, Option<VisibilityRange>)> {
        world.insert_resource(LodChoice(choice));
        world.run_system_once(apply_lod).unwrap();
        let mut parts = world.query::<(&TreePart, &Visibility, Option<&VisibilityRange>)>();
        parts
            .iter(world)
            .map(|(part, visibility, range)| (*part, *visibility, range.cloned()))
            .collect()
    }

    #[test]
    fn a_chosen_level_shows_alone_until_distance_switching_returns() {
        // Three bark levels with distance ranges, and an unranged trellis wire.
        let mut world = World::new();
        let ranges = [
            range(0.0, 100.0),
            range(100.0, 250.0),
            range(250.0, f32::INFINITY),
        ];
        for (lod, range) in ranges.iter().enumerate() {
            let kind = TreePartKind::Bark;
            world.spawn((TreePart { lod, kind }, Visibility::Inherited, range.clone()));
        }
        let trellis = TreePart {
            lod: 0,
            kind: TreePartKind::Trellis,
        };
        world.spawn((trellis, Visibility::Inherited));

        // Only level 2 and the trellis show, and level 2 shows at any distance.
        for (part, visibility, range) in apply(&mut world, Some(2)) {
            let shown = part.kind == TreePartKind::Trellis || part.lod == 2;
            assert_eq!(visibility == Visibility::Inherited, shown, "{part:?}");
            let expected = match part.kind {
                TreePartKind::Trellis => None,
                _ if part.lod == 2 => Some(every_distance()),
                _ => Some(ranges[part.lod].clone()),
            };
            assert!(range == expected, "{part:?} has the wrong range");
        }

        // Distance switching comes back with the original ranges.
        for (part, visibility, range) in apply(&mut world, None) {
            assert_eq!(visibility, Visibility::Inherited, "{part:?}");
            let original = (part.kind != TreePartKind::Trellis).then(|| ranges[part.lod].clone());
            assert!(range == original, "{part:?} has the wrong range");
        }
    }

    /// Runs [`apply_mushroom_lod`] with `choice`, and returns each part's
    /// level, visibility and distance range.
    fn apply_mushroom(
        world: &mut World,
        choice: Option<usize>,
    ) -> Vec<(usize, Visibility, Option<VisibilityRange>)> {
        world.insert_resource(MushroomLodChoice(choice));
        world.run_system_once(apply_mushroom_lod).unwrap();
        let mut parts = world.query::<(&MushroomPart, &Visibility, Option<&VisibilityRange>)>();
        parts
            .iter(world)
            .map(|(part, visibility, range)| (part.lod, *visibility, range.cloned()))
            .collect()
    }

    #[test]
    fn a_chosen_mushroom_level_shows_alone_even_on_a_new_mushroom() {
        // Two levels, the second with a stem strip as well as a cap.
        let mut world = World::new();
        let ranges = [range(0.0, 1.5), range(1.5, 25.0)];
        let spawn_parts = |world: &mut World| {
            for (lod, billboard) in [(0, false), (1, false), (1, true)] {
                let part = MushroomPart {
                    instance: 0,
                    lod,
                    billboard,
                };
                world.spawn((part, Visibility::Inherited, ranges[lod].clone()));
            }
        };
        spawn_parts(&mut world);

        // Only level 1 shows, at any distance; a replacement mushroom's new
        // parts follow the choice too.
        let check = |parts: Vec<(usize, Visibility, Option<VisibilityRange>)>| {
            for (lod, visibility, range) in parts {
                assert_eq!(visibility == Visibility::Inherited, lod == 1, "level {lod}");
                let expected = if lod == 1 {
                    every_distance()
                } else {
                    ranges[lod].clone()
                };
                assert!(range == Some(expected), "level {lod} has the wrong range");
            }
        };
        check(apply_mushroom(&mut world, Some(1)));
        spawn_parts(&mut world);
        let parts = apply_mushroom(&mut world, Some(1));
        assert_eq!(parts.len(), 6);
        check(parts);

        // Choosing another level gives level 1 its own range back.
        for (lod, _, range) in apply_mushroom(&mut world, Some(0)) {
            let expected = if lod == 0 {
                every_distance()
            } else {
                ranges[lod].clone()
            };
            assert!(range == Some(expected), "level {lod} has the wrong range");
        }

        // Distance switching comes back with the original ranges.
        for (lod, visibility, range) in apply_mushroom(&mut world, None) {
            assert_eq!(visibility, Visibility::Inherited, "level {lod}");
            assert!(
                range == Some(ranges[lod].clone()),
                "level {lod} has the wrong range"
            );
        }
    }

    #[test]
    fn camera_distance_picks_the_mushroom_level_from_its_ranges() {
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
    fn camera_distance_picks_the_level() {
        assert_eq!(lod_at(0.0), 0);
        assert_eq!(lod_at(174.0), 0);
        assert_eq!(lod_at(175.0), 1);
        assert_eq!(lod_at(275.0), 2);
        assert_eq!(lod_at(499.0), 2);
        assert_eq!(lod_at(500.0), 3);
        assert_eq!(lod_at(1000.0), 3);
    }

    #[test]
    fn every_tree_detail_level_has_a_distance() {
        assert_eq!(default_tree_lod().levels.len(), LOD_DISTANCES.len());
    }
}
