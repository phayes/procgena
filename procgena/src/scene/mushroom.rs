//! Mushroom subject: a single generated mushroom or cluster, with its
//! steps of the drawing chain (game level, lower mesh and impostor) as the
//! detail levels to choose from.

use std::sync::Arc;

use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use procgena_mushroom::{Mushroom, MushroomOptions};
use procgena_mushroom_bevy::{
    MushroomFinished, MushroomGenerating, MushroomMaterial, MushroomParams, MushroomPart,
};

use super::orbit::{OrbitCamera, OrbitLimits};
use super::protocol::{BuildStats, MeshSize, ViewerEvent};
use super::subject::{
    LevelPart, SavedRange, SceneState, Subject, SubjectLook, level_in_range, show_level,
};
use super::viewport::{ViewportActivity, ViewportCamera};
use super::{AddSubject, Shown};

/// The camera's nearest allowed distance, kept clear of the camera's 0.1 m
/// near clipping plane.
const MUSHROOM_NEAREST: f32 = 0.15;

/// A single mushroom, built from its [`MushroomOptions`].
///
/// A changed mushroom is rebuilt in the background and replaces the old one
/// once ready; the old one stays if the new settings are rejected.
/// [`ViewerEvent::MushroomBuilt`] reports each build, and
/// [`ViewerEvent::MushroomLodShown`] the step of the drawing chain on
/// screen, by [`Step::index`](procgena_core::chain::Step::index).
pub struct MushroomSubject;

impl Subject for MushroomSubject {
    type Settings = MushroomOptions;
    const NAME: &'static str = "mushroom";

    fn spawn(world: &mut World, settings: &MushroomOptions) -> Entity {
        world
            .spawn(MushroomParams::Options(Arc::new(settings.clone())))
            .id()
    }

    fn update(world: &mut World, shown: &mut Shown, _: &MushroomOptions, new: &MushroomOptions) {
        let target = shown.replacement::<MushroomGenerating>(world);
        world
            .entity_mut(target)
            .insert(MushroomParams::Options(Arc::new(new.clone())));
    }

    fn view(_: &World, settings: &MushroomOptions) -> OrbitCamera {
        mushroom_view(settings)
    }
}

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

/// Adds the mushroom subject.
pub(crate) fn plugin(app: &mut App) {
    app.add_subject::<MushroomSubject>()
        .add_systems(Update, (render_while_loading, report_lod))
        .add_observer(report_mushroom);
}

