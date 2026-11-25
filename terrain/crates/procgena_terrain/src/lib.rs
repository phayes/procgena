//! # procgena_terrain
//!
//! Procedural terrain for regions of a larger world. Each region gets one
//! terrain type (plains, hills, mountains and so on), and neighbouring
//! regions join without seams, whatever types they are.
//!
//! This is an early version. It makes ground heights, joins regions, and
//! cuts in rivers, ponds and roads, including rivers and roads that cross
//! from one region into the next. It meshes the ground in chunks with
//! levels of detail, for drawing (see [`ChunkSamples`] and
//! [`ChunkSelection`]). Labels, lakes and caves come later (see
//! `terrain/plan.md` in the repository). Rivers and roads are planned only
//! for regions with a polygon boundary.
//!
//! ## Rivers and roads across borders
//!
//! An offer also lists the terrain's own rivers that reach the border, and
//! where it would like roads to cross. Negotiation turns these into agreed
//! crossings ([`Port`]) only where they work for both sides: a river must
//! flow downhill into gently falling ground, and a road must be able to
//! reach both sides' own ground within its grade. Both terrains carve the
//! same short piece of river or road across the border at each crossing. A
//! river flowing in ends in a pond or joins one of the receiving terrain's
//! rivers.
//!
//! ## How regions join
//!
//! 1. Neighbouring terrains agree what their ground should be like where
//!    they meet, and how steep their roads may be: each states its
//!    [`relief_amplitude`](TerrainGenerator::relief_amplitude),
//!    [`relief_target`] picks one amplitude per edge and corner,
//!    [`blend_weights`] how much of each terrain's ground to mix there,
//!    and the road grades are the steepest of the terrains meeting there
//!    ([`RoadGrades::steepest`]).
//!    [`with_edge_targets`](TerrainGenerator::with_edge_targets) makes
//!    each terrain fade towards all of these near its borders, as the
//!    world's [`Convergence`] says. Terrains with the same seed share their
//!    noise, so alike terrains then nearly match already.
//! 2. Each region's [`TerrainGenerator`] makes an [`EdgeOffer`] for every
//!    border it shares, and a [`CornerOffer`] for every corner. An offer
//!    depends only on the terrain making it.
//! 3. [`negotiate_corner`] and [`negotiate`] turn the offers into agreements.
//!    They give the same result whichever side calls them.
//! 4. [`TerrainGenerator::build`] binds the generator to its region. The
//!    resulting [`Terrain`] bends itself near its borders to meet the
//!    agreements exactly, so heights and slopes match across every border.
//!
//! [`WorldTerrain`] does all four steps for an endless world of regions,
//! building each region's terrain when it is first asked for.
//! [`StitchedWorld`] does them for regions given as polygons:
//!
//! ```
//! use glam::DVec2;
//! use procgena_terrain::{
//!     Convergence, Elevation, HeightField, RegionId, RegionSpec, StitchedWorld, TerrainPreset,
//! };
//!
//! let west = vec![DVec2::new(-500.0, -500.0), DVec2::new(0.0, -500.0), DVec2::new(0.0, 500.0), DVec2::new(-500.0, 500.0)];
//! let east = vec![DVec2::new(0.0, -500.0), DVec2::new(500.0, -500.0), DVec2::new(500.0, 500.0), DVec2::new(0.0, 500.0)];
//! let world = StitchedWorld::build(vec![
//!     RegionSpec { id: RegionId(1), polygon: west, options: TerrainPreset::plains().terrain.clone() },
//!     RegionSpec { id: RegionId(2), polygon: east, options: TerrainPreset::mountains().terrain.clone() },
//! ], &Convergence::default(), &Elevation::flat())?;
//!
//! // Both sides meet at the same height on their shared border.
//! let on_border = DVec2::new(0.0, 120.0);
//! let [plains, mountains] = [&world.terrains()[0], &world.terrains()[1]];
//! assert!((plains.height(on_border) - mountains.height(on_border)).abs() < 1e-3);
//! assert!(world.height(DVec2::new(250.0, 0.0)).is_finite());
//! # Ok::<(), procgena_terrain::StitchError>(())
//! ```
//!
//! ## Units and coordinates
//!
//! Lengths and heights are in metres. The ground lies in the x–z plane with
//! y up. World positions are `f64`, so the terrain stays exact far from the
//! origin. [`HeightTile`] samples a grid of heights on the world lattice, so
//! separately sampled tiles agree exactly where they share points.
//!
//! The default `rayon` feature samples tiles on several threads; the results
//! are the same without it.

