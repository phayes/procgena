//! Chunk edge contracts: the vertices along a border between two chunks,
//! which both chunks use, so independently built chunks meet exactly.

use procgena_core::ContentHasher;

use crate::chunk::{CHUNK_SIZE, ChunkKey, ChunkSource};

use super::mesh::TinVertex;
use super::numeric::{MM_PER_M, Mm};
use super::probe::{ProbeAddr, TinError, sample_heights};
use super::tolerance::TinTolerance;

/// The width of a finest-level chunk in whole millimetres.
const CHUNK_MM: i64 = (CHUNK_SIZE * MM_PER_M) as i64;

/// How far apart the ground is sampled along a chunk edge, in millimetres.
/// Contract vertices sit on these samples.
const EDGE_PROBE_MM: i64 = 1000;

/// Which way a chunk edge runs from its first corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EdgeAxis {
    /// Along +x.
    X,
    /// Along +z.
    Z,
}

/// One edge of the finest chunk grid: from the grid corner `(x, z)` (in
/// chunks, so at `(x, z) * CHUNK_SIZE` metres) one chunk's width along
/// `axis`. The two chunks either side of it name it the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkEdgeKey {
    pub x: i64,
    pub z: i64,
    pub axis: EdgeAxis,
}

impl ChunkEdgeKey {
    /// The four edges of finest-level chunk `key`: its low-z, high-x,
    /// high-z and low-x sides, in that order (round the chunk from its
    /// smallest corner, towards +x first).
    ///
    /// # Panics
    ///
    /// If `key` is not at level 0.
    pub fn of_chunk(key: ChunkKey) -> [Self; 4] {
        assert_eq!(key.level, 0, "chunk edges are for level 0 chunks");
        let (x, z) = (key.x, key.z);
        [
            Self {
                x,
                z,
                axis: EdgeAxis::X,
            },
            Self {
                x: x + 1,
                z,
                axis: EdgeAxis::Z,
            },
            Self {
                x,
                z: z + 1,
                axis: EdgeAxis::X,
            },
            Self {
                x,
                z,
                axis: EdgeAxis::Z,
            },
        ]
    }

    /// The edge's first corner, in millimetres.
    pub fn start(self) -> (Mm, Mm) {
        (Mm(self.x * CHUNK_MM), Mm(self.z * CHUNK_MM))
    }

    /// The point `along` millimetres from the first corner.
    fn at(self, along: i64) -> (Mm, Mm) {
        let (x, z) = self.start();
        match self.axis {
            EdgeAxis::X => (Mm(x.0 + along), z),
            EdgeAxis::Z => (x, Mm(z.0 + along)),
        }
    }
}

/// The vertices along one chunk edge, from its first corner to its last,
/// both corners included. Built only from the edge and the ground, so the
/// chunks either side build the same contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeContract {
    pub key: ChunkEdgeKey,
    pub vertices: Vec<TinVertex>,
}

impl EdgeContract {
    /// The contract for edge `key` over the ground of `source` at the fine
    /// tier, within `tolerance`.
    ///
    /// The ground is sampled every metre along the edge. Starting from the
    /// two corners, a stretch between two vertices gets a new vertex where
    /// the straight line between them misses the samples by the most, while
    /// that is more than `tolerance.height` or the stretch is longer than
    /// `tolerance.max_edge`.
    ///
    /// # Panics
    ///
    /// If `tolerance` is not valid.
    pub fn build(
        source: &dyn ChunkSource,
        key: ChunkEdgeKey,
        tolerance: &TinTolerance,
    ) -> Result<Self, TinError> {
        assert!(tolerance.is_valid(), "invalid tolerance {tolerance:?}");
        // Samples every metre, corners included.
        let count = (CHUNK_MM / EDGE_PROBE_MM) as usize + 1;
        let probes: Vec<ProbeAddr> = (0..count)
            .map(|i| {
                let (x, z) = key.at(i as i64 * EDGE_PROBE_MM);
                ProbeAddr::new(x, z)
            })
            .collect();
        let heights = sample_heights(source, &probes)?;

        // Split stretches until each is close enough and short enough.
        let height_mm = tolerance.height * MM_PER_M;
        let max_samples = ((tolerance.max_edge * MM_PER_M) as i64 / EDGE_PROBE_MM).max(1) as usize;
        let mut kept = vec![false; count];
        let mut stack = vec![(0, count - 1)];
        kept[0] = true;
        kept[count - 1] = true;
        while let Some((a, b)) = stack.pop() {
            if b - a < 2 {
                continue;
            }
            let (ha, hb) = (f64::from(heights[a].0), f64::from(heights[b].0));
            // The worst sample between them, the first on a tie.
            let (worst, error) = (a + 1..b)
                .map(|i| {
                    let t = (i - a) as f64 / (b - a) as f64;
                    (i, (f64::from(heights[i].0) - (ha + (hb - ha) * t)).abs())
                })
                .fold((a + 1, -1.0), |best, c| if c.1 > best.1 { c } else { best });
            let split = if error > height_mm {
                worst
            } else if b - a > max_samples {
                (a + b) / 2
            } else {
                continue;
            };
            kept[split] = true;
            stack.push((a, split));
            stack.push((split, b));
        }
        let vertices = (0..count)
            .filter(|&i| kept[i])
            .map(|i| TinVertex {
                x: probes[i].x,
                z: probes[i].z,
                h: heights[i],
            })
            .collect();
        Ok(Self { key, vertices })
    }

    /// A hash of the contract, equal on every platform for equal
    /// contracts.
    pub fn content_hash(&self) -> u64 {
        let mut h = ContentHasher::new();
        h.write_str("procgena_terrain::EdgeContract");
        h.write_i64(self.key.x);
        h.write_i64(self.key.z);
        h.write_u32(match self.key.axis {
            EdgeAxis::X => 0,
            EdgeAxis::Z => 1,
        });
        h.write_usize(self.vertices.len());
        for v in &self.vertices {
            h.write_i64(v.x.0);
            h.write_i64(v.z.0);
            h.write_i32(v.h.0);
        }
        h.finish()
    }
}
