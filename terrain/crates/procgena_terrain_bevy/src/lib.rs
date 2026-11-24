//! # procgena_terrain_bevy
//!
//! Draws [`procgena_terrain`] ground in Bevy: chunks of terrain streamed
//! around the camera, finer near it and coarser far away, joined without
//! cracks.
//!
//! - Add [`TerrainPlugin`].
//! - Spawn a [`TerrainStream`] with the ground to draw: a
//!   [`Terrain`](procgena_terrain::Terrain), or a
//!   [`WorldTerrain`](procgena_terrain::WorldTerrain) of many regions.
//! - Put [`TerrainViewer`] on each camera the terrain is drawn for.
//!
//! Chunks are read on Bevy's async compute threads, and the drawn chunks
//! change all at once, when every chunk of the new set is ready. Each
//! chunk is a child entity of its stream, with a [`TerrainChunk`]
//! component and a [`TerrainMaterial`] coloured by slope and height.
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
//!     commands.spawn(TerrainStream::new(Arc::new(world)));
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
use procgena_terrain::{
    ChunkError, ChunkKey, ChunkMesh, ChunkSamples, ChunkSelection, ChunkSource, ChunkStage,
    Transitions, WaterMesh, default_chunk_lod,
};

pub use material::{TerrainExtension, TerrainMaterial, TerrainSurface};

use stream::{Read, Stream};

/// The view distance of [`TerrainPlugin::default`], in metres.
pub const DEFAULT_VIEW_DISTANCE: f32 = 4000.0;

/// Adds terrain streaming and drawing to a Bevy app. It also adds the
/// shared [`LodPlugin`], whose [`LodQuality`] scales every chunk level's
/// distance and the view distance.
///
/// # Panics
///
/// Adding the plugin panics if [`lod`](Self::lod) can't choose chunks (see
/// [`ChunkSelection::around`]).
#[derive(Debug, Clone)]
pub struct TerrainPlugin {
    /// The chunk levels: level `i` of the `Lod` is chunk level `i`, and
    /// its cull distance is the view distance. Defaults to
    /// [`default_chunk_lod`] drawn out to [`DEFAULT_VIEW_DISTANCE`].
    pub lod: Lod,
    /// How the ground is coloured.
    pub surface: TerrainSurface,
    /// How many chunks each stream reads at once.
    pub max_reads: usize,
}

impl Default for TerrainPlugin {
    fn default() -> Self {
        Self {
            lod: default_chunk_lod().with_cull(DEFAULT_VIEW_DISTANCE),
            surface: TerrainSurface::default(),
            max_reads: 4,
        }
    }
}

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        if let Err(error) = ChunkSelection::around(&self.lod, 1.0, &[], None) {
            panic!("TerrainPlugin::lod can't choose chunks: {error}");
        }
        embedded_asset!(app, "shaders/terrain_fragment.wesl");
        app.add_plugins((LodPlugin, MaterialPlugin::<TerrainMaterial>::default()))
            .insert_resource(TerrainSettings {
                lod: self.lod.clone(),
                surface: self.surface,
                max_reads: self.max_reads.max(1),
            })
            .init_resource::<TerrainDebug>()
            .add_systems(Startup, create_materials)
            .add_systems(
                Update,
                (reset_streams, stream_chunks, update_materials).chain(),
            );
    }
}

/// The ground a terrain is drawn from, and how.
///
/// Changing it (or inserting a new one) reads every chunk again; the
/// chunks drawn before stay until the new ones are ready.
#[derive(Component, Clone)]
#[require(Transform, Visibility, TerrainProgress)]
pub struct TerrainStream {
    /// Where the ground comes from.
    pub source: Arc<dyn ChunkSource + Send>,
    /// Chunk levels at and above this one are drawn from
    /// [`Provisional`](ChunkStage::Provisional) ground, which a
    /// [`WorldTerrain`](procgena_terrain::WorldTerrain) builds without
    /// routing roads, so far ground appears sooner. `None` draws canonical
    /// ground everywhere.
    pub provisional_from: Option<u8>,
}

impl TerrainStream {
    /// A stream of `source`'s canonical ground at every level.
    pub fn new(source: Arc<dyn ChunkSource + Send>) -> Self {
        Self {
            source,
            provisional_from: None,
        }
    }

    /// The stage chunks at `level` are read at.
    pub fn stage(&self, level: u8) -> ChunkStage {
        match self.provisional_from {
            Some(from) if level >= from => ChunkStage::Provisional,
            _ => ChunkStage::Canonical,
        }
    }
}

/// How far a [`TerrainStream`] has got. Updated every frame.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TerrainProgress {
    /// Chunks drawn.
    pub drawn: usize,
    /// Chunks wanted around the viewers now.
    pub wanted: usize,
    /// Chunks being read.
    pub reading: usize,
    /// Chunks that could not be read since the stream last changed; they
    /// are left as holes.
    pub failed: usize,
    /// Whether the drawn chunks are the ones wanted and nothing is being
    /// read.
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
    /// The stage of the ground it was read from.
    pub stage: ChunkStage,
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
    surface: TerrainSurface,
    max_reads: usize,
}

/// One material per chunk level.
#[derive(Resource)]
struct TerrainMaterials(Vec<Handle<TerrainMaterial>>);

/// Reading one chunk's samples in the background.
type ReadTask = Task<Result<ChunkSamples, ChunkError>>;

