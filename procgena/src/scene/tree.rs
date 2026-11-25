//! Tree subject: a single generated tree, drawn by the shared drawing
//! chain: its game-level mesh, its lower mesh, then its impostor.

use std::sync::Arc;

use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use procgena_core::chain::{DistanceChain, Step, ViewScale};
use procgena_tree::TreeOptions;
use procgena_tree_bevy::{
    TreeFinished, TreeGenerating, TreeMaterial, TreeParams, TreePart, TreePartKind,
    TreeScatterAssets,
};

use super::orbit::{OrbitCamera, OrbitLimits};
use super::protocol::{BuildStats, MeshSize, ViewerEvent};
use super::subject::{
    LevelPart, SavedRange, SceneState, Subject, SubjectLook, level_in_range, show_level,
};
use super::viewport::{ViewportActivity, ViewportCamera};
use super::{AddSubject, Shown};

/// The tree mesh steps the scene reports sizes for, and that can be chosen
/// to show alone: the game-level mesh, then the lower mesh. The impostor
/// is the step after them.
pub const TREE_STEPS: [Step; 2] = [Step::Game, Step::Lower];

/// How far away, in metres, a tree `size` metres across (its
/// bounding-sphere diameter) starts turning into its impostor, at High
/// quality.
pub(crate) fn impostor_distance(size: f32) -> f32 {
    DistanceChain::factor(size, ViewScale::DEFAULT) / DistanceChain::DEFAULT.impostor_below
}

/// A single tree, built from its [`TreeOptions`].
///
/// A changed tree is rebuilt in the background and replaces the old one
/// once ready; [`ViewerEvent::TreeBuilt`] reports each build. While it is
/// shown, [`ViewerEvent::LodShown`] reports the step on screen.
pub struct TreeSubject;

impl Subject for TreeSubject {
    type Settings = TreeOptions;
    const NAME: &'static str = "tree";

    fn spawn(world: &mut World, settings: &TreeOptions) -> Entity {
        world
            .spawn(TreeParams::Options(Arc::new(settings.clone())))
            .id()
    }

    fn update(world: &mut World, shown: &mut Shown, _: &TreeOptions, new: &TreeOptions) {
        let target = shown.replacement::<TreeGenerating>(world);
        world
            .entity_mut(target)
            .insert(TreeParams::Options(Arc::new(new.clone())));
    }

    fn view(_: &World, _: &TreeOptions) -> OrbitCamera {
        tree_view()
    }
}

/// The starting view of a single tree.
pub(crate) fn tree_view() -> OrbitCamera {
    OrbitCamera {
        focus: Vec3::new(0.0, 2.5, 0.0),
        yaw: 0.6,
        pitch: 0.05,
        distance: 12.5,
        limits: OrbitLimits {
            // Far enough out to see a 10 m tree switch to its lower mesh.
            distance: (0.5, 150.0),
            pitch: (-0.15, 1.3),
        },
    }
}

/// Adds the tree subject and the step handling of every tree part.
pub(crate) fn plugin(app: &mut App) {
    app.add_subject::<TreeSubject>()
        .add_systems(Update, (render_while_loading, report_lod))
        .add_observer(report_tree);
}

/// Shows each tree part as the shown subject's detail asks, by its step's
/// index. Trellis wires show at every step.
pub(crate) fn apply_lod(
    mut commands: Commands,
    look: Res<SubjectLook>,
    mut parts: Query<LevelPart<TreePart>>,
) {
    for (entity, part, visibility, range, saved) in &mut parts {
        if part.kind == TreePartKind::Trellis {
            continue;
        }
        let ranges = (range, saved);
        show_level(
            &mut commands,
            entity,
            part.step.index(),
            look.detail,
            visibility,
            ranges,
        );
    }
}

/// Reports the tree step on screen, by its index, whenever it changes, and
/// when a tree is first shown.
fn report_lod(
    state: Res<SceneState>,
    look: Res<SubjectLook>,
    camera: Query<&Transform, With<ViewportCamera>>,
    children: Query<&Children>,
    parts: Query<(&TreePart, Option<&VisibilityRange>, Option<&SavedRange>)>,
    mut reported: Local<Option<(u64, Option<usize>)>>,
    mut events: MessageWriter<ViewerEvent>,
) {
    let Some((shown, _)) = state.get::<TreeSubject>() else {
        return;
    };
    let Ok(camera) = camera.single() else {
        return;
    };

    // Each step's own distance range, from the shown tree's bark. Their
    // ranges grow with the tree's size.
    let kids = children.get(shown.entity).into_iter().flatten();
    let ranges: Vec<(usize, &VisibilityRange)> = parts
        .iter_many(kids)
        .flatten()
        .filter(|(part, ..)| matches!(part.kind, TreePartKind::Bark | TreePartKind::Impostor))
        .filter_map(|(part, range, saved)| {
            Some((part.step.index(), saved.map(|s| &s.0).or(range)?))
        })
        .collect();
    if ranges.is_empty() {
        return;
    }

    // The tree stands at the origin.
    let step = level_in_range(camera.translation.length(), &ranges);
    let step = match look.detail {
        super::Detail::Level(n) => Some(n),
        detail => step.map(|step| detail.level_for(step)),
    };
    let now = (state.generation(), step);
    if *reported != Some(now) {
        *reported = Some(now);
        events.write(ViewerEvent::LodShown(step));
    }
}

