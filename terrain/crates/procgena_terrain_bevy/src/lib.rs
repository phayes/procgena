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
//! component and its own [`TerrainMaterial`], coloured by the stream's
//! [`GroundPainter`]: bare terrain's look
//! ([`GroundStack::bare`](procgena_terrain::ground::GroundStack::bare)) unless
//! you give it another.
//!
//! [`TerrainMaterial`] can also draw ground weights over any other mesh,
//! such as a patch under an Ecotope: add [`TerrainMaterialPlugin`] and see
//! [`TerrainExtension::new`].
//!
//! Water is not drawn: each chunk with rivers or ponds gets a child entity
//! with [`TerrainWater`] and a mesh of the water surface, and no material.
//! Add your own water material to those entities.
//!
//! [`TerrainDebug`] tints each chunk by its level and draws chunk borders.
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

mod material;
mod progress;
mod stream;

use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::MaterialPlugin;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use glam::{DVec2, DVec3};
use procgena_core::Lod;
use procgena_core_bevy::{LodPlugin, LodQuality};
use procgena_terrain::ground::{GroundPainter, GroundSource, GroundStack, GroundWeights};
use procgena_terrain::{
    ChunkError, ChunkKey, ChunkMesh, ChunkSamples, ChunkSelection, ChunkSelectionError,
    ChunkSource, FeatureTier, RegionStage, Transitions, WaterMesh, WorldTerrain, chunk_lod,
};

pub use material::{DEFAULT_VARIATION, TerrainExtension, TerrainMaterial, TerrainSurface};

use material::weight_images;
pub use progress::RegionTracker;

use stream::{Read, Stream};

/// The view distance of [`TerrainPlugin::default`], in metres.
pub const DEFAULT_VIEW_DISTANCE: f32 = 4000.0;

/// Adds terrain streaming and drawing to a Bevy app. It also adds the
/// shared [`LodPlugin`], whose [`LodQuality`] scales every chunk level's
/// distance and the view distance.
///
/// Make one with [`new`](Self::new), which checks the chunk levels, or use
/// the default: [`chunk_lod`] drawn out to [`DEFAULT_VIEW_DISTANCE`].
#[derive(Debug, Clone)]
pub struct TerrainPlugin {
    lod: Lod,
    variation: f32,
    max_reads: usize,
}

impl Default for TerrainPlugin {
    fn default() -> Self {
        Self::new(chunk_lod(DEFAULT_VIEW_DISTANCE)).expect("the usual chunk levels are valid")
    }
}

impl TerrainPlugin {
    /// The plugin drawing chunk levels `lod`: level `i` of the `Lod` is
    /// chunk level `i`, and its cull distance is the view distance.
    /// [`chunk_lod`] gives the usual levels for any view distance.
    ///
    /// # Errors
    ///
    /// If `lod` can't choose chunks, for example because it has no cull
    /// distance or its last level starts too close to it (see
    /// [`ChunkSelection::around`]).
    pub fn new(lod: Lod) -> Result<Self, ChunkSelectionError> {
        ChunkSelection::around(&lod, 1.0, &[], None)?;
        Ok(Self {
            lod,
            variation: DEFAULT_VARIATION,
            max_reads: 4,
        })
    }

    /// The plugin, varying the ground's colours in patches by `variation`,
    /// from 0 (none) to 1 (see [`TerrainSurface::variation`]).
    pub fn with_variation(mut self, variation: f32) -> Self {
        self.variation = variation;
        self
    }

    /// The plugin, reading at most `max_reads` chunks at once per stream
    /// (at least one).
    pub fn with_max_reads(mut self, max_reads: usize) -> Self {
        self.max_reads = max_reads.max(1);
        self
    }

    /// The chunk levels.
    pub fn lod(&self) -> &Lod {
        &self.lod
    }
}

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<TerrainMaterialPlugin>() {
            app.add_plugins(TerrainMaterialPlugin);
        }
        app.add_plugins(LodPlugin)
            .insert_resource(TerrainSettings {
                lod: self.lod.clone(),
                variation: self.variation,
                max_reads: self.max_reads,
            })
            .init_resource::<TerrainDebug>()
            .add_systems(
                Update,
                (reset_streams, stream_chunks, update_materials).chain(),
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
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default());
    }
}

