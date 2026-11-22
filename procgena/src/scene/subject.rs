//! The subject being shown (a tree, a grass field, a rock or a mushroom), the camera, and command handling.

use std::sync::Arc;
use std::time::Instant;

use super::environment::{EnvironmentSettings, sky_fog};
use super::orbit::{OrbitCamera, OrbitLimits};
use super::protocol::{
    BuildStats, ClumpBuildError, ClumpSettings, ClumpSubject, FieldSettings, MeshSize, SceneMode,
    SceneStart, ViewerCommand, ViewerEvent,
};
use super::viewport::{ViewportActivity, ViewportCamera};
use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use procgena_mushroom::{Mushroom, MushroomOptions};
use procgena_mushroom_bevy::{
    MushroomFinished, MushroomGenerating, MushroomMaterial, MushroomParams, MushroomPart,
};
use procgena_plant::prelude::{PlantField, PlantGround, PlantSettings};
use procgena_stone::StoneOptions;
use procgena_stone_bevy::{
    StoneEmbedDepth, StoneFinished, StoneGenerating, StoneMaterial, StoneParams, StonePart,
};
use procgena_tree::TreeOptions;
use procgena_tree_bevy::{
    TreeFinished, TreeGenerating, TreeLodLevel, TreeMaterial, TreeParams, TreePart, TreePartKind,
    default_tree_lod_levels,
};

/// Camera distances at which each tree detail level starts, nearest first.
const LOD_DISTANCES: [f32; 4] = [0.0, 175.0, 275.0, 500.0];

/// The tree detail levels the scene shows, most detailed first: procgena_tree_bevy's
/// default levels, switched at greater camera distances so that full detail
/// lasts further out. The lowest level starts at 500 m.
pub fn lod_levels() -> Vec<TreeLodLevel> {
    let mut levels = default_tree_lod_levels();
    for (level, distance) in levels.iter_mut().zip(LOD_DISTANCES) {
        level.level.distance = distance;
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
    /// The tree detail level last reported with [`ViewerEvent::LodShown`].
    lod_reported: Option<usize>,
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
}

/// Matches tree, rock or mushroom entities whose meshes are still being built.
type Generating = Or<(
    With<TreeGenerating>,
    With<StoneGenerating>,
    With<MushroomGenerating>,
)>;

/// The tree detail level to show at every distance, or `None` to switch by
/// camera distance.
#[derive(Resource, Default)]
pub(crate) struct LodChoice(pub Option<usize>);

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

/// A tree part's camera-distance range, set aside while one detail level is shown.
#[derive(Component)]
pub(crate) struct SavedRange(VisibilityRange);

/// The settings that decide each mode's starting camera view.
struct ViewSubjects<'a> {
    stone: &'a StoneOptions,
    clump: &'a ClumpSettings,
    mushroom: &'a MushroomOptions,
}

/// Returns the starting camera view for a mode. In stone and mushroom mode,
/// the focus and distance follow the subject's size. Clump mode uses the
/// tree or stone view for its subject.
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

