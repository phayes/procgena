//! Chunk edge contracts: the vertices along a border between two chunks,
//! which both chunks use, so independently built chunks meet exactly.

use glam::{DVec2, Vec3};
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

/// The width of a chunk at chunk level `level` in whole centimetres.
pub(crate) fn chunk_cm(level: u8) -> i64 {
    CHUNK_CM << level
}

/// How far apart lattice points are at chunk level `level`, in
/// centimetres: the ground is sampled at these along a chunk edge, and
/// contract vertices sit on them.
pub(crate) fn step_cm(level: u8) -> i64 {
    STEP_CM << level
}

/// Which way a chunk edge runs from its first corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EdgeAxis {
    /// Along +x.
    X,
    /// Along +z.
    Z,
}

/// One edge of the chunk grid at chunk level `level`: from the grid corner
/// `(x, z)` (in that level's chunks, so at `(x, z)` times the level's
/// chunk width) one chunk's width along `axis`. The two chunks either side
/// of it name it the same way. Edges of different levels are different
/// edges, even where they lie on the same line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkEdgeKey {
    pub x: i64,
    pub z: i64,
    pub axis: EdgeAxis,
    /// The chunk level, at most [`MAX_CHUNK_LEVEL`](crate::MAX_CHUNK_LEVEL).
    pub level: u8,
}

impl ChunkEdgeKey {
    /// The four edges of chunk `key`: its low-z, high-x, high-z and low-x
    /// sides, in that order (round the chunk from its smallest corner,
    /// towards +x first).
    pub fn of_chunk(key: ChunkKey) -> [Self; 4] {
        let (x, z, level) = (key.x, key.z, key.level);
        [
            Self {
                x,
                z,
                axis: EdgeAxis::X,
                level,
            },
            Self {
                x: x + 1,
                z,
                axis: EdgeAxis::Z,
                level,
            },
            Self {
                x,
                z: z + 1,
                axis: EdgeAxis::X,
                level,
            },
            Self {
                x,
                z,
                axis: EdgeAxis::Z,
                level,
            },
        ]
    }

    /// The edge's first corner, in centimetres.
    pub fn start(self) -> (Cm, Cm) {
        let width = chunk_cm(self.level);
        (Cm(self.x * width), Cm(self.z * width))
    }

    /// The edge at chunk level `level` from grid corner `corner`
    /// (centimetres, on that level's chunk grid) along `axis`.
    pub(crate) fn from_corner(corner: Pt, axis: EdgeAxis, level: u8) -> Self {
        let width = chunk_cm(level);
        debug_assert!(corner.0 % width == 0 && corner.1 % width == 0);
        Self {
            x: corner.0.div_euclid(width),
            z: corner.1.div_euclid(width),
            axis,
            level,
        }
    }

