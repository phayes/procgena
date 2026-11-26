//! # procgena_terrain_bevy
//!
//! Draws [`procgena_terrain`] ground in Bevy: chunks of terrain streamed
//! around the camera, finer near it and coarser far away, joined without
//! cracks.
//!
//! - Add [`TerrainPlugin`].
//! - Spawn a [`TerrainStream`] with the ground to draw: a
//!   [`Terrain`](procgena_terrain::Terrain), or a
//!   [`WorldTerrain`] of many regions (see [`TerrainStream::world`]).
//! - Put [`TerrainViewer`] on each camera the terrain is drawn for.
//!
//! Chunks are read on Bevy's async compute threads, and the drawn chunks
//! change all at once, when every chunk of the new set is ready. Each
//! chunk is a child entity of its stream, with a [`TerrainChunk`]
//! component and its own [`TerrainMaterial`]. The ground is drawn in plain
//! grey: what it is made of isn't painted yet.
//!
//! Water is not drawn: each chunk with rivers or ponds gets a child entity
//! with [`TerrainWater`] and a mesh of the water surface, and no material.
//! Add your own water material to those entities.
//!
//! [`TerrainDebug`] tints each chunk by its level and draws chunk borders.
//! A [`TerrainOverlay`] on a chunk draws an image of your own over it.
//!
//! ```no_run
//! use std::sync::Arc;
//! use bevy::prelude::*;
//! use procgena_terrain::{Elevation, RegionId, TerrainPreset, WorldTerrain, WorldTerrainOptions};
//! use procgena_terrain_bevy::{TerrainPlugin, TerrainStream, TerrainViewer};
//!
//! fn setup(mut commands: Commands) {
//!     let source = |_: RegionId, _: glam::DVec2, _: &Elevation| {
//!         TerrainPreset::rolling_hills().terrain.clone()
//!     };
//!     let world = WorldTerrain::new(WorldTerrainOptions::default(), Arc::new(source)).unwrap();
//!     commands.spawn(TerrainStream::world(world));
//!     commands.spawn((
//!         Camera3d::default(),
//!         TerrainViewer,
//!         Transform::from_xyz(0.0, 80.0, 0.0).looking_at(Vec3::new(500.0, 0.0, 0.0), Vec3::Y),
//!     ));
//! }
//!
//! App::new()
//!     .add_plugins((DefaultPlugins, TerrainPlugin::default()))
//!     .add_systems(Startup, setup)
//!     .run();
//! ```

mod canonical;
mod material;
mod overlay;
mod progress;
mod reads;
mod stream;
mod worker;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::MaterialPlugin;
use bevy::prelude::*;
use glam::{DVec2, DVec3};
use procgena_core::chain::ViewScale;
use procgena_core_bevy::{LodPlugin, LodQuality};
use procgena_terrain::tin::{ChunkEdgeKey, ContractCache, ResidentTins, TinError};
use procgena_terrain::{
    ChunkDetail, ChunkKey, ChunkLevels, ChunkMesh, ChunkSamples, ChunkSelection,
    ChunkSelectionError, ChunkSide, ChunkSource, FeatureLevel, RegionStage, Transitions, WaterMesh,
    WorldTerrain,
};

pub use canonical::CanonicalGround;
use canonical::{ReadError, Seams, TinRead};
pub use material::{GROUND_GREY, TerrainExtension, TerrainMaterial, TerrainSurface};
pub use overlay::TerrainOverlay;

pub use progress::RegionTracker;

use stream::{Read, Stream};

/// The view distance of [`TerrainPlugin::default`], in metres.
pub const DEFAULT_VIEW_DISTANCE: f32 = 4000.0;

/// Adds terrain streaming and drawing to a Bevy app. It also adds the
/// shared [`LodPlugin`], whose [`LodQuality`] scales every chunk level's
/// distance and the view distance.
///
/// Make one with [`detailed`](Self::detailed), whose chunk levels follow
/// the viewers' field of view, or [`new`](Self::new), with fixed chunk
/// levels, or use the default: [`ChunkDetail::DEFAULT`] drawn out to
/// [`DEFAULT_VIEW_DISTANCE`].
#[derive(Debug, Clone)]
pub struct TerrainPlugin {
    levels: ChunkLevels,
    detail: Option<(ChunkDetail, f32)>,
}

impl Default for TerrainPlugin {
    fn default() -> Self {
        Self::detailed(ChunkDetail::DEFAULT, DEFAULT_VIEW_DISTANCE)
            .expect("the usual chunk levels are valid")
    }
}

impl TerrainPlugin {
    /// The plugin drawing chunk levels `levels`, out to their view
    /// distance. The levels stay the same whatever the viewers' field
    /// of view.
    /// [`chunk_levels`](procgena_terrain::chunk_levels) gives the usual levels
    /// for any view distance.
    ///
    /// # Errors
    ///
    /// If `levels` can't choose chunks, for example because its last level
    /// starts too close to the view distance (see
    /// [`ChunkSelection::around`]).
    pub fn new(levels: ChunkLevels) -> Result<Self, ChunkSelectionError> {
        ChunkSelection::around(&levels, 1.0, &[], None)?;
        Ok(Self {
            levels,
            detail: None,
        })
    }

    /// The plugin drawing chunks as fine as `detail` asks, out to
    /// `view_distance` metres. The levels follow the field of view of the
    /// [`TerrainViewer`] cameras: the narrowest, which needs the finest
    /// chunks, wins, and `detail`'s own [`view`](ChunkDetail::view) is used
    /// until a perspective viewer is seen.
    ///
    /// # Errors
    ///
    /// If `view_distance` isn't a positive number.
    ///
    /// # Panics
    ///
    /// If `detail`'s share of the screen isn't a positive number (see
    /// [`ChunkDetail::levels`]).
    pub fn detailed(detail: ChunkDetail, view_distance: f32) -> Result<Self, ChunkSelectionError> {
        Ok(Self {
            detail: Some((detail, view_distance)),
            ..Self::new(detail.levels(view_distance))?
        })
    }

    /// The chunk levels, before any viewer's field of view is seen.
    pub fn levels(&self) -> &ChunkLevels {
        &self.levels
    }
}

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<TerrainMaterialPlugin>() {
            app.add_plugins(TerrainMaterialPlugin);
        }
        app.add_plugins(LodPlugin)
            .insert_resource(TerrainSettings {
                levels: self.levels.clone(),
                detail: self.detail,
            })
            .init_resource::<TerrainDebug>()
            .add_systems(
                Update,
                (
                    follow_field_of_view,
                    reset_streams,
                    stream_chunks,
                    update_materials,
                )
                    .chain(),
            );
    }
}

/// Draws [`TerrainMaterial`]s. [`TerrainPlugin`] adds it; add it yourself
/// to draw the material without streaming terrain, and only if it isn't
/// added yet.
#[derive(Debug, Clone, Copy, Default)]
pub struct TerrainMaterialPlugin;

impl Plugin for TerrainMaterialPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/terrain_fragment.wesl");
        app.add_plugins((LodPlugin, MaterialPlugin::<TerrainMaterial>::default()))
            .add_systems(PostUpdate, overlay::apply_overlays);
    }
}

/// Ground a [`TerrainStream`] can draw: where its chunks' heights and
/// water come from. Every [`ChunkSource`] is one, such as a
/// [`Terrain`](procgena_terrain::Terrain) or a [`WorldTerrain`].
pub trait TerrainSource: ChunkSource + Send {}

impl<T: ChunkSource + Send + ?Sized> TerrainSource for T {}

