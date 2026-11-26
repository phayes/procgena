//! Canonical ground in a stream: finest-level chunks drawn from their
//! canonical meshes, and how they meet the regular chunks around them.

use std::fmt;
use std::sync::Arc;

use bevy::prelude::*;
use glam::DVec2;
use procgena_terrain::tin::{ResidentTins, TinError, TinIndex, TinTolerance};
use procgena_terrain::{
    ChunkError, ChunkKey, ChunkMesh, ChunkSamples, ChunkSide, ChunkSource, FeatureTier,
    Transitions, WaterMesh,
};

/// The canonical ground a [`TerrainStream`](crate::TerrainStream) has
/// loaded, on the stream's entity: the finest-level chunks it draws from
/// their canonical meshes (see [`TerrainStream::tin`](crate::TerrainStream::tin)).
///
/// Ask it for the physical ground near the viewers: where collision meets
/// the ground, and where nearby copies come to rest. It answers only for
/// the chunks loaded now, which follow the viewers, and never builds or
/// reads ground to answer. Placement, masks and slope tests read the
/// terrain's own ground instead.
#[derive(Component, Debug, Clone, Default)]
pub struct CanonicalGround {
    /// The loaded chunks.
    pub tins: ResidentTins,
}

/// A finest-level chunk's canonical mesh, ready to draw.
#[derive(Clone)]
pub(crate) struct TinRead {
    pub(crate) index: Arc<TinIndex>,
    /// One shading normal for each vertex.
    normals: Arc<Vec<Vec3>>,
    /// The water over the chunk, from its regular samples.
    water: Option<WaterMesh>,
}

impl TinRead {
    /// Builds chunk `key`'s canonical mesh from `source` within
    /// `tolerance`, its shading normals and its water.
    pub(crate) fn build(
        source: &(impl ChunkSource + ?Sized),
        key: ChunkKey,
        tolerance: &TinTolerance,
    ) -> Result<Self, ReadError> {
        let source = AsDyn(source);
        let chunk = procgena_terrain::tin::build_chunk(&source, key, tolerance)?;
        let normals = chunk.shading_normals(&source)?;
        // The mesh has no rivers or ponds of its own yet, so their water
        // comes from the regular samples.
        let water = ChunkSamples::sample(&source, key, FeatureTier::Fine)?
            .mesh(Transitions::NONE, 0.0)
            .water;
        Ok(Self {
            index: Arc::new(TinIndex::new(Arc::new(chunk))),
            normals: Arc::new(normals),
            water,
        })
    }

    /// The mesh to draw, with `split` sides split for regular neighbours and
    /// skirts `skirt` metres deep.
    pub(crate) fn mesh(&self, split: &[ChunkSide], skirt: f32) -> ChunkMesh {
        let mut mesh = self.index.chunk().render_mesh(&self.normals, split, skirt);
        if let Some(water) = &self.water {
            for p in &water.positions {
                mesh.heights.0 = mesh.heights.0.min(p.y);
                mesh.heights.1 = mesh.heights.1.max(p.y);
            }
            mesh.water = Some(water.clone());
        }
        mesh
    }

    /// The canonical height under `p`, if it is in this chunk.
    pub(crate) fn height(&self, p: DVec2) -> Option<f32> {
        Some(self.index.sample(p)?.height as f32)
    }
}

/// Lets a source of unknown size be passed where `&dyn ChunkSource` is
/// needed.
struct AsDyn<'a, S: ?Sized>(&'a S);

impl<S: ChunkSource + ?Sized> ChunkSource for AsDyn<'_, S> {
    fn surface(&self, p: DVec2, tier: FeatureTier) -> procgena_terrain::SurfacePoint {
        self.0.surface(p, tier)
    }
    fn surfaces(&self, points: &[DVec2], tier: FeatureTier) -> Vec<procgena_terrain::SurfacePoint> {
        self.0.surfaces(points, tier)
    }
    fn tier(&self, tier: FeatureTier) -> FeatureTier {
        self.0.tier(tier)
    }
    fn tier_difference(&self, min: DVec2, max: DVec2, from: FeatureTier) -> f32 {
        self.0.tier_difference(min, max, from)
    }
}

/// Why a chunk could not be read.
#[derive(Debug, Clone)]
pub(crate) enum ReadError {
    Chunk(ChunkError),
    Tin(TinError),
}

impl From<ChunkError> for ReadError {
    fn from(error: ChunkError) -> Self {
        Self::Chunk(error)
    }
}

impl From<TinError> for ReadError {
    fn from(error: TinError) -> Self {
        Self::Tin(error)
    }
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Chunk(error) => error.fmt(f),
            Self::Tin(error) => write!(f, "its canonical mesh failed: {error}"),
        }
    }
}

/// How a drawn chunk meets canonical chunks beside it.
#[derive(Clone, Default)]
pub(crate) struct Seams {
    /// For a canonical chunk: its sides that meet a regular chunk.
    pub(crate) split: Vec<ChunkSide>,
    /// For a regular chunk: the canonical chunks it touches.
    pub(crate) tins: Vec<Arc<TinIndex>>,
}

impl Seams {
    /// Whether these seams mesh a chunk the same way as `other`.
    pub(crate) fn same_as(&self, other: &Self) -> bool {
        self.split == other.split
            && self.tins.len() == other.tins.len()
            && self
                .tins
                .iter()
                .zip(&other.tins)
                .all(|(a, b)| Arc::ptr_eq(a, b))
    }
}

/// Whether `other` touches chunk `key` along a stretch of its `side`, not
/// just at a corner.
pub(crate) fn along_side(key: ChunkKey, side: ChunkSide, other: ChunkKey) -> bool {
    let (lo, hi) = (key.min_corner(), key.max_corner());
    let (a, b) = (other.min_corner(), other.max_corner());
    let overlaps = |lo1: f64, hi1: f64, lo2: f64, hi2: f64| lo1 < hi2 && lo2 < hi1;
    match side {
        ChunkSide::MinX => b.x == lo.x && overlaps(lo.y, hi.y, a.y, b.y),
        ChunkSide::MaxX => a.x == hi.x && overlaps(lo.y, hi.y, a.y, b.y),
        ChunkSide::MinZ => b.y == lo.y && overlaps(lo.x, hi.x, a.x, b.x),
        ChunkSide::MaxZ => a.y == hi.y && overlaps(lo.x, hi.x, a.x, b.x),
    }
}
