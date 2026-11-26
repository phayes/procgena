//! Where canonical ground is sampled: coarse to fine on a dyadic lattice,
//! finer only where the ground bends, and every metre near water and
//! roads.
//!
//! Samples sit on the 1 m lattice of the chunk grid. A chunk's inside
//! starts from cells [`COARSEST`] lattice steps wide. A cell's *bend* is
//! how far its middle and side midpoints stray from the flat (bilinear)
//! surface through its corners. A cell is left coarse when its bend is
//! within [`DETAIL_SHARE`] of the open land's height tolerance and the
//! cell it was cut from bent at most [`PARENT_FACTOR`] times that;
//! otherwise it is cut into four, down to cells two steps wide. Chunk
//! edges are sampled every metre. Which points are sampled depends only on
//! the ground at those points, the feature lines and the tolerance, never
//! on the order anything is built in.

use glam::DVec2;

use crate::carve::FeatureKind;
use crate::chunk::{ChunkFeature, ChunkFeatureShape, ChunkSource};
use crate::ports::BANK_REACH;

use super::numeric::{MM_PER_M, Mm, MmHeight};
use super::probe::{ProbeAddr, TinError, sample_heights};
use super::tolerance::{TinSampling, TinTolerance};
use super::water::Water;

/// How far apart lattice points are, in millimetres.
pub(crate) const STEP_MM: i64 = 1000;

/// The widest a cell (or a stretch of a chunk edge) starts, in lattice
/// steps. A power of two that divides a chunk's width.
pub(crate) const COARSEST: usize = 32;

/// How far a coarse cell's sampled points may stray from the flat surface
/// through its corners, as a share of the open land's height tolerance,
/// before the cell is cut finer.
pub(crate) const DETAIL_SHARE: f64 = 0.2;

/// How much closer than its tolerance the mesh follows the samples of a
/// cell left coarse, as a share of the open land's height tolerance, so
/// the ground between them stays within the tolerance.
pub(crate) const MARGIN_SHARE: f64 = 0.4;

/// How much more than [`DETAIL_SHARE`] the cell a coarse cell was cut from
/// may bend.
pub(crate) const PARENT_FACTOR: f64 = 4.0;

/// Which ground is always sampled every metre: within reach of water
/// (where slope is checked against neighbouring samples) and of roads
/// (whose cuts and fills have sharp edges the coarse test can step over),
/// or everywhere when open land's slope is checked or the tolerance asks
/// for [`TinSampling::EveryMetre`].
pub(crate) struct DenseNear<'a> {
    water: &'a Water,
    /// Each road segment's ends (metres) and how far its ground reaches
    /// from its line.
    roads: Vec<(DVec2, DVec2, f64)>,
    /// How far beyond the water's edge sampling stays dense, in metres.
    water_reach: f64,
    everywhere: bool,
}

impl<'a> DenseNear<'a> {
    /// Dense sampling near `water` and the roads among `features`, within
    /// `tolerance`.
    pub(crate) fn new(
        features: &[ChunkFeature],
        water: &'a Water,
        tolerance: &TinTolerance,
    ) -> Self {
        let roads = features
            .iter()
            .filter(|f| f.id.kind == FeatureKind::Road)
            .filter_map(|f| match &f.shape {
                ChunkFeatureShape::Line(line) => Some(line),
                ChunkFeatureShape::Pond(_) => None,
            })
            .flat_map(|line| {
                line.points
                    .windows(2)
                    .zip(line.widths.windows(2))
                    .map(|(p, w)| {
                        let half = 0.5 * f64::from(w[0].max(w[1]));
                        (p[0], p[1], half + BANK_REACH)
                    })
            })
            .collect();
        Self {
            water,
            roads,
            water_reach: tolerance.water_band + STEP_MM as f64 / MM_PER_M,
            everywhere: tolerance.general.gradient.is_some()
                || tolerance.sampling == TinSampling::EveryMetre,
        }
    }

    /// Whether any of the box from `lo` to `hi` (metres) must be sampled
    /// every metre.
    pub(crate) fn needs(&self, lo: DVec2, hi: DVec2) -> bool {
        if self.everywhere {
            return true;
        }
        let (centre, half) = ((lo + hi) * 0.5, (hi - lo).length() * 0.5);
        if self.water.gap(centre) - half < self.water_reach {
            return true;
        }
        self.roads.iter().any(|&(a, b, reach)| {
            let ab = b - a;
            let t = ((centre - a).dot(ab) / ab.length_squared().max(1e-18)).clamp(0.0, 1.0);
            centre.distance(a + ab * t) - half < reach
        })
    }
}

/// The largest a coarse cell's points may stray from flat, in millimetres
/// (see [`DETAIL_SHARE`]).
pub(crate) fn detail_limit(tolerance: &TinTolerance) -> f64 {
    DETAIL_SHARE * tolerance.general.height * MM_PER_M
}

/// How much closer than its tolerance the mesh follows a sample of a cell
/// left coarse, in millimetres (see [`MARGIN_SHARE`]).
pub(crate) fn coarse_margin(tolerance: &TinTolerance) -> f64 {
    MARGIN_SHARE * tolerance.general.height * MM_PER_M
}

/// Ground sampled on part of the lattice over a square `side` points
/// across.
pub(crate) struct SparseSquare {
    /// The square's smallest corner, in millimetres.
    pub(crate) origin: (i64, i64),
    /// Points along a side.
    pub(crate) side: usize,
    /// Heights, row by row along x; 0 where not sampled.
    pub(crate) heights: Vec<MmHeight>,
    /// Whether each point is sampled.
    pub(crate) sampled: Vec<bool>,
    /// Whether each point is a sample of a cell left coarse, with lattice
    /// points inside it that aren't sampled.
    pub(crate) coarse: Vec<bool>,
}