/// The ground a terrain is drawn from, and how.
///
/// Replacing its source reads every chunk again; the old chunks stay until
/// replacements are ready. To signal changed contents of a source, replace
/// its `Arc`.
///
/// Each chunk is drawn with the features of its own chunk level (see
/// [`FeatureLevel::of`]); which features that is, is the source's choice
/// (for a world, [`WorldTerrain::with_tier_levels`]).
#[derive(Component, Clone)]
#[require(Transform, Visibility, TerrainProgress)]
pub struct TerrainStream {
    /// Where the ground comes from.
    pub source: Arc<dyn TerrainSource>,
    /// Counts the source's region builds, so [`TerrainProgress`] can
    /// include them; `None` counts chunks only.
    pub regions: Option<RegionTracker>,
    /// Draw every chunk at [`FeatureLevel::COARSEST`] first, then read
    /// each chunk whose own level carries more features (see
    /// [`level`](Self::level)) again in the background and redraw it when
    /// ready. The view appears much sooner; near chunks briefly lack their
    /// costly features. `false` waits for each chunk's own level before
    /// drawing anything.
    pub coarse_first: bool,
    /// Draw finest-level chunks read at [`FeatureLevel::FINEST`] from their
    /// canonical meshes (see [`procgena_terrain::tin`]), built within the
    /// source's own
    /// [`tin_tolerance`](procgena_terrain::ChunkSource::tin_tolerance)
    /// (for a world, [`WorldTerrainOptions::tin`](procgena_terrain::WorldTerrainOptions::tin)),
    /// and load them into the stream's [`CanonicalGround`]. Coarser chunk
    /// levels stay regular. Where the source can't give the finest level,
    /// chunks stay regular too. `false`, the default, draws every chunk
    /// regular.
    pub tin: bool,
}

impl TerrainStream {
    /// A stream of `source`'s ground at every level, loaded coarse first,
    /// counting chunks only.
    pub fn new(source: Arc<dyn TerrainSource>) -> Self {
        Self {
            source,
            regions: None,
            coarse_first: true,
            tin: false,
        }
    }

    /// A stream of `world`'s ground at every level, loaded coarse first,
    /// with its region builds counted in [`TerrainProgress`]. This replaces
    /// any progress hook `world` had; to keep your own as well, make a
    /// [`RegionTracker`], call its hook from yours, and set
    /// [`regions`](Self::regions) yourself.
    pub fn world(world: WorldTerrain) -> Self {
        let tracker = RegionTracker::default();
        let world = world.with_progress(tracker.hook());
        Self {
            source: Arc::new(world),
            regions: Some(tracker),
            coarse_first: true,
            tin: false,
        }
    }

    /// The canonical feature level chunks at chunk level `level` end up
    /// drawn at (see [`ChunkSource::level`]).
    pub fn level(&self, level: u8) -> FeatureLevel {
        self.source.level(FeatureLevel(level))
    }
}

/// How far a [`TerrainStream`] has got. Updated every frame.
#[derive(Component, Debug, Clone, Copy, PartialEq, Default)]
pub struct TerrainProgress {
    /// How much of the work wanted now is done, from 0 to 1, for a loading
    /// bar: the region stages started in the current batch (with
    /// [`TerrainStream::regions`]) and the chunks wanted, each weighted by
    /// its usual cost. It can step back when the viewer moves on and more
    /// work is wanted. 1 once settled.
    pub fraction: f32,
    /// The costliest region stage being built, if any: what a loading
    /// screen can say the terrain is doing.
    pub stage: Option<RegionStage>,
    /// Chunks drawn.
    pub drawn: usize,
    /// Drawn chunks still with fewer features than they want, waiting to be
    /// read again (see [`TerrainStream::coarse_first`]).
    pub refining: usize,
    /// Chunks wanted around the viewers now.
    pub wanted: usize,
    /// Chunks being read or waiting to be read.
    pub reading: usize,
    /// Chunks that could not be read since the stream last changed; they
    /// are left as holes.
    pub failed: usize,
    /// Chunks sampled since the stream last changed, at any feature level.
    pub sampled: usize,
    /// Drawn chunks that a finer feature level left exactly as they were, so they
    /// stand for it without being read again, since the stream last
    /// changed.
    pub promoted: usize,
    /// Whether the drawn chunks are the ones wanted, each from the ground
    /// it should have, and nothing is being read.
    pub settled: bool,
}

/// Marks a camera that terrain chunks are chosen around. With several, each
/// gets the detail it needs.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct TerrainViewer;

/// One drawn chunk, on a child entity of its [`TerrainStream`].
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct TerrainChunk {
    /// Which chunk it is.
    pub key: ChunkKey,
    /// The finer chunks it is meshed to join.
    pub transitions: Transitions,
    /// The canonical feature level of the ground it was read from (see
    /// [`ChunkSource::level`]).
    pub level: FeatureLevel,
}

/// Marks a [`TerrainChunk`] drawn from its canonical mesh (see
/// [`TerrainStream::tin`]) rather than from regular samples.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct CanonicalChunk;

/// Marks the water surface of one chunk, on a child entity of its
/// [`TerrainChunk`]. It has a mesh and no material: add your own.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct TerrainWater;

/// Debug drawing for every terrain.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TerrainDebug {
    /// Tint each chunk by its level.
    pub levels: bool,
    /// Draw a dark line along chunk borders.
    pub borders: bool,
}

/// The plugin's settings.
#[derive(Resource)]
struct TerrainSettings {
    /// The chunk levels drawn now.
    levels: ChunkLevels,
    /// The detail and view distance the levels follow the viewers' field
    /// of view with, if they do.
    detail: Option<(ChunkDetail, f32)>,
}

/// A chunk's ground as read: regular samples, or its canonical mesh.
#[derive(Clone)]
pub(crate) enum Geometry {
    Regular(Arc<ChunkSamples>),
    Tin(TinRead),
}

impl Geometry {
    /// The chunk read.
    #[cfg(test)]
    pub(crate) fn key(&self) -> ChunkKey {
        match self {
            Self::Regular(samples) => samples.key(),
            Self::Tin(read) => read.index.chunk().key,
        }
    }

    /// The canonical feature level of the ground read.
    pub(crate) fn level(&self) -> FeatureLevel {
        match self {
            Self::Regular(samples) => samples.level(),
            Self::Tin(_) => FeatureLevel::FINEST,
        }
    }
}

/// One chunk as read, ready to draw.
pub(crate) struct ChunkRead {
    pub(crate) geometry: Geometry,
    /// The canonical feature level the chunk stands for: the level it was
    /// read at, or a finer one whose read it equals exactly (see
    /// [`promoted`](Self::promoted)).
    stands_for: FeatureLevel,
}

impl ChunkRead {
    /// Reads chunk `key` of `source` at feature level `level`.
    #[cfg(test)]
    pub(crate) fn read(
        source: &(impl TerrainSource + ?Sized),
        key: ChunkKey,
        level: FeatureLevel,
    ) -> Result<Self, ReadError> {
        Self::read_with(source, key, level, None)
    }

    /// Reads chunk `key` of `source` at feature level `level`: from its
    /// canonical mesh, with edge contracts from `tin`, if given and the
    /// chunk is a finest-level one read with every feature.
    fn read_with(
        source: &(impl TerrainSource + ?Sized),
        key: ChunkKey,
        level: FeatureLevel,
        tin: Option<&ContractCache>,
    ) -> Result<Self, ReadError> {
        // A finest-level chunk is canonical at the finest feature level,
        // and at a coarser one the source already knows the finest level
        // changes nothing about (asked without building, so the coarser
        // read stays quick); its canonical mesh is then exactly the finest
        // level's.
        let finest = source.level(level) == FeatureLevel::FINEST
            || source.level_change_known(key, level, FeatureLevel::FINEST) == Some(false);
        let canonical = tin.filter(|_| key.level == 0 && finest);
        let geometry = match canonical.map(|contracts| TinRead::build(source, key, contracts)) {
            Some(Ok(read)) => Geometry::Tin(read),
            // A source without the finest level has no canonical ground.
            None | Some(Err(ReadError::Tin(TinError::NotFinest(_)))) => {
                Geometry::Regular(Arc::new(ChunkSamples::sample(source, key, level)?))
            }
            Some(Err(error)) => return Err(error),
        };
        Ok(Self {
            stands_for: geometry.level(),
            geometry,
        })
    }

    /// The canonical feature level the chunk stands for: the level it was
    /// read at, or a finer one it was promoted to.
    pub(crate) fn level(&self) -> FeatureLevel {
        self.stands_for
    }

    /// This read standing for canonical feature level `level` as well,
    /// unchanged otherwise. Only for a level the source says changes
    /// nothing about the chunk (see [`ChunkSource::level_changes`]), so it
    /// is exactly that level's read.
    fn promoted(&self, level: FeatureLevel) -> Self {
        Self {
            geometry: self.geometry.clone(),
            stands_for: level,
        }
    }

