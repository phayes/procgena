//! Chunk meshes: the ground drawn as triangle meshes on one chunk grid
//! shared by every level of detail.
//!
//! The world is cut into square chunks. A chunk at level 0 is
//! [`CHUNK_SIZE`] metres across, and each level up doubles that, so a chunk
//! at level `L` covers four chunks at level `L − 1`. Every chunk has
//! [`CHUNK_CELLS`] cells a side whatever its level, so a far chunk covers
//! more ground with the same number of triangles.
//!
//! Building a chunk takes two steps:
//!
//! 1. [`ChunkSamples::sample`] reads the ground from a [`ChunkSource`]
//!    (a [`Terrain`](crate::Terrain) or a [`WorldTerrain`](crate::WorldTerrain)).
//!    This is the slow step.
//! 2. [`ChunkSamples::mesh`] turns the samples into a [`ChunkMesh`] that
//!    fits its neighbours, as given by its [`Transitions`]. It is cheap, so
//!    when a neighbour changes level, only this step runs again.
//!
//! [`ChunkSelection::around`] picks which chunks to draw around one or more
//! viewers, and each chunk's [`Transitions`].
//!
//! Chunk meshes are for drawing only. Placement, masks and queries read
//! the terrain itself ([`Ground`](procgena_core::Ground),
//! [`Terrain::query`](crate::Terrain::query)), never a mesh.
//!
//! ```
//! use glam::DVec3;
//! use procgena_terrain::{
//!     Boundary, ChunkSamples, ChunkSelection, FeatureTier, Region, RegionId, TerrainGenerator,
//!     TerrainPreset, chunk_levels,
//! };
//!
//! // One unbounded region of rolling hills.
//! let region = Region { id: RegionId(0), boundary: Boundary::Unbounded, edges: vec![], corners: vec![] };
//! let terrain = TerrainGenerator::new(TerrainPreset::rolling_hills().terrain.clone())?
//!     .build(region, &[])?;
//!
//! // The chunks to draw for a viewer standing 2 m above the ground.
//! let levels = chunk_levels(600.0);
//! let chosen = ChunkSelection::around(&levels, 1.0, &[DVec3::new(0.0, 2.0, 0.0)], None)?;
//! let (key, transitions) = chosen.iter().next().unwrap();
//!
//! let samples = ChunkSamples::sample(&terrain, key, FeatureTier::Fine)?;
//! let mesh = samples.mesh(transitions, 0.0);
//! assert!(!mesh.indices.is_empty());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use glam::{DVec2, DVec3, Vec3};
use procgena_core::chain::ViewScale;

use crate::{
    CURVATURE_STEP, ChunkLevels, ChunkLevelsError, FeatureId, FeatureLine, FeatureShape,
    FeatureTier, MEASURE_REACH, Pond, RegionId, SampleDetail, Terrain, TerrainFeature,
    WorldTerrain,
};

/// Width in metres of a chunk at level 0. Each level up doubles it.
pub const CHUNK_SIZE: f64 = 64.0;

/// Cells along each side of a chunk, at every level.
pub const CHUNK_CELLS: u32 = 64;

/// The coarsest chunk level: chunks up to about 4,200 km across.
pub const MAX_CHUNK_LEVEL: u8 = 16;

/// `CHUNK_CELLS` as a signed index.
const N: i64 = CHUNK_CELLS as i64;

/// One chunk of the shared grid: its position in chunks of its level, and
/// its level.
///
/// Chunk `(x, z)` at level `L` covers world x from `x · size` to
/// `(x + 1) · size`, and the same along z, where `size` is
/// [`CHUNK_SIZE`] · 2^`L`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkKey {
    /// Position along x, in chunks of this level.
    pub x: i64,
    /// Position along z, in chunks of this level.
    pub z: i64,
    /// The level: 0 is the finest. At most [`MAX_CHUNK_LEVEL`].
    pub level: u8,
}

impl ChunkKey {
    /// Chunk `(x, z)` at `level`.
    pub fn new(x: i64, z: i64, level: u8) -> Self {
        Self { x, z, level }
    }

    /// The chunk at `level` holding world position `p` (x and z, metres).
    pub fn containing(p: DVec2, level: u8) -> Self {
        let size = Self::size_at(level);
        Self::new(
            (p.x / size).floor() as i64,
            (p.y / size).floor() as i64,
            level,
        )
    }

    /// The width in metres of a chunk at `level`.
    pub fn size_at(level: u8) -> f64 {
        CHUNK_SIZE * f64::from(1u32 << level)
    }

    /// The chunk's width in metres.
    pub fn size(self) -> f64 {
        Self::size_at(self.level)
    }

    /// The distance in metres between neighbouring vertices of the chunk's
    /// mesh: 1 m at level 0, doubling each level.
    pub fn step(self) -> f64 {
        self.size() / f64::from(CHUNK_CELLS)
    }

    /// The corner with the smallest x and z, in world metres.
    pub fn min_corner(self) -> DVec2 {
        DVec2::new(self.x as f64, self.z as f64) * self.size()
    }

    /// The corner with the largest x and z, in world metres.
    pub fn max_corner(self) -> DVec2 {
        DVec2::new((self.x + 1) as f64, (self.z + 1) as f64) * self.size()
    }

    /// The chunk one level coarser that holds this one, or `None` at
    /// [`MAX_CHUNK_LEVEL`].
    pub fn parent(self) -> Option<Self> {
        (self.level < MAX_CHUNK_LEVEL)
            .then(|| Self::new(self.x.div_euclid(2), self.z.div_euclid(2), self.level + 1))
    }

    /// The four chunks one level finer that make up this one, or `None` at
    /// level 0.
    pub fn children(self) -> Option<[Self; 4]> {
        let level = self.level.checked_sub(1)?;
        let (x, z) = (self.x * 2, self.z * 2);
        Some([
            Self::new(x, z, level),
            Self::new(x + 1, z, level),
            Self::new(x, z + 1, level),
            Self::new(x + 1, z + 1, level),
        ])
    }

    /// The chunk of the same level `dx` chunks along x and `dz` along z.
    pub fn offset(self, dx: i64, dz: i64) -> Self {
        Self::new(self.x + dx, self.z + dz, self.level)
    }

    /// Whether this chunk lies inside `other` or is `other`.
    pub fn is_within(self, other: Self) -> bool {
        if self.level > other.level {
            return false;
        }
        let shift = other.level - self.level;
        self.x >> shift == other.x && self.z >> shift == other.z
    }

    /// The horizontal distance in metres from `p` to the nearest point of
    /// the chunk; 0 inside it.
    pub fn distance_to(self, p: DVec2) -> f64 {
        let nearest = p.clamp(self.min_corner(), self.max_corner());
        nearest.distance(p)
    }

    /// World position of the point `(u, v)` half steps from the chunk's
    /// smallest corner. Computed from integers, so every chunk that shares
    /// a point gives exactly the same position.
    pub(crate) fn half_point(self, u: i64, v: i64) -> DVec2 {
        // A half step is 2^(level − 1) metres; at level 0, 0.5 m.
        let half = 0.5 * f64::from(1u32 << self.level);
        DVec2::new(
            (self.x * 2 * N + u) as f64 * half,
            (self.z * 2 * N + v) as f64 * half,
        )
    }
}

impl fmt::Display for ChunkKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "chunk ({}, {}) at level {}", self.x, self.z, self.level)
    }
}

/// One side of a chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChunkSide {
    /// The side at the chunk's smallest x.
    MinX,
    /// The side at the chunk's largest x.
    MaxX,
    /// The side at the chunk's smallest z.
    MinZ,
    /// The side at the chunk's largest z.
    MaxZ,
}

impl ChunkSide {
    /// All four sides.
    pub const ALL: [ChunkSide; 4] = [Self::MinX, Self::MaxX, Self::MinZ, Self::MaxZ];

    /// Which way the neighbour on this side lies, in chunks.
    pub fn direction(self) -> (i64, i64) {
        match self {
            Self::MinX => (-1, 0),
            Self::MaxX => (1, 0),
            Self::MinZ => (0, -1),
            Self::MaxZ => (0, 1),
        }
    }
}

