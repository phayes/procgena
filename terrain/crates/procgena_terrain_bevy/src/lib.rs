//! # procgena_terrain_bevy
//!
//! Draws [`procgena_terrain`] ground in Bevy: chunks of terrain streamed
//! around the camera, finer near it and coarser far away.
//!
//! - Add [`TerrainPlugin`].
//! - Spawn a [`TerrainStream`] with the ground to draw: a
//!   [`Terrain`](procgena_terrain::Terrain), or a
//!   [`WorldTerrain`] of many regions (see [`TerrainStream::world`]).
//! - Put [`TerrainViewer`] on each camera the terrain is drawn for.
//!
//! Every chunk, at every level, is drawn from its canonical mesh: the same
//! triangles whatever the distance or order it was built in. Chunks of the
//! same level meet exactly; where levels meet, skirts hang from each
//! chunk's sides to hide the step. Besides the chunks it draws, a stream
//! builds every chunk within its level's [`BuildRadii`] around the
//! viewers, the ground placement reads. All of it is built on one
//! background thread. Until a chunk is built, a coarser chunk holding it
//! is drawn in its place.
//!
//! Each drawn chunk is a child entity of its stream, with a
//! [`TerrainChunk`] component and its own [`TerrainMaterial`]. The ground
//! is drawn in plain grey: what it is made of isn't painted yet.
//! [`CanonicalGround`] on the stream answers heights from the drawn
//! chunks, and [`TerrainChanged`] says where they changed.
//!
//! Water is not drawn: each finest chunk with rivers or ponds gets a child
//! entity with [`TerrainWater`] and a mesh of the water surface, and no
//! material. Add your own water material to those entities.
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
mod levels;
mod material;
mod overlay;
mod progress;
mod reads;
mod worker;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::MaterialPlugin;
use bevy::prelude::*;
use glam::{DVec2, DVec3};
use procgena_core::chain::ViewScale;
use procgena_core_bevy::{LodPlugin, LodQuality};
use procgena_terrain::tin::{ChunkEdgeKey, ContractCache, ResidentTins};
use procgena_terrain::{
    ChunkDetail, ChunkKey, ChunkLevels, ChunkMesh, ChunkSelection, ChunkSelectionError,
    ChunkSource, RegionStage, WaterMesh, WorldTerrain,
};

pub use canonical::CanonicalGround;
use canonical::{ReadError, TinRead};
pub use levels::BuildRadii;
use levels::LevelStream;
pub use material::{GROUND_GREY, TerrainExtension, TerrainMaterial, TerrainSurface};
pub use overlay::TerrainOverlay;

pub use progress::RegionTracker;

/// The view distance of [`TerrainPlugin::default`], in metres.
pub const DEFAULT_VIEW_DISTANCE: f32 = 4000.0;

/// Adds terrain streaming and drawing to a Bevy app. It also adds the
/// shared [`LodPlugin`], whose [`LodQuality`] scales every chunk level's
/// distance, the view distance and the build radii.
///
/// Make one with [`detailed`](Self::detailed), whose chunk levels follow
/// the viewers' field of view, or [`new`](Self::new), with fixed chunk
/// levels, or use the default: [`ChunkDetail::DEFAULT`] drawn out to
/// [`DEFAULT_VIEW_DISTANCE`]. Each builds [`BuildRadii::default`] unless
/// changed with [`with_build_radii`](Self::with_build_radii).
#[derive(Debug, Clone)]
pub struct TerrainPlugin {
    levels: ChunkLevels,
    detail: Option<(ChunkDetail, f32)>,
    radii: BuildRadii,
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
            radii: BuildRadii::default(),
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

    /// The plugin building each chunk level out to `radii` around the
    /// viewers, besides the chunks it draws.
    pub fn with_build_radii(self, radii: BuildRadii) -> Self {
        Self { radii, ..self }
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
                radii: self.radii.clone(),
            })
            .init_resource::<TerrainDebug>()
            .add_message::<TerrainChanged>()
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
/// features come from. Implemented for every [`ChunkSource`] that can be
/// sent to another thread, such as a
/// [`Terrain`](procgena_terrain::Terrain) or a [`WorldTerrain`].
pub trait TerrainSource: ChunkSource + Send {}

impl<T: ChunkSource + Send + ?Sized> TerrainSource for T {}