    /// A stand-in for chunk `key`, inside `parent`'s chunk: `parent`'s
    /// ground drawn at `key`'s size (see [`ChunkSamples::stand_in`]).
    ///
    /// # Panics
    ///
    /// If `key` is not inside `parent`'s chunk, or `parent` is drawn from
    /// a canonical mesh (only finest-level chunks are, and no chunk lies
    /// inside one).
    pub(crate) fn stand_in(parent: &ChunkRead, key: ChunkKey) -> Self {
        let Geometry::Regular(samples) = &parent.geometry else {
            panic!("a canonical chunk has no chunks inside it");
        };
        Self {
            geometry: Geometry::Regular(Arc::new(ChunkSamples::stand_in(samples, key))),
            stands_for: parent.stands_for,
        }
    }

    /// The chunk's canonical mesh, if it is drawn from one.
    pub(crate) fn tin(&self) -> Option<&TinRead> {
        match &self.geometry {
            Geometry::Tin(read) => Some(read),
            Geometry::Regular(_) => None,
        }
    }

    /// The chunk's mesh with `transitions` and skirts `skirt` metres deep,
    /// meeting the canonical chunks beside it as `seams` says.
    fn mesh(&self, transitions: Transitions, skirt: f32, seams: &Seams) -> ChunkMesh {
        match &self.geometry {
            Geometry::Regular(samples) => {
                let mut mesh = samples.mesh(transitions, skirt);
                for tin in &seams.tins {
                    tin.chunk().fit_neighbour(&mut mesh);
                }
                mesh.level = self.stands_for;
                mesh
            }
            Geometry::Tin(read) => read.mesh(&seams.split, skirt),
        }
    }
}

/// A stream's chunk reads on the terrain thread, each under its chunk and
/// the stream generation it was started in.
type Reads = reads::ReadQueue<(ChunkKey, u64), Result<ChunkRead, ReadError>>;

/// A stream's state: what is drawn, wanted and being read.
#[derive(Component, Default)]
struct StreamState {
    stream: Stream,
    /// Reads queued or running on the terrain thread, nearest first.
    reads: Reads,
    last: Option<TerrainStream>,
    /// The entity drawing each chunk.
    entities: HashMap<ChunkKey, Entity>,
    /// Each drawn chunk's material, with the read it was made from, so a
    /// chunk drawn again from the same read keeps it.
    materials: HashMap<ChunkKey, (Arc<ChunkRead>, Handle<TerrainMaterial>)>,
    /// Chunks that failed to read in this generation.
    failed: usize,
    /// Chunks sampled in this generation (see [`TerrainProgress::sampled`]).
    sampled: usize,
    /// Chunks promoted in this generation (see
    /// [`TerrainProgress::promoted`]).
    promoted: usize,
    /// Whether the chunk levels can't be chosen; logged once.
    broken: bool,
    /// The levels, distance factor and viewer positions the last chunk
    /// choice was made from, when it kept the target as it was. Choosing
    /// again from the same inputs keeps it again, so it is skipped.
    unchanged_from: Option<(ChunkLevels, f32, Vec<DVec3>)>,
    /// Whether a read finished, or the drawn chunks changed, since drawn
    /// chunks were last checked for promotion. Only then can the answer
    /// change: a finer feature level's region terrain is built inside a
    /// read.
    check_promotions: bool,
    /// Drawn chunks the feature level they want is known to change, so
    /// they need a read; not checked for promotion again.
    changes_known: HashSet<ChunkKey>,
    /// Skirts deeper than [`skirt_depth`], for chunks beside a chunk read
    /// at another feature level: how far that level moved the ground.
    skirts: HashMap<ChunkKey, f32>,
    /// How each drawn chunk was meshed to meet the canonical chunks
    /// beside it.
    seams: HashMap<ChunkKey, Seams>,
    /// Edge contracts the canonical chunks share, made when the first one
    /// is read and dropped when the stream starts over.
    contracts: Option<Arc<ContractCache>>,
}

/// How chunk `key`, drawn from `read`, meets the canonical chunks drawn
/// beside it in `stream`: a canonical chunk splits its sides that meet
/// regular chunks, and a regular chunk fits every canonical chunk it
/// touches.
fn seams_for(stream: &Stream, key: ChunkKey, read: &ChunkRead) -> Seams {
    let touching = stream.touching(key);
    if read.tin().is_some() {
        let regular = |other: ChunkKey| stream.read_of(other).is_some_and(|r| r.tin().is_none());
        let split = ChunkSide::ALL
            .into_iter()
            .filter(|&side| {
                touching
                    .iter()
                    .any(|&(other, _)| canonical::along_side(key, side, other) && regular(other))
            })
            .collect();
        Seams {
            split,
            tins: Vec::new(),
        }
    } else {
        let tins = touching
            .iter()
            .filter_map(|&(other, _)| Some(stream.read_of(other)?.tin()?.index.clone()))
            .collect();
        Seams {
            split: Vec::new(),
            tins,
        }
    }
}

/// The skirt hanging from each side of a chunk, in metres: deep enough to
/// hide small gaps where chunks of different feature levels meet, or rounding where
/// chunks meet.
pub fn skirt_depth(key: ChunkKey) -> f32 {
    (4.0 * key.step() as f32).max(8.0)
}

/// A Bevy mesh of a chunk's ground, with UVs running from 0 to 1 across
/// the chunk along x and z, as [`TerrainMaterial`] reads its weights.
pub fn chunk_mesh(chunk: &ChunkMesh) -> Mesh {
    let size = ChunkKey::size_at(chunk.key.level) as f32;
    let uvs: Vec<[f32; 2]> = chunk
        .positions
        .iter()
        .map(|p| [p.x / size, p.z / size])
        .collect();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, chunk.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, chunk.normals.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(chunk.indices.clone()))
}

/// A Bevy mesh of a chunk's water surface, with upward normals.
pub fn water_mesh(water: &WaterMesh) -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, water.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![Vec3::Y; water.positions.len()])
    .with_inserted_indices(Indices::U32(water.indices.clone()))
}

/// The drawing settings every chunk shares: the fade at the end of the
/// view, and the debug tint and borders.
struct ChunkLook {
    fade: Vec4,
    debug: TerrainDebug,
}

impl ChunkLook {
    fn new(settings: &TerrainSettings, quality: &LodQuality, debug: TerrainDebug) -> Self {
        let fade_out = settings.levels.fade_out(quality.0.distance_scale());
        let fade = Vec4::new(0.0, 0.0, fade_out.start, fade_out.end);
        Self { fade, debug }
    }

    /// Sets `surface`'s drawing settings for a chunk at `level`.
    fn apply(&self, surface: &mut TerrainSurface, level: u8) {
        surface.fade = self.fade;
        surface.tint = if self.debug.levels {
            material::level_tint(level).extend(0.55)
        } else {
            Vec4::ZERO
        };
        surface.border = if self.debug.borders {
            ChunkKey::size_at(level) as f32
        } else {
            0.0
        };
    }
}

/// A chunk's material at `level`, drawn with `look`.
fn chunk_material(
    level: u8,
    look: &ChunkLook,
    materials: &mut Assets<TerrainMaterial>,
) -> Handle<TerrainMaterial> {
    let mut extension = TerrainExtension::default();
    look.apply(&mut extension.surface, level);
    materials.add(TerrainMaterial {
        base: StandardMaterial {
            reflectance: 0.2,
            ..default()
        },
        extension,
    })
}

/// With levels that follow a [`ChunkDetail`], sets them for the narrowest
/// field of view among the perspective [`TerrainViewer`] cameras, when it
/// changes.
fn follow_field_of_view(
    mut settings: ResMut<TerrainSettings>,
    viewers: Query<&Projection, With<TerrainViewer>>,
) {
    let Some((detail, view_distance)) = settings.detail else {
        return;
    };
    let narrowest = viewers
        .iter()
        .filter_map(|projection| match projection {
            Projection::Perspective(p) if p.fov > 0.0 && p.fov < std::f32::consts::PI => {
                Some(ViewScale::perspective(p.fov))
            }
            _ => None,
        })
        .max_by(|a, b| a.get().total_cmp(&b.get()));
    let Some(view) = narrowest else { return };
    let levels = ChunkDetail { view, ..detail }.levels(view_distance);
    if levels != settings.levels {
        settings.levels = levels;
    }
}