/// One corner of a chunk, named by the sides that meet there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChunkCorner {
    /// Smallest x, smallest z.
    MinXMinZ,
    /// Largest x, smallest z.
    MaxXMinZ,
    /// Smallest x, largest z.
    MinXMaxZ,
    /// Largest x, largest z.
    MaxXMaxZ,
}

impl ChunkCorner {
    /// All four corners.
    pub const ALL: [ChunkCorner; 4] = [
        Self::MinXMinZ,
        Self::MaxXMinZ,
        Self::MinXMaxZ,
        Self::MaxXMaxZ,
    ];

    /// Which way the chunk diagonally across this corner lies, in chunks.
    pub fn direction(self) -> (i64, i64) {
        match self {
            Self::MinXMinZ => (-1, -1),
            Self::MaxXMinZ => (1, -1),
            Self::MinXMaxZ => (-1, 1),
            Self::MaxXMaxZ => (1, 1),
        }
    }

    /// The two sides that meet at this corner, the x side first.
    pub fn sides(self) -> [ChunkSide; 2] {
        match self {
            Self::MinXMinZ => [ChunkSide::MinX, ChunkSide::MinZ],
            Self::MaxXMinZ => [ChunkSide::MaxX, ChunkSide::MinZ],
            Self::MinXMaxZ => [ChunkSide::MinX, ChunkSide::MaxZ],
            Self::MaxXMaxZ => [ChunkSide::MaxX, ChunkSide::MaxZ],
        }
    }
}

/// Which sides and corners of a chunk touch a chunk one level finer.
///
/// A chunk's mesh must know this to join its finer neighbours without
/// cracks: along such a side, the coarser chunk takes the finer chunk's
/// extra vertices into its own edge. Neighbours are never more than one
/// level apart (see [`ChunkSelection`]). The default is none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Transitions(u8);

impl Transitions {
    /// No finer neighbours.
    pub const NONE: Self = Self(0);

    /// Every side and corner touching a finer chunk.
    pub const ALL: Self = Self(0xff);

    /// The bit for `side`.
    fn side_bit(side: ChunkSide) -> u8 {
        match side {
            ChunkSide::MinX => 1,
            ChunkSide::MaxX => 2,
            ChunkSide::MinZ => 4,
            ChunkSide::MaxZ => 8,
        }
    }

    /// The bit for `corner`.
    fn corner_bit(corner: ChunkCorner) -> u8 {
        match corner {
            ChunkCorner::MinXMinZ => 16,
            ChunkCorner::MaxXMinZ => 32,
            ChunkCorner::MinXMaxZ => 64,
            ChunkCorner::MaxXMaxZ => 128,
        }
    }

    /// These transitions with `side` touching a finer chunk.
    pub fn with_side(self, side: ChunkSide) -> Self {
        Self(self.0 | Self::side_bit(side))
    }

    /// These transitions with the chunk diagonally across `corner` finer.
    pub fn with_corner(self, corner: ChunkCorner) -> Self {
        Self(self.0 | Self::corner_bit(corner))
    }

    /// Whether the neighbour on `side` is finer.
    pub fn side(self, side: ChunkSide) -> bool {
        self.0 & Self::side_bit(side) != 0
    }

    /// Whether the chunk diagonally across `corner` is finer.
    pub fn corner(self, corner: ChunkCorner) -> bool {
        self.0 & Self::corner_bit(corner) != 0
    }

    /// Whether any of the three other chunks at `corner` is finer.
    pub fn finer_at(self, corner: ChunkCorner) -> bool {
        let [a, b] = corner.sides();
        self.corner(corner) || self.side(a) || self.side(b)
    }
}

/// The ground at one point, as a chunk reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfacePoint {
    /// Ground height in metres. NaN where there is no ground.
    pub height: f32,
    /// The water surface height in metres, if a river or pond covers the
    /// point.
    pub water: Option<f32>,
}

/// A river, pond or road as a [`ChunkSource`] gives it (see
/// [`ChunkSource::lines_within`]): its id and its whole shape.
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkFeature {
    /// Which feature it is.
    pub id: FeatureId,
    /// Its whole shape, not only the part inside the box asked about.
    pub shape: ChunkFeatureShape,
}

/// The shape of a [`ChunkFeature`].
#[derive(Debug, Clone, PartialEq)]
pub enum ChunkFeatureShape {
    /// A river or road.
    Line(FeatureLine),
    /// A pond.
    Pond(Pond),
}

impl ChunkFeature {
    /// The feature `feature` of a terrain, copied.
    fn of(feature: TerrainFeature<'_>) -> Self {
        Self {
            id: feature.id,
            shape: match feature.shape {
                FeatureShape::Line(line) => ChunkFeatureShape::Line(line.clone()),
                FeatureShape::Pond(pond) => ChunkFeatureShape::Pond(*pond),
            },
        }
    }

    /// The order [`ChunkSource::lines_within`] lists features in: by id,
    /// then by shape, so equal features are next to each other.
    fn order(&self, other: &Self) -> std::cmp::Ordering {
        fn bits(shape: &ChunkFeatureShape) -> Vec<u64> {
            match shape {
                ChunkFeatureShape::Line(line) => line
                    .points
                    .iter()
                    .flat_map(|p| [p.x.to_bits(), p.y.to_bits()])
                    .chain(line.levels.iter().map(|l| u64::from(l.to_bits())))
                    .chain(line.widths.iter().map(|w| u64::from(w.to_bits())))
                    .chain([u64::from(line.depth.to_bits())])
                    .collect(),
                ChunkFeatureShape::Pond(pond) => vec![
                    pond.centre.x.to_bits(),
                    pond.centre.y.to_bits(),
                    u64::from(pond.level.to_bits()),
                    pond.radius.to_bits(),
                    u64::from(pond.depth.to_bits()),
                ],
            }
        }
        self.id
            .cmp(&other.id)
            .then_with(|| bits(&self.shape).cmp(&bits(&other.shape)))
    }
}

/// Something chunks can be built from: a terrain, or a world of them.
pub trait ChunkSource: Sync {
    /// The ground at world position `p` (x and z, metres), with its
    /// features planned to `tier`, or as near to it as the source has (see
    /// [`tier`](Self::tier)).
    fn surface(&self, p: DVec2, tier: FeatureTier) -> SurfacePoint;

    /// Surfaces in the order of `points`, with exactly the same values as
    /// [`surface`](Self::surface). Worlds reuse region boundaries and
    /// terrains across the batch; other sources may use the default.
    fn surfaces(&self, points: &[DVec2], tier: FeatureTier) -> Vec<SurfacePoint> {
        points.iter().map(|&p| self.surface(p, tier)).collect()
    }

    /// The tier [`surface`](Self::surface) gives when asked for `tier`.
    fn tier(&self, tier: FeatureTier) -> FeatureTier;

    /// The most, in metres, the ground between `min` and `max` (x and z)
    /// at the fine tier differs from the same ground at tier `from` (see
    /// [`Terrain::tier_difference`]). It sizes the skirts between chunks of
    /// different tiers. Builds nothing: ground that isn't built yet counts
    /// as not moving, so ask after reading the area at the fine tier.
    fn tier_difference(&self, min: DVec2, max: DVec2, from: FeatureTier) -> f32;

    /// Every river, pond and road that can change the ground at `tier`
    /// within the box from `min` to `max` (x and z, metres), whole, each
    /// once, sorted by id. The same features as [`surface`](Self::surface)
    /// cuts into the ground there, so a feature reaching into the box is
    /// listed whatever box around it is asked about.
    ///
    /// The default lists nothing: a source without features, such as
    /// ground given by a formula, keeps it.
    fn lines_within(&self, min: DVec2, max: DVec2, tier: FeatureTier) -> Vec<ChunkFeature> {
        let _ = (min, max, tier);
        Vec::new()
    }

    /// How closely canonical ground meshes of this source follow its
    /// ground (see [`tin`](crate::tin)). Build every chunk of one source
    /// within it, so neighbours meet exactly.
    ///
    /// The default is [`TinTolerance::GENERAL`](crate::tin::TinTolerance::GENERAL);
    /// a world gives its own
    /// [`WorldTerrainOptions::tin`](crate::WorldTerrainOptions::tin).
    fn tin_tolerance(&self) -> crate::tin::TinTolerance {
        crate::tin::TinTolerance::GENERAL
    }

