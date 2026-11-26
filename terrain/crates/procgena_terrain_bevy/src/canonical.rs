//! Canonical ground in a stream: finest-level chunks drawn from their
//! canonical meshes, and how they meet the regular chunks around them.

use std::fmt;
use std::sync::Arc;

use bevy::prelude::*;
use glam::DVec2;
use procgena_terrain::tin::{
    ContractCache, ResidentTins, TinError, TinIndex, TinTolerance, build_chunk_with,
};
use procgena_terrain::{
    ChunkError, ChunkFeature, ChunkKey, ChunkMesh, ChunkSamples, ChunkSide, ChunkSource,
    FeatureKind, FeatureTier, Transitions, WaterMesh,
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
    /// Builds chunk `key`'s canonical mesh from `source` within its
    /// [`tin_tolerance`](ChunkSource::tin_tolerance), taking edge contracts
    /// from `contracts` (which must hold only `source`'s), with its shading
    /// normals and its water.
    pub(crate) fn build(
        source: &(impl ChunkSource + ?Sized),
        key: ChunkKey,
        contracts: &ContractCache,
    ) -> Result<Self, ReadError> {
        let source = AsDyn(source);
        let tolerance = source.tin_tolerance();
        let chunk = build_chunk_with(&source, key, &tolerance, contracts)?;
        let normals = chunk.shading_normals(&source)?;
        // The mesh has no rivers or ponds of its own yet, so their water
        // comes from the regular samples, read only where a river or pond
        // reaches the chunk.
        let wet = source
            .lines_within(key.min_corner(), key.max_corner(), FeatureTier::Fine)
            .iter()
            .any(|f| f.id.kind != FeatureKind::Road);
        let water = if wet {
            ChunkSamples::sample(&source, key, FeatureTier::Fine)?
                .mesh(Transitions::NONE, 0.0)
                .water
        } else {
            None
        };
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
    fn lines_within(&self, min: DVec2, max: DVec2, tier: FeatureTier) -> Vec<ChunkFeature> {
        self.0.lines_within(min, max, tier)
    }
    fn tin_tolerance(&self) -> TinTolerance {
        self.0.tin_tolerance()
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use procgena_terrain::tin::build_chunk;
    use procgena_terrain::{ChunkFeatureShape, FeatureId, FeatureLine, SurfacePoint};

    use super::*;

    /// Gentle swells, with a river along z = 20 m when `river`: its water
    /// 3 m either side of its line. Counts the heights read.
    struct Field {
        river: bool,
        reads: AtomicUsize,
    }

    impl Field {
        fn new(river: bool) -> Self {
            Self {
                river,
                reads: AtomicUsize::new(0),
            }
        }

        fn take(&self) -> usize {
            self.reads.swap(0, Ordering::Relaxed)
        }
    }

    impl ChunkSource for Field {
        fn surface(&self, p: DVec2, _: FeatureTier) -> SurfacePoint {
            self.reads.fetch_add(1, Ordering::Relaxed);
            let wet = self.river && (p.y - 20.0).abs() < 3.0;
            SurfacePoint {
                height: (10.0 + 2.0 * (p.x / 41.0).sin() - if wet { 1.0 } else { 0.0 }) as f32,
                water: wet.then_some(10.0),
            }
        }
        fn tier(&self, _: FeatureTier) -> FeatureTier {
            FeatureTier::Fine
        }
        fn tier_difference(&self, _: DVec2, _: DVec2, _: FeatureTier) -> f32 {
            0.0
        }
        fn lines_within(&self, _: DVec2, _: DVec2, _: FeatureTier) -> Vec<ChunkFeature> {
            if !self.river {
                return Vec::new();
            }
            let points = vec![DVec2::new(-100.0, 20.0), DVec2::new(100.0, 20.0)];
            vec![ChunkFeature {
                id: FeatureId {
                    kind: FeatureKind::River,
                    key: 1,
                },
                shape: ChunkFeatureShape::Line(FeatureLine {
                    kind: FeatureKind::River,
                    points,
                    levels: vec![10.0, 10.0],
                    widths: vec![6.0, 6.0],
                    depth: 1.0,
                }),
            }]
        }
    }

    #[test]
    fn a_dry_chunk_reads_its_mesh_and_normals_only() {
        // No river or pond reaches it, so it reads no regular samples for
        // water: only what its mesh needs, and four heights a vertex for
        // its shading normals.
        let field = Field::new(false);
        let key = ChunkKey::new(0, 0, 0);
        let mesh = build_chunk(&field, key, &field.tin_tolerance()).unwrap();
        let mesh_reads = field.take();
        let read =
            TinRead::build(&field, key, &ContractCache::new(&field.tin_tolerance())).unwrap();
        assert!(read.water.is_none());
        assert_eq!(field.take(), mesh_reads + 4 * mesh.vertices.len());
    }

    #[test]
    fn a_chunk_with_a_river_keeps_its_water() {
        let field = Field::new(true);
        let key = ChunkKey::new(0, 0, 0);
        let read =
            TinRead::build(&field, key, &ContractCache::new(&field.tin_tolerance())).unwrap();
        assert!(read.water.is_some(), "the river's water is drawn");
    }
}
