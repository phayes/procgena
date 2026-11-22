//! Cap outline shapes, and the smooth 2D curves that cap and stem surfaces
//! are spun from.

use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Points computed between each pair of anchors when a curve is turned into
/// a fine chain of straight lines. Every detail level samples this same
/// chain, so coarse and fine meshes lie on one shape.
const DENSE_STEPS: usize = 24;

/// The cap's outline seen from the side, named after the shapes field
/// guides use to describe mushroom caps.
///
/// The set of shapes follows Infinigen's mushroom generator (BSD-3-Clause,
/// Princeton University), redrawn as outlines of the cap's top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapFamily {
    /// Bell-shaped, with the rim hanging well below the top.
    Campanulate,
    /// A pointed cone.
    Conical,
    /// A rounded dome; the most common shape.
    #[default]
    Convex,
    /// Nearly flat with a shallow dip in the middle.
    Depressed,
    /// Flat on top with a rounded rim.
    Flat,
    /// A funnel or trumpet, with the rim higher than the centre.
    Infundibuliform,
    /// Egg-shaped, taller than it is wide.
    Ovate,
    /// Flat with a small, sharp navel-like dip at the centre.
    Umbilicate,
    /// Flat with a raised knob at the centre.
    Umbonate,
    /// A near-sphere sitting on a very short stem, like a puffball.
    Globe,
}

impl CapFamily {
    /// Every family, in declaration order.
    pub const ALL: [CapFamily; 10] = [
        CapFamily::Campanulate,
        CapFamily::Conical,
        CapFamily::Convex,
        CapFamily::Depressed,
        CapFamily::Flat,
        CapFamily::Infundibuliform,
        CapFamily::Ovate,
        CapFamily::Umbilicate,
        CapFamily::Umbonate,
        CapFamily::Globe,
    ];

    /// Returns a short display name, such as `"Convex"`.
    pub fn name(self) -> &'static str {
        match self {
            CapFamily::Campanulate => "Campanulate",
            CapFamily::Conical => "Conical",
            CapFamily::Convex => "Convex",
            CapFamily::Depressed => "Depressed",
            CapFamily::Flat => "Flat",
            CapFamily::Infundibuliform => "Infundibuliform",
            CapFamily::Ovate => "Ovate",
            CapFamily::Umbilicate => "Umbilicate",
            CapFamily::Umbonate => "Umbonate",
            CapFamily::Globe => "Globe",
        }
    }

    /// Returns the points a curve through the cap's top outline passes, from
    /// the rim to the top centre, and which of those points are sharp
    /// corners rather than smooth.
    ///
    /// Points are `(distance from the axis, height)` for a cap of radius 1,
    /// with height 0 where the underside meets the stem. The last point is
    /// on the axis.
    pub(crate) fn top_anchors(self) -> (&'static [[f32; 2]], &'static [usize]) {
        match self {
            CapFamily::Campanulate => (
                &[
                    [1.0, -0.55],
                    [0.97, -0.3],
                    [0.76, 0.22],
                    [0.42, 0.58],
                    [0.0, 0.7],
                ],
                &[],
            ),
            CapFamily::Conical => (&[[1.0, -0.42], [0.55, 0.42], [0.06, 1.08], [0.0, 1.1]], &[]),
            CapFamily::Convex => (
                &[
                    [1.0, -0.22],
                    [0.97, -0.04],
                    [0.76, 0.3],
                    [0.42, 0.5],
                    [0.0, 0.55],
                ],
                &[],
            ),
            CapFamily::Depressed => (
                &[
                    [1.0, -0.06],
                    [0.92, 0.14],
                    [0.62, 0.22],
                    [0.28, 0.14],
                    [0.0, 0.1],
                ],
                &[],
            ),
            CapFamily::Flat => (&[[1.0, -0.1], [0.93, 0.12], [0.62, 0.2], [0.0, 0.22]], &[]),
            CapFamily::Infundibuliform => (
                &[
                    [1.0, 0.45],
                    [0.93, 0.5],
                    [0.62, 0.36],
                    [0.26, 0.15],
                    [0.0, 0.1],
                ],
                &[],
            ),
            CapFamily::Ovate => (
                &[
                    [1.0, -0.72],
                    [1.04, -0.2],
                    [0.86, 0.45],
                    [0.46, 0.82],
                    [0.0, 0.9],
                ],
                &[],
            ),
            CapFamily::Umbilicate => (
                &[
                    [1.0, -0.06],
                    [0.92, 0.16],
                    [0.5, 0.25],
                    [0.2, 0.22],
                    [0.08, 0.13],
                    [0.0, 0.11],
                ],
                &[],
            ),
            CapFamily::Umbonate => (
                &[
                    [1.0, -0.15],
                    [0.94, 0.08],
                    [0.56, 0.2],
                    [0.26, 0.27],
                    [0.11, 0.4],
                    [0.0, 0.42],
                ],
                &[],
            ),
            CapFamily::Globe => (
                &[
                    [0.3, 0.02],
                    [0.82, 0.36],
                    [1.0, 0.92],
                    [0.86, 1.42],
                    [0.5, 1.78],
                    [0.0, 1.9],
                ],
                &[],
            ),
        }
    }
}