    /// Whether reading chunk `key` at tier `to` could give anything
    /// different from reading it at tier `from`: its samples, or any
    /// ground site a painter reads inside it, at any detail. `false` means
    /// a read at `from` is exactly a read at `to`, bit for bit, so it can
    /// stand for one.
    ///
    /// The default answers `true` whenever the tiers differ, which is
    /// always safe.
    fn tier_changes(&self, key: ChunkKey, from: FeatureTier, to: FeatureTier) -> bool {
        let _ = key;
        self.tier(from) != self.tier(to)
    }
}

/// How far past a chunk's sides its read looks: the ring of samples
/// around its mesh, plus how far a ground site reads features and
/// neighbouring heights from its point.
fn read_reach(key: ChunkKey) -> f64 {
    2.0 * key.step() + f64::from(MEASURE_REACH) + 2.0 * CURVATURE_STEP
}

/// `features` sorted by id, each once.
fn sorted_features(mut features: Vec<ChunkFeature>) -> Vec<ChunkFeature> {
    features.sort_by(ChunkFeature::order);
    features.dedup();
    features
}

impl ChunkSource for Terrain {
    /// The terrain's own ground, whatever tier is asked for.
    fn surface(&self, p: DVec2, _: FeatureTier) -> SurfacePoint {
        let sample = self.query(p, SampleDetail::Height);
        SurfacePoint {
            height: sample.height,
            water: sample.water,
        }
    }

    /// The terrain's own tier (see [`Terrain::tier`]), whatever is asked
    /// for.
    fn tier(&self, _: FeatureTier) -> FeatureTier {
        Terrain::tier(self)
    }

    /// The terrain's own [`Terrain::tier_difference`], anywhere.
    fn tier_difference(&self, _: DVec2, _: DVec2, from: FeatureTier) -> f32 {
        Terrain::tier_difference(self, from)
    }

    /// The terrain's own features (at its own tier, whatever is asked for)
    /// that can change the ground in the box.
    fn lines_within(&self, min: DVec2, max: DVec2, _: FeatureTier) -> Vec<ChunkFeature> {
        sorted_features(
            self.features_within(min, max)
                .into_iter()
                .filter_map(|id| self.feature(id))
                .map(ChunkFeature::of)
                .collect(),
        )
    }
}

impl ChunkSource for WorldTerrain {
    /// The world's own [`WorldTerrainOptions::tin`](crate::WorldTerrainOptions::tin).
    fn tin_tolerance(&self) -> crate::tin::TinTolerance {
        self.options().tin
    }

    fn surfaces(&self, points: &[DVec2], tier: FeatureTier) -> Vec<SurfacePoint> {
        self.sample_batch(points, tier, |terrain, p| {
            terrain.map_or(
                SurfacePoint {
                    height: f32::NAN,
                    water: None,
                },
                |terrain| terrain.surface(p, tier),
            )
        })
    }

    /// The ground of the region holding `p` at `tier` (see
    /// [`WorldTerrain::terrain_tier`]). NaN heights where the region can't
    /// be built.
    fn surface(&self, p: DVec2, tier: FeatureTier) -> SurfacePoint {
        match self.terrain_tier(self.region_at(p), tier) {
            Ok(terrain) => terrain.surface(p, tier),
            Err(_) => SurfacePoint {
                height: f32::NAN,
                water: None,
            },
        }
    }

    /// Always the tier asked for.
    fn tier(&self, tier: FeatureTier) -> FeatureTier {
        tier
    }

    /// The largest [`Terrain::tier_difference`] of the fine terrains kept
    /// for the regions holding a 5 × 5 grid of points over the area.
    fn tier_difference(&self, min: DVec2, max: DVec2, from: FeatureTier) -> f32 {
        let mut regions = std::collections::BTreeSet::new();
        for j in 0..5 {
            for i in 0..5 {
                let t = DVec2::new(f64::from(i), f64::from(j)) / 4.0;
                regions.insert(self.region_at(min + (max - min) * t));
            }
        }
        regions
            .into_iter()
            .filter_map(|region| self.kept_terrain(region, FeatureTier::Fine))
            .map(|terrain| terrain.tier_difference(from))
            .fold(0.0, f32::max)
    }

    /// The features at `tier` of every region reaching into the box that
    /// can change the ground there. A feature carved by two regions (a
    /// river or road crossing their border) is listed once. Regions that
    /// can't be built add nothing.
    fn lines_within(&self, min: DVec2, max: DVec2, tier: FeatureTier) -> Vec<ChunkFeature> {
        let mut features = Vec::new();
        for id in regions_reaching(self, min, max) {
            if let Ok(terrain) = self.terrain_tier(id, tier) {
                features.extend(terrain.lines_within(min, max, tier));
            }
        }
        sorted_features(features)
    }

    /// Whether, in any region holding a point that chunk `key`'s read
    /// samples, a feature within reach of those points differs between the
    /// tiers. A point's ground and site come only from its own region's
    /// features near it, and a later tier only adds and reshapes roads, so
    /// equal features mean an equal read. Builds those regions, and only
    /// those, at both tiers, as the read itself would.
    fn tier_changes(&self, key: ChunkKey, from: FeatureTier, to: FeatureTier) -> bool {
        if from == to {
            return false;
        }
        let ring = DVec2::splat(2.0 * key.step());
        let reach = DVec2::splat(read_reach(key));
        let (min, max) = (key.min_corner() - reach, key.max_corner() + reach);
        regions_reaching(self, key.min_corner() - ring, key.max_corner() + ring)
            .into_iter()
            .any(
                |id| match (self.terrain_tier(id, from), self.terrain_tier(id, to)) {
                    (Ok(a), Ok(b)) => {
                        a.lines_within(min, max, from) != b.lines_within(min, max, to)
                    }
                    _ => true,
                },
            )
    }
}

/// Every region of `world` reaching into the box from `min` to `max`: the
/// one at its centre, then each neighbour across a border edge that passes
/// through the box.
fn regions_reaching(world: &WorldTerrain, min: DVec2, max: DVec2) -> BTreeSet<RegionId> {
    let mut regions = BTreeSet::from([world.region_at((min + max) * 0.5)]);
    let mut next: Vec<_> = regions.iter().copied().collect();
    while let Some(id) = next.pop() {
        for edge in &world.region(id).edges {
            let Some(neighbour) = edge.neighbor else {
                continue;
            };
            let crosses = edge
                .polyline
                .windows(2)
                .any(|w| segment_meets_box(w[0], w[1], min, max));
            if crosses && regions.insert(neighbour) {
                next.push(neighbour);
            }
        }
    }
    regions
}

/// Whether the segment from `a` to `b` touches the box from `min` to `max`
/// (edges included).
fn segment_meets_box(a: DVec2, b: DVec2, min: DVec2, max: DVec2) -> bool {
    // Clip the segment's parameter range to each slab in turn.
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    let d = b - a;
    for (start, step, low, high) in [(a.x, d.x, min.x, max.x), (a.y, d.y, min.y, max.y)] {
        if step == 0.0 {
            if start < low || start > high {
                return false;
            }
            continue;
        }
        let (t0, t1) = ((low - start) / step, (high - start) / step);
        lo = lo.max(t0.min(t1));
        hi = hi.min(t0.max(t1));
    }
    lo <= hi
}

/// A chunk that could not be built.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ChunkError {
    /// The source has no ground at this world position (x and z, metres),
    /// for example because a region there could not be built.
    NoGround(DVec2),
}

impl fmt::Display for ChunkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoGround(p) => write!(f, "there is no ground at ({}, {})", p.x, p.y),
        }
    }
}

impl std::error::Error for ChunkError {}

/// The ground read for one chunk, ready to be meshed for any
/// [`Transitions`].
///
/// It holds the chunk's vertices, a ring of samples just outside them for
/// the normals, and the points a finer neighbour adds along each side.
#[derive(Debug, Clone)]
pub struct ChunkSamples {
    key: ChunkKey,
    tier: FeatureTier,
    /// The vertex lattice with a one-sample ring around it: `(N + 3)²`
    /// samples, row by row along x, starting one step outside the smallest
    /// corner.
    grid: Vec<SurfacePoint>,
    /// For each side (in [`ChunkSide::ALL`] order), three rows along it at
    /// half steps: one half step outside, on the side, and one half step
    /// inside. Each row runs from one half step before the side's start to
    /// one after its end (`2N + 3` samples). Empty at level 0, which never
    /// has finer neighbours.
    sides: [[Vec<SurfacePoint>; 3]; 4],
}

