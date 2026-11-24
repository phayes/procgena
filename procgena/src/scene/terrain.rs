//! Terrain mode: a world of terrain streamed around the orbit camera, every
//! region built from the same settings.

use std::sync::Arc;

use bevy::math::DVec2;
use bevy::prelude::*;
use procgena_terrain::{
    Elevation, RegionId, TerrainOptions, WorldError, WorldTerrain, WorldTerrainOptions, chunk_lod,
};
use procgena_terrain_bevy::{TerrainDebug, TerrainPlugin, TerrainProgress, TerrainStream};

use super::environment::Ground;
use super::orbit::{OrbitCamera, OrbitLimits};
use super::protocol::{SceneMode, ViewerEvent};
use super::subject::SceneState;
use super::viewport::ViewportActivity;

/// How far terrain is drawn, in metres.
pub(crate) const VIEW_DISTANCE: f32 = 3000.0;

/// Chunk levels from this one up are drawn at the rough tier only, without
/// roads, so far ground appears before the roads are routed.
const ROUGH_FROM: u8 = 3;

/// What terrain mode shows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TerrainSettings {
    /// The settings of every region. Their
    /// [`default_seed`](TerrainOptions::default_seed) is the world seed.
    pub terrain: TerrainOptions,
    /// Debug drawing.
    pub show: TerrainShow,
}

/// Debug drawing over the terrain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TerrainShow {
    /// Tint each chunk by its detail level.
    pub levels: bool,
    /// Draw a line along chunk borders.
    pub borders: bool,
}

/// The terrain plugin, drawing out to [`VIEW_DISTANCE`].
pub(crate) fn plugin() -> TerrainPlugin {
    TerrainPlugin::new(chunk_lod(VIEW_DISTANCE)).expect("the view distance is positive")
}

/// The ground for `settings`: an endless world of regions, each with the
/// same terrain settings, so rivers and roads run between them.
///
/// # Errors
///
/// If the world's own settings are refused; the terrain settings are
/// checked when the first region is built.
pub(crate) fn world(settings: &TerrainSettings) -> Result<WorldTerrain, WorldError> {
    let options = settings.terrain.clone();
    let world = WorldTerrainOptions {
        seed: options.default_seed,
        ..WorldTerrainOptions::default()
    };
    let source = move |_: RegionId, _: DVec2, _: &Elevation| options.clone();
    WorldTerrain::new(world, Arc::new(source))
}

/// Spawns the terrain for `settings`, or an empty entity if the world's
/// settings are refused.
pub(crate) fn spawn(commands: &mut Commands, settings: &TerrainSettings) -> Entity {
    match world(settings) {
        Ok(world) => commands.spawn(stream(world)).id(),
        Err(error) => {
            error!("terrain refused: {error}");
            commands.spawn_empty().id()
        }
    }
}

/// Replaces the ground `entity` draws with `settings`. Its chunks stay until
/// the new ones are ready.
pub(crate) fn replace(commands: &mut Commands, entity: Entity, settings: &TerrainSettings) {
    match world(settings) {
        Ok(world) => {
            commands.entity(entity).insert(stream(world));
        }
        Err(error) => error!("terrain refused: {error}"),
    }
}

/// The stream drawing `world`.
fn stream(world: WorldTerrain) -> TerrainStream {
    TerrainStream {
        rough_from: Some(ROUGH_FROM),
        ..TerrainStream::world(world)
    }
}

/// The starting camera view over `settings`' ground: from above, looking
/// across a few kilometres.
pub(crate) fn view(settings: &TerrainSettings) -> OrbitCamera {
    let height = world(settings)
        .ok()
        .map(|world| world.elevation().height(DVec2::ZERO) + settings.terrain.base.elevation)
        .filter(|h| h.is_finite())
        .unwrap_or(0.0);
    OrbitCamera {
        focus: Vec3::new(0.0, height, 0.0),
        yaw: 0.6,
        pitch: 0.35,
        distance: 900.0,
        limits: OrbitLimits {
            distance: (20.0, 0.9 * VIEW_DISTANCE),
            pitch: (0.02, 1.5),
        },
    }
}

/// Sets the terrain's debug drawing from what terrain mode shows.
pub(crate) fn apply_show(state: Res<SceneState>, mut debug: ResMut<TerrainDebug>) {
    let show = state.terrain().show;
    let wanted = TerrainDebug {
        levels: show.levels,
        borders: show.borders,
    };
    if *debug != wanted {
        *debug = wanted;
    }
}

/// Hides the ground plane in terrain mode, which has ground of its own, and
/// shows it again as the settings say on leaving.
pub(crate) fn hide_ground(
    state: Res<SceneState>,
    settings: Res<super::EnvironmentSettings>,
    mut was_terrain: Local<bool>,
    mut ground: Query<&mut Visibility, With<Ground>>,
) {
    let terrain = state.mode() == SceneMode::Terrain;
    if std::mem::replace(&mut *was_terrain, terrain) == terrain && !settings.is_changed() {
        return;
    }
    for mut visibility in &mut ground {
        *visibility = if settings.show_ground && !terrain {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

/// Reports how far the terrain has got whenever that changes, and asks for
/// frames until every chunk is drawn.
pub(crate) fn report_progress(
    state: Res<SceneState>,
    streams: Query<&TerrainProgress>,
    mut reported: Local<Option<TerrainProgress>>,
    mut events: MessageWriter<ViewerEvent>,
    mut activity: ResMut<ViewportActivity>,
) {
    if state.mode() != SceneMode::Terrain {
        *reported = None;
        return;
    }
    let Ok(progress) = streams.get(state.subject()) else {
        return;
    };
    if !progress.settled {
        activity.request_frames(ViewportActivity::SETTLE_FRAMES);
    }
    if *reported != Some(*progress) {
        *reported = Some(*progress);
        events.write(ViewerEvent::TerrainProgress(*progress));
    }
}
