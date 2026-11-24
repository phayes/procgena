//! Grove subject: a tree Scatter on a square of flat ground.

use bevy::prelude::*;
use procgena_impostor::{BakedImpostor, ImpostorRequest};
use procgena_tree::TreeScatter;
use procgena_tree_bevy::{
    TreeScatterAssets, TreeScatterFinished, TreeScatterGenerating, TreeScatterParams,
    TreeScatterStats,
};

use super::environment::EnvironmentSettings;
use super::orbit::{OrbitCamera, OrbitLimits};
use super::protocol::{BuildStats, GroveProgress, GroveStats, MeshSize, ViewerEvent};
use super::subject::{Detail, SceneState, Subject};
use super::tree::{IMPOSTOR_DISTANCE, lod_levels};
use super::viewport::ViewportActivity;
use super::{AddSubject, Shown};

/// The farthest the grove camera can always back away, in metres: far enough
/// to see the trees drawn as impostors.
const GROVE_FARTHEST: f32 = 1.25 * IMPOSTOR_DISTANCE;

/// A tree Scatter on a square of flat ground
/// [`EnvironmentSettings::grove_size`] across.
///
/// A changed Scatter updates in place: trees that keep their spot, species
/// and shape stay, and the rest are added, removed or rebuilt.
/// [`ViewerEvent::GroveBuilt`] reports each build and
/// [`ViewerEvent::GroveProgress`] how far it has got. Each tree shows the
/// detail level that suits its distance, whatever detail is chosen.
pub struct GroveSubject;

impl Subject for GroveSubject {
    type Settings = TreeScatter;
    const NAME: &'static str = "tree Scatter";

    fn spawn(world: &mut World, settings: &TreeScatter) -> Entity {
        let size = world.resource::<EnvironmentSettings>().grove_size;
        world.spawn(grove_params(settings, size)).id()
    }

    fn update(world: &mut World, shown: &mut Shown, _: &TreeScatter, new: &TreeScatter) {
        let size = world.resource::<EnvironmentSettings>().grove_size;
        shown.start_build(world);
        world
            .entity_mut(shown.entity)
            .insert(grove_params(new, size));
    }

    fn view(world: &World, _: &TreeScatter) -> OrbitCamera {
        // Look down over the whole square of ground from beyond its corner.
        let size = world.resource::<EnvironmentSettings>().grove_size;
        OrbitCamera {
            focus: Vec3::new(0.0, 3.0, 0.0),
            yaw: 0.6,
            pitch: 0.45,
            distance: 1.4 * size.max(10.0),
            limits: OrbitLimits {
                // Far enough out to reach the impostors, and to see the
                // whole grove.
                distance: (2.0, (2.0 * size).max(GROVE_FARTHEST)),
                pitch: (-0.05, 1.5),
            },
        }
    }

    fn environment_changed(
        world: &mut World,
        shown: &mut Shown,
        settings: &TreeScatter,
        old: &EnvironmentSettings,
    ) {
        let size = world.resource::<EnvironmentSettings>().grove_size;
        if size != old.grove_size {
            shown.start_build(world);
            world
                .entity_mut(shown.entity)
                .insert(grove_params(settings, size));
        }
    }

    fn detail(_: Detail) -> Detail {
        Detail::default()
    }
}

/// The grove component for `grove` on a square of flat ground `size` metres
/// across, centred on the origin.
fn grove_params(grove: &TreeScatter, size: f32) -> TreeScatterParams {
    TreeScatterParams::new(grove.clone(), Vec2::splat(size))
}

/// Adds the grove subject.
pub(crate) fn plugin(app: &mut App) {
    app.add_subject::<GroveSubject>()
        .add_systems(Update, render_while_generating)
        // After Update, once the tree plugin has asked for this frame's bakes.
        .add_systems(PostUpdate, report_grove_progress)
        .add_observer(report_grove);
}

/// Matches requests for impostors that are not baked yet.
type PendingImpostor = (With<ImpostorRequest>, Without<BakedImpostor>);

/// Requests more frames while groves are generating.
fn render_while_generating(
    generating: Query<(), With<TreeScatterGenerating>>,
    mut activity: ResMut<ViewportActivity>,
) {
    if !generating.is_empty() {
        activity.request_frames(ViewportActivity::SETTLE_FRAMES);
    }
}

/// Reports how far the shown grove has got whenever that changes: its tree
/// shapes generating, then their impostors baking.
fn report_grove_progress(
    state: Res<SceneState>,
    groves: Query<(Option<&TreeScatterAssets>, Has<TreeScatterGenerating>)>,
    baking: Query<(), PendingImpostor>,
    mut reported: Local<Option<(u64, GroveProgress)>>,
    mut events: MessageWriter<ViewerEvent>,
) {
    let Some((shown, _)) = state.get::<GroveSubject>() else {
        return;
    };
    let Ok((assets, generating)) = groves.get(shown.entity) else {
        return;
    };
    let variants = assets.map(TreeScatterAssets::variants).unwrap_or_default();
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
    let now = (state.generation(), progress);
    if *reported != Some(now) {
        *reported = Some(now);
        events.write(ViewerEvent::GroveProgress(progress));
    }
}

/// Reports the shown grove's tree count and size once a build ends, or why
/// it could not be built.
fn report_grove(
    event: On<TreeScatterFinished>,
    state: Res<SceneState>,
    mut events: MessageWriter<ViewerEvent>,
    groves: Query<&TreeScatterStats>,
) {
    let Some((shown, _)) = state.get::<GroveSubject>() else {
        return;
    };
    if event.entity != shown.entity {
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
                build_time: shown.requested.elapsed(),
            },
        }
    });
    events.write(ViewerEvent::GroveBuilt(result));
}
