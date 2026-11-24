//! Terrain subject: a world of terrain streamed around the orbit camera,
//! every region built from the same settings.

use std::sync::Arc;

use bevy::math::DVec2;
use bevy::prelude::*;
use procgena_terrain::{
    Elevation, RegionId, TerrainOptions, WorldError, WorldTerrain, WorldTerrainOptions, chunk_lod,
};
use procgena_terrain_bevy::{TerrainDebug, TerrainPlugin, TerrainProgress, TerrainStream};

use super::orbit::{OrbitCamera, OrbitLimits};
use super::protocol::ViewerEvent;
use super::subject::{SceneState, Subject};
use super::viewport::ViewportActivity;
use super::{AddSubject, Shown};

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

/// An endless world of terrain, streamed around the camera, from
/// [`TerrainSettings`].
///
/// Changed terrain settings build a new world, whose chunks replace the old
/// ones once ready; changed debug drawing alone applies at once.
/// [`ViewerEvent::TerrainProgress`] reports how far it has got. The terrain
/// is its own ground.
pub struct TerrainSubject;

impl Subject for TerrainSubject {
    type Settings = TerrainSettings;
    const NAME: &'static str = "terrain";
    const OWN_GROUND: bool = true;

    /// Spawns the terrain for `settings`, or an empty entity if the world's
    /// settings are refused.
    fn spawn(world: &mut World, settings: &TerrainSettings) -> Entity {
        match self::world(settings) {
            Ok(terrain) => world.spawn(stream(terrain)).id(),
            Err(error) => {
                error!("terrain refused: {error}");
                world.spawn_empty().id()
            }
        }
    }

    /// Replaces the ground with `new`'s. The old chunks stay until the new
    /// ones are ready.
    fn update(world: &mut World, shown: &mut Shown, old: &TerrainSettings, new: &TerrainSettings) {
        if old.terrain == new.terrain {
            return;
        }
        shown.start_build(world);
        match self::world(new) {
            Ok(terrain) => {
                world.entity_mut(shown.entity).insert(stream(terrain));
            }
            Err(error) => error!("terrain refused: {error}"),
        }
    }

    fn view(_: &World, settings: &TerrainSettings) -> OrbitCamera {
        view(settings)
    }
}

/// Adds the terrain subject, its debug drawing and its progress reports.
pub(crate) fn add_subject(app: &mut App) {
    app.add_subject::<TerrainSubject>()
        .add_systems(Update, (apply_show, report_progress));
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

/// Sets the terrain's debug drawing from the shown terrain's settings, and
/// turns it off when no terrain is shown.
fn apply_show(state: Res<SceneState>, mut debug: ResMut<TerrainDebug>) {
    let show = state
        .get::<TerrainSubject>()
        .map(|(_, settings)| settings.show)
        .unwrap_or_default();
    let wanted = TerrainDebug {
        levels: show.levels,
        borders: show.borders,
    };
    if *debug != wanted {
        *debug = wanted;
    }
}

/// Reports how far the shown terrain has got whenever that changes, and
/// when terrain is first shown, and asks for frames until every chunk is
/// drawn.
fn report_progress(
    state: Res<SceneState>,
    streams: Query<&TerrainProgress>,
    mut reported: Local<Option<(u64, TerrainProgress)>>,
    mut events: MessageWriter<ViewerEvent>,
    mut activity: ResMut<ViewportActivity>,
) {
    let Some((shown, _)) = state.get::<TerrainSubject>() else {
        return;
    };
    let Ok(progress) = streams.get(shown.entity) else {
        return;
    };
    if !progress.settled {
        activity.request_frames(ViewportActivity::SETTLE_FRAMES);
    }
    let now = (state.generation(), *progress);
    if *reported != Some(now) {
        *reported = Some(now);
        events.write(ViewerEvent::TerrainProgress(*progress));
    }
}