/// A smooth 2D curve, stored as a fine chain of straight lines and looked up
/// by distance along it, so coarse and fine meshes sample the same shape.
#[derive(Debug, Clone)]
pub(crate) struct Profile {
    /// Closely spaced points along the curve.
    points: Vec<Vec2>,
    /// Distance along the curve from the start to each point.
    lengths: Vec<f32>,
    /// Distances along the curve of each sharp corner and of both ends, in
    /// order.
    breaks: Vec<f32>,
}

impl Profile {
    /// Builds a smooth curve that passes through every point in `anchors`,
    /// with sharp corners at the indices listed in `sharp`. Indices of the
    /// first or last anchor, or out of range, are ignored.
    ///
    /// # Panics
    ///
    /// Panics if `anchors` has fewer than two points.
    pub(crate) fn new(anchors: &[Vec2], sharp: &[usize]) -> Self {
        assert!(anchors.len() >= 2, "a profile needs two anchors");

        // Split the anchors into smooth runs at each sharp corner; each run
        // is a Catmull-Rom spline (a curve through its points).
        let mut cuts: Vec<usize> = vec![0];
        cuts.extend(
            sharp
                .iter()
                .copied()
                .filter(|&i| i > 0 && i < anchors.len() - 1),
        );
        cuts.push(anchors.len() - 1);
        cuts.dedup();

        let mut points = vec![anchors[0]];
        let mut break_points = vec![0usize];
        for span in cuts.windows(2) {
            let span = &anchors[span[0]..=span[1]];
            let at = |i: isize| span[i.clamp(0, span.len() as isize - 1) as usize];
            for i in 0..span.len() - 1 {
                let i = i as isize;
                let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
                for step in 1..DENSE_STEPS {
                    let t = step as f32 / DENSE_STEPS as f32;
                    points.push(catmull_rom(p0, p1, p2, p3, t));
                }
                // End exactly on the anchor: rounding would otherwise move a
                // point on the axis slightly off it and open a pinhole in the
                // mesh.
                points.push(p2);
            }
            break_points.push(points.len() - 1);
        }

        // Measure the distance along the curve to each point.
        let mut lengths = Vec::with_capacity(points.len());
        let mut total = 0.0;
        for (i, p) in points.iter().enumerate() {
            if i > 0 {
                total += p.distance(points[i - 1]);
            }
            lengths.push(total);
        }
        let breaks = break_points.iter().map(|&i| lengths[i]).collect();
        Self {
            points,
            lengths,
            breaks,
        }
    }

    /// Returns the distances along the curve of its start, each sharp
    /// corner and its end, in order.
    pub(crate) fn corners(&self) -> &[f32] {
        &self.breaks
    }

    /// Returns the curve's total length.
    pub(crate) fn length(&self) -> f32 {
        *self.lengths.last().expect("profiles are never empty")
    }

