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
//!     Boundary, ChunkSamples, ChunkSelection, ChunkStage, Region, RegionId, TerrainGenerator,
//!     TerrainPreset, chunk_lod,
//! };
//!
//! // One unbounded region of rolling hills.
//! let region = Region { id: RegionId(0), boundary: Boundary::Unbounded, edges: vec![], corners: vec![] };
//! let terrain = TerrainGenerator::new(TerrainPreset::rolling_hills().terrain.clone())?
//!     .build(region, &[])?;
//!
//! // The chunks to draw for a viewer standing 2 m above the ground.
//! let lod = chunk_lod(600.0);
//! let chosen = ChunkSelection::around(&lod, 1.0, &[DVec3::new(0.0, 2.0, 0.0)], None)?;
//! let (key, transitions) = chosen.iter().next().unwrap();
//!
//! let samples = ChunkSamples::sample(&terrain, key, ChunkStage::Canonical)?;
//! let mesh = samples.mesh(transitions, 0.0);
//! assert!(!mesh.indices.is_empty());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use glam::{DVec2, DVec3, Vec3};
use procgena_core::{Lod, LodLevel};

use crate::{Terrain, WorldTerrain};

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
    fn half_point(self, u: i64, v: i64) -> DVec2 {
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

/// How complete the ground a chunk was built from is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ChunkStage {
    /// The terrain with every planning stage, roads included: the same
    /// ground placement and queries read.
    #[default]
    Canonical,
    /// The terrain without its roads, which are the slowest stage to plan.
    /// Good enough to draw far away while the roads are routed; never for
    /// placement or queries. It matches the canonical ground everywhere
    /// except near roads and their border crossings.
    Provisional,
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

/// Something chunks can be built from: a terrain, or a world of them.
pub trait ChunkSource: Sync {
    /// The ground at world position `p` (x and z, metres), at `stage` or
    /// as near to it as the source has (see [`stage`](Self::stage)).
    fn surface(&self, p: DVec2, stage: ChunkStage) -> SurfacePoint;

    /// The stage [`surface`](Self::surface) gives when asked for `stage`.
    fn stage(&self, stage: ChunkStage) -> ChunkStage;
}

impl ChunkSource for Terrain {
    /// The terrain's own ground, whatever stage is asked for.
    fn surface(&self, p: DVec2, _: ChunkStage) -> SurfacePoint {
        let sample = self.query(p, false);
        SurfacePoint {
            height: sample.height,
            water: sample.water,
        }
    }

    /// [`Canonical`](ChunkStage::Canonical) once the terrain has its roads
    /// (see [`Terrain::with_roads`]), otherwise
    /// [`Provisional`](ChunkStage::Provisional).
    fn stage(&self, _: ChunkStage) -> ChunkStage {
        if self.has_roads() {
            ChunkStage::Canonical
        } else {
            ChunkStage::Provisional
        }
    }
}

impl ChunkSource for WorldTerrain {
    /// The ground of the region holding `p`: its full terrain, or for
    /// [`Provisional`](ChunkStage::Provisional) its terrain without roads
    /// (see [`WorldTerrain::provisional_terrain`]). NaN heights where the
    /// region can't be built.
    fn surface(&self, p: DVec2, stage: ChunkStage) -> SurfacePoint {
        let region = self.region_at(p);
        let terrain = match stage {
            ChunkStage::Canonical => self.terrain(region),
            ChunkStage::Provisional => self.provisional_terrain(region),
        };
        match terrain {
            Ok(terrain) => terrain.surface(p, stage),
            Err(_) => SurfacePoint {
                height: f32::NAN,
                water: None,
            },
        }
    }

    /// Always the stage asked for.
    fn stage(&self, stage: ChunkStage) -> ChunkStage {
        stage
    }
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
    stage: ChunkStage,
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
    /// Reads the ground of chunk `key` from `source` at `stage`.
    ///
    /// # Errors
    ///
    /// [`ChunkError::NoGround`] if any point of the chunk, or of the ring
    /// just around it, has no ground.
    pub fn sample(
        source: &(impl ChunkSource + ?Sized),
        key: ChunkKey,
        stage: ChunkStage,
    ) -> Result<Self, ChunkError> {
        let surface = |u: i64, v: i64| {
            let p = key.half_point(u, v);
            let point = source.surface(p, stage);
            if point.height.is_finite() {
                Ok(point)
            } else {
                Err(ChunkError::NoGround(p))
            }
        };

        // The vertex lattice and its ring, row by row.
        let width = N + 3;
        let row = |j: i64| -> Result<Vec<SurfacePoint>, ChunkError> {
            (0..width)
                .map(|i| surface(2 * (i - 1), 2 * (j - 1)))
                .collect()
        };
        let grid = rows(width, row)?.concat();

        // Three half-step rows along each side.
        let mut sides: [[Vec<SurfacePoint>; 3]; 4] = Default::default();
        if key.level > 0 {
            let length = 2 * N + 3;
            for (s, side) in ChunkSide::ALL.into_iter().enumerate() {
                let row = |t: i64| -> Result<Vec<SurfacePoint>, ChunkError> {
                    (0..length)
                        .map(|k| {
                            let (u, v) = side_point(side, k - 1, t - 1);
                            surface(u, v)
                        })
                        .collect()
                };
                let [outside, on, inside] = rows(3, row)?.try_into().expect("three rows");
                sides[s] = [outside, on, inside];
            }
        }

        Ok(Self {
            key,
            stage: source.stage(stage),
            grid,
            sides,
        })
    }

    /// The chunk these samples are of.
    pub fn key(&self) -> ChunkKey {
        self.key
    }

    /// The stage of the ground the samples were read from.
    pub fn stage(&self) -> ChunkStage {
        self.stage
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
    /// other ground, such as a provisional chunk beside a canonical one.
    /// Chunks of the same stage, at the same level or one level apart,
    /// join without gaps when each is meshed with its own transitions.
    pub fn mesh(&self, transitions: Transitions, skirt: f32) -> ChunkMesh {
        let step = self.key.step() as f32;
        let half = 0.5 * step;
        let mut mesh = ChunkMesh {
            key: self.key,
            transitions,
            stage: self.stage,
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

/// Builds `count` rows with `row`, in parallel with the `rayon` feature.
fn rows<T: Send>(
    count: i64,
    row: impl Fn(i64) -> Result<T, ChunkError> + Sync,
) -> Result<Vec<T>, ChunkError> {
    #[cfg(feature = "rayon")]
    {
        use rayon::prelude::*;
        (0..count).into_par_iter().map(&row).collect()
    }
    #[cfg(not(feature = "rayon"))]
    {
        (0..count).map(row).collect()
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
    /// The stage of the ground it was built from.
    pub stage: ChunkStage,
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
/// level 0 out to 128 m, and each level after that out to twice as far, up
/// to level 6 (4 km chunks). Levels that would start too close to the view
/// distance to be seen are left out, so any positive view distance gives
/// levels [`ChunkSelection::around`] accepts.
///
/// Level `i` of the `Lod` is chunk level `i`: its distance is where that
/// chunk level starts, and its cull distance is `view_distance`.
pub fn chunk_lod(view_distance: f32) -> Lod {
    const HYSTERESIS: f32 = 0.1;
    let mut levels = vec![LodLevel::new(0.0, HYSTERESIS, ())];
    levels.extend(
        (1..=6)
            .map(|level| LodLevel::new(64.0 * f32::powi(2.0, level), HYSTERESIS, ()))
            // A level must finish fading in before the ground fades out.
            .take_while(|level| {
                level.distance * (1.0 + HYSTERESIS) <= view_distance * (1.0 - HYSTERESIS)
            }),
    );
    Lod::new(CHUNK_SIZE as f32, levels).with_cull(view_distance)
}

/// Why chunks could not be chosen.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ChunkSelectionError {
    /// The levels are not valid (see [`Lod::validate`]).
    Lod(procgena_core::LodError),
    /// The levels have no cull distance, so there is no end to the view.
    NoCullDistance,
    /// More levels than [`MAX_CHUNK_LEVEL`] allows.
    TooManyLevels(usize),
    /// The factor is not a positive number.
    Factor(f32),
}

impl fmt::Display for ChunkSelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lod(e) => write!(f, "invalid chunk levels: {e}"),
            Self::NoCullDistance => write!(f, "the chunk levels need a cull distance"),
            Self::TooManyLevels(n) => write!(
                f,
                "{n} chunk levels, but at most {} are allowed",
                MAX_CHUNK_LEVEL as usize + 1
            ),
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
    /// The chunks to draw around `viewers`, with chunk levels from `lod`
    /// (see [`chunk_lod`]) scaled by `factor`.
    ///
    /// Level `i` of `lod` is chunk level `i`, and nothing is drawn past its
    /// cull distance. A chunk is split into finer ones while its nearest
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
    /// If `lod` is invalid, has no cull distance or has more than
    /// [`MAX_CHUNK_LEVEL`] + 1 levels, or `factor` isn't positive.
    pub fn around(
        lod: &Lod,
        factor: f32,
        viewers: &[DVec3],
        previous: Option<&ChunkSelection>,
    ) -> Result<Self, ChunkSelectionError> {
        lod.validate().map_err(ChunkSelectionError::Lod)?;
        let cull = lod
            .cull_distance
            .ok_or(ChunkSelectionError::NoCullDistance)?;
        if lod.levels.len() > MAX_CHUNK_LEVEL as usize + 1 {
            return Err(ChunkSelectionError::TooManyLevels(lod.levels.len()));
        }
        if !(factor.is_finite() && factor > 0.0) {
            return Err(ChunkSelectionError::Factor(factor));
        }
        let top = (lod.levels.len() - 1) as u8;
        let reach = f64::from(cull * factor);

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
            let level = &lod.levels[usize::from(key.level)];
            let start = f64::from(level.distance * factor);
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