/// Gives new streams their state, and starts changed ones over.
fn reset_streams(
    mut commands: Commands,
    mut streams: Query<(Entity, Ref<TerrainStream>, Option<&mut StreamState>)>,
) {
    for (entity, stream, state) in &mut streams {
        match state {
            None => {
                commands.entity(entity).insert(StreamState {
                    last: Some((*stream).clone()),
                    ..default()
                });
            }
            Some(mut state) if stream.is_changed() && !stream.is_added() => {
                let same_ground = state.last.as_ref().is_some_and(|last| {
                    Arc::ptr_eq(&last.source, &stream.source)
                        && last.coarse_first == stream.coarse_first
                        && last.tin == stream.tin
                });
                if !same_ground {
                    state.stream.reset();
                    state.unchanged_from = None;
                    state.reads.cancel();
                    state.check_promotions = true;
                    state.changes_known.clear();
                    state.failed = 0;
                    state.sampled = 0;
                    state.promoted = 0;
                    state.skirts.clear();
                    state.seams.clear();
                    state.contracts = None;
                    commands.entity(entity).remove::<CanonicalGround>();
                }
                state.last = Some((*stream).clone());
            }
            Some(_) => {}
        }
    }
}

/// Chooses each stream's chunks around the viewers, reads them, and swaps
/// the drawn chunks once the new set is ready.
#[expect(clippy::too_many_arguments, reason = "one Bevy system")]
fn stream_chunks(
    mut commands: Commands,
    settings: Res<TerrainSettings>,
    quality: Res<LodQuality>,
    debug: Res<TerrainDebug>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    viewers: Query<&GlobalTransform, With<TerrainViewer>>,
    mut streams: Query<(
        Entity,
        &TerrainStream,
        &GlobalTransform,
        &mut StreamState,
        &mut TerrainProgress,
    )>,
) {
    let factor = quality.0.distance_scale();
    let look = ChunkLook::new(&settings, &quality, *debug);
    for (entity, source, transform, mut state, mut progress) in &mut streams {
        let state = &mut *state;

        // The viewers in the stream's own frame, with their height above
        // the ground as far as the drawn chunks know it.
        let local = transform.affine().inverse();
        let positions: Vec<DVec3> = viewers
            .iter()
            .map(|v| {
                let p = local.transform_point3(v.translation()).as_dvec3();
                let ground = state.stream.ground_height(DVec2::new(p.x, p.z));
                DVec3::new(p.x, p.y - f64::from(ground.unwrap_or(0.0)), p.z)
            })
            .collect();
        let flat: Vec<DVec2> = positions.iter().map(|p| DVec2::new(p.x, p.z)).collect();

        // The chunks wanted now. The choice depends only on its inputs and
        // the target it keeps or replaces, so when the last choice kept the
        // target and its inputs haven't changed, it would keep it again.
        let inputs = (settings.levels.clone(), factor, positions);
        if state.unchanged_from.as_ref() != Some(&inputs) {
            let (levels, factor, positions) = &inputs;
            match ChunkSelection::around(levels, *factor, positions, Some(&state.stream.target)) {
                Ok(target) => {
                    if target == state.stream.target {
                        state.unchanged_from = Some(inputs);
                    } else {
                        state.stream.retarget(target);
                        state.unchanged_from = None;
                    }
                }
                Err(error) => {
                    if !state.broken {
                        error!("terrain chunks can't be chosen: {error}");
                        state.broken = true;
                    }
                    continue;
                }
            }
        }

        // Collect finished reads.
        for ((key, generation), result) in state.reads.finished() {
            state.check_promotions = true;
            let current = generation == state.stream.generation;
            if let (true, Ok(read)) = (current, &result) {
                if read.geometry.level() > read.level() {
                    state.promoted += 1;
                } else {
                    state.sampled += 1;
                }
            }
            let read: Read = match result {
                Ok(read) => Some(Arc::new(read)),
                // A failed upgrade keeps the ground already drawn.
                Err(error) if current && state.stream.is_upgrade(key) => {
                    warn!("terrain {key} keeps its coarser features: {error}");
                    None
                }
                Err(error) => {
                    if current {
                        warn!("terrain {key} left out: {error}");
                        state.failed += 1;
                    }
                    None
                }
            };
            state.stream.finish(key, generation, read);
        }

        // The reads still waiting are chosen again below, from what is
        // wanted now.
        let generation = state.stream.generation;
        for (key, started_in) in state.reads.cancel() {
            if started_in == generation {
                state.stream.cancel(key);
            }
        }

        // A drawn chunk a finer feature level is already known to leave
        // alone stands for that level as it is, at once and without a
        // read. A canonical chunk always needs its own read.
        let wanted = |key: ChunkKey| source.level(key.level);
        let canonical = |key: ChunkKey, level: FeatureLevel| {
            source.tin && key.level == 0 && source.source.level(level) == FeatureLevel::FINEST
        };
        if std::mem::take(&mut state.check_promotions) {
            let upgrades = state.stream.upgrades(&flat, wanted);
            state.changes_known.retain(|key| upgrades.contains(key));
            for key in upgrades {
                let level = wanted(key);
                let Some(drawn) = state.stream.read_of(key).filter(|_| !canonical(key, level))
                else {
                    continue;
                };
                if state.changes_known.contains(&key) {
                    continue;
                }
                match source.source.level_change_known(key, drawn.level(), level) {
                    Some(false) => {
                        state.stream.start(key);
                        let promoted = Arc::new(drawn.promoted(level));
                        state.stream.finish(key, generation, Some(promoted));
                        state.promoted += 1;
                    }
                    Some(true) => {
                        state.changes_known.insert(key);
                    }
                    None => {}
                }
            }
        }

        // Queue the missing chunks, nearest first: at the coarsest feature
        // level when the stream loads coarse first. Only when none is
        // missing, queue drawn chunks to read again at the level they want.
        let missing = state.stream.missing(&flat);
        let mut reads: Vec<(ChunkKey, FeatureLevel)> = missing
            .iter()
            .map(|&key| {
                let level = if source.coarse_first {
                    FeatureLevel::COARSEST
                } else {
                    wanted(key)
                };
                (key, level)
            })
            .collect();
        if missing.is_empty() {
            let upgrades = state.stream.upgrades(&flat, wanted);
            reads.extend(upgrades.into_iter().map(|k| (k, wanted(k))));
        }
        let mut jobs: Vec<((ChunkKey, u64), reads::Job<_>)> = Vec::with_capacity(reads.len());
        for (key, level) in reads {
            // A drawn chunk being read again at a finer feature level: if
            // that level changes nothing about it, the drawn read stands
            // for it as it is.
            let drawn = state
                .stream
                .is_upgrade(key)
                .then(|| state.stream.read_of(key))
                .flatten()
                .filter(|_| !canonical(key, level));
            state.stream.start(key);
            let ground = Arc::clone(&source.source);
            let tin = source.tin.then(|| {
                state
                    .contracts
                    .get_or_insert_with(|| {
                        Arc::new(ContractCache::new(&source.source.tin_tolerance()))
                    })
                    .clone()
            });
            let job = Box::new(move || {
                if let Some(drawn) = drawn
                    && !ground.level_changes(key, drawn.level(), level)
                {
                    return Ok(drawn.promoted(level));
                }
                ChunkRead::read_with(&*ground, key, level, tin.as_deref())
            });
            jobs.push(((key, generation), job));
        }
        state.reads.push(jobs);

        // Swap the drawn chunks once the whole new set is read; redraw
        // upgraded chunks one by one. `redraw` removes a chunk's entity and
        // draws its samples, if any.
        let mut redraw = |state: &mut StreamState, key: ChunkKey, transitions, read: Read| {
            if let Some(chunk) = state.entities.remove(&key) {
                commands.entity(chunk).despawn();
            }
            let Some(read) = read else {
                state.materials.remove(&key);
                state.seams.remove(&key);
                return;
            };
            // Keep the material while the read is the same.
            let material = match state.materials.get(&key) {
                Some((drawn, material)) if Arc::ptr_eq(drawn, &read) => material.clone(),
                _ => {
                    let material = chunk_material(key.level, &look, &mut materials);
                    state
                        .materials
                        .insert(key, (Arc::clone(&read), material.clone()));
                    material
                }
            };
            let skirt = skirt_depth(key).max(state.skirts.get(&key).copied().unwrap_or(0.0));
            let seams = seams_for(&state.stream, key, &read);
            let chunk = spawn_chunk(
                &mut commands,
                &mut meshes,
                &material,
                entity,
                &read.mesh(transitions, skirt, &seams),
                read.tin().is_some(),
            );
            state.entities.insert(key, chunk);
            state.seams.insert(key, seams);
        };
        // Every chunk drawn now, whether new to the view or read again at a
        // finer feature level. Chunks touching at different levels can
        // stand apart by up to how far the finer level moved the ground:
        // deepen both chunks' skirts to cover it, redrawing touching
        // chunks whose skirts grow.
        let mut drawn_now: Vec<(ChunkKey, Transitions, Read)> = Vec::new();
        if let Some(swap) = state.stream.swap() {
            for key in swap.remove {
                state.skirts.remove(&key);
                redraw(state, key, Transitions::NONE, None);
            }
            drawn_now.extend(swap.add);
        }
        drawn_now.extend(state.stream.take_refreshed());
        let changed = !drawn_now.is_empty();
        state.check_promotions |= changed;
        let mut near_changes: Vec<ChunkKey> = Vec::new();
        for (key, transitions, read) in drawn_now {
            near_changes.push(key);
            near_changes.extend(state.stream.touching(key).into_iter().map(|(k, _)| k));
            let level = read.as_ref().map(|r| r.level());
            let mut deepest = 0.0f32;
            for (other, other_transitions) in state.stream.touching(key) {
                let (Some(level), Some(other_level)) = (level, state.stream.level_of(other)) else {
                    continue;
                };
                if level == other_level {
                    continue;
                }
                // The ground of the chunk with more features, against the
                // coarser level.
                let finer = if level < other_level { key } else { other };
                let moved = source.source.level_difference(
                    finer.min_corner(),
                    finer.max_corner(),
                    level.max(other_level),
                );
                if moved == 0.0 {
                    continue;
                }
                deepest = deepest.max(moved);
                let skirt = state.skirts.entry(other).or_default();
                if moved > *skirt {
                    *skirt = moved;
                    let other_read = state.stream.read_of(other);
                    redraw(state, other, other_transitions, other_read);
                }
            }
            let skirt = state.skirts.entry(key).or_default();
            *skirt = skirt.max(deepest);
            redraw(state, key, transitions, read);
        }

        // Chunks drawn now, and those beside them, may meet canonical
        // chunks differently than when they were drawn: redraw those that
        // do, so regular sides fit canonical ones and canonical sides split
        // where regular ones meet them.
        near_changes.sort_unstable();
        near_changes.dedup();
        for key in near_changes {
            let (Some(transitions), Some(read)) =
                (state.stream.drawn.get(key), state.stream.read_of(key))
            else {
                continue;
            };
            let seams = seams_for(&state.stream, key, &read);
            if state.entities.contains_key(&key)
                && !state.seams.get(&key).is_some_and(|s| s.same_as(&seams))
            {
                redraw(state, key, transitions, Some(read));
            }
        }

        // The canonical chunks drawn now, for ground queries; the shared
        // edge contracts are kept only for their edges.
        if changed && source.tin {
            let mut tins = ResidentTins::new();
            let mut edges = HashSet::new();
            for key in state.entities.keys() {
                if let Some(tin) = state.stream.read_of(*key).as_ref().and_then(|r| r.tin()) {
                    tins.insert(tin.index.clone());
                    edges.extend(ChunkEdgeKey::of_chunk(*key));
                }
            }
            if let Some(contracts) = &state.contracts {
                contracts.retain(|edge| edges.contains(&edge));
            }
            commands.entity(entity).insert(CanonicalGround { tins });
        }

        // Region work from the hook, and chunk reads, by their usual cost;
        // chunks waiting for a finer feature level count as not read.
        let refining = state.stream.upgrades(&flat, wanted).len();
        let settled = state.stream.is_settled() && refining == 0;
        let regions = source
            .regions
            .as_ref()
            .map(|r| r.counts())
            .unwrap_or_default();
        let wanted = (state.stream.target.len() + refining) as f32 * progress::CHUNK_COST;
        let read = state.stream.read_in_target() as f32 * progress::CHUNK_COST;
        let total = regions.total + wanted;
        let fraction = if settled || total == 0.0 {
            1.0
        } else {
            ((regions.done + read) / total).clamp(0.0, 1.0)
        };
        *progress = TerrainProgress {
            fraction,
            stage: regions.stage(),
            drawn: state.entities.len(),
            refining,
            wanted: state.stream.target.len(),
            reading: state.stream.reading(),
            failed: state.failed,
            sampled: state.sampled,
            promoted: state.promoted,
            settled,
        };
    }
}

