//! Canonical ground in a stream: each chunk, at every level, built and
//! drawn from its canonical mesh.

use std::fmt;
use std::sync::Arc;

use bevy::prelude::*;
use glam::DVec2;
use procgena_terrain::tin::{
    ContractCache, ResidentTins, TinError, TinIndex, TinTolerance, build_chunk_with,
};
use procgena_terrain::{
    ChunkError, ChunkFeature, ChunkKey, ChunkMesh, ChunkSamples, ChunkSource, FeatureKind,
    FeatureLevel, Transitions, WaterMesh,
};

use crate::levels::Ground;

/// The ground a [`TerrainStream`](crate::TerrainStream) draws now, on the
/// stream's entity: the canonical mesh of every drawn chunk, at every level.
///
/// Ask it where things rest on the ground you see: where collision meets
/// the ground near the viewers, and what height copies of Scatters stand
/// at, at any distance. Drawn chunks never overlap, so each point has one
/// answer, from the chunk drawn there. It answers only for the chunks
/// drawn now, which follow the viewers, and never builds or reads ground to
/// answer. When the drawn ground changes, the stream sends a
/// [`TerrainChanged`](crate::TerrainChanged) naming where.
#[derive(Component, Debug, Clone, Default)]
pub struct CanonicalGround {
    /// The drawn chunks.
    pub tins: ResidentTins,
}

/// A chunk's canonical mesh, ready to draw.
pub(crate) struct TinRead {
    pub(crate) index: Arc<TinIndex>,
    /// One shading normal for each vertex.
    normals: Vec<Vec3>,
    /// The water over the chunk, from its regular samples.
    water: Option<WaterMesh>,
}

impl TinRead {
    /// Builds chunk `key`'s canonical mesh from `source` within its
    /// [`tin_tolerance`](ChunkSource::tin_tolerance), taking edge contracts
    /// from `contracts` (which must hold only `source`'s), with its shading
    /// normals, and its water at level 0.
    pub(crate) fn build(
        source: &(impl ChunkSource + ?Sized),
        key: ChunkKey,
        contracts: &ContractCache,
    ) -> Result<Self, ReadError> {
        let source = AsDyn(source);
        let tolerance = source.tin_tolerance();
        let chunk = build_chunk_with(&source, key, &tolerance, contracts)?;
        let normals = chunk.shading_normals(&source)?;
        // The mesh has no rivers or ponds of its own yet, so at level 0
        // their water comes from the chunk's regular samples, read only
        // where a river or pond reaches it. Coarser levels draw no water
        // until the mesh carries it.
        let wet = key.level == 0
            && source
                .lines_within(key.min_corner(), key.max_corner(), FeatureLevel::FINEST)
                .iter()
                .any(|f| f.id.kind != FeatureKind::Road);
        let water = if wet {
            ChunkSamples::sample(&source, key, FeatureLevel::FINEST)?
                .mesh(Transitions::NONE, 0.0)
                .water
        } else {
            None
        };
        Ok(Self {
            index: Arc::new(TinIndex::new(Arc::new(chunk))),
            normals,
            water,
        })
    }

    /// The mesh to draw, with skirts `skirt` metres deep.
    pub(crate) fn mesh(&self, skirt: f32) -> ChunkMesh {
        let mut mesh = self.index.chunk().render_mesh(&self.normals, &[], skirt);
        if let Some(water) = &self.water {
            for p in &water.positions {
                mesh.heights.0 = mesh.heights.0.min(p.y);
                mesh.heights.1 = mesh.heights.1.max(p.y);
            }
            mesh.water = Some(water.clone());
        }
        mesh
    }
}

impl Ground for TinRead {
    /// The canonical height under `p`, if it is in this chunk.
    fn height(&self, p: DVec2) -> Option<f32> {
        Some(self.index.sample(p)?.height as f32)
    }
}

/// The largest height difference between `a` (drawn for chunk `a_key`) and
/// `b` (for `b_key`) along the stretch of side they share, sampled at the
/// finer chunk's step; 0 if they share no stretch of side.
pub(crate) fn edge_gap(
    (a_key, a): (ChunkKey, &impl Ground),
    (b_key, b): (ChunkKey, &impl Ground),
) -> f32 {
    let (a_lo, a_hi) = (a_key.min_corner(), a_key.max_corner());
    let (b_lo, b_hi) = (b_key.min_corner(), b_key.max_corner());
    let lo = a_lo.max(b_lo);
    let hi = a_hi.min(b_hi);
    // A shared stretch of side is a segment: one coordinate equal, the
    // other a range of positive length.
    let along = hi - lo;
    if !(along.x == 0.0 && along.y > 0.0 || along.y == 0.0 && along.x > 0.0) {
        return 0.0;
    }
    let step = a_key.step().min(b_key.step());
    let samples = (along.length() / step).round() as usize;
    // A point on the shared side, nudged 1 cm into each chunk, since a
    // chunk owns only some of its sides.
    let inside = |p: DVec2, lo: DVec2, hi: DVec2| p.clamp(lo + 0.01, hi - 0.01);
    (0..=samples)
        .filter_map(|i| {
            let p = lo + along * (i as f64 / samples as f64);
            let ha = a.height(inside(p, a_lo, a_hi))?;
            let hb = b.height(inside(p, b_lo, b_hi))?;
            Some((ha - hb).abs())
        })
        .fold(0.0, f32::max)
}