mod border;
mod calibration;
mod carve;
mod chunk;
mod converge;
// Measurements only, for now: see the module's own documentation.
#[cfg(test)]
mod drainage;
mod elevation;
mod feature;
mod graph;
mod grid;
pub mod ground;
mod gully;
mod hash;
mod measure;
mod noise;
mod options;
mod plan;
mod ports;
mod presets;
mod region;
mod relief;
mod river;
mod road;
mod seam;
mod stitch;
mod terrain;
mod warp;
mod world;

pub use carve::{FeatureKind, FeatureLine, PoiKind, PointOfInterest, Pond, RoadTags};
pub use chunk::{
    CHUNK_CELLS, CHUNK_SIZE, ChunkCorner, ChunkError, ChunkKey, ChunkMesh, ChunkSamples,
    ChunkSelection, ChunkSelectionError, ChunkSide, ChunkSource, MAX_CHUNK_LEVEL, SurfacePoint,
    Transitions, WaterMesh, chunk_lod,
};
pub use converge::{
    BorderTarget, Convergence, DEFAULT_RELIEF_BLEND, DEFAULT_RELIEF_MEETING, EdgeTarget,
    blend_weights, relief_meeting_is_valid, relief_target,
};
pub use elevation::{Elevation, ElevationError, ElevationOptions, HeightBump};
pub use feature::{FeatureId, FeatureShape, FeatureTier, TerrainFeature};
pub use graph::{
    MAX_JITTER, MAX_SIZE, MIN_EDGE, RegionGraph, RegionGraphError, RegionGraphOptions, RegionKey,
};
pub use hash::region_local_seed;
pub use measure::{CURVATURE_STEP, MEASURE_REACH, Measures, SampleDetail, WATER_HEIGHT_REACH};
pub use noise::MAX_OCTAVES;
pub use options::{
    BaseOptions, EdgeOptions, FeatureOptions, MAX_EROSION, MAX_HEIGHT, MAX_MARGIN,
    MAX_RIVER_DENSITY, MAX_WAVELENGTH, MountainLayer, NoiseLayer, OptionsError, RiverOptions,
    RoadGrades, RoadOptions, TerrainOptions,
};
pub use ports::{
    FEATURE_CLEARANCE, INNER_DEPTHS, MAX_PORT_WIDTH, Port, PortKind, PortWish, Side, inner_depth,
};
pub use presets::TerrainPreset;
/// The text of `terrain.meta.yml`: each Terrain setting's label,
/// description and range, for tools that edit Terrain presets. Read it with
/// [`KindMeta::parse`](procgena_core::meta::KindMeta::parse).
pub const TERRAIN_META: &str = include_str!("../presets/terrain.meta.yml");
pub use procgena_core::preset::PresetFileError;
pub use region::{
    Boundary, CornerId, EdgeId, Region, RegionCorner, RegionEdge, RegionError, RegionId, corner_id,
};
pub use seam::{
    CornerAgreement, CornerOffer, EDGE_STEP, EdgeAgreement, EdgeOffer, EdgeSample, SeamError,
    negotiate, negotiate_corner, sample_count,
};
pub use stitch::{RegionSpec, StitchError, StitchedWorld};
pub use terrain::{
    HeightField, HeightTile, RiverTrace, Terrain, TerrainError, TerrainGenerator, TerrainSample,
};
pub use warp::{BorderWarp, BorderWarpError, MIN_WARP_WAVELENGTH, warp_borders};
pub use world::{
    DEFAULT_CACHE, ProgressHook, RegionProgress, RegionStage, TerrainSource, WorldError,
    WorldTerrain, WorldTerrainOptions,
};
