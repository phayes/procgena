//! Relief convergence: near each shared edge a terrain fades its relief
//! towards an amplitude agreed with its neighbours, so the two sides are
//! already alike before their border is negotiated.

use glam::DVec2;

use crate::region::{EdgeId, Region, Segment, segments};
use crate::seam::smootherstep;
use crate::terrain::{ON_EDGE, SHEPARD_POWER, SOFT_DISTANCE_POWER};

/// The default [`relief_target`] exponent: each side of an edge changes its
/// relief in proportion to its own relief amplitude.
pub const DEFAULT_RELIEF_MEETING: f32 = -1.0;

/// The fastest a terrain's relief amplitude changes with distance from a
/// border, in metres of amplitude per metre: a 280 m change takes about
/// 1.1 km.
const FADE_RATE: f64 = 0.25;

/// Returns true if `meeting` is an accepted [`relief_target`] exponent:
/// at most 1, negative infinity included, and not NaN.
pub fn relief_meeting_is_valid(meeting: f32) -> bool {
    meeting <= 1.0
}

/// The relief amplitude that terrains with `amplitudes` (metres) agree to
/// meet at, where they share an edge or a corner.
///
/// The result is the power mean of `amplitudes` with exponent `meeting`:
/// negative infinity gives the smallest amplitude (only the rougher sides
/// change), −1 the harmonic mean (each side changes in proportion to its
/// own amplitude, see [`DEFAULT_RELIEF_MEETING`]), 0 the geometric mean
/// and 1 the plain mean. It doesn't depend on the order of `amplitudes`,
/// bit for bit. If any amplitude is 0 and `meeting` is 0 or less, the
/// result is 0.
///
/// # Panics
///
/// If `amplitudes` is empty or holds a negative or non-finite value, or if
/// `meeting` isn't valid (see [`relief_meeting_is_valid`]).
pub fn relief_target(amplitudes: &[f32], meeting: f32) -> f32 {
    assert!(!amplitudes.is_empty(), "no amplitudes to meet");
    assert!(
        amplitudes.iter().all(|a| a.is_finite() && *a >= 0.0),
        "amplitudes must be finite and not negative: {amplitudes:?}"
    );
    assert!(
        relief_meeting_is_valid(meeting),
        "relief meeting exponent {meeting} is above 1 or NaN"
    );

    // Sorted, so the sums below don't depend on the order given.
    let mut sorted: Vec<f64> = amplitudes.iter().map(|&a| f64::from(a)).collect();
    sorted.sort_by(f64::total_cmp);
    let smallest = sorted[0];
    let n = sorted.len() as f64;
    if meeting == f32::NEG_INFINITY || (smallest == 0.0 && meeting <= 0.0) {
        return smallest as f32;
    }
    if sorted[sorted.len() - 1] == 0.0 {
        return 0.0;
    }

    // Relative to the largest, so the powers stay in range.
    let largest = sorted[sorted.len() - 1];
    let p = f64::from(meeting);
    let mean = if p == 0.0 {
        (sorted.iter().map(|a| (a / largest).ln()).sum::<f64>() / n).exp()
    } else {
        (sorted.iter().map(|a| (a / largest).powf(p)).sum::<f64>() / n).powf(1.0 / p)
    };
    // Rounding must never take the mean outside the amplitudes.
    (largest * mean).clamp(smallest, largest) as f32
}

/// The agreed relief amplitude along one shared edge of a region.
///
/// Along most of the edge the target is `amplitude`. Towards each end it
/// changes smoothly to the target agreed for that corner, so that every
/// edge meeting at a corner asks for the same amplitude there. Both
/// terrains on the edge must be given the same target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReliefTarget {
    /// The edge.
    pub edge: EdgeId,
    /// The target along the edge, in metres. Zero or more.
    pub amplitude: f32,
    /// The target at the edge's start corner, in metres. Zero or more.
    pub start: f32,
    /// The target at the edge's end corner, in metres. Zero or more.
    pub end: f32,
}

impl ReliefTarget {
    /// Returns true if every amplitude is finite and not negative.
    pub(crate) fn is_valid(&self) -> bool {
        [self.amplitude, self.start, self.end]
            .iter()
            .all(|a| a.is_finite() && *a >= 0.0)
    }

    /// The target at `along` metres from the start of an edge `length`
    /// metres long.
    fn at(&self, along: f64, length: f64) -> f64 {
        let middle = f64::from(self.amplitude);
        let towards = |corner: f32, from_corner: f64| {
            let change = f64::from(corner) - middle;
            let ramp = (change.abs() / FADE_RATE).min(0.5 * length);
            if ramp <= 0.0 {
                return 0.0;
            }
            change * (1.0 - smootherstep(from_corner / ramp))
        };
        middle + towards(self.start, along) + towards(self.end, length - along)
    }
}