impl ChunkSamples {
    /// Reads the ground of chunk `key` from `source`, with its features
    /// planned to `tier`.
    ///
    /// # Errors
    ///
    /// [`ChunkError::NoGround`] if any point of the chunk, or of the ring
    /// just around it, has no ground.
    pub fn sample(
        source: &(impl ChunkSource + ?Sized),
        key: ChunkKey,
        tier: FeatureTier,
    ) -> Result<Self, ChunkError> {
        // Read the vertex lattice, normal ring and transition samples in
        // one batch, so a world holds each needed terrain only once.
        let width = N + 3;
        let mut points: Vec<_> = (0..width)
            .flat_map(|j| (0..width).map(move |i| key.half_point(2 * (i - 1), 2 * (j - 1))))
            .collect();
        let grid_len = points.len();
        let length = 2 * N + 3;
        if key.level > 0 {
            for side in ChunkSide::ALL {
                for t in 0..3 {
                    points.extend((0..length).map(|k| {
                        let (u, v) = side_point(side, k - 1, t - 1);
                        key.half_point(u, v)
                    }));
                }
            }
        }
        let surfaces = source.surfaces(&points, tier);
        assert_eq!(surfaces.len(), points.len(), "one surface per point");
        for (p, surface) in points.iter().zip(&surfaces) {
            if !surface.height.is_finite() {
                return Err(ChunkError::NoGround(*p));
            }
        }
        let mut surfaces = surfaces.into_iter();
        let grid = surfaces.by_ref().take(grid_len).collect();
        let mut sides: [[Vec<SurfacePoint>; 3]; 4] = Default::default();
        if key.level > 0 {
            for side in &mut sides {
                for row in side {
                    *row = surfaces.by_ref().take(length as usize).collect();
                }
            }
        }

        Ok(Self {
            key,
            tier: source.tier(tier),
            grid,
            sides,
        })
    }

    /// Samples of chunk `key` that show the ground of `parent`, a coarser
    /// chunk around it, exactly as `parent`'s own mesh shows it, with no
    /// detail of its own: a stand-in that can be drawn beside finer chunks
    /// until `key` itself is read. Its tier is `parent`'s.
    ///
    /// Where it meets a chunk read from the ground itself, its edge can
    /// stand apart from that chunk's by as much as `parent`'s mesh differs
    /// from the ground; a skirt covers that.
    ///
    /// # Panics
    ///
    /// If `key` is not a chunk inside `parent`'s chunk.
    pub fn stand_in(parent: &ChunkSamples, key: ChunkKey) -> Self {
        assert!(
            key.level < parent.key.level && key.is_within(parent.key),
            "{key} is not inside {}",
            parent.key
        );
        Self::sample(&ParentSurface(parent), key, parent.tier)
            .expect("a parent's samples cover the chunks inside it")
    }

    /// The chunk these samples are of.
    pub fn key(&self) -> ChunkKey {
        self.key
    }

    /// The tier of the ground the samples were read from.
    pub fn tier(&self) -> FeatureTier {
        self.tier
    }

    /// The ground at vertex `(i, j)` of the chunk, each from 0 to
    /// [`CHUNK_CELLS`] inclusive.
    ///
    /// # Panics
    ///
    /// If `i` or `j` is out of that range.
    pub fn vertex(&self, i: u32, j: u32) -> SurfacePoint {
        assert!(i <= CHUNK_CELLS && j <= CHUNK_CELLS, "vertex out of range");
        self.at(2 * i64::from(i), 2 * i64::from(j))
    }

    /// The sample `(u, v)` half steps from the chunk's smallest corner,
    /// which must be one these samples hold.
    fn at(&self, u: i64, v: i64) -> SurfacePoint {
        if u % 2 == 0 && v % 2 == 0 {
            let (i, j) = (u / 2 + 1, v / 2 + 1);
            return self.grid[(j * (N + 3) + i) as usize];
        }
        let near = |a: i64| -> Option<(i64, bool)> {
            if (-1..=1).contains(&a) {
                Some((a, false))
            } else if (2 * N - 1..=2 * N + 1).contains(&a) {
                Some((2 * N - a, true))
            } else {
                None
            }
        };
        let (side, t, k) = match (near(u), near(v)) {
            (_, Some((t, max))) => (if max { 3 } else { 2 }, t, u),
            (Some((t, max)), None) => (if max { 1 } else { 0 }, t, v),
            (None, None) => unreachable!("({u}, {v}) is not a sampled point"),
        };
        self.sides[side][(t + 1) as usize][(k + 1) as usize]
    }

    /// Builds the chunk's mesh for `transitions`, with a skirt hanging
    /// `skirt` metres down from every side (0 for none).
    ///
    /// A skirt hides small gaps where a neighbouring chunk was built from
    /// other ground, such as a chunk of an earlier tier beside a fine one.
    /// Chunks of the same tier, at the same level or one level apart,
    /// join without gaps when each is meshed with its own transitions.
    ///
    /// A chunk at level 0 has no finer neighbours, so its transitions are
    /// ignored and the mesh records none.
    pub fn mesh(&self, transitions: Transitions, skirt: f32) -> ChunkMesh {
        let transitions = if self.key.level == 0 {
            Transitions::NONE
        } else {
            transitions
        };
        let step = self.key.step() as f32;
        let half = 0.5 * step;
        let mut mesh = ChunkMesh {
            key: self.key,
            transitions,
            tier: self.tier,
            origin: self.key.min_corner(),
            positions: Vec::new(),
            normals: Vec::new(),
            indices: Vec::new(),
            water: None,
            heights: (f32::INFINITY, f32::NEG_INFINITY),
        };

        // Each vertex is at a point `(u, v)` half steps from the corner:
        // the lattice first, row by row, then the half-step points of each
        // split side.
        let vertices = VertexLayout::new(transitions);
        let points = vertices.points();
        let mut vertex = |u: i64, v: i64| vertices.index(u, v);

        // The ground's triangles, cell by cell.
        let mut triangles: Vec<[u32; 3]> = Vec::with_capacity((N * N * 2 + 4 * N) as usize);
        for j in 0..N {
            for i in 0..N {
                cell_triangles(i, j, transitions, &mut vertex, &mut triangles);
            }
        }

        // Positions and normals of the ground's vertices.
        for &(u, v) in &points {
            let point = self.at(u, v);
            mesh.positions
                .push(Vec3::new(u as f32 * half, point.height, v as f32 * half));
            mesh.normals.push(self.normal(u, v, transitions));
            mesh.heights.0 = mesh.heights.0.min(point.height);
            mesh.heights.1 = mesh.heights.1.max(point.height);
        }
        mesh.indices = triangles.iter().flatten().copied().collect();

        // Water over every triangle that has some.
        mesh.water = self.water(&points, &triangles, half);
        if let Some(water) = &mesh.water {
            for p in &water.positions {
                mesh.heights.0 = mesh.heights.0.min(p.y);
                mesh.heights.1 = mesh.heights.1.max(p.y);
            }
        }

        // Skirts: each side's edge, copied `skirt` metres lower.
        if skirt > 0.0 {
            for side in ChunkSide::ALL {
                let edge = side_vertices(side, transitions.side(side));
                let outward = {
                    let (dx, dz) = side.direction();
                    Vec3::new(dx as f32, 0.0, dz as f32)
                };
                let top: Vec<u32> = edge.iter().map(|&(u, v)| vertices.index(u, v)).collect();
                let bottom: Vec<u32> = top
                    .iter()
                    .map(|&t| {
                        let p = mesh.positions[t as usize] - Vec3::Y * skirt;
                        mesh.positions.push(p);
                        mesh.normals.push(mesh.normals[t as usize]);
                        (mesh.positions.len() - 1) as u32
                    })
                    .collect();
                for w in 0..top.len() - 1 {
                    let (a, b) = (top[w], top[w + 1]);
                    let (c, d) = (bottom[w + 1], bottom[w]);
                    // Face outward: the side's edge runs one way or the
                    // other, so check against `outward`.
                    let along = mesh.positions[b as usize] - mesh.positions[a as usize];
                    let facing = along.cross(-Vec3::Y).dot(outward);
                    if facing > 0.0 {
                        mesh.indices.extend([a, d, c, a, c, b]);
                    } else {
                        mesh.indices.extend([a, c, d, a, b, c]);
                    }
                }
            }
            mesh.heights.0 -= skirt;
        }
        mesh
    }

