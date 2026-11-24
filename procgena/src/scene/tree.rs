//! Tree subject: a single generated tree, with its detail levels.

use std::sync::Arc;

use bevy::prelude::*;
use procgena_core::LodLevel;
use procgena_tree::TreeOptions;
use procgena_tree_bevy::{
    TreeFinished, TreeGenerating, TreeLevel, TreeMaterial, TreeParams, TreePart, TreePartKind,
    TreeScatterAssets, default_tree_lod,
};

use super::orbit::{OrbitCamera, OrbitLimits};
use super::protocol::{BuildStats, MeshSize, ViewerEvent};
use super::subject::{LevelPart, SceneState, Subject, SubjectLook, show_level};
use super::viewport::{ViewportActivity, ViewportCamera};
use super::{AddSubject, Shown};

/// Camera distances at which each tree detail level starts, nearest first.
const LOD_DISTANCES: [f32; 4] = [0.0, 175.0, 275.0, 500.0];

/// Camera distance in metres beyond which trees, alone or in a grove, are
/// drawn as impostors: flat pictures baked from the tree. It is beyond the
/// farthest mesh level, which starts at 500 m.
pub(crate) const IMPOSTOR_DISTANCE: f32 = 800.0;

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

/// A single tree, built from its [`TreeOptions`].
///
/// A changed tree is rebuilt in the background and replaces the old one
/// once ready; [`ViewerEvent::TreeBuilt`] reports each build. While it is
/// shown, [`ViewerEvent::LodShown`] reports the detail level on screen.
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
            // Far enough out to reach the lowest detail level.
            distance: (0.5, 60.0),
            pitch: (-0.15, 1.3),
        },
    }
}

/// Adds the tree subject and the detail level handling of every tree part.
pub(crate) fn plugin(app: &mut App) {
    app.add_subject::<TreeSubject>()
        .add_systems(Update, (render_while_loading, report_lod))
        .add_observer(report_tree);
}

/// Shows each tree part as the shown subject's detail asks. Trellis wires
/// show at every level.
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
            part.lod,
            look.detail,
            visibility,
            ranges,
        );
    }
}

/// The detail level that the camera distance `distance` selects.
fn lod_at(distance: f32) -> usize {
    lod_levels()
        .iter()
        .rposition(|level| distance >= level.distance)
        .unwrap_or(0)
}

/// Reports the tree detail level on screen whenever it changes, and when a
/// tree is first shown.
fn report_lod(
    state: Res<SceneState>,
    look: Res<SubjectLook>,
    camera: Query<&Transform, With<ViewportCamera>>,
    mut reported: Local<Option<(u64, usize)>>,
    mut events: MessageWriter<ViewerEvent>,
) {
    if !state.shows::<TreeSubject>() {
        return;
    }
    let Ok(camera) = camera.single() else {
        return;
    };
    // The tree stands at the origin.
    let shown = look.detail.level_for(lod_at(camera.translation.length()));
    let now = (state.generation(), shown);
    if *reported != Some(now) {
        *reported = Some(now);
        events.write(ViewerEvent::LodShown(shown));
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

/// The size of each detail level of the tree `tree`.
pub(crate) fn tree_sizes(
    tree: Entity,
    children: &Query<&Children>,
    parts: &Query<(&TreePart, &Mesh3d)>,
    meshes: &Assets<Mesh>,
) -> Vec<MeshSize> {
    let mut levels = vec![MeshSize::default(); lod_levels().len()];
    let kids = children.get(tree).into_iter().flatten();
    add_tree_parts(&mut levels, parts.iter_many(kids).flatten(), meshes);
    levels
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

    /// Three bark levels with distance ranges, and an unranged trellis wire.
    fn spawn_tree(world: &mut World) -> [VisibilityRange; 3] {
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
        ranges
    }

    #[test]
    fn a_chosen_level_shows_alone_until_distance_switching_returns() {
        let mut world = World::new();
        let ranges = spawn_tree(&mut world);

        // Only level 2 and the trellis show, and level 2 shows at any distance.
        for (part, visibility, range) in apply(&mut world, Detail::Level(2)) {
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
        for (part, visibility, range) in apply(&mut world, Detail::default()) {
            assert_eq!(visibility, Visibility::Inherited, "{part:?}");
            let original = (part.kind != TreePartKind::Trellis).then(|| ranges[part.lod].clone());
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
                part.lod >= 1,
                "{part:?}"
            );
            let expected = match part.lod {
                1 => range_from_camera(&ranges[1]),
                lod => ranges[lod].clone(),
            };
            assert!(range == Some(expected), "{part:?} has the wrong range");
        }

        // Every level comes back with its own range.
        for (part, visibility, range) in apply(&mut world, Detail::default()) {
            assert_eq!(visibility, Visibility::Inherited, "{part:?}");
            let original = (part.kind != TreePartKind::Trellis).then(|| ranges[part.lod].clone());
            assert!(range == original, "{part:?} has the wrong range");
        }
    }

    fn range_from_camera(range: &VisibilityRange) -> VisibilityRange {
        VisibilityRange {
            start_margin: 0.0..0.0,
            ..range.clone()
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
        assert_eq!(default_tree_lod().levels.len(), LOD_DISTANCES.len());
    }
}