impl SparseSquare {
    /// Samples the ground of `source` over the square from `origin`
    /// (millimetres) `side` points across: first every [`COARSEST`] steps,
    /// then finer in each cell that bends more than `limit` millimetres or
    /// that `dense` needs, down to every step. `known` are points already
    /// sampled (lattice column, row and height), which aren't read again.
    ///
    /// # Panics
    ///
    /// If `side - 1` is not a multiple of [`COARSEST`].
    pub(crate) fn sample(
        source: &dyn ChunkSource,
        origin: (i64, i64),
        side: usize,
        known: &[(usize, usize, MmHeight)],
        dense: &DenseNear,
        limit: f64,
    ) -> Result<Self, TinError> {
        assert_eq!((side - 1) % COARSEST, 0, "side {side}");
        let mut square = Self {
            origin,
            side,
            heights: vec![MmHeight(0); side * side],
            sampled: vec![false; side * side],
            coarse: vec![false; side * side],
        };
        for &(col, row, h) in known {
            square.heights[row * side + col] = h;
            square.sampled[row * side + col] = true;
        }

        // The coarsest cells' corners.
        let roots = (side - 1) / COARSEST;
        let corners: Vec<(usize, usize)> = (0..=roots)
            .flat_map(|r| (0..=roots).map(move |c| (c * COARSEST, r * COARSEST)))
            .collect();
        square.fetch(source, &corners)?;
        // Each cell: column, row, width, and the bend of the cell it was
        // cut from (infinite for the coarsest, or where that cell needed
        // dense samples).
        let mut cells: Vec<(usize, usize, usize, f64)> = (0..roots)
            .flat_map(|r| {
                (0..roots).map(move |c| (c * COARSEST, r * COARSEST, COARSEST, f64::INFINITY))
            })
            .collect();
        let parent_limit = limit * PARENT_FACTOR;

        // Level by level: sample every cell's middle and side midpoints at
        // once. A cell is left coarse if it bends at most `limit`, needn't
        // be dense, and the cell it was cut from bent at most
        // `parent_limit`; it is cut into four otherwise.
        while !cells.is_empty() {
            let points: Vec<(usize, usize)> = cells
                .iter()
                .flat_map(|&(c, r, s, _)| {
                    let h = s / 2;
                    [
                        (c + h, r),
                        (c, r + h),
                        (c + h, r + h),
                        (c + s, r + h),
                        (c + h, r + s),
                    ]
                })
                .collect();
            square.fetch(source, &points)?;
            let mut finer = Vec::new();
            for (c, r, s, above) in cells {
                if s == 2 {
                    continue;
                }
                let bend = if square.needs(dense, c, r, s) {
                    f64::INFINITY
                } else {
                    square.bend(c, r, s)
                };
                if bend <= limit && above <= parent_limit {
                    for j in 0..3 {
                        for i in 0..3 {
                            square.coarse[(r + j * s / 2) * side + c + i * s / 2] = true;
                        }
                    }
                } else {
                    let h = s / 2;
                    finer.extend([
                        (c, r, h, bend),
                        (c + h, r, h, bend),
                        (c, r + h, h, bend),
                        (c + h, r + h, h, bend),
                    ]);
                }
            }
            cells = finer;
        }
        Ok(square)
    }

    /// Samples each of `points` (lattice column and row) not yet sampled,
    /// in one batch, in row order.
    fn fetch(
        &mut self,
        source: &dyn ChunkSource,
        points: &[(usize, usize)],
    ) -> Result<(), TinError> {
        let mut wanted: Vec<usize> = points
            .iter()
            .map(|&(c, r)| r * self.side + c)
            .filter(|&i| !self.sampled[i])
            .collect();
        wanted.sort_unstable();
        wanted.dedup();
        let probes: Vec<ProbeAddr> = wanted
            .iter()
            .map(|&i| {
                let (c, r) = ((i % self.side) as i64, (i / self.side) as i64);
                ProbeAddr::new(
                    Mm(self.origin.0 + c * STEP_MM),
                    Mm(self.origin.1 + r * STEP_MM),
                )
            })
            .collect();
        let heights = sample_heights(source, &probes)?;
        for (i, h) in wanted.into_iter().zip(heights) {
            self.heights[i] = h;
            self.sampled[i] = true;
        }
        Ok(())
    }

    /// How far the middle and side midpoints of the cell `s` steps wide at
    /// column `c` and row `r` stray from the flat surface through its
    /// corners, in millimetres.
    fn bend(&self, c: usize, r: usize, s: usize) -> f64 {
        let h = |c: usize, r: usize| i64::from(self.heights[r * self.side + c].0);
        let half = s / 2;
        let [a, b, d, e] = [h(c, r), h(c + s, r), h(c, r + s), h(c + s, r + s)];
        let sides = [
            (h(c + half, r), a + b),
            (h(c, r + half), a + d),
            (h(c + s, r + half), b + e),
            (h(c + half, r + s), d + e),
        ]
        .map(|(mid, ends)| (2 * mid - ends).abs() as f64 / 2.0);
        let middle = (4 * h(c + half, r + half) - (a + b + d + e)).abs() as f64 / 4.0;
        sides.into_iter().fold(middle, f64::max)
    }

    /// Whether `dense` needs the cell `s` steps wide at column `c` and row
    /// `r` sampled every step.
    fn needs(&self, dense: &DenseNear, c: usize, r: usize, s: usize) -> bool {
        let at = |c: usize, r: usize| {
            DVec2::new(
                Mm(self.origin.0 + c as i64 * STEP_MM).to_m(),
                Mm(self.origin.1 + r as i64 * STEP_MM).to_m(),
            )
        };
        dense.needs(at(c, r), at(c + s, r + s))
    }
}