    /// The upward unit normal at `(u, v)`, from differences of neighbouring
    /// samples. Along a side or at a corner touching a finer chunk, the
    /// differences are taken half a step apart, as that finer chunk takes
    /// them, so both give the same normal there.
    fn normal(&self, u: i64, v: i64, transitions: Transitions) -> Vec3 {
        let fine = {
            let on_min_x = u == 0;
            let on_max_x = u == 2 * N;
            let on_min_z = v == 0;
            let on_max_z = v == 2 * N;
            let corner = match (on_min_x, on_max_x, on_min_z, on_max_z) {
                (true, _, true, _) => Some(ChunkCorner::MinXMinZ),
                (_, true, true, _) => Some(ChunkCorner::MaxXMinZ),
                (true, _, _, true) => Some(ChunkCorner::MinXMaxZ),
                (_, true, _, true) => Some(ChunkCorner::MaxXMaxZ),
                _ => None,
            };
            match corner {
                Some(corner) => transitions.finer_at(corner),
                None => {
                    (on_min_x && transitions.side(ChunkSide::MinX))
                        || (on_max_x && transitions.side(ChunkSide::MaxX))
                        || (on_min_z && transitions.side(ChunkSide::MinZ))
                        || (on_max_z && transitions.side(ChunkSide::MaxZ))
                }
            }
        };
        // A vertex halfway along a side is only ever there for a finer
        // neighbour, and has no samples a whole step away.
        let d = if fine || u % 2 != 0 || v % 2 != 0 {
            1
        } else {
            2
        };
        let spacing = d as f32 * 0.5 * self.key.step() as f32;
        let dx = self.at(u + d, v).height - self.at(u - d, v).height;
        let dz = self.at(u, v + d).height - self.at(u, v - d).height;
        Vec3::new(-dx, 2.0 * spacing, -dz).normalize()
    }

    /// The water surface over `triangles`: every triangle with water at one
    /// or more corners, with the dry corners at the mean water height of
    /// the wet ones, so the surface runs into the banks. `None` if there is
    /// no water.
    fn water(&self, points: &[(i64, i64)], triangles: &[[u32; 3]], half: f32) -> Option<WaterMesh> {
        let levels: Vec<Option<f32>> = points.iter().map(|&(u, v)| self.at(u, v).water).collect();
        let mut positions = Vec::new();
        for triangle in triangles {
            let wet: Vec<f32> = triangle
                .iter()
                .filter_map(|&t| levels[t as usize])
                .collect();
            if wet.is_empty() {
                continue;
            }
            let mean = wet.iter().sum::<f32>() / wet.len() as f32;
            for &t in triangle {
                let (u, v) = points[t as usize];
                let y = levels[t as usize].unwrap_or(mean);
                positions.push(Vec3::new(u as f32 * half, y, v as f32 * half));
            }
        }
        (!positions.is_empty()).then(|| WaterMesh {
            indices: (0..positions.len() as u32).collect(),
            positions,
        })
    }
}

/// Where each vertex of a chunk mesh sits in its vertex list, for one set
/// of transitions: the `(N + 1)²` lattice vertices row by row, then `N`
/// half-step vertices along each split side, in [`ChunkSide::ALL`] order.
struct VertexLayout {
    /// The index of each side's first half-step vertex, if it is split.
    sides: [Option<u32>; 4],
    /// How many vertices there are.
    count: u32,
}

impl VertexLayout {
    /// The layout for `transitions`.
    fn new(transitions: Transitions) -> Self {
        let mut count = ((N + 1) * (N + 1)) as u32;
        let sides = ChunkSide::ALL.map(|side| {
            transitions.side(side).then(|| {
                count += N as u32;
                count - N as u32
            })
        });
        Self { sides, count }
    }

    /// The index of the vertex at `(u, v)` half steps from the corner,
    /// which must be a lattice point or a split side's half-step point.
    fn index(&self, u: i64, v: i64) -> u32 {
        if u % 2 == 0 && v % 2 == 0 {
            return ((v / 2) * (N + 1) + u / 2) as u32;
        }
        let (side, k) = match (u, v) {
            (0, k) => (0, k),
            (u, k) if u == 2 * N => (1, k),
            (k, 0) => (2, k),
            (k, _) => (3, k),
        };
        let first = self.sides[side].expect("a half-step vertex lies on a split side");
        first + ((k - 1) / 2) as u32
    }

    /// Every vertex's `(u, v)`, in order.
    fn points(&self) -> Vec<(i64, i64)> {
        let mut points = Vec::with_capacity(self.count as usize);
        for v in 0..=N {
            for u in 0..=N {
                points.push((2 * u, 2 * v));
            }
        }
        for (side, first) in ChunkSide::ALL.into_iter().zip(self.sides) {
            if first.is_some() {
                points.extend((0..N).map(|k| side_point(side, 2 * k + 1, 0)));
            }
        }
        points
    }
}

/// The point `k` half steps along `side` and `t` half steps inward from
/// it, as `(u, v)` half steps from the chunk's smallest corner.
fn side_point(side: ChunkSide, k: i64, t: i64) -> (i64, i64) {
    match side {
        ChunkSide::MinX => (t, k),
        ChunkSide::MaxX => (2 * N - t, k),
        ChunkSide::MinZ => (k, t),
        ChunkSide::MaxZ => (k, 2 * N - t),
    }
}

/// The mesh vertices along `side`, in order of increasing x or z: every
/// lattice vertex, and between them the half-step vertices when the
/// neighbour on that side is finer.
fn side_vertices(side: ChunkSide, finer: bool) -> Vec<(i64, i64)> {
    let stride = if finer { 1 } else { 2 };
    (0..=2 * N)
        .step_by(stride)
        .map(|k| side_point(side, k, 0))
        .collect()
}

/// Adds the triangles of cell `(i, j)` to `triangles`, wound to face up.
///
/// A cell away from every finer neighbour is two triangles. A cell along a
/// side with a finer neighbour also has that side's half-step vertex, and
/// becomes a fan from a corner that isn't on such a side, so its edge
/// matches the finer chunk's two edges exactly.
fn cell_triangles(
    i: i64,
    j: i64,
    transitions: Transitions,
    vertex: &mut impl FnMut(i64, i64) -> u32,
    triangles: &mut Vec<[u32; 3]>,
) {
    let (u, v) = (2 * i, 2 * j);

    // The cell's edges in upward-facing order: a (min x, min z), d (min x,
    // max z), c (max x, max z), b (max x, min z). Each edge says whether it
    // lies on a side with a finer neighbour.
    let corners = [(u, v), (u, v + 2), (u + 2, v + 2), (u + 2, v)];
    let split = [
        i == 0 && transitions.side(ChunkSide::MinX),
        j == N - 1 && transitions.side(ChunkSide::MaxZ),
        i == N - 1 && transitions.side(ChunkSide::MaxX),
        j == 0 && transitions.side(ChunkSide::MinZ),
    ];

    if !split.iter().any(|&s| s) {
        let [a, d, c, b] = corners.map(|(u, v)| vertex(u, v));
        // Alternate the diagonal on the world lattice, so no direction is
        // favoured.
        if (i + j) % 2 == 0 {
            triangles.push([a, d, c]);
            triangles.push([a, c, b]);
        } else {
            triangles.push([a, d, b]);
            triangles.push([d, c, b]);
        }
        return;
    }

    // The cell's outline, with the half-step points of its split edges.
    let mut outline = Vec::with_capacity(8);
    for e in 0..4 {
        let (p, q) = (corners[e], corners[(e + 1) % 4]);
        outline.push((p, e));
        if split[e] {
            outline.push((((p.0 + q.0) / 2, (p.1 + q.1) / 2), usize::MAX));
        }
    }

    // Fan from the corner whose two edges are both whole: corner `e` sits
    // between edge `e − 1` and edge `e`.
    let hub = (0..4)
        .find(|&e| !split[e] && !split[(e + 3) % 4])
        .expect("a cell touches at most two adjacent sides");
    let start = outline
        .iter()
        .position(|&(_, e)| e == hub)
        .expect("every corner is in the outline");
    let ring: Vec<u32> = (0..outline.len())
        .map(|k| {
            let (p, _) = outline[(start + k) % outline.len()];
            vertex(p.0, p.1)
        })
        .collect();
    for k in 1..ring.len() - 1 {
        triangles.push([ring[0], ring[k], ring[k + 1]]);
    }
}