/// The ground a terrain is drawn from.
///
/// Replacing its source builds every chunk again; the old chunks stay until
/// the new ones can replace them. To signal changed contents of a source,
/// replace its `Arc`.
///
/// Every chunk, at every level, is built from the source's finest ground
/// ([`FeatureLevel::FINEST`](procgena_terrain::FeatureLevel::FINEST)),
/// within a tolerance that grows with the
/// chunk's level.
#[derive(Component, Clone)]
#[require(Transform, Visibility, TerrainProgress)]
pub struct TerrainStream {
    /// Where the ground comes from.
    pub source: Arc<dyn TerrainSource>,
    /// Counts the source's region builds, so [`TerrainProgress`] can
    /// include them; `None` counts chunks only.
    pub regions: Option<RegionTracker>,
}

impl TerrainStream {
    /// A stream of `source`'s ground at every level, counting chunks only.
    pub fn new(source: Arc<dyn TerrainSource>) -> Self {
        Self {
            source,
            regions: None,
        }
    }

    /// A stream of `world`'s ground at every level, with its region builds
    /// counted in [`TerrainProgress`]. This replaces any progress hook
    /// `world` had; to keep your own as well, make a [`RegionTracker`],
    /// call its hook from yours, and set [`regions`](Self::regions)
    /// yourself.
    pub fn world(world: WorldTerrain) -> Self {
        let tracker = RegionTracker::default();
        let world = world.with_progress(tracker.hook());
        Self {
            source: Arc::new(world),
            regions: Some(tracker),
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
    /// Chunks wanted to draw that a coarser chunk is drawn in place of for
    /// now, until they and the chunks beside them are built.
    pub refining: usize,
    /// Chunks wanted to draw around the viewers now.
    pub wanted: usize,
    /// Chunks wanted to build around the viewers now, drawn or not: those
    /// to draw, and those within a build radius.
    pub building: usize,
    /// Chunks being built or waiting to be built.
    pub reading: usize,
    /// Chunks that could not be built since the stream last changed; they
    /// are left as holes.
    pub failed: usize,
    /// Whether the drawn chunks are the ones wanted, every chunk within a
    /// build radius is built, and nothing is being built.
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
}

/// Marks the water surface of one chunk, on a child entity of its
/// [`TerrainChunk`]. It has a mesh and no material: add your own.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct TerrainWater;

/// Sent when a stream's drawn ground changes: copies of Scatters resting on
/// the ground within these chunks' footprints should rest again, on
/// [`CanonicalGround`].
#[derive(Message, Debug, Clone, PartialEq)]
pub struct TerrainChanged {
    /// The stream's entity.
    pub stream: Entity,
    /// The chunks no longer drawn, or drawn now, each once.
    pub chunks: Vec<ChunkKey>,
}

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
    /// How far around the viewers each chunk level is built.
    radii: BuildRadii,
}

/// A stream's chunk builds on the terrain thread, each under its chunk and
/// the stream generation it was started in.
type Reads = reads::ReadQueue<(ChunkKey, u64), Result<TinRead, ReadError>>;

/// A stream's state: what is drawn, wanted and being built.
#[derive(Component, Default)]
struct StreamState {
    stream: LevelStream<TinRead>,
    /// Builds queued or running on the terrain thread, in order.
    reads: Reads,
    last: Option<TerrainStream>,
    /// The chunks to draw last chosen, for the next choice's hysteresis.
    selection: ChunkSelection,
    /// The chunks within a build radius last chosen, likewise.
    radius: BTreeSet<ChunkKey>,
    /// The entity drawing each chunk.
    entities: HashMap<ChunkKey, Entity>,
    /// Each drawn chunk's material, with the build it was made from, so a
    /// chunk drawn again from the same build keeps it.
    materials: HashMap<ChunkKey, (Arc<TinRead>, Handle<TerrainMaterial>)>,
    /// The skirt depth each drawn chunk is meshed with.
    skirts: HashMap<ChunkKey, f32>,
    /// Chunks that failed to build in this generation.
    failed: usize,
    /// Whether the chunk levels can't be chosen; logged once.
    broken: bool,
    /// The levels, distance factor and viewer positions the last choice
    /// was made from, when it kept the chunks wanted as they were. Choosing
    /// again from the same inputs keeps them again, so it is skipped.
    unchanged_from: Option<(ChunkLevels, f32, Vec<DVec3>)>,
    /// Edge contracts the chunks share, made when the first one is built
    /// and dropped when the stream starts over.
    contracts: Option<Arc<ContractCache>>,
}

/// The skirt hanging from each side of a chunk, at least: deep enough to
/// hide rounding where chunks meet. Where a chunk meets one of another
/// level, its skirt is deepened by the height step between them.
pub fn skirt_depth(key: ChunkKey) -> f32 {
    (4.0 * key.step() as f32).max(8.0)
}

/// The skirt chunk `key` needs beside the chunks drawn around it in
/// `stream` now: [`skirt_depth`], deepened by the largest height step to a
/// chunk of another level along a shared side.
fn skirt_beside(stream: &LevelStream<TinRead>, key: ChunkKey, read: &TinRead) -> f32 {
    let step = stream
        .touching(key)
        .into_iter()
        .filter(|other| other.level != key.level)
        .filter_map(|other| {
            let other_read = stream.drawn_of(other)?;
            Some(canonical::edge_gap(
                (key, read),
                (other, other_read.as_ref()),
            ))
        })
        .fold(0.0, f32::max);
    skirt_depth(key) + step
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
                let same_ground = state
                    .last
                    .as_ref()
                    .is_some_and(|last| Arc::ptr_eq(&last.source, &stream.source));
                if !same_ground {
                    state.stream.reset();
                    state.unchanged_from = None;
                    state.reads.cancel();
                    state.failed = 0;
                    state.contracts = None;
                }
                state.last = Some((*stream).clone());
            }
            Some(_) => {}
        }
    }
}