/// Ground a [`TerrainStream`] can draw: where its chunks' heights and
/// water come from ([`ChunkSource`]), and the sites its ground is painted
/// from ([`GroundSource`]). Every type with both is one, such as a
/// [`Terrain`](procgena_terrain::Terrain) or a [`WorldTerrain`].
pub trait TerrainSource: ChunkSource + GroundSource + Send {}

impl<T: ChunkSource + GroundSource + Send + ?Sized> TerrainSource for T {}

/// The ground a terrain is drawn from, and how.
///
/// Changing it (or inserting a new one) reads every chunk again; the
/// chunks drawn before stay until the new ones are ready.
#[derive(Component, Clone)]
#[require(Transform, Visibility, TerrainProgress)]
pub struct TerrainStream {
    /// Where the ground comes from.
    pub source: Arc<dyn TerrainSource>,
    /// What the ground is made of where: each chunk is painted with it
    /// when read, at the tier it is read at.
    pub ground: Arc<dyn GroundPainter + Send>,
    /// Chunk levels at and above this one are drawn only at the
    /// [`Rough`](FeatureTier::Rough) tier, which a [`WorldTerrain`] builds
    /// without its costly features (today the roads), so far ground
    /// appears sooner. `None` draws the fine tier everywhere.
    pub rough_from: Option<u8>,
    /// Counts the source's region builds, so [`TerrainProgress`] can
    /// include them; `None` counts chunks only.
    pub regions: Option<RegionTracker>,
    /// Draw every chunk at the roughest tier first, then read each chunk
    /// that wants a later tier (see [`tier`](Self::tier)) again in the
    /// background and redraw it when ready. The view appears much sooner;
    /// near chunks briefly lack their fine features. `false` waits for each
    /// chunk's own tier before drawing anything.
    pub coarse_first: bool,
}

impl TerrainStream {
    /// A stream of `source`'s fine ground at every level, loaded coarse
    /// first, counting chunks only, with bare terrain's look seeded 0 at a
    /// vertical scale of 1.
    pub fn new(source: Arc<dyn TerrainSource>) -> Self {
        Self {
            source,
            ground: Arc::new(GroundStack::bare(0, 1.0)),
            rough_from: None,
            regions: None,
            coarse_first: true,
        }
    }

    /// A stream of `world`'s fine ground at every level, loaded
    /// coarse first, with its
    /// region builds counted in [`TerrainProgress`], and bare terrain's
    /// look seeded with the world's seed at a vertical scale of 1. This
    /// replaces any progress hook `world` had; to keep your own as well,
    /// make a [`RegionTracker`], call its hook from yours, and set
    /// [`regions`](Self::regions) yourself.
    pub fn world(world: WorldTerrain) -> Self {
        let tracker = RegionTracker::default();
        let world = world.with_progress(tracker.hook());
        Self {
            ground: Arc::new(GroundStack::bare(world.options().seed, 1.0)),
            source: Arc::new(world),
            rough_from: None,
            regions: Some(tracker),
            coarse_first: true,
        }
    }

    /// The tier chunks at `level` end up drawn at.
    pub fn tier(&self, level: u8) -> FeatureTier {
        match self.rough_from {
            Some(from) if level >= from => FeatureTier::Rough,
            _ => FeatureTier::Fine,
        }
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
    /// Drawn chunks still at an earlier tier than they want, waiting to be
    /// read again (see [`TerrainStream::coarse_first`]).
    pub refining: usize,
    /// Chunks wanted around the viewers now.
    pub wanted: usize,
    /// Chunks being read.
    pub reading: usize,
    /// Chunks that could not be read since the stream last changed; they
    /// are left as holes.
    pub failed: usize,
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
    /// The tier of the ground it was read from.
    pub tier: FeatureTier,
}

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
    lod: Lod,
    variation: f32,
    max_reads: usize,
}