/// A chunk's ground as a triangle mesh, ready to draw.
///
/// Positions are relative to [`origin`](Self::origin) along x and z, and
/// absolute along y (y up, metres), so a chunk is drawn at
/// `(origin.x, 0, origin.y)`. Triangles face up (counter-clockwise seen
/// from above in a right-handed frame with y up).
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkMesh {
    /// Which chunk this is.
    pub key: ChunkKey,
    /// The finer neighbours it was meshed to join.
    pub transitions: Transitions,
    /// The tier of the ground it was built from.
    pub tier: FeatureTier,
    /// World x and z of the chunk's smallest corner.
    pub origin: DVec2,
    /// Vertex positions.
    pub positions: Vec<Vec3>,
    /// Unit normals, one per vertex.
    pub normals: Vec<Vec3>,
    /// Triangles, three vertex indices each.
    pub indices: Vec<u32>,
    /// The surface of the rivers and ponds over the chunk, if any. It is
    /// for the user's own water material; it is not part of the ground.
    pub water: Option<WaterMesh>,
    /// The lowest and highest y of the mesh, skirts and water included.
    pub heights: (f32, f32),
    // Labels (milestone G4) will add per-vertex label weights here, and
    // split the triangles into parts by landform.
}

impl ChunkMesh {
    /// The chunk's world position: where its local origin sits.
    pub fn translation(&self) -> DVec3 {
        DVec3::new(self.origin.x, 0.0, self.origin.y)
    }
}

/// The water surface over a chunk, in the same frame as its
/// [`ChunkMesh`]. Its normals all point straight up.
#[derive(Debug, Clone, PartialEq)]
pub struct WaterMesh {
    /// Vertex positions.
    pub positions: Vec<Vec3>,
    /// Triangles, three vertex indices each, facing up.
    pub indices: Vec<u32>,
}

/// The terrain's usual chunk levels, drawn out to `view_distance` metres:
/// [`ChunkDetail::DEFAULT`]'s levels (level 0 out to 256 m, and each level
/// after that out to twice as far). See [`ChunkDetail::levels`].
pub fn chunk_levels(view_distance: f32) -> ChunkLevels {
    ChunkDetail::DEFAULT.levels(view_distance)
}

/// How fine the terrain looks on screen: which chunk level is drawn at
/// each distance from the camera.
///
/// A chunk at level `L` is a grid of cells `2^L` metres across
/// ([`ChunkKey::step`]). Each level takes over from the finer one before it
/// where one of its cells, seen face on, covers
/// [`cell_on_screen`](Self::cell_on_screen) of the screen's height. Past
/// level 0, no cell is drawn larger than that, whatever the screen's
/// resolution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChunkDetail {
    /// The largest share of the screen's height one cell covers, past
    /// level 0. Smaller values draw finer ground and more triangles:
    /// halving it draws about four times as many. Must be positive.
    pub cell_on_screen: f32,
    /// How large things look through the camera the terrain is drawn for.
    pub view: ViewScale,
}

impl ChunkDetail {
    /// The usual detail: with Bevy's default 45° field of view, a cell
    /// covers at most about 7 pixels of a 720-pixel-high screen (14 of
    /// 1440), and level `L` takes over at 128 m × 2^`L`.
    pub const DEFAULT: Self = Self {
        cell_on_screen: ViewScale::DEFAULT.get() / 128.0,
        view: ViewScale::DEFAULT,
    };

    /// The chunk levels for this detail, drawn out to `view_distance`
    /// metres, up to level 6 (4 km chunks). Levels that would start too
    /// close to the view distance to be seen are left out, so any positive
    /// view distance gives levels [`ChunkSelection::around`] accepts.
    ///
    /// # Panics
    ///
    /// Panics if [`cell_on_screen`](Self::cell_on_screen) isn't a positive
    /// number.
    pub fn levels(self, view_distance: f32) -> ChunkLevels {
        const HYSTERESIS: f32 = 0.1;
        assert!(
            self.cell_on_screen.is_finite() && self.cell_on_screen > 0.0,
            "a cell's share of the screen, {}, is not a positive number",
            self.cell_on_screen
        );

        // Level `L` starts where one of its cells covers `cell_on_screen`.
        let start = |level: u8| {
            let cell = ChunkKey::size_at(level) / f64::from(CHUNK_CELLS);
            cell as f32 * self.view.get() / self.cell_on_screen
        };
        let starts = (1..=6)
            .map(start)
            // A level must finish switching in before the ground fades out.
            .take_while(|&start| start * (1.0 + HYSTERESIS) <= view_distance * (1.0 - HYSTERESIS));
        ChunkLevels::new(starts, HYSTERESIS, view_distance)
    }
}

impl Default for ChunkDetail {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Why chunks could not be chosen.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ChunkSelectionError {
    /// The levels are not valid (see [`ChunkLevels::validate`]).
    Levels(ChunkLevelsError),
    /// The factor is not a positive number.
    Factor(f32),
}

impl fmt::Display for ChunkSelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Levels(e) => write!(f, "invalid chunk levels: {e}"),
            Self::Factor(x) => write!(f, "the distance factor {x} is not positive"),
        }
    }
}

impl std::error::Error for ChunkSelectionError {}

/// The chunks to draw, each with its [`Transitions`].
///
/// They cover the ground out to the view distance without overlapping,
/// and chunks that touch, along a side or at a corner, are never more than
/// one level apart.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChunkSelection {
    chunks: BTreeMap<ChunkKey, Transitions>,
}

impl ChunkSelection {
    /// The chunks to draw around `viewers`, with chunk levels from `levels`
    /// (see [`chunk_levels`]) scaled by `factor`.
    ///
    /// Nothing is drawn past the levels' view distance. A chunk is split into finer ones while its nearest
    /// point is closer to a viewer than where its own level starts. Each
    /// viewer is a position whose y is its height above the ground (not
    /// its absolute height), so a high viewer sees coarser chunks.
    ///
    /// Passing the selection from the last frame as `previous` applies
    /// each level's hysteresis: a chunk splits only once the viewer is
    /// well inside the switching distance, and merges only once well
    /// outside it, so moving back and forth across it doesn't rebuild
    /// chunks every frame. `factor` is usually the quality's
    /// [`distance_scale`](procgena_core::Quality::distance_scale).
    ///
    /// # Errors
    ///
    /// If `levels` is invalid (see [`ChunkLevels::validate`]), or `factor`
    /// isn't positive.
    pub fn around(
        levels: &ChunkLevels,
        factor: f32,
        viewers: &[DVec3],
        previous: Option<&ChunkSelection>,
    ) -> Result<Self, ChunkSelectionError> {
        levels.validate().map_err(ChunkSelectionError::Levels)?;
        if !(factor.is_finite() && factor > 0.0) {
            return Err(ChunkSelectionError::Factor(factor));
        }
        let top = (levels.levels.len() - 1) as u8;
        let reach = f64::from(levels.view_distance * factor);

        // The distance from the nearest viewer to a chunk.
        let distance = |key: ChunkKey| -> f64 {
            viewers
                .iter()
                .map(|v| key.distance_to(DVec2::new(v.x, v.z)).hypot(v.y))
                .fold(f64::INFINITY, f64::min)
        };

        // Whether `key` should be split, with hysteresis against the last
        // selection.
        let was_split = |key: ChunkKey| -> Option<bool> {
            let previous = previous?;
            let mut at = Some(key);
            while let Some(k) = at {
                if previous.chunks.contains_key(&k) {
                    return Some(false);
                }
                at = k.parent().filter(|p| p.level <= top);
            }
            Some(previous.chunks.keys().any(|k| k.is_within(key)))
        };
        let split = |key: ChunkKey| -> bool {
            if key.level == 0 {
                return false;
            }
            let level = &levels.levels[usize::from(key.level)];
            let start = f64::from(level.start * factor);
            let band = f64::from(level.hysteresis) * start;
            let threshold = match was_split(key) {
                Some(true) => start + band,
                Some(false) => start - band,
                None => start,
            };
            distance(key) < threshold
        };

        // The coarsest chunks within reach of a viewer, split down.
        let mut leaves: BTreeSet<ChunkKey> = BTreeSet::new();
        for v in viewers {
            if v.y.abs() >= reach {
                continue;
            }
            let lo = ChunkKey::containing(DVec2::new(v.x, v.z) - reach, top);
            let hi = ChunkKey::containing(DVec2::new(v.x, v.z) + reach, top);
            for z in lo.z..=hi.z {
                for x in lo.x..=hi.x {
                    let key = ChunkKey::new(x, z, top);
                    if distance(key) < reach && !leaves.contains(&key) {
                        add_split(key, &split, &mut leaves);
                    }
                }
            }
        }

        // Split coarse chunks until neighbours are at most a level apart.
        balance(&mut leaves);

        // Each chunk's finer neighbours.
        let chunks = leaves
            .iter()
            .map(|&key| (key, transitions_of(key, &leaves)))
            .collect();
        Ok(Self { chunks })
    }

