//! Chunk edge contracts: the vertices along a border between two chunks,
//! which both chunks use, so independently built chunks meet exactly.

use glam::DVec2;
use procgena_core::ContentHasher;

use crate::chunk::{CHUNK_SIZE, ChunkKey, ChunkSource};
use crate::feature::{FeatureId, FeatureLevel};

use super::mesh::{ConstraintKind, TinVertex};
use super::numeric::{CM_PER_M, Cm, CmHeight};
use super::probe::{ProbeAddr, TinError, sample_heights};
use super::resolve::Pt;
use super::sampling::STEP_CM;
use super::tolerance::TinTolerance;
use super::water::Water;

/// The width of a finest-level chunk in whole centimetres.
pub(crate) const CHUNK_CM: i64 = (CHUNK_SIZE * CM_PER_M) as i64;

/// How far apart the ground is sampled along a chunk edge, in centimetres:
/// every lattice point. Contract vertices sit on these samples.
const EDGE_PROBE_CM: i64 = STEP_CM;

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

    /// The edge's first corner, in centimetres.
    pub fn start(self) -> (Cm, Cm) {
        (Cm(self.x * CHUNK_CM), Cm(self.z * CHUNK_CM))
    }

    /// The edge from grid corner `corner` (centimetres, on the chunk grid)
    /// along `axis`.
    pub(crate) fn from_corner(corner: Pt, axis: EdgeAxis) -> Self {
        debug_assert!(corner.0 % CHUNK_CM == 0 && corner.1 % CHUNK_CM == 0);
        Self {
            x: corner.0.div_euclid(CHUNK_CM),
            z: corner.1.div_euclid(CHUNK_CM),
            axis,
        }
    }

    /// The edge's last corner, in centimetres.
    fn end(self) -> (Cm, Cm) {
        self.at(CHUNK_CM)
    }

    /// The point `along` centimetres from the first corner.
    fn at(self, along: i64) -> (Cm, Cm) {
        let (x, z) = self.start();
        match self.axis {
            EdgeAxis::X => (Cm(x.0 + along), z),
            EdgeAxis::Z => (x, Cm(z.0 + along)),
        }
    }
}

/// Where a feature line crosses a chunk edge: a vertex of the edge's
/// contract that the line's piece in each chunk ends at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdgeCrossing {
    /// Where, x and z.
    pub at: (Cm, Cm),
    /// What the line is.
    pub kind: ConstraintKind,
    /// The feature it follows.
    pub feature: FeatureId,
}

/// The vertices along one chunk edge, from its first corner to its last,
/// both corners included, and where feature lines cross it. Built only
/// from the edge and the ground, so the chunks either side build the same
/// contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeContract {
    pub key: ChunkEdgeKey,
    pub vertices: Vec<TinVertex>,
    /// The feature lines crossing the edge, sorted by position, then by
    /// kind and feature. Each is at one of `vertices`.
    pub crossings: Vec<EdgeCrossing>,
}

impl EdgeContract {
    /// The contract for edge `key` over the ground of `source` at
    /// [`FeatureLevel::FINEST`], within `tolerance`.
    ///
    /// The ground is sampled every metre along the edge, and where each
    /// river or pond line (see [`ChunkSource::lines_within`]) crosses it.
    /// The corners and the crossings are always vertices; a water's edge
    /// crossing inside other water is not a crossing. Then a stretch between
    /// two vertices gets a new vertex at the sample it misses most among
    /// those it misses by more than their height tolerance, or at its middle
    /// sample if it is longer than its longest edge allows. Samples within
    /// `tolerance.water_band` of water follow `tolerance.water`, and a
    /// stretch with any such sample takes the water's longest edge; the
    /// rest follow `tolerance.general`.
    ///
    /// # Panics
    ///
    /// If `tolerance` is not valid.
    pub fn build(
        source: &dyn ChunkSource,
        key: ChunkEdgeKey,
        tolerance: &TinTolerance,
    ) -> Result<Self, TinError> {
        Self::build_sampled(source, key, tolerance).map(|(contract, _)| contract)
    }

