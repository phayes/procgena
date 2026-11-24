//! Clump mode: growing a clump on the shown tree or rock. Also grows dead
//! wood mode's cover, such as moss, on its piece.

use bevy::prelude::*;
use procgena_clump_bevy::{ClumpFinished, ClumpGenerating, ClumpMaterial, ClumpParams, ClumpPart};

use super::protocol::{
    BuildStats, ClumpBuildError, DeadwoodView, MeshSize, SceneMode, ViewerEvent,
};
use super::subject::SceneState;
use super::viewport::ViewportActivity;

/// The clump mode's clump: a [`ClumpParams`] child of the shown subject, or
/// `None` outside clump mode.
#[derive(Resource, Default)]
pub(crate) struct ShownClump(Option<Entity>);

/// Keeps one clump on the clump mode's subject, with the current settings,
/// and removes it in other modes.
///
/// A new subject gets a new clump, which grows once the subject is built.
/// Changed settings replace the clump's settings: the previous clump stays
/// shown until the new one is ready, and a new colour alone only recolours
/// it.
pub(crate) fn show_clump(
    mut shown: ResMut<ShownClump>,
    state: Res<SceneState>,
    mut commands: Commands,
    clumps: Query<(&ClumpParams, &ChildOf, Has<ClumpGenerating>)>,
    mut activity: ResMut<ViewportActivity>,
) {
    if state.mode() != SceneMode::Clump {
        if let Some(clump) = shown.0.take()
            && let Ok(mut clump) = commands.get_entity(clump)
        {
            clump.despawn();
        }
        return;
    }

    // Grow on the current subject. A replaced subject took its clump with it.
    let subject = state.subject();
    let wanted = &state.clump().clump;
    let current = shown
        .0
        .and_then(|clump| Some((clump, clumps.get(clump).ok()?)));
    match current {
        Some((clump, (params, parent, generating))) if parent.parent() == subject => {
            if *params.0 != *wanted {
                commands
                    .entity(clump)
                    .insert(ClumpParams::new(wanted.clone()));
                activity.request_frames(ViewportActivity::SETTLE_FRAMES);
            }
            if generating {
                activity.request_frames(ViewportActivity::SETTLE_FRAMES);
            }
        }
        stale => {
            if let Some((clump, _)) = stale {
                commands.entity(clump).despawn();
            }
            let clump = commands
                .spawn((ClumpParams::new(wanted.clone()), ChildOf(subject)))
                .id();
            shown.0 = Some(clump);
            activity.request_frames(ViewportActivity::SETTLE_FRAMES);
        }
    }
}

/// The cover, such as moss, grown on dead wood mode's piece: a
/// [`ClumpParams`] child of the piece, or `None` when there is none.
#[derive(Resource, Default)]
pub(crate) struct ShownCover(Option<Entity>);

/// Keeps the chosen cover growing on dead wood mode's piece, and removes it
/// in other modes, for scatters, or when no cover is chosen.
///
/// A replaced piece gets a new cover, which grows once the piece is built.
/// A changed cover replaces the old one's settings.
pub(crate) fn show_deadwood_cover(
    mut shown: ResMut<ShownCover>,
    state: Res<SceneState>,
    mut commands: Commands,
    covers: Query<(&ClumpParams, &ChildOf, Has<ClumpGenerating>)>,
    mut activity: ResMut<ViewportActivity>,
) {
    let settings = state.deadwood();
    let wanted = match settings.cover {
        Some(preset)
            if state.mode() == SceneMode::Deadwood && settings.view == DeadwoodView::Piece =>
        {
            preset.clump.clone()
        }
        _ => {
            if let Some(cover) = shown.0.take()
                && let Ok(mut cover) = commands.get_entity(cover)
            {
                cover.despawn();
            }
            return;
        }
    };

    // Grow on the current piece. A replaced piece took its cover with it.
    let subject = state.subject();
    let current = shown
        .0
        .and_then(|cover| Some((cover, covers.get(cover).ok()?)));
    match current {
        Some((cover, (params, parent, generating))) if parent.parent() == subject => {
            if *params.0 != wanted {
                commands.entity(cover).insert(ClumpParams::new(wanted));
                activity.request_frames(ViewportActivity::SETTLE_FRAMES);
            }
            if generating {
                activity.request_frames(ViewportActivity::SETTLE_FRAMES);
            }
        }
        stale => {
            if let Some((cover, _)) = stale
                && let Ok(mut cover) = commands.get_entity(cover)
            {
                cover.despawn();
            }
            let cover = commands
                .spawn((ClumpParams::new(wanted), ChildOf(subject)))
                .id();
            shown.0 = Some(cover);
            activity.request_frames(ViewportActivity::SETTLE_FRAMES);
        }
    }
}

/// Requests more frames while the clump's textures are loading, so an idle
/// viewport still shows the textured clump.
pub(crate) fn render_while_loading(
    parts: Query<&MeshMaterial3d<ClumpMaterial>, With<ClumpPart>>,
    materials: Res<Assets<ClumpMaterial>>,
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
            let textures = &material.extension;
            loading(&textures.color) || loading(&textures.normal) || loading(&textures.orh)
        });
    if textures_loading {
        activity.request_frames(ViewportActivity::SETTLE_FRAMES);
    }
}

/// Reports each finished build of the shown clump with
/// [`ViewerEvent::ClumpBuilt`]: its mesh size, as a single detail level, and
/// build time, or why it couldn't be built.
pub(crate) fn report_clump(
    finished: On<ClumpFinished>,
    shown: Res<ShownClump>,
    mut events: MessageWriter<ViewerEvent>,
    mut activity: ResMut<ViewportActivity>,
) {
    if shown.0 != Some(finished.entity) {
        return;
    }
    activity.request_frames(ViewportActivity::SETTLE_FRAMES);
    let result = finished
        .result
        .clone()
        .map(|built| BuildStats {
            levels: vec![MeshSize {
                vertices: built.vertices,
                triangles: built.triangles,
            }],
            build_time: built.build_time,
        })
        .map_err(ClumpBuildError::Clump);
    events.write(ViewerEvent::ClumpBuilt(result));
}