/// One chunk as read: its samples, and its ground ready to draw.
pub(crate) struct ChunkRead {
    pub(crate) samples: ChunkSamples,
    /// The two weight images.
    images: [Image; 2],
    /// The palette and texel count; the drawing settings are filled in
    /// when the chunk is drawn.
    surface: TerrainSurface,
}

impl ChunkRead {
    /// Reads chunk `key` of `source` at `tier`, and paints its ground with
    /// `painter`.
    pub(crate) fn read(
        source: &(impl TerrainSource + ?Sized),
        painter: &(impl GroundPainter + ?Sized),
        key: ChunkKey,
        tier: FeatureTier,
    ) -> Result<Self, ChunkError> {
        let samples = ChunkSamples::sample(source, key, tier)?;
        let weights = GroundWeights::chunk(painter, source, key, tier)?;
        Ok(Self {
            samples,
            images: weight_images(&weights),
            surface: TerrainSurface::new(&weights, painter.catalogue()),
        })
    }
}

/// Reading one chunk in the background.
type ReadTask = Task<Result<ChunkRead, ChunkError>>;

/// A stream's state: what is drawn, wanted and being read.
#[derive(Component, Default)]
struct StreamState {
    stream: Stream,
    /// Reads in flight: the chunk, its generation and the task.
    tasks: Vec<(ChunkKey, u64, ReadTask)>,
    /// The entity drawing each chunk.
    entities: HashMap<ChunkKey, Entity>,
    /// Each drawn chunk's material, with the read it was made from, so a
    /// chunk drawn again from the same read keeps it.
    materials: HashMap<ChunkKey, (Arc<ChunkRead>, Handle<TerrainMaterial>)>,
    /// Chunks that failed to read in this generation.
    failed: usize,
    /// Whether the chunk levels can't be chosen; logged once.
    broken: bool,
    /// Skirts deeper than [`skirt_depth`], for chunks beside a chunk of
    /// another tier: how far that chunk's tier moved the ground.
    skirts: HashMap<ChunkKey, f32>,
}

/// The skirt hanging from each side of a chunk, in metres: deep enough to
/// hide small gaps where chunks of different tiers meet, or rounding where
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

/// The drawing settings every chunk shares: the colour variation, the fade
/// at the end of the view, and the debug tint and borders.
struct ChunkLook {
    variation: f32,
    fade: Vec4,
    debug: TerrainDebug,
}

impl ChunkLook {
    fn new(settings: &TerrainSettings, quality: &LodQuality, debug: TerrainDebug) -> Self {
        let ranges = settings.lod.ranges(quality.0.distance_scale());
        let fade = ranges.last().map_or(Vec4::ZERO, |last| {
            Vec4::new(0.0, 0.0, last.fade_out.start, last.fade_out.end)
        });
        Self {
            variation: settings.variation,
            fade,
            debug,
        }
    }