    /// The chosen chunks with their transitions, in a fixed order.
    pub fn iter(&self) -> impl Iterator<Item = (ChunkKey, Transitions)> + '_ {
        self.chunks.iter().map(|(&k, &t)| (k, t))
    }

    /// The transitions of `key`, if it is chosen.
    pub fn get(&self, key: ChunkKey) -> Option<Transitions> {
        self.chunks.get(&key).copied()
    }

    /// How many chunks are chosen.
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    /// Whether no chunk is chosen.
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// The chunks to draw on the way from `drawn` to this selection, using
    /// only the chunks of this selection that `ready` says can be drawn.
    ///
    /// Each part of the ground is drawn from this selection where its
    /// chunks there are ready, and from `drawn` elsewhere. Where a ready
    /// chunk sits inside, or next to, a drawn chunk too coarse to sit
    /// beside it, the drawn chunk is split into stand-ins: finer chunks
    /// that show the same ground (see [`ChunkSamples::stand_in`]). Ground
    /// that neither can draw, or that only `drawn` covers, is left out.
    ///
    /// Like any selection, the result never overlaps itself, chunks that
    /// touch are at most one level apart, and its transitions fit the
    /// chunks it actually holds, so a chunk kept from `drawn` may need
    /// meshing again. With every chunk ready, it is this selection; with
    /// none ready, it is `drawn` within this selection's ground.
    pub fn blend(&self, drawn: &ChunkSelection, ready: impl Fn(ChunkKey) -> bool) -> ChunkBlend {
        let target: BTreeSet<ChunkKey> = self.chunks.keys().copied().collect();
        let old: BTreeSet<ChunkKey> = drawn.chunks.keys().copied().collect();

        // The areas to fill: this selection's chunks, grouped under their
        // ancestors at the coarsest level either selection uses.
        let Some(top) = target.iter().chain(&old).map(|k| k.level).max() else {
            return ChunkBlend::default();
        };
        let roots: BTreeSet<ChunkKey> = target
            .iter()
            .map(|&k| ancestor_at(k, top).expect("no chunk is above the top level"))
            .collect();

        // Fill each area, then even out chunks more than a level apart:
        // split a drawn chunk into stand-ins, or, where the coarse one is
        // new, draw its area from `drawn` instead and fill again.
        let mut forced: BTreeSet<ChunkKey> = BTreeSet::new();
        'fill: loop {
            let mut picked: BTreeMap<ChunkKey, Source> = BTreeMap::new();
            for &root in &roots {
                let mut out = Vec::new();
                if pick(root, &target, &old, &ready, &forced, &mut out) {
                    picked.extend(out);
                }
            }
            loop {
                let leaves: BTreeSet<ChunkKey> = picked.keys().copied().collect();
                let Some((_, coarse)) = unbalanced(&leaves) else {
                    return ChunkBlend::new(&picked);
                };
                match picked[&coarse] {
                    Source::Target => {
                        forced.insert(coarse);
                        continue 'fill;
                    }
                    Source::Drawn => split(&mut picked, coarse, coarse),
                    Source::StandIn(from) => split(&mut picked, coarse, from),
                }
            }
        }
    }
}

/// The surface of a chunk's mesh, at full detail and with no transitions,
/// read as a [`ChunkSource`]: what a stand-in inside it samples. It reaches
/// one step past the chunk's sides, as far as the chunk's samples go.
struct ParentSurface<'a>(&'a ChunkSamples);

impl ChunkSource for ParentSurface<'_> {
    fn surface(&self, p: DVec2, _: FeatureTier) -> SurfacePoint {
        let samples = self.0;

        // The cell holding `p`, and where in it `p` lies, from 0 to 1.
        let local = (p - samples.key.min_corner()) / samples.key.step();
        let cell = local
            .floor()
            .clamp(DVec2::splat(-1.0), DVec2::splat(N as f64));
        let (i, j) = (cell.x as i64, cell.y as i64);
        let (fx, fz) = ((local.x - cell.x) as f32, (local.y - cell.y) as f32);
        let corner = |di: i64, dj: i64| samples.at(2 * (i + di), 2 * (j + dj));
        let [a, b, c, d] = [corner(0, 0), corner(1, 0), corner(1, 1), corner(0, 1)];

        // Each value across the triangle holding `p`, split along the
        // diagonal `ChunkSamples::mesh` draws in this cell.
        let across = |a: f32, b: f32, c: f32, d: f32| {
            if (i + j).rem_euclid(2) == 0 {
                if fz >= fx {
                    a + fx * (c - d) + fz * (d - a)
                } else {
                    a + fx * (b - a) + fz * (c - b)
                }
            } else if fx + fz <= 1.0 {
                a + fx * (b - a) + fz * (d - a)
            } else {
                c + (1.0 - fx) * (d - c) + (1.0 - fz) * (b - c)
            }
        };
        let height = across(a.height, b.height, c.height, d.height);

        // Water where a corner has some, with dry corners at the mean of
        // the wet ones, as the mesh's water runs into the banks.
        let wet: Vec<f32> = [a, b, c, d].iter().filter_map(|s| s.water).collect();
        let water = (!wet.is_empty()).then(|| {
            let mean = wet.iter().sum::<f32>() / wet.len() as f32;
            let w = |s: SurfacePoint| s.water.unwrap_or(mean);
            across(w(a), w(b), w(c), w(d))
        });
        SurfacePoint { height, water }
    }

    fn tier(&self, _: FeatureTier) -> FeatureTier {
        self.0.tier
    }

    fn tier_difference(&self, _: DVec2, _: DVec2, _: FeatureTier) -> f32 {
        0.0
    }
}

/// The chunks to draw while moving between two selections, from
/// [`ChunkSelection::blend`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChunkBlend {
    /// The chunks to draw, with their transitions.
    pub selection: ChunkSelection,
    /// The stand-ins among them, each with the drawn chunk it is split
    /// from: build each with [`ChunkSamples::stand_in`] from that chunk's
    /// samples.
    pub stand_ins: BTreeMap<ChunkKey, ChunkKey>,
}

impl ChunkBlend {
    /// The blend of the chunks `picked`.
    fn new(picked: &BTreeMap<ChunkKey, Source>) -> Self {
        let leaves: BTreeSet<ChunkKey> = picked.keys().copied().collect();
        let chunks = leaves
            .iter()
            .map(|&key| (key, transitions_of(key, &leaves)))
            .collect();
        let stand_ins = picked
            .iter()
            .filter_map(|(&key, source)| match source {
                Source::StandIn(from) => Some((key, *from)),
                _ => None,
            })
            .collect();
        Self {
            selection: ChunkSelection { chunks },
            stand_ins,
        }
    }
}

/// Where a chunk of a blend comes from.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Source {
    /// The selection being moved to.
    Target,
    /// The drawn selection.
    Drawn,
    /// A stand-in split from this drawn chunk.
    StandIn(ChunkKey),
}