/// Lets a source of unknown size be passed where `&dyn ChunkSource` is
/// needed.
struct AsDyn<'a, S: ?Sized>(&'a S);

impl<S: ChunkSource + ?Sized> ChunkSource for AsDyn<'_, S> {
    fn surface(&self, p: DVec2, level: FeatureLevel) -> procgena_terrain::SurfacePoint {
        self.0.surface(p, level)
    }
    fn surfaces(
        &self,
        points: &[DVec2],
        level: FeatureLevel,
    ) -> Vec<procgena_terrain::SurfacePoint> {
        self.0.surfaces(points, level)
    }
    fn level(&self, level: FeatureLevel) -> FeatureLevel {
        self.0.level(level)
    }
    fn level_difference(&self, min: DVec2, max: DVec2, from: FeatureLevel) -> f32 {
        self.0.level_difference(min, max, from)
    }
    fn lines_within(&self, min: DVec2, max: DVec2, level: FeatureLevel) -> Vec<ChunkFeature> {
        self.0.lines_within(min, max, level)
    }
    fn tin_tolerance(&self) -> TinTolerance {
        self.0.tin_tolerance()
    }
    fn level_changes(&self, key: ChunkKey, from: FeatureLevel, to: FeatureLevel) -> bool {
        self.0.level_changes(key, from, to)
    }
    fn level_change_known(
        &self,
        key: ChunkKey,
        from: FeatureLevel,
        to: FeatureLevel,
    ) -> Option<bool> {
        self.0.level_change_known(key, from, to)
    }
}

/// Why a chunk could not be built.
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
            Self::Chunk(error) => write!(f, "its water failed: {error}"),
            Self::Tin(error) => write!(f, "its canonical mesh failed: {error}"),
        }
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
        fn surface(&self, p: DVec2, _: FeatureLevel) -> SurfacePoint {
            self.reads.fetch_add(1, Ordering::Relaxed);
            let wet = self.river && (p.y - 20.0).abs() < 3.0;
            SurfacePoint {
                height: (10.0 + 2.0 * (p.x / 41.0).sin() - if wet { 1.0 } else { 0.0 }) as f32,
                water: wet.then_some(10.0),
            }
        }
        fn level(&self, _: FeatureLevel) -> FeatureLevel {
            FeatureLevel::FINEST
        }
        fn level_difference(&self, _: DVec2, _: DVec2, _: FeatureLevel) -> f32 {
            0.0
        }
        fn lines_within(&self, _: DVec2, _: DVec2, _: FeatureLevel) -> Vec<ChunkFeature> {
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
        // water: only what its mesh needs, and four heights for each border
        // vertex's shading normal (inside vertices read none).
        let field = Field::new(false);
        let key = ChunkKey::new(0, 0, 0);
        let mesh = build_chunk(&field, key, &field.tin_tolerance()).unwrap();
        let mesh_reads = field.take();
        let read =
            TinRead::build(&field, key, &ContractCache::new(&field.tin_tolerance())).unwrap();
        assert!(read.water.is_none());
        let border = mesh
            .vertices
            .iter()
            .filter(|v| v.x.0 % 6_400 == 0 || v.z.0 % 6_400 == 0)
            .count();
        assert!(
            border < mesh.vertices.len(),
            "the chunk has inside vertices"
        );
        assert_eq!(field.take(), mesh_reads + 4 * border);
    }

    #[test]
    fn a_chunk_with_a_river_keeps_its_water() {
        let field = Field::new(true);
        let key = ChunkKey::new(0, 0, 0);
        let read =
            TinRead::build(&field, key, &ContractCache::new(&field.tin_tolerance())).unwrap();
        assert!(read.water.is_some(), "the river's water is drawn");
    }

    /// Flat ground at `height`, everywhere.
    struct Level(f32);

    impl Ground for Level {
        fn height(&self, _: DVec2) -> Option<f32> {
            Some(self.0)
        }
    }

    #[test]
    fn the_gap_between_chunks_is_read_only_along_a_shared_side() {
        let coarse = ChunkKey::new(0, 0, 1);
        let beside = ChunkKey::new(2, 0, 0);
        let corner = ChunkKey::new(2, 2, 0);
        assert_eq!(edge_gap((coarse, &Level(3.0)), (beside, &Level(1.5))), 1.5);
        assert_eq!(edge_gap((beside, &Level(1.5)), (coarse, &Level(3.0))), 1.5);
        assert_eq!(edge_gap((coarse, &Level(3.0)), (corner, &Level(0.0))), 0.0);
    }
}