/// Returns the starting view for the mushroom built from `options`: aimed at
/// half its height and far enough out to show all of it. Settings that can't
/// be built fall back to a view sized from the cap width alone.
fn mushroom_view(options: &MushroomOptions) -> OrbitCamera {
    let height = Mushroom::new(options.clone())
        .map(|mushroom| mushroom.apex().y)
        .unwrap_or(2.0 * options.size);
    let extent = height.max(options.size);
    OrbitCamera {
        focus: Vec3::new(0.0, 0.5 * height, 0.0),
        yaw: 0.6,
        pitch: 0.2,
        distance: (2.5 * extent).max(MUSHROOM_NEAREST),
        limits: OrbitLimits {
            distance: (MUSHROOM_NEAREST, (60.0 * extent).max(2.0)),
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
        stone: start.stone.clone(),
        clump: start.clump.clone(),
        mushroom: start.mushroom.clone(),
        lod_reported: None,
    };

    let view = view_for(start.mode);
    commands.spawn((
        Camera3d::default(),
        ViewportCamera,
        view,
        view.transform(),
        sky_fog(),
    ));

    let subject = spawn_subject(&mut commands, &state, start.environment.field_size);
    commands.insert_resource(SceneState { subject, ..state });
    commands.remove_resource::<Start>();
}

/// Adds the tree, plant field, stone or mushroom for the selected scene mode,
/// or in clump mode, the clump's subject. Plant fields are square, with each side
/// `field_size` metres long.
fn spawn_subject(commands: &mut Commands, state: &SceneState, field_size: f32) -> Entity {
    match state.mode {
        SceneMode::Tree => commands.spawn(TreeParams::Options(state.tree.clone())).id(),
        SceneMode::Plants => commands.spawn(grass_field(state, field_size)).id(),
        SceneMode::Stone => commands
            .spawn(StoneParams::Options(state.stone.clone()))
            .id(),
        SceneMode::Mushroom => commands
            .spawn(MushroomParams::Options(state.mushroom.clone()))
            .id(),
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
                if *mode == SceneMode::Tree {
                    state.lod_reported = None;
                }
                state.subject = spawn_subject(&mut commands, state, environment.field_size);
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
                        state.subject = spawn_subject(&mut commands, state, environment.field_size);
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
                environment.set_if_neq(*settings);

                // Resize the plant field, including one created earlier in this frame.
                if resized && state.mode == SceneMode::Plants {
                    commands
                        .entity(state.subject)
                        .insert(grass_field(state, settings.field_size));
                }
            }
            ViewerCommand::SetLod(choice) => {
                lod.0 = *choice;
            }
            ViewerCommand::SetPlantLod(choice) => {
                plant_lod.0 = *choice;
            }
            ViewerCommand::ResetCamera => {
                if let Ok(mut orbit) = camera.single_mut() {
                    let subjects = ViewSubjects {
                        stone: &state.stone,
                        clump: &state.clump,
                        mushroom: &state.mushroom,
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
    }
}

/// Requests more frames while tree, rock or mushroom meshes or textures are
/// loading, so an idle viewport still shows the completed result.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_while_loading(
    generating: Query<(), Generating>,
    parts: Query<&MeshMaterial3d<TreeMaterial>, With<TreePart>>,
    materials: Res<Assets<TreeMaterial>>,
    stone_parts: Query<&MeshMaterial3d<StoneMaterial>, With<StonePart>>,
    stone_materials: Res<Assets<StoneMaterial>>,
    mushroom_parts: Query<&MeshMaterial3d<MushroomMaterial>, With<MushroomPart>>,
    mushroom_materials: Res<Assets<MushroomMaterial>>,
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
    let textures_loading = parts
        .iter()
        .filter_map(|part| materials.get(&part.0))
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
    if !generating.is_empty()
        || textures_loading
        || stone_textures_loading
        || mushroom_textures_loading
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
/// shows level 0, the level its clump grows on.
pub(crate) fn apply_lod(
    mut commands: Commands,
    choice: Res<LodChoice>,
    state: Option<Res<SceneState>>,
    mut parts: Query<LodPart>,
) {
    let clump_mode = state.is_some_and(|state| state.mode == SceneMode::Clump);
    let choice = if clump_mode { Some(0) } else { choice.0 };
    for (entity, part, visibility, range, saved) in &mut parts {
        if part.kind == TreePartKind::Trellis {
            continue;
        }

        // Set the distance range aside while one level is shown, and put it back after.
        let mut entity = commands.entity(entity);
        let wanted = match choice {
            Some(level) => {
                if let Some(range) = range {
                    entity
                        .remove::<VisibilityRange>()
                        .insert(SavedRange(range.clone()));
                }
                if part.lod == level {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                }
            }
            None => {
                if let Some(SavedRange(range)) = saved {
                    entity.remove::<SavedRange>().insert(range.clone());
                }
                Visibility::Inherited
            }
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
}

/// The detail level that the camera distance `distance` selects.
fn lod_at(distance: f32) -> usize {
    lod_levels()
        .iter()
        .rposition(|level| distance >= level.level.distance)
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
        // Add each part to its level's size; trellis wires show at every level.
        let mut levels = vec![MeshSize::default(); lod_levels().len()];
        let kids = children.get(event.entity).into_iter().flatten();
        for (part, mesh) in parts.iter_many(kids).flatten() {
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

        // Only level 2 and the trellis show, at any distance.
        for (part, visibility, range) in apply(&mut world, Some(2)) {
            let shown = part.kind == TreePartKind::Trellis || part.lod == 2;
            assert_eq!(visibility == Visibility::Inherited, shown, "{part:?}");
            assert!(range.is_none(), "{part:?} keeps its range");
        }

        // Distance switching comes back with the original ranges.
        for (part, visibility, range) in apply(&mut world, None) {
            assert_eq!(visibility, Visibility::Inherited, "{part:?}");
            let original = (part.kind != TreePartKind::Trellis).then(|| ranges[part.lod].clone());
            assert!(range == original, "{part:?} has the wrong range");
        }
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
        assert_eq!(default_tree_lod_levels().len(), LOD_DISTANCES.len());
    }
}