    /// Returns the point at distance `s` along the curve. Values before the
    /// start or past the end give the first or last point.
    pub(crate) fn at(&self, s: f32) -> Vec2 {
        // Return the exact end points, so surfaces that meet there share
        // vertices at exactly the same position and join without gaps.
        if s <= 0.0 {
            return self.points[0];
        }
        if s >= self.length() {
            return *self.points.last().expect("profiles are never empty");
        }
        let i = self.lengths.partition_point(|&l| l < s).max(1);
        let (l0, l1) = (self.lengths[i - 1], self.lengths[i]);
        let t = if l1 > l0 { (s - l0) / (l1 - l0) } else { 0.0 };
        self.points[i - 1].lerp(self.points[i], t)
    }

    /// Returns increasing distances along the curve, from 0 to the full
    /// length, for a mesh with about `segments` steps along it.
    ///
    /// The distances are spread evenly but always include every sharp
    /// corner, so corners stay sharp at any detail. Each smooth run gets at
    /// least one step, so the result can have more than `segments + 1`
    /// values.
    pub(crate) fn stations(&self, segments: u32) -> Vec<f32> {
        let total = self.length();
        let spans = self.breaks.len() - 1;
        let segments = (segments as usize).max(spans);
        let mut out = vec![0.0];
        let mut assigned = 0usize;
        for (k, span) in self.breaks.windows(2).enumerate() {
            let (a, b) = (span[0], span[1]);
            // Share steps by length; the last run takes what is left so the
            // total matches when nothing has been over-assigned.
            let n = if k == spans - 1 {
                segments.saturating_sub(assigned).max(1)
            } else {
                (((b - a) / total * segments as f32).round() as usize).max(1)
            };
            assigned += n;
            for i in 1..n {
                out.push(a + (b - a) * i as f32 / n as f32);
            }
            out.push(b);
        }
        out
    }
}

/// Returns the point at `t` (0 to 1) on the smooth Catmull-Rom curve from
/// `p1` to `p2`, shaped by the neighbouring points `p0` and `p3`.
fn catmull_rom(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f32) -> Vec2 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1)
        + (p2 - p0) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_passes_through_anchors() {
        let anchors = [
            Vec2::new(1.0, 0.0),
            Vec2::new(0.5, 0.5),
            Vec2::new(0.0, 0.6),
        ];
        let p = Profile::new(&anchors, &[]);
        assert!(p.at(0.0).distance(anchors[0]) < 1e-6);
        assert!(p.at(p.length()).distance(anchors[2]) < 1e-6);
    }

    #[test]
    fn stations_include_sharp_corners() {
        let anchors = [Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(1.0, 3.0)];
        let p = Profile::new(&anchors, &[1]);
        let s = p.stations(4);
        assert_eq!(s.first(), Some(&0.0));
        assert!((s.last().unwrap() - p.length()).abs() < 1e-5);
        assert!(s.iter().any(|&x| (x - 1.0).abs() < 1e-5), "{s:?}");
        assert!(s.windows(2).all(|w| w[1] > w[0]));
    }

    #[test]
    fn stations_survive_runs_that_round_up() {
        // Runs of length 0.001, 0.001, 1.3 and 0.7 with 4 steps: the two tiny
        // runs get one step each and the 1.3 run rounds up to 3, so the first
        // three runs take 5 steps, more than the 4 asked for.
        let anchors = [
            Vec2::ZERO,
            Vec2::new(0.001, 0.0),
            Vec2::new(0.002, 0.0),
            Vec2::new(1.302, 0.0),
            Vec2::new(1.302, 0.7),
        ];
        let p = Profile::new(&anchors, &[1, 2, 3]);
        let s = p.stations(4);
        assert!((s.last().unwrap() - p.length()).abs() < 1e-5);
        assert!(s.windows(2).all(|w| w[1] > w[0]), "{s:?}");
    }

    #[test]
    fn every_family_ends_on_the_axis() {
        for family in CapFamily::ALL {
            let (anchors, _) = family.top_anchors();
            assert_eq!(anchors.last().unwrap()[0], 0.0, "{family:?}");
        }
    }
}
