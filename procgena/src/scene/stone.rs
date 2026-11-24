//! Stone subject: a single generated rock.

use std::sync::Arc;

use bevy::prelude::*;
use procgena_stone::StoneOptions;
use procgena_stone_bevy::{
    StoneEmbedDepth, StoneFinished, StoneGenerating, StoneMaterial, StoneParams, StonePart,
};

use super::orbit::{OrbitCamera, OrbitLimits};
use super::protocol::{BuildStats, MeshSize, ViewerEvent};
use super::subject::{SceneState, Subject};
use super::viewport::ViewportActivity;
use super::{AddSubject, Shown};

/// A single rock, built from its [`StoneOptions`].
///
/// A changed rock is rebuilt in the background and replaces the old one
/// once ready; the old rock stays if the new settings are rejected.
/// [`ViewerEvent::StoneBuilt`] reports each build. `options.mesh.detail`
/// sets the rock's detail levels: each level's mesh resolution, switching
/// distance and texture size.
pub struct StoneSubject;

impl Subject for StoneSubject {
    type Settings = StoneOptions;
    const NAME: &'static str = "rock";

    fn spawn(world: &mut World, settings: &StoneOptions) -> Entity {
        world
            .spawn(StoneParams::Options(Arc::new(settings.clone())))
            .id()
    }

    fn update(world: &mut World, shown: &mut Shown, _: &StoneOptions, new: &StoneOptions) {
        let target = shown.replacement::<StoneGenerating>(world);
        world
            .entity_mut(target)
            .insert(StoneParams::Options(Arc::new(new.clone())));
    }

    fn view(_: &World, settings: &StoneOptions) -> OrbitCamera {
        stone_view(settings)
    }
}

/// The starting view of a rock: aimed above the ground, with the viewing
/// distance scaled with the rock's size.
pub(crate) fn stone_view(options: &StoneOptions) -> OrbitCamera {
    OrbitCamera {
        focus: Vec3::new(0.0, 0.3 * options.scale, 0.0),
        yaw: 0.6,
        pitch: 0.3,
        distance: 2.2 * options.scale,
        limits: OrbitLimits {
            distance: (0.1, 150.0),
            pitch: (-0.05, 1.5),
        },
    }
}

/// Adds the stone subject.
pub(crate) fn plugin(app: &mut App) {
    app.add_subject::<StoneSubject>()
        .add_systems(Update, render_while_loading)
        .add_observer(report_stone);
}

/// Displays a successfully built rock sunk into the ground by its suggested
/// embed depth, and reports its mesh counts and build time. Reports rejected
/// settings without replacing the old rock.
fn report_stone(
    event: On<StoneFinished>,
    mut commands: Commands,
    mut state: ResMut<SceneState>,
    mut events: MessageWriter<ViewerEvent>,
    rocks: Query<(&StoneParams, Option<&Children>, Option<&StoneEmbedDepth>)>,
    parts: Query<(&StonePart, &Mesh3d)>,
    meshes: Res<Assets<Mesh>>,
) {
    // Remove the old rock only after its replacement has built successfully.
    let Some(shown) = state.get_mut::<StoneSubject>() else {
        return;
    };
    if !shown.finish(&mut commands, event.entity, event.result.is_ok()) {
        return;
    }
    let requested = shown.requested;
    let result = event.result.clone().map(|()| {
        let levels = settle_rock(&mut commands, event.entity, &rocks, &parts, &meshes);
        BuildStats {
            levels,
            build_time: requested.elapsed(),
        }
    });
    events.write(ViewerEvent::StoneBuilt(result));
}

/// Sinks the built rock `rock` into the ground by its embed depth, and
/// returns the size of each of its detail levels. Levels without a mesh
/// count zero.
pub(crate) fn settle_rock(
    commands: &mut Commands,
    rock: Entity,
    rocks: &Query<(&StoneParams, Option<&Children>, Option<&StoneEmbedDepth>)>,
    parts: &Query<(&StonePart, &Mesh3d)>,
    meshes: &Assets<Mesh>,
) -> Vec<MeshSize> {
    let Ok((stone, kids, embed)) = rocks.get(rock) else {
        return Vec::new();
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
        .entity(rock)
        .insert(Transform::from_xyz(0.0, -embed, 0.0));
    levels
}

/// Requests more frames while rocks are generating or their textures are
/// loading, so an idle viewport still shows the finished rock.
fn render_while_loading(
    generating: Query<(), With<StoneGenerating>>,
    parts: Query<&MeshMaterial3d<StoneMaterial>, With<StonePart>>,
    materials: Res<Assets<StoneMaterial>>,
    server: Res<AssetServer>,
    mut activity: ResMut<ViewportActivity>,
) {
    let loading = |texture: &Option<Handle<Image>>| super::is_loading(&server, texture);
    let textures_loading = parts
        .iter()
        .filter_map(|part| materials.get(&part.0))
        .any(|material| {
            let textures = &material.extension;
            loading(&textures.color) || loading(&textures.normal) || loading(&textures.orh)
        });
    if !generating.is_empty() || textures_loading {
        activity.request_frames(ViewportActivity::SETTLE_FRAMES);
    }
}