/// Swaps in a finished replacement tree, and reports the shown tree's size
/// once it finishes generating.
fn report_tree(
    event: On<TreeFinished>,
    mut commands: Commands,
    mut state: ResMut<SceneState>,
    mut events: MessageWriter<ViewerEvent>,
    children: Query<&Children>,
    parts: Query<(&TreePart, &Mesh3d)>,
    meshes: Res<Assets<Mesh>>,
) {
    let Some(shown) = state.get_mut::<TreeSubject>() else {
        return;
    };
    // The new parts already exist, so removing the old tree in this same
    // frame never leaves the view empty. A rejected tree leaves the old one shown.
    if !shown.finish(&mut commands, event.entity, event.result.is_ok()) {
        return;
    }
    let result = event.result.clone().map(|()| BuildStats {
        levels: tree_sizes(event.entity, &children, &parts, &meshes),
        build_time: shown.requested.elapsed(),
    });
    events.write(ViewerEvent::TreeBuilt(result));
}

/// The size of the game-level and lower meshes of the tree `tree`.
pub(crate) fn tree_sizes(
    tree: Entity,
    children: &Query<&Children>,
    parts: &Query<(&TreePart, &Mesh3d)>,
    meshes: &Assets<Mesh>,
) -> Vec<MeshSize> {
    let mut levels = vec![MeshSize::default(); TREE_STEPS.len()];
    let kids = children.get(tree).into_iter().flatten();
    add_tree_parts(&mut levels, parts.iter_many(kids).flatten(), meshes);
    levels
}

/// Adds the size of each mesh part in `parts` to its step's entry in
/// `levels`. Trellis wires show at every step, so they count toward each.
/// The impostor is not counted.
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
        let index = part.step.index();
        let sizes = match part.kind {
            TreePartKind::Trellis => &mut levels[..],
            TreePartKind::Impostor => &mut [],
            _ => levels.get_mut(index..=index).unwrap_or_default(),
        };
        for size in sizes {
            size.vertices += vertices;
            size.triangles += triangles;
        }
    }
}

/// Requests more frames while trees are generating or their textures are
/// loading, alone or in groves, so an idle viewport still shows the
/// finished trees.
fn render_while_loading(
    generating: Query<(), With<TreeGenerating>>,
    parts: Query<&MeshMaterial3d<TreeMaterial>, With<TreePart>>,
    groves: Query<&TreeScatterAssets>,
    materials: Res<Assets<TreeMaterial>>,
    server: Res<AssetServer>,
    mut activity: ResMut<ViewportActivity>,
) {
    let loading = |texture: &Option<Handle<Image>>| super::is_loading(&server, texture);
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
    if !generating.is_empty() || textures_loading {
        activity.request_frames(ViewportActivity::SETTLE_FRAMES);
    }
}

#[cfg(test)]
mod tests {
    use bevy::camera::visibility::VisibilityRange;
    use bevy::ecs::system::RunSystemOnce;

    use super::super::subject::{Detail, every_distance};
    use super::*;

    fn range(start: f32, end: f32) -> VisibilityRange {
        VisibilityRange {
            start_margin: start..start,
            end_margin: end..end,
            use_aabb: false,
        }
    }

    /// Runs [`apply_lod`] with `detail`, and returns each part's visibility
    /// and distance range.
    fn apply(
        world: &mut World,
        detail: Detail,
    ) -> Vec<(TreePart, Visibility, Option<VisibilityRange>)> {
        world.insert_resource(SubjectLook {
            detail,
            ..default()
        });
        world.run_system_once(apply_lod).unwrap();
        let mut parts = world.query::<(&TreePart, &Visibility, Option<&VisibilityRange>)>();
        parts
            .iter(world)
            .map(|(part, visibility, range)| (*part, *visibility, range.cloned()))
            .collect()
    }