    /// Sets `surface`'s drawing settings for a chunk at `level`.
    fn apply(&self, surface: &mut TerrainSurface, level: u8) {
        surface.variation = self.variation;
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

/// A chunk's material, drawing `read`'s ground at `level` with `look`.
fn chunk_material(
    read: &ChunkRead,
    level: u8,
    look: &ChunkLook,
    images: &mut Assets<Image>,
    materials: &mut Assets<TerrainMaterial>,
) -> Handle<TerrainMaterial> {
    let [low, high] = read.images.clone();
    let mut surface = read.surface;
    look.apply(&mut surface, level);
    materials.add(TerrainMaterial {
        base: StandardMaterial {
            reflectance: 0.2,
            ..default()
        },
        extension: TerrainExtension {
            surface,
            weights_low: images.add(low),
            weights_high: images.add(high),
        },
    })
}

/// Gives new streams their state, and starts changed ones over.
fn reset_streams(
    mut commands: Commands,
    mut streams: Query<(Entity, Ref<TerrainStream>, Option<&mut StreamState>)>,
) {
    for (entity, stream, state) in &mut streams {
        match state {
            None => {
                commands.entity(entity).insert(StreamState::default());
            }
            Some(mut state) if stream.is_changed() && !stream.is_added() => {
                state.stream.reset();
                state.tasks.clear();
                state.failed = 0;
                state.skirts.clear();
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
    mut images: ResMut<Assets<Image>>,
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

        // The chunks wanted now.
        match ChunkSelection::around(
            &settings.lod,
            factor,
            &positions,
            Some(&state.stream.target),
        ) {
            Ok(target) => {
                if target != state.stream.target {
                    state.stream.retarget(target);
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

        // Collect finished reads.
        let mut finished = Vec::new();
        state
            .tasks
            .retain_mut(|(key, generation, task)| match block_on(poll_once(task)) {
                Some(result) => {
                    finished.push((*key, *generation, result));
                    false
                }
                None => true,
            });
        for (key, generation, result) in finished {
            let current = generation == state.stream.generation;
            let read: Read = match result {
                Ok(read) => Some(Arc::new(read)),
                // A failed upgrade keeps the ground already drawn.
                Err(error) if current && state.stream.is_upgrade(key) => {
                    warn!("terrain {key} keeps its earlier tier: {error}");
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

        // Start reading the nearest missing chunks: at the roughest tier
        // first when the stream loads coarse first. Only when none is
        // missing, read drawn chunks again at the tier they want with the
        // reads left over.
        let mut free = settings.max_reads.saturating_sub(state.stream.reading());
        let missing = state.stream.missing(&flat);
        let wanted = |key: ChunkKey| source.tier(key.level);
        let upgrades = state.stream.upgrades(&flat, wanted);
        let mut reads: Vec<(ChunkKey, FeatureTier)> = Vec::new();
        for &key in missing.iter().take(free) {
            let tier = if source.coarse_first {
                FeatureTier::ALL[0]
            } else {
                wanted(key)
            };
            reads.push((key, tier));
        }
        free -= reads.len();
        if missing.is_empty() {
            reads.extend(upgrades.iter().take(free).map(|&k| (k, wanted(k))));
        }
        for (key, tier) in reads {
            state.stream.start(key);
            let ground = Arc::clone(&source.source);
            let painter = Arc::clone(&source.ground);
            let task = AsyncComputeTaskPool::get()
                .spawn(async move { ChunkRead::read(&*ground, &*painter, key, tier) });
            state.tasks.push((key, state.stream.generation, task));
        }

        // Swap the drawn chunks once the whole new set is read; redraw
        // upgraded chunks one by one. `redraw` removes a chunk's entity and
        // draws its samples, if any.
        let mut redraw = |state: &mut StreamState, key: ChunkKey, transitions, read: Read| {
            if let Some(chunk) = state.entities.remove(&key) {
                commands.entity(chunk).despawn();
            }
            let Some(read) = read else {
                state.materials.remove(&key);
                return;
            };
            // Keep the material while the read is the same.
            let material = match state.materials.get(&key) {
                Some((drawn, material)) if Arc::ptr_eq(drawn, &read) => material.clone(),
                _ => {
                    let material =
                        chunk_material(&read, key.level, &look, &mut images, &mut materials);
                    state
                        .materials
                        .insert(key, (Arc::clone(&read), material.clone()));
                    material
                }
            };
            let skirt = skirt_depth(key).max(state.skirts.get(&key).copied().unwrap_or(0.0));
            let chunk = spawn_chunk(
                &mut commands,
                &mut meshes,
                &material,
                entity,
                &read.samples.mesh(transitions, skirt),
            );
            state.entities.insert(key, chunk);
        };
        // Every chunk drawn now, whether new to the view or read again at a
        // later tier. Chunks touching at different tiers can stand apart
        // by up to how far the later tier moved the ground: deepen both
        // chunks' skirts to cover it, redrawing touching chunks whose
        // skirts grow.
        let mut drawn_now: Vec<(ChunkKey, Transitions, Read)> = Vec::new();
        if let Some(swap) = state.stream.swap() {
            for key in swap.remove {
                state.skirts.remove(&key);
                redraw(state, key, Transitions::NONE, None);
            }
            drawn_now.extend(swap.add);
        }
        drawn_now.extend(state.stream.take_refreshed());
        for (key, transitions, read) in drawn_now {
            let tier = read.as_ref().map(|r| r.samples.tier());
            let mut deepest = 0.0f32;
            for (other, other_transitions) in state.stream.touching(key) {
                let (Some(tier), Some(other_tier)) = (tier, state.stream.tier_of(other)) else {
                    continue;
                };
                if tier == other_tier {
                    continue;
                }
                // The ground of the later-tier chunk, against the earlier
                // tier.
                let later = if tier > other_tier { key } else { other };
                let moved = source.source.tier_difference(
                    later.min_corner(),
                    later.max_corner(),
                    tier.min(other_tier),
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

        // Region work from the hook, and chunk reads, by their usual cost;
        // chunks waiting for a later tier count as not read.
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
            settled,
        };
    }
}

/// Spawns `mesh` as a chunk of `stream`, with its water as a child, and
/// returns the chunk entity.
fn spawn_chunk(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: &Handle<TerrainMaterial>,
    stream: Entity,
    mesh: &ChunkMesh,
) -> Entity {
    let chunk = commands
        .spawn((
            TerrainChunk {
                key: mesh.key,
                transitions: mesh.transitions,
                tier: mesh.tier,
            },
            Mesh3d(meshes.add(chunk_mesh(mesh))),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(mesh.translation().as_vec3()),
            ChildOf(stream),
        ))
        .id();
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
    use procgena_terrain::{Boundary, Region, RegionId, TerrainGenerator, TerrainPreset};

    use super::*;

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
        let plugin = TerrainPlugin::new(chunk_lod(300.0)).unwrap();
        // Only the streaming, not the material plugin, which needs a
        // renderer.
        app.insert_resource(TerrainSettings {
            lod: plugin.lod.clone(),
            variation: plugin.variation,
            max_reads: plugin.max_reads,
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
        let lod = TerrainPlugin::default().lod().clone();
        assert_eq!(lod.cull_distance, Some(DEFAULT_VIEW_DISTANCE));
        // A view distance inside the last level's switching band.
        let mut short = chunk_lod(6000.0);
        short.cull_distance = Some(4000.0);
        assert!(TerrainPlugin::new(short).is_err());
        let mut endless = chunk_lod(6000.0);
        endless.cull_distance = None;
        assert!(TerrainPlugin::new(endless).is_err());
        assert!(TerrainPlugin::new(chunk_lod(4000.0)).is_ok());
    }

    #[test]
    fn chunks_stream_in_around_the_viewer_and_follow_it() {
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

        // Each drawn chunk has its own material and two weight images;
        // those of chunks no longer drawn are freed.
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
        assert_eq!(
            app.world().resource::<Assets<Image>>().len(),
            2 * progress.drawn
        );
        for (_, material) in materials.iter() {
            assert!(material.extension.surface.palette[0].w > 0.0, "a palette");
        }
    }

    #[test]
    fn progress_counts_region_builds_until_the_terrain_settles() {
        use procgena_terrain::{
            Elevation, MIN_EDGE, RegionGraphOptions, WorldTerrain, WorldTerrainOptions,
        };

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

        // Coarse first: chunks were drawn rough while the fine tier was
        // still being built, and all end fine.
        assert!(seen.iter().any(|p| p.drawn > 0 && p.refining > 0));
        let mut chunks = app.world_mut().query::<&TerrainChunk>();
        assert!(
            chunks
                .iter(app.world())
                .all(|c| c.tier == FeatureTier::Fine)
        );
        assert!(seen.iter().all(|p| (0.0..=1.0).contains(&p.fraction)));
    }

    #[test]
    fn a_changed_stream_replaces_its_chunks() {
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
}