/// Shows each mushroom part as the shown subject's detail asks.
pub(crate) fn apply_lod(
    mut commands: Commands,
    look: Res<SubjectLook>,
    mut parts: Query<LevelPart<MushroomPart>>,
) {
    for (entity, part, visibility, range, saved) in &mut parts {
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

/// Reports the mushroom detail level on screen whenever it changes, and
/// when a mushroom is first shown.
fn report_lod(
    state: Res<SceneState>,
    look: Res<SubjectLook>,
    camera: Query<&Transform, With<ViewportCamera>>,
    children: Query<&Children>,
    parts: Query<(&MushroomPart, Option<&VisibilityRange>, Option<&SavedRange>)>,
    mut reported: Local<Option<(u64, Option<usize>)>>,
    mut events: MessageWriter<ViewerEvent>,
) {
    let Some((shown, _)) = state.get::<MushroomSubject>() else {
        return;
    };
    let Ok(camera) = camera.single() else {
        return;
    };

    // Each step's own distance range, from the shown mushroom's parts.
    // Their ranges grow with the mushroom's size on screen.
    let kids = children.get(shown.entity).into_iter().flatten();
    let ranges: Vec<(usize, &VisibilityRange)> = parts
        .iter_many(kids)
        .flatten()
        .filter_map(|(part, range, saved)| {
            Some((part.step.index(), saved.map(|s| &s.0).or(range)?))
        })
        .collect();
    if ranges.is_empty() {
        return;
    }

    // The mushroom stands at the origin.
    let level = level_in_range(camera.translation.length(), &ranges);
    let level = match look.detail {
        super::Detail::Level(n) => Some(n),
        detail => level.map(|level| detail.level_for(level)),
    };
    let now = (state.generation(), level);
    if *reported != Some(now) {
        *reported = Some(now);
        events.write(ViewerEvent::MushroomLodShown(level));
    }
}

/// Displays a successfully built mushroom in place of the previous one, and
/// reports its mesh counts per detail level and build time. Reports rejected
/// settings without replacing the old mushroom.
fn report_mushroom(
    event: On<MushroomFinished>,
    mut commands: Commands,
    mut state: ResMut<SceneState>,
    mut events: MessageWriter<ViewerEvent>,
    children: Query<&Children>,
    parts: Query<(&MushroomPart, &Mesh3d)>,
    meshes: Res<Assets<Mesh>>,
) {
    // Remove the old mushroom only after its replacement has built successfully.
    let Some(shown) = state.get_mut::<MushroomSubject>() else {
        return;
    };
    if !shown.finish(&mut commands, event.entity, event.result.is_ok()) {
        return;
    }

    // Add up the game-level and lower meshes of every member of a cluster.
    let result = event.result.clone().map(|()| {
        let mut levels = vec![MeshSize::default(); 2];
        let kids = children.get(event.entity).into_iter().flatten();
        for (part, mesh) in parts.iter_many(kids).flatten() {
            let size = levels.get_mut(part.step.index());
            let (Some(mesh), Some(size)) = (meshes.get(&mesh.0), size) else {
                continue;
            };
            size.vertices += mesh.count_vertices();
            size.triangles += mesh.indices().map_or(0, |i| i.len() / 3);
        }
        BuildStats {
            levels,
            build_time: shown.requested.elapsed(),
        }
    });
    events.write(ViewerEvent::MushroomBuilt(result));
}

/// Requests more frames while mushrooms are generating or their textures
/// are loading, so an idle viewport still shows the finished mushroom.
fn render_while_loading(
    generating: Query<(), With<MushroomGenerating>>,
    parts: Query<&MeshMaterial3d<MushroomMaterial>, With<MushroomPart>>,
    materials: Res<Assets<MushroomMaterial>>,
    server: Res<AssetServer>,
    mut activity: ResMut<ViewportActivity>,
) {
    let loading = |texture: &Option<Handle<Image>>| super::is_loading(&server, texture);
    let textures_loading = parts
        .iter()
        .filter_map(|part| materials.get(&part.0))
        .any(|material| {
            let cap = &material.extension;
            loading(&cap.cap_color) || loading(&cap.cap_normal) || loading(&cap.cap_orh)
        });
    if !generating.is_empty() || textures_loading {
        activity.request_frames(ViewportActivity::SETTLE_FRAMES);
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;
    use procgena_core::chain::Step;

    use super::super::subject::{Detail, every_distance};
    use super::*;

    fn range(start: f32, end: f32) -> VisibilityRange {
        VisibilityRange {
            start_margin: start..start,
            end_margin: end..end,
            use_aabb: false,
        }
    }

    /// Runs [`apply_lod`] with `detail`, and returns each part's level,
    /// visibility and distance range.
    fn apply(
        world: &mut World,
        detail: Detail,
    ) -> Vec<(usize, Visibility, Option<VisibilityRange>)> {
        world.insert_resource(SubjectLook {
            detail,
            ..default()
        });
        world.run_system_once(apply_lod).unwrap();
        let mut parts = world.query::<(&MushroomPart, &Visibility, Option<&VisibilityRange>)>();
        parts
            .iter(world)
            .map(|(part, visibility, range)| (part.step.index(), *visibility, range.cloned()))
            .collect()
    }

    #[test]
    fn a_chosen_mushroom_level_shows_alone_even_on_a_new_mushroom() {
        // A two-member cluster's game-level and lower meshes, and the
        // impostor of the whole cluster.
        let mut world = World::new();
        let ranges = [range(0.0, 1.5), range(1.5, 10.0), range(10.0, 25.0)];
        let spawn_parts = |world: &mut World| {
            for (instance, step) in [
                (0, Step::Game),
                (0, Step::Lower),
                (1, Step::Game),
                (1, Step::Lower),
                (0, Step::Impostor),
            ] {
                let part = MushroomPart { instance, step };
                let range = ranges[step.index()].clone();
                world.spawn((part, Visibility::Inherited, range));
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
        check(apply(&mut world, Detail::Level(1)));
        spawn_parts(&mut world);
        let parts = apply(&mut world, Detail::Level(1));
        assert_eq!(parts.len(), 10);
        check(parts);

        // Choosing another level gives level 1 its own range back.
        for (lod, _, range) in apply(&mut world, Detail::Level(0)) {
            let expected = if lod == 0 {
                every_distance()
            } else {
                ranges[lod].clone()
            };
            assert!(range == Some(expected), "level {lod} has the wrong range");
        }

        // Distance switching comes back with the original ranges.
        for (lod, visibility, range) in apply(&mut world, Detail::default()) {
            assert_eq!(visibility, Visibility::Inherited, "level {lod}");
            assert!(
                range == Some(ranges[lod].clone()),
                "level {lod} has the wrong range"
            );
        }
    }
}