    /// Bark at both mesh steps and the impostor, with distance ranges, and
    /// an unranged trellis wire.
    fn spawn_tree(world: &mut World) -> [VisibilityRange; 3] {
        let ranges = [
            range(0.0, 100.0),
            range(100.0, 250.0),
            range(250.0, f32::INFINITY),
        ];
        for (step, range) in Step::ALL.into_iter().zip(&ranges) {
            let kind = match step {
                Step::Impostor => TreePartKind::Impostor,
                _ => TreePartKind::Bark,
            };
            world.spawn((
                TreePart { step, kind },
                Visibility::Inherited,
                range.clone(),
            ));
        }
        let trellis = TreePart {
            step: Step::Game,
            kind: TreePartKind::Trellis,
        };
        world.spawn((trellis, Visibility::Inherited));
        ranges
    }

    #[test]
    fn a_chosen_level_shows_alone_until_distance_switching_returns() {
        let mut world = World::new();
        let ranges = spawn_tree(&mut world);

        // Only level 2 and the trellis show, and level 2 shows at any distance.
        for (part, visibility, range) in apply(&mut world, Detail::Level(2)) {
            let shown = part.kind == TreePartKind::Trellis || part.step.index() == 2;
            assert_eq!(visibility == Visibility::Inherited, shown, "{part:?}");
            let expected = match part.kind {
                TreePartKind::Trellis => None,
                _ if part.step.index() == 2 => Some(every_distance()),
                _ => Some(ranges[part.step.index()].clone()),
            };
            assert!(range == expected, "{part:?} has the wrong range");
        }

        // Distance switching comes back with the original ranges.
        for (part, visibility, range) in apply(&mut world, Detail::default()) {
            assert_eq!(visibility, Visibility::Inherited, "{part:?}");
            let original =
                (part.kind != TreePartKind::Trellis).then(|| ranges[part.step.index()].clone());
            assert!(range == original, "{part:?} has the wrong range");
        }
    }

    #[test]
    fn the_finest_level_reaches_the_camera_in_place_of_finer_ones() {
        let mut world = World::new();
        let ranges = spawn_tree(&mut world);

        // Level 0 is hidden, and level 1 reaches in to the camera.
        for (part, visibility, range) in apply(&mut world, Detail::ByDistance { finest: 1 }) {
            if part.kind == TreePartKind::Trellis {
                assert_eq!(visibility, Visibility::Inherited);
                continue;
            }
            assert_eq!(
                visibility == Visibility::Inherited,
                part.step.index() >= 1,
                "{part:?}"
            );
            let expected = match part.step.index() {
                1 => range_from_camera(&ranges[1]),
                lod => ranges[lod].clone(),
            };
            assert!(range == Some(expected), "{part:?} has the wrong range");
        }

        // Every level comes back with its own range.
        for (part, visibility, range) in apply(&mut world, Detail::default()) {
            assert_eq!(visibility, Visibility::Inherited, "{part:?}");
            let original =
                (part.kind != TreePartKind::Trellis).then(|| ranges[part.step.index()].clone());
            assert!(range == original, "{part:?} has the wrong range");
        }
    }

    fn range_from_camera(range: &VisibilityRange) -> VisibilityRange {
        VisibilityRange {
            start_margin: 0.0..0.0,
            ..range.clone()
        }
    }

    /// Sizes count each mesh step's parts, trellis wires at both steps and
    /// the impostor at neither.
    #[test]
    fn sizes_count_each_mesh_step() {
        let mut meshes = Assets::<Mesh>::default();
        let mut quad = || Mesh3d(meshes.add(Mesh::from(Rectangle::default())));
        let parts = [
            (Step::Game, TreePartKind::Bark),
            (Step::Lower, TreePartKind::Leaves),
            (Step::Game, TreePartKind::Trellis),
            (Step::Impostor, TreePartKind::Impostor),
        ]
        .map(|(step, kind)| (TreePart { step, kind }, quad()));
        let mut levels = vec![MeshSize::default(); TREE_STEPS.len()];
        add_tree_parts(&mut levels, parts.iter().map(|(p, m)| (p, m)), &meshes);
        // Each step has its own quad and the trellis wire's.
        let two_quads = MeshSize {
            vertices: 8,
            triangles: 4,
        };
        assert_eq!(levels, [two_quads, two_quads]);
    }

    /// A taller tree turns into its impostor farther away, in proportion.
    #[test]
    fn the_impostor_distance_grows_with_the_tree() {
        let near = impostor_distance(10.0);
        assert!((impostor_distance(20.0) - 2.0 * near).abs() < 1e-3);
        assert!(near > 100.0, "{near}");
    }
}