/// The spread of each rolling and mountain layer of a terrain, in metres:
/// swells, hills, then mountains, from smoothest to roughest.
pub(crate) type LayerSpreads = [f32; 3];

/// How much of each layer a terrain keeps at one point, for the layers of
/// [`LayerSpreads`].
pub(crate) type LayerScales = [f32; 3];

/// The relief amplitude of a terrain with layer `spreads`, in metres.
pub(crate) fn total_spread(spreads: LayerSpreads) -> f32 {
    spreads.iter().map(|s| s * s).sum::<f32>().sqrt()
}

/// The level that keeps every layer as it is. Levels below fade the layers
/// out, roughest first; levels above scale every layer up together.
const UNCHANGED: f64 = 3.0;

/// A smooth step from 0 to 1 with flat ends, and its inverse.
fn step(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn step_inverse(s: f64) -> f64 {
    let s = s.clamp(0.0, 1.0);
    0.5 - ((1.0 - 2.0 * s).asin() / 3.0).sin()
}

/// The level whose layer scales give a total spread of `target` metres.
/// A terrain with no relief at all can't be scaled up and stays unchanged.
fn level_for(spreads: LayerSpreads, target: f64) -> f64 {
    let [swells, hills, mountains] = spreads.map(f64::from);
    // The same rounding as the stated amplitude, so a target equal to it
    // keeps every layer exactly.
    let total = f64::from(total_spread(spreads));
    let t2 = target * target;
    if target >= total {
        return if total == 0.0 {
            UNCHANGED
        } else {
            UNCHANGED + target / total - 1.0
        };
    }

    // Fade the roughest layer that has to go, keeping the smoother ones.
    let rolling = swells * swells + hills * hills;
    if t2 >= rolling {
        2.0 + step_inverse((t2 - rolling).sqrt() / mountains)
    } else if t2 >= swells * swells {
        1.0 + step_inverse((t2 - swells * swells).sqrt() / hills)
    } else {
        step_inverse(target / swells)
    }
}

/// The layer scales at `level`.
fn scales_at(level: f64) -> LayerScales {
    if level >= UNCHANGED {
        return [(level - 2.0) as f32; 3];
    }
    [0.0, 1.0, 2.0].map(|start| step(level - start) as f32)
}

/// One shared edge's segments and target.
#[derive(Debug, Clone)]
struct FadeEdge {
    segments: Vec<Segment>,
    length: f64,
    target: ReliefTarget,
}

/// A terrain's relief fade towards its edges' targets.
#[derive(Debug, Clone)]
pub(crate) struct Fade {
    edges: Vec<FadeEdge>,
    spreads: LayerSpreads,
    width: f64,
}

impl Fade {
    /// Fades a terrain with layer `spreads` towards `targets`, each for an
    /// edge of `region`, over at least `min_width` metres. Returns `None`
    /// if `targets` is empty.
    ///
    /// # Panics
    ///
    /// If a target's edge isn't an edge of `region`.
    pub(crate) fn new(
        region: &Region,
        targets: &[ReliefTarget],
        spreads: LayerSpreads,
        min_width: f64,
    ) -> Option<Self> {
        let total = total_spread(spreads);
        let mut width = min_width;
        let mut edges = Vec::with_capacity(targets.len());
        for target in targets {
            let edge = region
                .edges
                .iter()
                .find(|e| e.id == target.edge)
                .expect("targets are for edges of the region");
            let change = [target.amplitude, target.start, target.end]
                .iter()
                .map(|t| (f64::from(*t) - f64::from(total)).abs())
                .fold(0.0, f64::max);
            width = width.max(change / FADE_RATE);
            edges.push(FadeEdge {
                segments: segments(&edge.polyline),
                length: edge.length(),
                target: *target,
            });
        }
        (!edges.is_empty()).then_some(Self {
            edges,
            spreads,
            width,
        })
    }

    /// The layer scales at `p`, or `None` where the fade doesn't reach and
    /// every layer is kept as it is.
    pub(crate) fn scales(&self, p: DVec2) -> Option<LayerScales> {
        // Mix every segment's change of each layer's scale, weighted by
        // inverse distance, and find the (soft) distance to the faded edges.
        // Each layer is mixed on its own, so a layer the terrain doesn't have
        // can't disturb the others; mixing changes keeps unchanged layers
        // exact.
        let mut sum = [0.0f64; 3];
        let mut total = 0.0f64;
        let mut closeness = 0.0f64;
        for edge in &self.edges {
            for segment in &edge.segments {
                let projection = segment.project(p);
                let level = level_for(self.spreads, edge.target.at(projection.along, edge.length));
                let scales = scales_at(level);
                if projection.distance < ON_EDGE {
                    return Some(scales);
                }
                let w = projection.distance.powi(-SHEPARD_POWER);
                for (s, k) in sum.iter_mut().zip(scales) {
                    *s += (f64::from(k) - 1.0) * w;
                }
                total += w;
                closeness += (projection.distance / self.width).powi(-SOFT_DISTANCE_POWER);
            }
        }
        // In fade-width units, so the powers stay in range at any scale.
        let distance = self.width * closeness.powf(-1.0 / f64::from(SOFT_DISTANCE_POWER));
        if distance >= self.width {
            return None;
        }

        // From the edges' scales to unchanged across the fade width.
        let fade = 1.0 - smootherstep(distance / self.width);
        Some(sum.map(|s| (1.0 + s / total * fade) as f32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_follow_the_meeting_exponent() {
        let pair = [10.0, 150.0];
        let close = |a: f32, b: f32| (a - b).abs() < 1e-3 * b.max(1.0);
        assert_eq!(relief_target(&pair, f32::NEG_INFINITY), 10.0);
        assert!(close(relief_target(&pair, -1.0), 18.75));
        assert!(close(relief_target(&pair, 0.0), 1500f32.sqrt()));
        assert!(close(relief_target(&pair, 1.0), 80.0));
        assert_eq!(relief_target(&[0.0, 5.0], -1.0), 0.0);
        assert_eq!(relief_target(&[0.0, 5.0], 0.0), 0.0);
        assert!(close(relief_target(&[0.0, 5.0], 1.0), 2.5));
        assert_eq!(relief_target(&[0.0, 0.0], 1.0), 0.0);
        assert_eq!(relief_target(&[7.0], -1.0), 7.0);
    }

    #[test]
    fn targets_ignore_order() {
        let amplitudes = [3.1f32, 282.0, 14.6, 0.5];
        for meeting in [f32::NEG_INFINITY, -2.5, -1.0, 0.0, 0.5, 1.0] {
            let expected = relief_target(&amplitudes, meeting);
            for rotation in 0..amplitudes.len() {
                let mut turned = amplitudes;
                turned.rotate_left(rotation);
                assert_eq!(relief_target(&turned, meeting), expected);
                turned.reverse();
                assert_eq!(relief_target(&turned, meeting), expected);
            }
        }
    }

    #[test]
    #[should_panic(expected = "above 1")]
    fn targets_reject_exponents_above_one() {
        relief_target(&[1.0, 2.0], 1.5);
    }

    #[test]
    fn levels_give_the_target_spread() {
        let spreads = [20.0, 15.0, 280.0];
        for target in [0.0, 3.0, 20.0, 22.0, 25.0, 100.0, 282.0, 400.0] {
            let scales = scales_at(level_for(spreads, target));
            let spread = spreads
                .iter()
                .zip(scales)
                .map(|(s, k)| f64::from(s * k).powi(2))
                .sum::<f64>()
                .sqrt();
            assert!(
                (spread - target).abs() < 1e-3 * target.max(1.0),
                "{target}: {spread}"
            );
        }
        // The roughest layer goes first.
        let [swells, hills, mountains] = scales_at(level_for(spreads, 22.0));
        assert_eq!((swells, mountains), (1.0, 0.0));
        assert!(hills > 0.0 && hills < 1.0);
    }

    #[test]
    fn own_amplitude_keeps_every_layer() {
        let spreads = [3.0f32, 0.75, 0.0];
        let total = total_spread(spreads);
        assert_eq!(scales_at(level_for(spreads, f64::from(total))), [1.0; 3]);
    }

    #[test]
    fn edge_targets_reach_their_corners() {
        let target = ReliefTarget {
            edge: EdgeId(0),
            amplitude: 20.0,
            start: 50.0,
            end: 20.0,
        };
        assert_eq!(target.at(0.0, 1000.0), 50.0);
        assert_eq!(target.at(1000.0, 1000.0), 20.0);
        assert_eq!(target.at(500.0, 1000.0), 20.0);
        assert!(target.at(60.0, 1000.0) > 20.0 && target.at(60.0, 1000.0) < 50.0);
    }
}