/// Replaces `key` in `picked` with its four children, as stand-ins split
/// from the drawn chunk `from`.
fn split(picked: &mut BTreeMap<ChunkKey, Source>, key: ChunkKey, from: ChunkKey) {
    picked.remove(&key);
    let children = key.children().expect("a chunk too coarse has children");
    picked.extend(children.map(|c| (c, Source::StandIn(from))));
}

/// The chunk at `level` that holds `key`, or `key` itself at that level;
/// `None` if `key` is coarser.
fn ancestor_at(key: ChunkKey, level: u8) -> Option<ChunkKey> {
    let mut at = key;
    while at.level < level {
        at = at.parent()?;
    }
    (at.level == level).then_some(at)
}

/// Adds to `out` the chunks that draw `node`, each with where it comes
/// from, and returns whether `node` can be drawn. `target`'s chunks are
/// used where they are `ready` and not `forced`, as long as some part of
/// `node` uses them; `old`'s elsewhere, split into stand-ins where an
/// `old` chunk is coarser than the part to fill. On `false`, `out` is
/// left as it was.
fn pick(
    node: ChunkKey,
    target: &BTreeSet<ChunkKey>,
    old: &BTreeSet<ChunkKey>,
    ready: &impl Fn(ChunkKey) -> bool,
    forced: &BTreeSet<ChunkKey>,
    out: &mut Vec<(ChunkKey, Source)>,
) -> bool {
    // From the target: the chunk itself, or its children each drawn as
    // best they can, if any of them uses the target.
    let mark = out.len();
    if !forced.contains(&node) {
        if target.contains(&node) {
            if ready(node) {
                out.push((node, Source::Target));
                return true;
            }
        } else if let Some(children) = node.children() {
            let all = children
                .iter()
                .all(|&c| pick(c, target, old, ready, forced, out));
            if all && out[mark..].iter().any(|(_, s)| *s == Source::Target) {
                return true;
            }
            out.truncate(mark);
        }
    }

    // From what is drawn: its chunks that make up `node`, or a stand-in
    // split from the drawn chunk around it.
    if cover(node, old, out) {
        return true;
    }
    out.truncate(mark);
    match covering(node, old) {
        Some(from) => {
            out.push((node, Source::StandIn(from)));
            true
        }
        None => false,
    }
}

/// Adds to `out` the chunks of `old` that make up exactly `node`, and
/// returns whether they do. On `false`, `out` may hold some of them.
fn cover(node: ChunkKey, old: &BTreeSet<ChunkKey>, out: &mut Vec<(ChunkKey, Source)>) -> bool {
    if old.contains(&node) {
        out.push((node, Source::Drawn));
        return true;
    }
    node.children()
        .is_some_and(|children| children.iter().all(|&c| cover(c, old, out)))
}

/// Two chunks of `leaves` that touch along a side or at a corner and are
/// more than a level apart, the finer first, if any.
fn unbalanced(leaves: &BTreeSet<ChunkKey>) -> Option<(ChunkKey, ChunkKey)> {
    leaves.iter().find_map(|&key| {
        AROUND.iter().find_map(|&(dx, dz)| {
            let coarse = covering(key.offset(dx, dz), leaves)?;
            (coarse.level > key.level + 1).then_some((key, coarse))
        })
    })
}

/// Adds `key` to `leaves`, or its children recursively where `split` says.
fn add_split(key: ChunkKey, split: &impl Fn(ChunkKey) -> bool, leaves: &mut BTreeSet<ChunkKey>) {
    match key.children().filter(|_| split(key)) {
        Some(children) => {
            for child in children {
                add_split(child, split, leaves);
            }
        }
        None => {
            leaves.insert(key);
        }
    }
}

/// The leaf of `leaves` that holds chunk `key` or a chunk around it at a
/// coarser level, if any.
fn covering(key: ChunkKey, leaves: &BTreeSet<ChunkKey>) -> Option<ChunkKey> {
    let mut at = Some(key);
    while let Some(k) = at {
        if leaves.contains(&k) {
            return Some(k);
        }
        at = k.parent();
    }
    None
}

/// The eight directions to a chunk's neighbours.
const AROUND: [(i64, i64); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// Splits leaves until every pair touching along a side or at a corner
/// is at most one level apart. Splitting only adds finer chunks where
/// coarse ones were, so it never uncovers ground.
fn balance(leaves: &mut BTreeSet<ChunkKey>) {
    let mut queue: Vec<ChunkKey> = leaves.iter().copied().collect();
    while let Some(key) = queue.pop() {
        if !leaves.contains(&key) {
            continue;
        }
        for (dx, dz) in AROUND {
            let Some(coarse) = covering(key.offset(dx, dz), leaves) else {
                continue;
            };
            if coarse.level > key.level + 1 {
                leaves.remove(&coarse);
                let children = coarse.children().expect("a coarse chunk has children");
                leaves.extend(children);
                queue.extend(children);
                // This chunk may need to look again at what replaced it.
                queue.push(key);
            }
        }
    }
}

/// Which sides and corners of `key` touch a finer leaf.
fn transitions_of(key: ChunkKey, leaves: &BTreeSet<ChunkKey>) -> Transitions {
    // Whether the same-level chunk at `(dx, dz)` is split into finer
    // leaves: not a leaf, not inside one, and with a leaf inside it.
    let finer = |dx: i64, dz: i64| -> bool {
        let neighbour = key.offset(dx, dz);
        covering(neighbour, leaves).is_none()
            && neighbour
                .children()
                .is_some_and(|c| c.iter().any(|c| leaves.contains(c)))
    };
    let mut transitions = Transitions::NONE;
    for side in ChunkSide::ALL {
        let (dx, dz) = side.direction();
        if finer(dx, dz) {
            transitions = transitions.with_side(side);
        }
    }
    for corner in ChunkCorner::ALL {
        let (dx, dz) = corner.direction();
        if finer(dx, dz) {
            transitions = transitions.with_corner(corner);
        }
    }
    transitions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_nest_and_cover_the_world_lattice() {
        let key = ChunkKey::new(-3, 5, 2);
        assert_eq!(key.size(), 256.0);
        assert_eq!(key.step(), 4.0);
        assert_eq!(key.min_corner(), DVec2::new(-768.0, 1280.0));
        let parent = key.parent().unwrap();
        assert_eq!(parent, ChunkKey::new(-2, 2, 3));
        assert!(key.is_within(parent));
        assert!(parent.children().unwrap().contains(&key));
        assert_eq!(
            ChunkKey::containing(DVec2::new(-0.5, 63.9), 0),
            ChunkKey::new(-1, 0, 0)
        );
        // A half step from the corner of chunk (1, 0) at level 1 is the
        // first vertex of chunk (2, 0) at level 0 plus one metre.
        assert_eq!(
            ChunkKey::new(1, 0, 1).half_point(1, 0),
            ChunkKey::new(2, 0, 0).half_point(2, 0)
        );
    }

    #[test]
    fn a_cell_fan_covers_the_cell_once() {
        // Every combination of split sides a corner cell can have.
        for transitions in [Transitions::NONE, Transitions::ALL] {
            for (i, j) in [
                (0, 0),
                (N - 1, 0),
                (0, N - 1),
                (N - 1, N - 1),
                (5, 0),
                (5, 7),
            ] {
                let mut points = Vec::new();
                let mut vertex = |u: i64, v: i64| {
                    points.push((u, v));
                    (points.len() - 1) as u32
                };
                let mut triangles = Vec::new();
                cell_triangles(i, j, transitions, &mut vertex, &mut triangles);
                let area: f64 = triangles
                    .iter()
                    .map(|t| {
                        let [a, b, c] = t.map(|k| {
                            let (u, v) = points[k as usize];
                            DVec2::new(u as f64, v as f64)
                        });
                        // Positive when the triangle faces up (x right, z
                        // down the page).
                        let (e, f) = (b - a, c - a);
                        e.y * f.x - e.x * f.y
                    })
                    .inspect(|&a| assert!(a > 0.0, "cell ({i}, {j}) has a flipped triangle"))
                    .sum::<f64>()
                    / 2.0;
                assert_eq!(area, 4.0, "cell ({i}, {j}) covers its 2 × 2 half steps");
            }
        }
    }
}