/// Spawns `mesh` as a chunk of `stream`, with its water as a child, and
/// returns the chunk entity. `canonical` says whether the mesh is a
/// canonical chunk's.
fn spawn_chunk(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: &Handle<TerrainMaterial>,
    stream: Entity,
    mesh: &ChunkMesh,
    canonical: bool,
) -> Entity {
    let chunk = commands
        .spawn((
            TerrainChunk {
                key: mesh.key,
                transitions: mesh.transitions,
                level: mesh.level,
            },
            Mesh3d(meshes.add(chunk_mesh(mesh))),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(mesh.translation().as_vec3()),
            ChildOf(stream),
        ))
        .id();
    if canonical {
        commands.entity(chunk).insert(CanonicalChunk);
    }
    if let Some(water) = &mesh.water {
        commands.spawn((
            TerrainWater,
            Mesh3d(meshes.add(water_mesh(water))),
            Transform::default(),
            ChildOf(chunk),
        ));
    }
    chunk
}

/// Keeps the chunks' materials' fade and debug drawing up to date when the
/// quality or debug drawing changes. Chunks drawn later get them when
/// their material is made.
fn update_materials(
    settings: Res<TerrainSettings>,
    quality: Res<LodQuality>,
    debug: Res<TerrainDebug>,
    streams: Query<&StreamState>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
) {
    if !(quality.is_changed() || debug.is_changed()) {
        return;
    }
    let look = ChunkLook::new(&settings, &quality, *debug);
    for state in &streams {
        for (key, (_, handle)) in &state.materials {
            if let Some(mut material) = materials.get_mut(handle) {
                look.apply(&mut material.extension.surface, key.level);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::asset::AssetPlugin;
    use bevy::time::TimeUpdateStrategy;
    use procgena_terrain::{
        Boundary, ChunkDetail, Region, RegionId, TerrainGenerator, TerrainPreset, chunk_levels,
    };

    use super::*;

    /// Streaming tests share Bevy's async and rayon pools, so they run one
    /// at a time to keep within their frame budgets.
    static STREAMING: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Holds [`STREAMING`] for the rest of a streaming test, even after
    /// another streaming test panicked.
    fn streaming() -> std::sync::MutexGuard<'static, ()> {
        STREAMING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// One unbounded region of rolling hills.
    fn hills() -> Arc<dyn TerrainSource> {
        let region = Region {
            id: RegionId(0),
            boundary: Boundary::Unbounded,
            edges: vec![],
            corners: vec![],
        };
        let terrain = TerrainGenerator::new(TerrainPreset::rolling_hills().terrain.clone())
            .unwrap()
            .build(region, &[])
            .unwrap();
        Arc::new(terrain)
    }

    /// An app with the plugin and no renderer, drawing to 300 m.
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            AssetPlugin::default(),
            bevy::mesh::MeshPlugin,
        ))
        .init_asset::<TerrainMaterial>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Image>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            16,
        )));
        // Coarse levels (level 1 from 128 m), so a short view still spans
        // two levels and few chunks.
        let coarse = ChunkDetail {
            cell_on_screen: 2.0 * ChunkDetail::DEFAULT.cell_on_screen,
            ..ChunkDetail::DEFAULT
        };
        let plugin = TerrainPlugin::new(coarse.levels(300.0)).unwrap();
        // Only the streaming, not the material plugin, which needs a
        // renderer.
        app.insert_resource(TerrainSettings {
            levels: plugin.levels.clone(),
            detail: plugin.detail,
        })
        .init_resource::<LodQuality>()
        .init_resource::<TerrainDebug>()
        .add_systems(
            Update,
            (reset_streams, stream_chunks, update_materials).chain(),
        );
        app
    }

    /// Runs `app` until every stream settles, for at most `frames` frames.
    fn settle(app: &mut App, frames: usize) -> TerrainProgress {
        for _ in 0..frames {
            app.update();
            std::thread::sleep(Duration::from_millis(5));
            let mut query = app.world_mut().query::<&TerrainProgress>();
            let progress = *query.single(app.world()).unwrap();
            if progress.settled && progress.drawn > 0 {
                return progress;
            }
        }
        panic!("the terrain did not settle in {frames} frames");
    }

    #[test]
    fn invalid_levels_are_refused_when_the_plugin_is_made() {
        let levels = TerrainPlugin::default().levels().clone();
        assert_eq!(levels.view_distance, DEFAULT_VIEW_DISTANCE);
        // A view distance inside the last level's switching band.
        let mut short = chunk_levels(6000.0);
        short.view_distance = 4000.0;
        assert!(TerrainPlugin::new(short).is_err());
        let mut endless = chunk_levels(6000.0);
        endless.view_distance = f32::INFINITY;
        assert!(TerrainPlugin::new(endless).is_err());
        assert!(TerrainPlugin::new(chunk_levels(4000.0)).is_ok());
        assert!(TerrainPlugin::detailed(ChunkDetail::DEFAULT, -1.0).is_err());
    }

    #[test]
    fn detailed_levels_follow_the_narrowest_field_of_view() {
        let mut app = App::new();
        let plugin = TerrainPlugin::detailed(ChunkDetail::DEFAULT, 4000.0).unwrap();
        app.insert_resource(TerrainSettings {
            levels: plugin.levels.clone(),
            detail: plugin.detail,
        })
        .add_systems(Update, follow_field_of_view);
        let level_1 = |app: &App| app.world().resource::<TerrainSettings>().levels.levels[1].start;
        let usual = level_1(&app);

        // A viewer with Bevy's usual 45° keeps the usual levels; a second,
        // zoomed in to 15°, needs finer ground three times as far out.
        let perspective = |fov: f32| {
            (
                TerrainViewer,
                Projection::Perspective(PerspectiveProjection { fov, ..default() }),
            )
        };
        app.world_mut().spawn(perspective(45f32.to_radians()));
        app.update();
        assert!((level_1(&app) - usual).abs() < 1e-3);
        app.world_mut().spawn(perspective(15f32.to_radians()));
        app.update();
        let ratio = level_1(&app) / usual;
        let expected = (22.5f32.to_radians().tan()) / (7.5f32.to_radians().tan());
        assert!((ratio - expected).abs() < 1e-3, "{ratio} times as far");

        // Levels fixed by `new` stay put.
        let fixed = TerrainPlugin::new(chunk_levels(4000.0)).unwrap();
        app.world_mut().resource_mut::<TerrainSettings>().detail = fixed.detail;
        app.world_mut().resource_mut::<TerrainSettings>().levels = fixed.levels.clone();
        app.update();
        assert_eq!(
            app.world().resource::<TerrainSettings>().levels,
            fixed.levels
        );
    }

    #[test]
    fn chunks_stream_in_around_the_viewer_and_follow_it() {
        let _streaming = streaming();
        let mut app = app();
        let stream = app.world_mut().spawn(TerrainStream::new(hills())).id();
        let camera = app
            .world_mut()
            .spawn((TerrainViewer, Transform::from_xyz(0.0, 40.0, 0.0)))
            .id();

        let progress = settle(&mut app, 2000);
        assert_eq!(progress.failed, 0);
        assert_eq!(progress.drawn, progress.wanted);
        let mut chunks = app.world_mut().query::<(&TerrainChunk, &ChildOf)>();
        let drawn: Vec<TerrainChunk> = chunks
            .iter(app.world())
            .map(|(c, parent)| {
                assert_eq!(parent.parent(), stream);
                *c
            })
            .collect();
        assert_eq!(drawn.len(), progress.drawn);
        assert!(drawn.iter().any(|c| c.key.level == 0));
        assert!(drawn.iter().any(|c| c.transitions != Transitions::NONE));

        // Move the viewer: the chunks follow, and none is drawn twice.
        app.world_mut()
            .entity_mut(camera)
            .insert(Transform::from_xyz(600.0, 40.0, 0.0));
        // The camera's global transform follows at the end of this frame;
        // the chunks follow it from the next.
        app.update();
        let progress = settle(&mut app, 2000);
        let keys: Vec<ChunkKey> = chunks.iter(app.world()).map(|(c, _)| c.key).collect();
        assert_eq!(keys.len(), progress.drawn);
        let under = ChunkKey::containing(DVec2::new(600.0, 0.0), 0);
        assert!(keys.contains(&under), "the chunk under the viewer is drawn");
        let unique: std::collections::HashSet<_> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len());

        // Each drawn chunk has its own flat grey material; those of chunks
        // no longer drawn are freed.
        for _ in 0..3 {
            app.update();
        }
        let mut drawn = app
            .world_mut()
            .query::<(&TerrainChunk, &MeshMaterial3d<TerrainMaterial>)>();
        let handles: std::collections::HashSet<AssetId<TerrainMaterial>> =
            drawn.iter(app.world()).map(|(_, m)| m.id()).collect();
        assert_eq!(handles.len(), progress.drawn);
        let materials = app.world().resource::<Assets<TerrainMaterial>>();
        assert_eq!(materials.len(), progress.drawn);
        for (_, material) in materials.iter() {
            assert_eq!(
                material.extension.surface.colour,
                TerrainSurface::default().colour
            );
        }
    }

    #[test]
    fn progress_counts_region_builds_until_the_terrain_settles() {
        use procgena_terrain::{
            Elevation, MIN_EDGE, RegionGraphOptions, WorldTerrain, WorldTerrainOptions,
        };

        let _streaming = streaming();
        let mut app = app();
        let options = WorldTerrainOptions {
            regions: RegionGraphOptions {
                size: 2000.0,
                min_edge: MIN_EDGE,
                ..RegionGraphOptions::default()
            },
            ..WorldTerrainOptions::default()
        };
        let source = |_: RegionId, _: DVec2, _: &Elevation| TerrainPreset::plains().terrain.clone();
        let world = WorldTerrain::new(options, Arc::new(source)).unwrap();
        app.world_mut().spawn(TerrainStream::world(world));
        app.world_mut()
            .spawn((TerrainViewer, Transform::from_xyz(0.0, 40.0, 0.0)));

        // While regions build, the stage is named and the fraction is
        // below 1; once settled it is 1.
        let mut seen: Vec<TerrainProgress> = Vec::new();
        for _ in 0..20_000 {
            app.update();
            std::thread::sleep(Duration::from_millis(2));
            let mut query = app.world_mut().query::<&TerrainProgress>();
            let progress = *query.single(app.world()).unwrap();
            seen.push(progress);
            if progress.settled && progress.drawn > 0 {
                break;
            }
        }
        let last = seen.last().unwrap();
        assert!(last.settled, "the terrain settles");
        assert_eq!(last.fraction, 1.0);
        assert_eq!(last.refining, 0);
        assert!(seen.iter().any(|p| p.stage.is_some() && p.fraction < 1.0));

        // Coarse first: chunks were drawn rough while the finest level was
        // still being built, and all end with every feature.
        assert!(seen.iter().any(|p| p.drawn > 0 && p.refining > 0));
        let mut chunks = app.world_mut().query::<&TerrainChunk>();
        assert!(
            chunks
                .iter(app.world())
                .all(|c| c.level == FeatureLevel::FINEST)
        );
        assert!(seen.iter().all(|p| (0.0..=1.0).contains(&p.fraction)));
    }

    #[test]
    fn a_changed_stream_replaces_its_chunks() {
        let _streaming = streaming();
        let mut app = app();
        let stream = app.world_mut().spawn(TerrainStream::new(hills())).id();
        app.world_mut()
            .spawn((TerrainViewer, Transform::from_xyz(0.0, 40.0, 0.0)));
        let before = settle(&mut app, 2000);
        let mut chunks = app.world_mut().query::<(Entity, &TerrainChunk)>();
        let old: Vec<Entity> = chunks.iter(app.world()).map(|(e, _)| e).collect();

        // New ground: every chunk is replaced once the new ones are ready.
        app.world_mut()
            .entity_mut(stream)
            .insert(TerrainStream::new(hills()));
        let after = settle(&mut app, 2000);
        assert_eq!(after.drawn, before.drawn);
        for (entity, _) in chunks.iter(app.world()) {
            assert!(!old.contains(&entity));
        }
    }

    use std::collections::BTreeMap;

    /// The content hash of each canonical chunk the stream has loaded.
    fn canonical_hashes(app: &mut App) -> BTreeMap<ChunkKey, u64> {
        let mut query = app.world_mut().query::<&CanonicalGround>();
        let ground = query.single(app.world()).unwrap();
        ground
            .tins
            .iter()
            .map(|index| (index.chunk().key, index.chunk().content_hash()))
            .collect()
    }

    /// The highest mesh height at each world position, in whole
    /// millimetres across.
    type Tops = HashMap<(i64, i64), f32>;

    /// Each drawn chunk: its key, whether it is canonical, and the world
    /// position of each mesh vertex, rounded to whole millimetres across,
    /// with the highest height at each position (so skirts hanging below
    /// an edge are left out).
    fn drawn_edges(app: &mut App) -> Vec<(ChunkKey, bool, Tops)> {
        let mut query = app
            .world_mut()
            .query::<(&TerrainChunk, &Mesh3d, &Transform, Has<CanonicalChunk>)>();
        let meshes = app.world().resource::<Assets<Mesh>>();
        query
            .iter(app.world())
            .map(|(chunk, mesh, transform, canonical)| {
                let mesh = meshes.get(&mesh.0).unwrap();
                let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
                    mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    panic!("chunk meshes have positions");
                };
                let mut tops: HashMap<(i64, i64), f32> = HashMap::new();
                for p in positions {
                    let world = transform.translation.as_dvec3() + Vec3::from(*p).as_dvec3();
                    let at = (
                        (world.x * 1000.0).round() as i64,
                        (world.z * 1000.0).round() as i64,
                    );
                    let top = tops.entry(at).or_insert(f32::NEG_INFINITY);
                    *top = top.max(p[1]);
                }
                (chunk.key, canonical, tops)
            })
            .collect()
    }

    /// Checks that every two drawn chunks meeting along a side, one of
    /// them canonical, have the same vertices along it at the same
    /// heights. Returns how many seams between a canonical and a regular
    /// chunk it checked (each counted from both sides).
    fn assert_canonical_seams_close(app: &mut App) -> usize {
        let chunks = drawn_edges(app);
        let mut checked = 0;
        for (key, canonical, edges) in &chunks {
            for side in ChunkSide::ALL {
                for (other, other_canonical, other_edges) in &chunks {
                    if !(canonical | other_canonical) || !canonical::along_side(*key, side, *other)
                    {
                        continue;
                    }
                    // The shared stretch of the side, in millimetres.
                    let (lo, hi) = (key.min_corner(), key.max_corner());
                    let (a, b) = (other.min_corner(), other.max_corner());
                    let mm = |m: f64| (m * 1000.0).round() as i64;
                    let on = |p: &(i64, i64)| match side {
                        ChunkSide::MinX | ChunkSide::MaxX => {
                            let x = if side == ChunkSide::MinX { lo.x } else { hi.x };
                            p.0 == mm(x) && p.1 >= mm(lo.y.max(a.y)) && p.1 <= mm(hi.y.min(b.y))
                        }
                        ChunkSide::MinZ | ChunkSide::MaxZ => {
                            let z = if side == ChunkSide::MinZ { lo.y } else { hi.y };
                            p.1 == mm(z) && p.0 >= mm(lo.x.max(a.x)) && p.0 <= mm(hi.x.min(b.x))
                        }
                    };
                    let ours: BTreeMap<_, _> = edges.iter().filter(|(p, _)| on(p)).collect();
                    let theirs: BTreeMap<_, _> =
                        other_edges.iter().filter(|(p, _)| on(p)).collect();
                    assert_eq!(
                        ours.keys().collect::<Vec<_>>(),
                        theirs.keys().collect::<Vec<_>>(),
                        "{key} and {other} have different vertices along {side:?}"
                    );
                    for (p, y) in &ours {
                        assert_eq!(y, &theirs[p], "{key} and {other} stand apart at {p:?} mm");
                    }
                    // Count only the seams between a canonical and a
                    // regular chunk.
                    if canonical != other_canonical {
                        checked += 1;
                    }
                }
            }
        }
        checked
    }

    #[test]
    fn canonical_chunks_close_their_seams_and_keep_their_hashes() {
        let _streaming = streaming();
        let source = hills();
        let mut app = app();
        app.world_mut().spawn(TerrainStream {
            tin: true,
            ..TerrainStream::new(source.clone())
        });
        let camera = app
            .world_mut()
            .spawn((TerrainViewer, Transform::from_xyz(10.0, 30.0, 10.0)))
            .id();
        let progress = settle(&mut app, 3000);
        assert_eq!(progress.failed, 0);

        // Only finest chunks are canonical, and every one is loaded for
        // queries and hashes as it does when built on its own.
        let first = canonical_hashes(&mut app);
        assert!(!first.is_empty());
        let drawn = drawn_edges(&mut app);
        let canonical: Vec<ChunkKey> = drawn.iter().filter(|c| c.1).map(|c| c.0).collect();
        assert!(canonical.iter().all(|k| k.level == 0));
        assert_eq!(canonical.len(), first.len());
        assert!(drawn.iter().any(|c| !c.1), "regular chunks beside them");
        let built = |key: ChunkKey| {
            procgena_terrain::tin::build_chunk(&*source, key, &source.tin_tolerance())
                .unwrap()
                .content_hash()
        };
        for (&key, &hash) in &first {
            assert_eq!(hash, built(key), "{key}");
        }

        // No cracks: canonical chunks meet each other and regular chunks of
        // level 0 and 1 on the same vertices.
        assert!(assert_canonical_seams_close(&mut app) > 0);

        // Once settled, nothing is drawn again.
        let mut chunks = app
            .world_mut()
            .query_filtered::<Entity, With<TerrainChunk>>();
        let before: std::collections::BTreeSet<Entity> = chunks.iter(app.world()).collect();
        for _ in 0..10 {
            app.update();
        }
        let after: std::collections::BTreeSet<Entity> = chunks.iter(app.world()).collect();
        assert_eq!(before, after, "a settled stream redraws nothing");

        // Ground queries answer from the loaded chunks.
        let mut query = app.world_mut().query::<&CanonicalGround>();
        let ground = query.single(app.world()).unwrap().tins.clone();
        use procgena_terrain::tin::GroundSurface;
        let under = ground
            .sample(DVec2::new(10.0, 10.0))
            .expect("ground under the camera");
        assert_eq!(under.chunk, ChunkKey::new(0, 0, 0));
        assert!(
            (f64::from(
                source
                    .surface(DVec2::new(10.0, 10.0), FeatureLevel::FINEST)
                    .height
            ) - under.height)
                .abs()
                < 0.6
        );

        // Another camera, other levels and another quality: chunks still
        // loaded keep their hashes, and new ones hash as built alone.
        app.world_mut()
            .entity_mut(camera)
            .insert(Transform::from_xyz(150.0, 20.0, -40.0));
        app.world_mut().resource_mut::<TerrainSettings>().levels =
            ChunkDetail::DEFAULT.levels(400.0);
        app.world_mut().resource_mut::<LodQuality>().0 = procgena_core::Quality::Low;
        app.update();
        settle(&mut app, 3000);
        let second = canonical_hashes(&mut app);
        assert!(second.keys().any(|k| !first.contains_key(k)), "new chunks");
        for (&key, &hash) in &second {
            if let Some(&before) = first.get(&key) {
                assert_eq!(hash, before, "{key} changed");
            }
            assert_eq!(hash, built(key), "{key}");
        }
        assert!(assert_canonical_seams_close(&mut app) > 0);
    }

    #[test]
    fn without_the_switch_every_chunk_is_regular() {
        let _streaming = streaming();
        let mut app = app();
        app.world_mut().spawn(TerrainStream::new(hills()));
        app.world_mut()
            .spawn((TerrainViewer, Transform::from_xyz(0.0, 40.0, 0.0)));
        settle(&mut app, 2000);
        let mut canonical = app.world_mut().query::<&CanonicalChunk>();
        assert_eq!(canonical.iter(app.world()).count(), 0);
        let mut ground = app.world_mut().query::<&CanonicalGround>();
        assert_eq!(ground.iter(app.world()).count(), 0);
    }

    #[test]
    fn chunks_a_finer_level_leaves_alone_are_promoted_not_read_again() {
        use std::collections::BTreeMap;

        use procgena_terrain::{Elevation, WorldTerrain, fixtures};

        // Medium fixtures with rivers and roads switched on.
        let featured = |id: RegionId, _: DVec2, _: &Elevation| {
            let preset = if id.0.is_multiple_of(2) {
                fixtures::plains_medium()
            } else {
                fixtures::rolling_hills_medium()
            };
            let mut options = preset.terrain.clone();
            options.features.roads.enabled = true;
            options.features.rivers.density = 1.0;
            options
        };

        // A chunk's ground as text that tells apart any two bit patterns.
        let text = |read: &ChunkRead| {
            let Geometry::Regular(samples) = &read.geometry else {
                panic!("no canonical chunks without the switch");
            };
            let mut mesh = samples.mesh(Transitions::NONE, 0.0);
            mesh.level = read.level();
            format!("{mesh:?}")
        };

        // Each drawn chunk's ground once the stream settles, and how many
        // chunks stand for a finer feature level than they were read at.
        let settled = |coarse_first: bool| {
            let _streaming = streaming();
            let mut app = app();
            let world = WorldTerrain::new(fixtures::world_medium(), Arc::new(featured)).unwrap();
            let stream = app
                .world_mut()
                .spawn(TerrainStream {
                    coarse_first,
                    ..TerrainStream::world(world)
                })
                .id();
            app.world_mut()
                .spawn((TerrainViewer, Transform::from_xyz(0.0, 40.0, 0.0)));
            settle(&mut app, 20_000);
            let state = app.world().get::<StreamState>(stream).unwrap();
            let mut promoted = 0;
            let reads: BTreeMap<ChunkKey, String> = state
                .materials
                .iter()
                .map(|(&key, (read, _))| {
                    assert_eq!(read.level(), FeatureLevel::FINEST, "{key:?}");
                    promoted += usize::from(read.geometry.level() > read.level());
                    (key, text(read))
                })
                .collect();
            (reads, promoted)
        };

        // Read coarse first, some chunks are promoted; each is drawn exactly
        // as when every chunk is read once at its own level.
        let (coarse, promoted) = settled(true);
        let (direct, none) = settled(false);
        assert!(promoted > 0, "some chunks should be promoted");
        assert_eq!(none, 0);
        assert_eq!(coarse.len(), direct.len());
        for (key, read) in &coarse {
            assert!(read == &direct[key], "{key:?} differs");
        }

        // And each is exactly the chunk read on its own at its own level,
        // whatever order the stream read it in.
        let world = WorldTerrain::new(fixtures::world_medium(), Arc::new(featured)).unwrap();
        for (key, read) in &coarse {
            let alone = ChunkRead::read(&world, *key, FeatureLevel::of(*key)).unwrap();
            assert!(read == &text(&alone), "{key:?} differs from its read alone");
        }
    }

    #[test]
    fn canonical_chunks_known_unchanged_are_built_on_their_first_read() {
        use procgena_terrain::tin::build_chunk;
        use procgena_terrain::{Elevation, WorldTerrain, fixtures};

        // Medium fixtures with rivers and roads switched on, so some
        // chunks change at the fine tier and some don't.
        let featured = |_: RegionId, _: DVec2, _: &Elevation| {
            let mut options = fixtures::rolling_hills_medium().terrain.clone();
            options.features.roads.enabled = true;
            options.features.rivers.density = 1.0;
            options
        };
        let world = || WorldTerrain::new(fixtures::world_medium(), Arc::new(featured)).unwrap();

        // Settles a coarse-first canonical stream viewed from a point on
        // the border of the region at the origin, with that region (but not
        // its neighbour) built at every tier beforehand if `warm`: its
        // canonical chunks' hashes, and how many chunk reads it took.
        let settled = |warm: bool| {
            let _streaming = streaming();
            let mut app = app();
            let world = world();
            let origin = world.region_at(DVec2::ZERO);
            let border = &world.region(origin).edges[0].polyline;
            let on_border = border[border.len() / 2];
            let site = world.site(origin);
            let eye = on_border + (site - on_border).normalize() * 200.0;
            if warm {
                for tier in procgena_terrain::FeatureTier::ALL {
                    world.terrain_tier(origin, tier).unwrap();
                }
            }
            app.world_mut().spawn(TerrainStream {
                tin: true,
                ..TerrainStream::world(world)
            });
            app.world_mut().spawn((
                TerrainViewer,
                Transform::from_xyz(eye.x as f32, 40.0, eye.y as f32),
            ));
            let progress = settle(&mut app, 20_000);
            assert_eq!(progress.failed, 0);
            (canonical_hashes(&mut app), progress.sampled)
        };
        let (cold, cold_reads) = settled(false);
        let (warm, warm_reads) = settled(true);

        // With every tier kept, some finest chunks are canonical from
        // their first read and aren't read again; others still are.
        assert!(warm_reads < cold_reads, "{warm_reads} against {cold_reads}");
        let level0 = cold.len();
        assert!(warm_reads > cold_reads - level0, "some chunks can't");

        // Every canonical chunk is the same either way, and as built alone.
        assert_eq!(warm, cold);
        let alone = world();
        for (&key, &hash) in &warm {
            let built = build_chunk(&alone, key, &alone.tin_tolerance()).unwrap();
            assert_eq!(hash, built.content_hash(), "{key:?}");
        }
    }

    #[test]
    fn a_still_viewer_skips_choosing_chunks_and_a_moving_one_does_not() {
        let _streaming = streaming();
        let mut app = app();
        let stream = app.world_mut().spawn(TerrainStream::new(hills())).id();
        let viewer = app
            .world_mut()
            .spawn((TerrainViewer, Transform::from_xyz(0.0, 40.0, 0.0)))
            .id();
        settle(&mut app, 2000);
        fn state(app: &App, stream: Entity) -> &StreamState {
            app.world().get::<StreamState>(stream).unwrap()
        }

        // Settled and still: the last choice kept the target, so the next
        // frames don't choose again, and the target stays.
        let target = state(&app, stream).stream.target.clone();
        app.update();
        assert!(state(&app, stream).unchanged_from.is_some());
        assert!(state(&app, stream).stream.target == target);

        // Moved far: the chunks are chosen again around the new place.
        app.world_mut()
            .get_mut::<Transform>(viewer)
            .unwrap()
            .translation
            .x = 900.0;
        // The move reaches the viewer's global transform a frame later.
        for _ in 0..3 {
            app.update();
        }
        assert!(state(&app, stream).stream.target != target);
        settle(&mut app, 2000);

        // Skipping changes nothing: choosing afresh from the same inputs
        // gives the target the stream holds.
        let state = state(&app, stream);
        let (levels, factor, positions) = state.unchanged_from.clone().unwrap();
        let again = ChunkSelection::around(&levels, factor, &positions, Some(&state.stream.target))
            .unwrap();
        assert!(again == state.stream.target);
    }
}