    /// [`build`](Self::build), with the lattice points it sampled along
    /// the edge: how far each is from the first corner (in lattice steps)
    /// and its height.
    pub(crate) fn build_sampled(
        source: &dyn ChunkSource,
        key: ChunkEdgeKey,
        tolerance: &TinTolerance,
    ) -> Result<(Self, Vec<(usize, CmHeight)>), TinError> {
        assert!(tolerance.is_valid(), "invalid tolerance {tolerance:?}");
        let (start, end) = (key.start(), key.end());
        let corner_m = |(x, z): (Cm, Cm)| DVec2::new(x.to_m(), z.to_m());
        let (lo, hi) = (corner_m(start), corner_m(end));
        let features = source.lines_within(lo, hi, FeatureLevel::FINEST);
        let water = Water::new(&features, tolerance, lo, hi);

        // Where the lines cross, unless a water's edge is inside water.
        let mut crossings: Vec<EdgeCrossing> = water
            .pieces()
            .iter()
            .filter_map(|piece| {
                let at = piece.edge_crossing(key)?;
                (!water.hidden_at(piece, at)).then_some(EdgeCrossing {
                    at: (Cm(at.0), Cm(at.1)),
                    kind: piece.kind,
                    feature: piece.feature,
                })
            })
            .collect();
        crossings.sort();
        crossings.dedup();

        // Samples every metre, corners included, and at each crossing; the
        // corners and crossings are kept.
        let along = |at: (Cm, Cm)| (at.0.0 - start.0.0) + (at.1.0 - start.1.0);
        let mut stations: Vec<(i64, bool)> = (0..=CHUNK_CM / EDGE_PROBE_CM)
            .map(|i| (i * EDGE_PROBE_CM, i == 0 || i == CHUNK_CM / EDGE_PROBE_CM))
            .chain(crossings.iter().map(|c| (along(c.at), true)))
            .collect();
        stations.sort();
        stations.dedup_by(|later, earlier| {
            let same = later.0 == earlier.0;
            if same {
                earlier.1 |= later.1;
            }
            same
        });
        let count = stations.len();
        let probes: Vec<ProbeAddr> = stations
            .iter()
            .map(|&(s, _)| {
                let (x, z) = key.at(s);
                ProbeAddr::new(x, z)
            })
            .collect();
        let heights = sample_heights(source, &probes)?;
        let near_water: Vec<bool> = probes
            .iter()
            .map(|p| water.gap(p.to_m()) < tolerance.water_band)
            .collect();
        let domain = |i: usize| {
            if near_water[i] {
                &tolerance.water
            } else {
                &tolerance.general
            }
        };

        // Split stretches until each is close enough and short enough.
        let mut kept: Vec<bool> = stations.iter().map(|&(_, k)| k).collect();
        let mut stack: Vec<(usize, usize)> = Vec::new();
        let ends: Vec<usize> = (0..count).filter(|&i| kept[i]).collect();
        for w in ends.windows(2).rev() {
            stack.push((w[0], w[1]));
        }
        while let Some((a, b)) = stack.pop() {
            if b - a < 2 {
                continue;
            }
            let (ha, hb) = (f64::from(heights[a].0), f64::from(heights[b].0));
            let (sa, sb) = (stations[a].0, stations[b].0);
            // The worst sample missed by more than its tolerance, the first
            // on a tie.
            let worst = (a + 1..b)
                .filter_map(|i| {
                    let t = (stations[i].0 - sa) as f64 / (sb - sa) as f64;
                    let error = (f64::from(heights[i].0) - (ha + (hb - ha) * t)).abs();
                    (error > domain(i).height * CM_PER_M).then_some((i, error))
                })
                .fold(None, |best: Option<(usize, f64)>, c| match best {
                    Some(best) if c.1 <= best.1 => Some(best),
                    _ => Some(c),
                });
            let max_edge = if (a..=b).any(|i| near_water[i]) {
                tolerance.water.max_edge
            } else {
                tolerance.general.max_edge
            };
            let split = if let Some((worst, _)) = worst {
                worst
            } else if (sb - sa) as f64 > max_edge * CM_PER_M {
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
        let samples = stations
            .iter()
            .zip(&heights)
            .filter(|((s, _), _)| s % EDGE_PROBE_CM == 0)
            .map(|(&(s, _), &h)| ((s / EDGE_PROBE_CM) as usize, h))
            .collect();
        Ok((
            Self {
                key,
                vertices,
                crossings,
            },
            samples,
        ))
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
        h.write_usize(self.crossings.len());
        for c in &self.crossings {
            h.write_i64(c.at.0.0);
            h.write_i64(c.at.1.0);
            h.write_u32(c.kind.code());
            h.write_u64(c.feature.key);
        }
        h.finish()
    }
}