/// Chooses each stream's chunks around the viewers, builds them, and
/// changes the drawn chunks as builds come in.
#[expect(clippy::too_many_arguments, reason = "one Bevy system")]
fn stream_chunks(
    mut commands: Commands,
    settings: Res<TerrainSettings>,
    quality: Res<LodQuality>,
    debug: Res<TerrainDebug>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut changes: MessageWriter<TerrainChanged>,
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

        // The chunks wanted now: to draw, and within a build radius. Both
        // depend only on their inputs and the last choice, so when the last
        // choice kept them and its inputs haven't changed, it would keep
        // them again.
        let inputs = (settings.levels.clone(), factor, positions);
        if state.unchanged_from.as_ref() != Some(&inputs) {
            let (levels, factor, positions) = &inputs;
            match ChunkSelection::around(levels, *factor, positions, Some(&state.selection)) {
                Ok(selection) => {
                    let radius = settings.radii.around(*factor, &flat, &state.radius);
                    if selection == state.selection && radius == state.radius {
                        state.unchanged_from = Some(inputs);
                    } else {
                        state.stream.retarget(&selection, radius.clone());
                        state.selection = selection;
                        state.radius = radius;
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

        // Collect finished builds.
        let generation = state.stream.generation;
        for ((key, started_in), result) in state.reads.finished() {
            let built = match result {
                Ok(read) => Some(Arc::new(read)),
                Err(error) => {
                    if started_in == generation {
                        warn!("terrain {key} left out: {error}");
                        state.failed += 1;
                    }
                    None
                }
            };
            state.stream.finish(key, started_in, built);
        }

        // Queue the chunks still wanted, in the order wanted now; the
        // builds still waiting from last frame are chosen again with them.
        for (key, started_in) in state.reads.cancel() {
            if started_in == generation {
                state.stream.cancel(key);
            }
        }
        let wanted = state.stream.wanted(&flat);
        let contracts = state
            .contracts
            .get_or_insert_with(|| Arc::new(ContractCache::new(&source.source.tin_tolerance())))
            .clone();
        let mut jobs: Vec<((ChunkKey, u64), reads::Job<_>)> = Vec::with_capacity(wanted.len());
        for key in wanted {
            state.stream.start(key);
            let ground = Arc::clone(&source.source);
            let contracts = Arc::clone(&contracts);
            let job = Box::new(move || TinRead::build(&*ground, key, &contracts));
            jobs.push(((key, generation), job));
        }
        state.reads.push(jobs);

        // Change the drawn chunks as builds come in. `redraw` removes a
        // chunk's entity and draws its build, if any, with skirts `skirt`
        // metres deep.
        let mut redraw =
            |state: &mut StreamState, key: ChunkKey, read: Option<&Arc<TinRead>>, skirt| {
                if let Some(chunk) = state.entities.remove(&key) {
                    commands.entity(chunk).despawn();
                }
                let Some(read) = read else {
                    state.materials.remove(&key);
                    state.skirts.remove(&key);
                    return;
                };
                // Keep the material while the build is the same.
                let material = match state.materials.get(&key) {
                    Some((drawn, material)) if Arc::ptr_eq(drawn, read) => material.clone(),
                    _ => {
                        let material = chunk_material(key.level, &look, &mut materials);
                        state
                            .materials
                            .insert(key, (Arc::clone(read), material.clone()));
                        material
                    }
                };
                let chunk = spawn_chunk(
                    &mut commands,
                    &mut meshes,
                    &material,
                    entity,
                    &read.mesh(skirt),
                );
                state.entities.insert(key, chunk);
                state.skirts.insert(key, skirt);
            };
        let Some(swap) = state.stream.swap() else {
            update_progress(state, source, &mut progress);
            continue;
        };
        let mut changed: Vec<ChunkKey> = swap.remove.clone();
        changed.extend(swap.add.iter().map(|(key, _)| *key));
        changed.sort_unstable();
        changed.dedup();
        for &key in &swap.remove {
            redraw(state, key, None, 0.0);
        }

        // Chunks drawn now, and those beside them, may meet chunks of other
        // levels differently than when they were drawn: redraw those whose
        // skirts change.
        let mut near: Vec<ChunkKey> = Vec::new();
        for &key in &changed {
            if state.stream.drawn_of(key).is_some() {
                near.push(key);
            }
            near.extend(state.stream.touching(key));
        }
        near.sort_unstable();
        near.dedup();
        for key in near {
            let Some(read) = state.stream.drawn_of(key).cloned() else {
                continue;
            };
            let skirt = skirt_beside(&state.stream, key, &read);
            let fresh = !state.entities.contains_key(&key)
                || !state
                    .materials
                    .get(&key)
                    .is_some_and(|(drawn, _)| Arc::ptr_eq(drawn, &read));
            if fresh || state.skirts.get(&key) != Some(&skirt) {
                redraw(state, key, Some(&read), skirt);
            }
        }

        // The drawn chunks, for ground queries, and where they changed.
        let mut tins = ResidentTins::new();
        for (_, read) in state.stream.drawn() {
            if let Some(read) = read {
                tins.insert(read.index.clone());
            }
        }
        commands.entity(entity).insert(CanonicalGround { tins });
        // The shared edge contracts are kept only for built chunks' edges.
        if let Some(contracts) = &state.contracts {
            let edges: HashSet<ChunkEdgeKey> = state
                .stream
                .built()
                .flat_map(|(key, _)| ChunkEdgeKey::of_chunk(key))
                .collect();
            contracts.retain(|edge| edges.contains(&edge));
        }
        changes.write(TerrainChanged {
            stream: entity,
            chunks: changed,
        });
        update_progress(state, source, &mut progress);
    }
}

/// Sets `progress` from `state`: region work from the source's hook, and
/// chunk builds, by their usual cost.
fn update_progress(state: &StreamState, source: &TerrainStream, progress: &mut TerrainProgress) {
    let settled = state.stream.is_settled();
    let regions = source
        .regions
        .as_ref()
        .map(|r| r.counts())
        .unwrap_or_default();
    let (built, building) = state.stream.builds();
    let total = regions.total + building as f32 * progress::CHUNK_COST;
    let done = regions.done + built as f32 * progress::CHUNK_COST;
    let fraction = if settled || total == 0.0 {
        1.0
    } else {
        (done / total).clamp(0.0, 1.0)
    };
    *progress = TerrainProgress {
        fraction,
        stage: regions.stage(),
        drawn: state.entities.len(),
        refining: state.stream.standing_in(),
        wanted: state.stream.target_len(),
        building,
        reading: state.stream.reading(),
        failed: state.failed,
        settled,
    };
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
            TerrainChunk { key: mesh.key },
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
    use std::collections::{BTreeMap, HashSet};
    use std::time::Duration;

    use bevy::asset::AssetPlugin;
    use bevy::time::TimeUpdateStrategy;
    use procgena_terrain::tin::{GroundSurface, build_chunk};
    use procgena_terrain::{
        Boundary, ChunkDetail, FeatureLevel, Region, RegionId, TerrainGenerator, TerrainPreset,
        chunk_levels,
    };

    use super::*;

    /// Streaming tests share the terrain thread, so they run one at a time
    /// to keep within their frame budgets.
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

    /// An app with the plugin and no renderer, drawing to 300 m and building
    /// level 0 to 60 m and level 1 to 200 m.
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
        let plugin = TerrainPlugin::new(coarse.levels(300.0))
            .unwrap()
            .with_build_radii(BuildRadii {
                radii: vec![60.0, 200.0],
            });
        // Only the streaming, not the material plugin, which needs a
        // renderer.
        app.insert_resource(TerrainSettings {
            levels: plugin.levels.clone(),
            detail: plugin.detail,
            radii: plugin.radii.clone(),
        })
        .init_resource::<LodQuality>()
        .init_resource::<TerrainDebug>()
        .add_message::<TerrainChanged>()
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

    /// The state of `stream`.
    fn state(app: &App, stream: Entity) -> &StreamState {
        app.world().get::<StreamState>(stream).unwrap()
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
            radii: plugin.radii.clone(),
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

        let progress = settle(&mut app, 3000);
        assert_eq!(progress.failed, 0);
        assert_eq!(progress.drawn, progress.wanted);
        assert_eq!(progress.refining, 0);
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
        assert!(drawn.iter().any(|c| c.key.level == 1));

        // Built besides those drawn: level-1 chunks within 200 m, under the
        // level-0 chunks drawn there.
        assert!(progress.building > progress.wanted);
        let built: Vec<ChunkKey> = state(&app, stream).stream.built().map(|(k, _)| k).collect();
        assert_eq!(built.len(), progress.building);
        assert!(
            built
                .iter()
                .any(|k| k.level == 1 && !drawn.iter().any(|c| c.key == *k))
        );

        // Move the viewer: the chunks follow, and none is drawn twice.
        app.world_mut()
            .entity_mut(camera)
            .insert(Transform::from_xyz(600.0, 40.0, 0.0));
        // The camera's global transform follows at the end of this frame;
        // the chunks follow it from the next.
        app.update();
        let progress = settle(&mut app, 3000);
        let keys: Vec<ChunkKey> = chunks.iter(app.world()).map(|(c, _)| c.key).collect();
        assert_eq!(keys.len(), progress.drawn);
        let under = ChunkKey::containing(DVec2::new(600.0, 0.0), 0);
        assert!(keys.contains(&under), "the chunk under the viewer is drawn");
        let unique: HashSet<_> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len());

        // Each drawn chunk has its own flat grey material; those of chunks
        // no longer drawn are freed.
        for _ in 0..3 {
            app.update();
        }
        let mut drawn = app
            .world_mut()
            .query::<(&TerrainChunk, &MeshMaterial3d<TerrainMaterial>)>();
        let handles: HashSet<AssetId<TerrainMaterial>> =
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

        // Coarse first: chunks were drawn in place of finer ones while
        // those were still being built.
        assert!(seen.iter().any(|p| p.drawn > 0 && p.refining > 0));
        assert!(seen.iter().all(|p| (0.0..=1.0).contains(&p.fraction)));
    }

    #[test]
    fn a_changed_stream_replaces_its_chunks() {
        let _streaming = streaming();
        let mut app = app();
        let stream = app.world_mut().spawn(TerrainStream::new(hills())).id();
        app.world_mut()
            .spawn((TerrainViewer, Transform::from_xyz(0.0, 40.0, 0.0)));
        let before = settle(&mut app, 3000);
        let mut chunks = app.world_mut().query::<(Entity, &TerrainChunk)>();
        let old: Vec<Entity> = chunks.iter(app.world()).map(|(e, _)| e).collect();

        // New ground: every chunk is replaced once the new ones can be
        // drawn, never side by side with the old.
        app.world_mut()
            .entity_mut(stream)
            .insert(TerrainStream::new(hills()));
        let mut mixed = false;
        for _ in 0..3000 {
            app.update();
            std::thread::sleep(Duration::from_millis(5));
            let entities: Vec<Entity> = chunks.iter(app.world()).map(|(e, _)| e).collect();
            let some_old = entities.iter().any(|e| old.contains(e));
            let some_new = entities.iter().any(|e| !old.contains(e));
            mixed |= some_old && some_new;
            let progress = *app
                .world_mut()
                .query::<&TerrainProgress>()
                .single(app.world())
                .unwrap();
            if progress.settled && !some_old {
                break;
            }
        }
        assert!(!mixed, "old and new ground were drawn side by side");
        let after = settle(&mut app, 3000);
        assert_eq!(after.drawn, before.drawn);
        for (entity, _) in chunks.iter(app.world()) {
            assert!(!old.contains(&entity));
        }
    }

    /// The content hash of each chunk drawn now, from [`CanonicalGround`].
    fn drawn_hashes(app: &mut App) -> BTreeMap<ChunkKey, u64> {
        let mut query = app.world_mut().query::<&CanonicalGround>();
        let ground = query.single(app.world()).unwrap();
        ground
            .tins
            .iter()
            .map(|index| (index.chunk().key, index.chunk().content_hash()))
            .collect()
    }

    /// The highest and lowest mesh height at each world position, in whole
    /// millimetres across: a chunk's surface, and its skirts' feet.
    type Heights = HashMap<(i64, i64), (f32, f32)>;

    /// Each drawn chunk's key and [`Heights`].
    fn drawn_meshes(app: &mut App) -> Vec<(ChunkKey, Heights)> {
        let mut query = app
            .world_mut()
            .query::<(&TerrainChunk, &Mesh3d, &Transform)>();
        let meshes = app.world().resource::<Assets<Mesh>>();
        query
            .iter(app.world())
            .map(|(chunk, mesh, transform)| {
                let mesh = meshes.get(&mesh.0).unwrap();
                let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
                    mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    panic!("chunk meshes have positions");
                };
                let mut heights: Heights = HashMap::new();
                for p in positions {
                    let world = transform.translation.as_dvec3() + Vec3::from(*p).as_dvec3();
                    let at = (
                        (world.x * 1000.0).round() as i64,
                        (world.z * 1000.0).round() as i64,
                    );
                    let y = world.y as f32;
                    let h = heights
                        .entry(at)
                        .or_insert((f32::NEG_INFINITY, f32::INFINITY));
                    *h = (h.0.max(y), h.1.min(y));
                }
                (chunk.key, heights)
            })
            .collect()
    }

    #[test]
    fn every_chunk_is_its_canonical_mesh_and_levels_meet_behind_skirts() {
        let _streaming = streaming();
        let source = hills();
        let mut app = app();
        app.world_mut().spawn(TerrainStream::new(source.clone()));
        let camera = app
            .world_mut()
            .spawn((TerrainViewer, Transform::from_xyz(10.0, 30.0, 10.0)))
            .id();
        let progress = settle(&mut app, 3000);
        assert_eq!(progress.failed, 0);

        // Every drawn chunk, at every level, is loaded for queries and
        // hashes as it does when built on its own.
        let first = drawn_hashes(&mut app);
        assert_eq!(first.len(), progress.drawn);
        assert!(first.keys().any(|k| k.level > 0));
        let built = |key: ChunkKey| {
            build_chunk(&*source, key, &source.tin_tolerance())
                .unwrap()
                .content_hash()
        };
        for (&key, &hash) in &first {
            assert_eq!(hash, built(key), "{key}");
        }

        // Where two drawn chunks meet along a side, chunks of one level have
        // the same vertices there at the same heights, and chunks of
        // different levels hang skirts reaching below each other's edge.
        let meshes = drawn_meshes(&mut app);
        let mut across_levels = 0;
        for (key, ours) in &meshes {
            for (other, theirs) in &meshes {
                if key == other {
                    continue;
                }
                let lo = key.min_corner().max(other.min_corner());
                let hi = key.max_corner().min(other.max_corner());
                let span = hi - lo;
                if !(span.x == 0.0 && span.y > 0.0 || span.y == 0.0 && span.x > 0.0) {
                    continue;
                }
                let mm = |m: f64| (m * 1000.0).round() as i64;
                let on = |p: &(i64, i64)| {
                    p.0 >= mm(lo.x) && p.0 <= mm(hi.x) && p.1 >= mm(lo.y) && p.1 <= mm(hi.y)
                };
                if key.level == other.level {
                    for (p, (top, _)) in ours.iter().filter(|(p, _)| on(p)) {
                        let theirs = theirs.get(p).map(|h| h.0);
                        assert_eq!(Some(*top), theirs, "{key} and {other} at {p:?} mm");
                    }
                } else {
                    let foot = theirs
                        .iter()
                        .filter(|(p, _)| on(p))
                        .map(|(_, h)| h.1)
                        .fold(f32::INFINITY, f32::min);
                    for (_, (top, _)) in ours.iter().filter(|(p, _)| on(p)) {
                        assert!(foot <= *top, "{other}'s skirt stops above {key}'s edge");
                    }
                    across_levels += 1;
                }
            }
        }
        assert!(
            across_levels > 0,
            "some chunks meet a chunk of another level"
        );

        // Once settled, nothing is drawn again.
        let mut chunks = app
            .world_mut()
            .query_filtered::<Entity, With<TerrainChunk>>();
        let before: BTreeSet<Entity> = chunks.iter(app.world()).collect();
        for _ in 0..10 {
            app.update();
        }
        let after: BTreeSet<Entity> = chunks.iter(app.world()).collect();
        assert_eq!(before, after, "a settled stream redraws nothing");

        // Ground queries answer from the drawn chunks, at every level.
        let mut query = app.world_mut().query::<&CanonicalGround>();
        let ground = query.single(app.world()).unwrap().tins.clone();
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
        let far = *first.keys().max_by_key(|k| k.level).unwrap();
        let inside = far.min_corner() + DVec2::splat(1.0);
        assert_eq!(ground.sample(inside).map(|s| s.chunk), Some(far));

        // Another camera, other levels and another quality: chunks still
        // drawn keep their hashes, and new ones hash as built alone.
        app.world_mut()
            .entity_mut(camera)
            .insert(Transform::from_xyz(150.0, 20.0, -40.0));
        app.world_mut().resource_mut::<TerrainSettings>().levels =
            ChunkDetail::DEFAULT.levels(400.0);
        app.world_mut().resource_mut::<LodQuality>().0 = procgena_core::Quality::Low;
        app.update();
        settle(&mut app, 3000);
        let second = drawn_hashes(&mut app);
        assert!(second.keys().any(|k| !first.contains_key(k)), "new chunks");
        for (&key, &hash) in &second {
            if let Some(&before) = first.get(&key) {
                assert_eq!(hash, before, "{key} changed");
            }
            assert_eq!(hash, built(key), "{key}");
        }
    }

    #[test]
    fn changed_ground_is_reported_where_it_changed() {
        let _streaming = streaming();
        let mut app = app();
        let stream = app.world_mut().spawn(TerrainStream::new(hills())).id();
        app.world_mut()
            .spawn((TerrainViewer, Transform::from_xyz(0.0, 40.0, 0.0)));
        let mut reported: BTreeSet<ChunkKey> = BTreeSet::new();
        for _ in 0..3000 {
            app.update();
            std::thread::sleep(Duration::from_millis(5));
            let messages = app.world().resource::<Messages<TerrainChanged>>();
            for message in messages.iter_current_update_messages() {
                assert_eq!(message.stream, stream);
                reported.extend(message.chunks.iter().copied());
            }
            let progress = *app
                .world_mut()
                .query::<&TerrainProgress>()
                .single(app.world())
                .unwrap();
            if progress.settled {
                break;
            }
        }
        // Every chunk drawn now, and every stand-in it replaced, was
        // reported.
        let mut chunks = app.world_mut().query::<&TerrainChunk>();
        let drawn: Vec<ChunkKey> = chunks.iter(app.world()).map(|c| c.key).collect();
        for key in &drawn {
            assert!(reported.contains(key), "{key}");
        }
        assert!(reported.len() > drawn.len(), "stand-ins were replaced");
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
        settle(&mut app, 3000);

        // Settled and still: the last choice kept the chunks wanted, so the
        // next frames don't choose again, and they stay.
        let selection = state(&app, stream).selection.clone();
        app.update();
        assert!(state(&app, stream).unchanged_from.is_some());
        assert!(state(&app, stream).selection == selection);

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
        assert!(state(&app, stream).selection != selection);
        settle(&mut app, 3000);

        // Skipping changes nothing: choosing afresh from the same inputs
        // gives the chunks the stream holds.
        let state = state(&app, stream);
        let (levels, factor, positions) = state.unchanged_from.clone().unwrap();
        let again =
            ChunkSelection::around(&levels, factor, &positions, Some(&state.selection)).unwrap();
        assert!(again == state.selection);
        let flat: Vec<DVec2> = positions.iter().map(|p| DVec2::new(p.x, p.z)).collect();
        let radius =
            app.world()
                .resource::<TerrainSettings>()
                .radii
                .around(factor, &flat, &state.radius);
        assert!(radius == state.radius);
    }
}