    /// The edge's last corner, in centimetres.
    fn end(self) -> (Cm, Cm) {
        self.at(chunk_cm(self.level))
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
/// both corners included, where feature lines cross it, and each vertex's
/// shading normal. Built only from the edge and the ground, so the chunks
/// either side build the same contract.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeContract {
    pub key: ChunkEdgeKey,
    pub vertices: Vec<TinVertex>,
    /// The feature lines crossing the edge, sorted by position, then by
    /// kind and feature. Each is at one of `vertices`.
    pub crossings: Vec<EdgeCrossing>,
    /// One upward unit shading normal for each of `vertices`, from the
    /// ground's heights one lattice step either side of the vertex along x
    /// and z. It depends only on the vertex's position and the edge's
    /// level, so every edge through a chunk corner gives the corner the
    /// same normal. At level 0 it is the normal a regular level-0 chunk
    /// gives its vertex there. Not part of
    /// [`content_hash`](Self::content_hash).
    pub normals: Vec<Vec3>,
}

/// The shading normal of the ground of `source` at each of `points` for
/// chunks at chunk level `level`: from the heights one lattice step
/// (2^`level` metres) either side along x and z, with every feature.
///
/// # Errors
///
/// [`TinError::NoGround`] (at the point) if any of the four heights is
/// missing.
pub(crate) fn border_normals(
    source: &dyn ChunkSource,
    points: &[(Cm, Cm)],
    level: u8,
) -> Result<Vec<Vec3>, TinError> {
    let step = step_cm(level) as f64 / CM_PER_M;
    let offsets = [
        DVec2::new(step, 0.0),
        DVec2::new(-step, 0.0),
        DVec2::new(0.0, step),
        DVec2::new(0.0, -step),
    ];
    let around: Vec<DVec2> = points
        .iter()
        .flat_map(|&(x, z)| {
            let at = DVec2::new(x.to_m(), z.to_m());
            offsets.map(|o| at + o)
        })
        .collect();
    let heights = source.surfaces(&around, FeatureLevel::FINEST);
    points
        .iter()
        .zip(heights.as_chunks::<4>().0)
        .map(|(&(x, z), s)| {
            let [east, west, north, south] = s.map(|p| p.height);
            if [east, west, north, south].iter().any(|h| h.is_nan()) {
                return Err(TinError::NoGround(ProbeAddr::new(x, z)));
            }
            let spacing = (2.0 * step) as f32;
            Ok(Vec3::new(west - east, spacing, south - north).normalize())
        })
        .collect()
}

impl EdgeContract {
    /// The contract for edge `key` over the ground of `source` at
    /// [`FeatureLevel::FINEST`], within `tolerance` as the edge's level
    /// follows it ([`TinTolerance::at_level`]). Pass the world's own
    /// tolerance whatever the level.
    ///
    /// The ground is sampled at every lattice point along the edge (every
    /// metre at level 0, every 2^L metres at level `L`). At level 0 it is
    /// also sampled where each river or pond line (see
    /// [`ChunkSource::lines_within`]) crosses the edge; above level 0 no
    /// feature line is kept, though the ground sampled is still the carved
    /// ground. The corners and the crossings are always vertices; a water's
    /// edge crossing inside other water is not a crossing. Then a stretch
    /// between two vertices gets a new vertex at the sample it misses most
    /// among those it misses by more than their height tolerance, or at its
    /// middle sample if it is longer than its longest edge allows. Samples
    /// within `tolerance.water_band` of water follow `tolerance.water`, and
    /// a stretch with any such sample takes the water's longest edge; the
    /// rest follow `tolerance.general`.
    ///
    /// # Panics
    ///
    /// If `tolerance` is not valid, or the edge's level is above
    /// [`MAX_CHUNK_LEVEL`](crate::MAX_CHUNK_LEVEL).
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
        let tolerance = &tolerance.at_level(key.level);
        let (start, end) = (key.start(), key.end());
        let corner_m = |(x, z): (Cm, Cm)| DVec2::new(x.to_m(), z.to_m());
        let (lo, hi) = (corner_m(start), corner_m(end));
        // Feature lines are kept as edges at level 0 only.
        let features = if key.level == 0 {
            source.lines_within(lo, hi, FeatureLevel::FINEST)
        } else {
            Vec::new()
        };
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

        // Samples at every lattice point, corners included, and at each
        // crossing; the corners and crossings are kept.
        let along = |at: (Cm, Cm)| (at.0.0 - start.0.0) + (at.1.0 - start.1.0);
        let step = step_cm(key.level);
        let last = chunk_cm(key.level) / step;
        let mut stations: Vec<(i64, bool)> = (0..=last)
            .map(|i| (i * step, i == 0 || i == last))
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
        let vertices: Vec<TinVertex> = (0..count)
            .filter(|&i| kept[i])
            .map(|i| TinVertex {
                x: probes[i].x,
                z: probes[i].z,
                h: heights[i],
            })
            .collect();
        let at: Vec<(Cm, Cm)> = vertices.iter().map(|v| (v.x, v.z)).collect();
        let normals = border_normals(source, &at, key.level)?;
        let samples = stations
            .iter()
            .zip(&heights)
            .filter(|((s, _), _)| s % step == 0)
            .map(|(&(s, _), &h)| ((s / step) as usize, h))
            .collect();
        Ok((
            Self {
                key,
                vertices,
                crossings,
                normals,
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
        // Written only above level 0, so level-0 contracts keep their
        // hashes.
        if self.key.level > 0 {
            h.write_str("level");
            h.write_u32(u32::from(self.key.level));
        }
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