/// A stream's state: what is drawn, wanted and being read.
#[derive(Component, Default)]
struct StreamState {
    stream: Stream,
    /// Reads in flight: the chunk, its generation and the task.
    tasks: Vec<(ChunkKey, u64, ReadTask)>,
    /// The entity drawing each chunk.
    entities: HashMap<ChunkKey, Entity>,
    /// Chunks that failed to read in this generation.
    failed: usize,
    /// Whether the chunk levels can't be chosen; logged once.
    broken: bool,
}

/// The skirt hanging from each side of a chunk, in metres: deep enough to
/// hide where a provisional chunk meets a canonical one, or rounding
/// where chunks meet.
pub fn skirt_depth(key: ChunkKey) -> f32 {
    (4.0 * key.step() as f32).max(8.0)
}

/// A Bevy mesh of a chunk's ground.
pub fn chunk_mesh(chunk: &ChunkMesh) -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, chunk.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, chunk.normals.clone())
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

/// Makes one material per chunk level.
fn create_materials(
    mut commands: Commands,
    settings: Res<TerrainSettings>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
) {
    let handles = (0..settings.lod.levels.len())
        .map(|_| {
            materials.add(TerrainMaterial {
                base: StandardMaterial {
                    perceptual_roughness: 0.95,
                    reflectance: 0.2,
                    ..default()
                },
                extension: TerrainExtension {
                    surface: settings.surface,
                },
            })
        })
        .collect();
    commands.insert_resource(TerrainMaterials(handles));
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
            }
            Some(_) => {}
        }
    }
}

/// Chooses each stream's chunks around the viewers, reads them, and swaps
/// the drawn chunks once the new set is ready.
fn stream_chunks(
    mut commands: Commands,
    settings: Res<TerrainSettings>,
    quality: Res<LodQuality>,
    materials: Option<Res<TerrainMaterials>>,
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
    let Some(materials) = materials else { return };
    let factor = quality.0.distance_scale();
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
            let read: Read = match result {
                Ok(samples) => Some(Arc::new(samples)),
                Err(error) => {
                    if generation == state.stream.generation {
                        warn!("terrain {key} left out: {error}");
                        state.failed += 1;
                    }
                    None
                }
            };
            state.stream.finish(key, generation, read);
        }

        // Start reading the nearest missing chunks.
        let free = settings.max_reads.saturating_sub(state.stream.reading());
        for key in state.stream.missing(&flat).into_iter().take(free) {
            state.stream.start(key);
            let ground = Arc::clone(&source.source);
            let stage = source.stage(key.level);
            let task = AsyncComputeTaskPool::get()
                .spawn(async move { ChunkSamples::sample(&*ground, key, stage) });
            state.tasks.push((key, state.stream.generation, task));
        }

        // Swap the drawn chunks once the whole new set is read.
        if let Some(swap) = state.stream.swap() {
            for key in swap.remove {
                if let Some(chunk) = state.entities.remove(&key) {
                    commands.entity(chunk).despawn();
                }
            }
            for (key, transitions, read) in swap.add {
                let Some(samples) = read else { continue };
                let mesh = samples.mesh(transitions, skirt_depth(key));
                let material = &materials.0[usize::from(key.level).min(materials.0.len() - 1)];
                let origin = mesh.translation().as_vec3();
                let chunk = commands
                    .spawn((
                        TerrainChunk {
                            key,
                            transitions,
                            stage: mesh.stage,
                        },
                        Mesh3d(meshes.add(chunk_mesh(&mesh))),
                        MeshMaterial3d(material.clone()),
                        Transform::from_translation(origin),
                        ChildOf(entity),
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
                state.entities.insert(key, chunk);
            }
        }

        *progress = TerrainProgress {
            drawn: state.entities.len(),
            wanted: state.stream.target.len(),
            reading: state.stream.reading(),
            failed: state.failed,
            settled: state.stream.is_settled(),
        };
    }
}

/// Keeps the materials' fade and debug drawing up to date.
fn update_materials(
    settings: Res<TerrainSettings>,
    quality: Res<LodQuality>,
    debug: Res<TerrainDebug>,
    handles: Option<Res<TerrainMaterials>>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
) {
    let Some(handles) = handles else { return };
    if !(quality.is_changed() || debug.is_changed() || handles.is_added()) {
        return;
    }
    let ranges = settings.lod.ranges(quality.0.distance_scale());
    let fade = ranges.last().map_or(Vec4::ZERO, |last| {
        Vec4::new(0.0, 0.0, last.fade_out.start, last.fade_out.end)
    });
    for (level, handle) in handles.0.iter().enumerate() {
        let Some(mut material) = materials.get_mut(handle) else {
            continue;
        };
        let surface = &mut material.extension.surface;
        surface.fade = fade;
        surface.tint = if debug.levels {
            material::level_tint(level as u8).extend(0.55)
        } else {
            Vec4::ZERO
        };
        surface.border = if debug.borders {
            ChunkKey::size_at(level as u8) as f32
        } else {
            0.0
        };
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
    fn hills() -> Arc<dyn ChunkSource + Send> {
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
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            16,
        )));
        let plugin = TerrainPlugin {
            lod: Lod::new(
                64.0,
                vec![
                    procgena_core::LodLevel::new(0.0, 0.1, ()),
                    procgena_core::LodLevel::new(96.0, 0.1, ()),
                    procgena_core::LodLevel::new(192.0, 0.1, ()),
                ],
            )
            .with_cull(300.0),
            ..TerrainPlugin::default()
        };
        // Only the streaming, not the material plugin, which needs a
        // renderer.
        app.insert_resource(TerrainSettings {
            lod: plugin.lod.clone(),
            surface: plugin.surface,
            max_reads: plugin.max_reads,
        })
        .init_resource::<LodQuality>()
        .init_resource::<TerrainDebug>()
        .add_systems(Startup, create_materials)
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
