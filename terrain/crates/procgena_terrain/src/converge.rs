//! Relief convergence: near each shared edge a terrain fades its relief
//! towards an amplitude agreed with its neighbours, so the two sides are
//! already alike before their border is negotiated.

use crate::options::RoadGrades;
use crate::region::{EdgeId, RegionId};
use crate::seam::smootherstep;

/// The default [`relief_target`] exponent: each side of an edge changes its
/// relief in proportion to its own relief amplitude.
pub const DEFAULT_RELIEF_MEETING: f32 = -1.0;

/// The fastest a terrain's relief amplitude changes with distance from a
/// border, in metres of amplitude per metre: a 280 m change takes about
/// 1.1 km.
const FADE_RATE: f64 = 0.25;

/// The default [`Convergence::blend`], which currently has no effect.
pub const DEFAULT_RELIEF_BLEND: f32 = 0.5;

/// Returns true if `meeting` is an accepted [`relief_target`] exponent:
/// at most 1, negative infinity included, and not NaN.
pub fn relief_meeting_is_valid(meeting: f32) -> bool {
    meeting <= 1.0
}

/// How neighbouring terrains become alike near their shared borders. It
/// must be the same for the whole world, since both sides of a border
/// compute the same targets from it.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Convergence {
    /// The exponent of the [`relief_target`] power mean, which decides how
    /// much each side changes. Must be valid (see
    /// [`relief_meeting_is_valid`]).
    pub meeting: f32,
    /// Currently has no effect, and will be removed: terrains now always
    /// scale their own relief towards the agreed roughness and meet a
    /// shared height profile on the border. Must be from 0 to 1.
    pub blend: f32,
}

impl Default for Convergence {
    fn default() -> Self {
        Self {
            meeting: DEFAULT_RELIEF_MEETING,
            blend: DEFAULT_RELIEF_BLEND,
        }
    }
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

/// The bisection steps [`blend_weights`] takes to find its exponent.
const WEIGHT_STEPS: usize = 60;

/// The share of each terrain's ground in the mix that terrains with
/// `amplitudes` (metres) agree to meet at, where they share an edge or a
/// corner, in the order given. The shares are zero or more and add up to 1.
///
/// Rougher terrains get smaller shares, so the mix is about as rough as
/// [`relief_target`] with the same `meeting`: at negative infinity the
/// smoothest terrains share everything, and towards 1 the shares even out.
/// Mixing terrains whose ground is unrelated lowers the roughness, so
/// between similar amplitudes the mix is smoother than the target and the
/// shares are close to even, leaning a little towards the rougher terrain.
/// Equal amplitudes get equal shares. The shares don't depend on the order of `amplitudes`, bit for
/// bit.
///
/// # Panics
///
/// As for [`relief_target`].
pub fn blend_weights(amplitudes: &[f32], meeting: f32) -> Vec<f32> {
    let target = f64::from(relief_target(amplitudes, meeting));

    // Sorted, so the sums below don't depend on the order given.
    let mut order: Vec<usize> = (0..amplitudes.len()).collect();
    order.sort_by(|&i, &j| amplitudes[i].total_cmp(&amplitudes[j]));
    let sorted: Vec<f64> = order.iter().map(|&i| f64::from(amplitudes[i])).collect();
    let (smallest, largest) = (sorted[0], sorted[sorted.len() - 1]);
    let shares = |weights: Vec<f64>| -> Vec<f32> {
        let total: f64 = weights.iter().sum();
        let mut out = vec![0.0f32; amplitudes.len()];
        for (&i, w) in order.iter().zip(weights) {
            out[i] = (w / total) as f32;
        }
        out
    };

    // The smoothest terrains alone, when only the rougher ones change or
    // the target is perfectly flat.
    if meeting == f32::NEG_INFINITY || target == 0.0 || largest == 0.0 {
        return shares(
            sorted
                .iter()
                .map(|&a| if a == smallest { 1.0 } else { 0.0 })
                .collect(),
        );
    }

    // Weights `a^q` (relative to the largest, and with amplitudes floored
    // just above zero so every power is finite), with q chosen so the mix
    // of unrelated grounds is as rough as the target.
    let floor = largest * 1e-6;
    let relative: Vec<f64> = sorted.iter().map(|&a| a.max(floor) / largest).collect();
    let weights = |q: f64| -> Vec<f64> { relative.iter().map(|r| r.powf(q)).collect() };
    let roughness = |q: f64| {
        let w = weights(q);
        let total: f64 = w.iter().sum();
        w.iter()
            .zip(&sorted)
            .map(|(w, a)| (w / total * a).powi(2))
            .sum::<f64>()
            .sqrt()
    };
    // Over this range the mix only gets rougher as q rises.
    let (mut lo, mut hi) = (-2.0f64, 1.0f64);
    let q = if target <= roughness(lo) {
        lo
    } else if target >= roughness(hi) {
        hi
    } else {
        for _ in 0..WEIGHT_STEPS {
            let middle = 0.5 * (lo + hi);
            if roughness(middle) < target {
                lo = middle;
            } else {
                hi = middle;
            }
        }
        hi
    };
    shares(weights(q))
}

/// How far along an edge, in metres, road grades and blend shares change
/// from the edge's to a corner's. Fixed, so the shares still add up to 1
/// all along the edge.
const CORNER_RAMP: f64 = 500.0;

/// The most the blend shares of a [`BorderTarget`] may add up to more or
/// less than 1.
const SHARE_TOLERANCE: f32 = 1e-4;

/// What a terrain fades towards at one place on a shared edge.
#[derive(Debug, Clone, PartialEq)]
pub struct BorderTarget {
    /// The relief amplitude, in metres (see [`relief_target`]). Zero or
    /// more.
    pub amplitude: f32,
    /// The share of each region's own ground in the mix the terrains meet
    /// at (see [`blend_weights`]), sorted by region id with each region
    /// once. Shares are zero or more and add up to 1.
    pub blend: Vec<(RegionId, f32)>,
    /// The road grades. Usually the steepest of the terrains meeting there
    /// (see [`RoadGrades::steepest`]).
    pub roads: RoadGrades,
}

impl BorderTarget {
    /// Returns true if the amplitude is finite and not negative, the blend
    /// shares are valid, and the road grades are valid.
    fn is_valid(&self) -> bool {
        let sorted = self.blend.windows(2).all(|w| w[0].0 < w[1].0);
        let shares_ok = self.blend.iter().all(|(_, s)| s.is_finite() && *s >= 0.0);
        let total: f32 = self.blend.iter().map(|(_, s)| s).sum();
        self.amplitude.is_finite()
            && self.amplitude >= 0.0
            && !self.blend.is_empty()
            && sorted
            && shares_ok
            && (total - 1.0).abs() <= SHARE_TOLERANCE
            && self.roads.is_valid()
    }
}

/// What a terrain fades towards along one of its shared edges.
///
/// Along most of the edge the target is `middle`. Towards each end it
/// changes smoothly to the target agreed for that corner, so that every
/// edge meeting at a corner asks for the same there. Both terrains on the
/// edge must be given the same target.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeTarget {
    /// The edge.
    pub edge: EdgeId,
    /// The target along the edge.
    pub middle: BorderTarget,
    /// The target at the edge's start corner.
    pub start: BorderTarget,
    /// The target at the edge's end corner.
    pub end: BorderTarget,
}

/// The value at `along` metres from the start of an edge `length` metres
/// long that is `middle` along most of the edge and changes smoothly to
/// `start` and `end` at its ends, over `ramp(change)` metres from each end.
fn along_edge(
    [middle, start, end]: [f32; 3],
    along: f64,
    length: f64,
    ramp: impl Fn(f64) -> f64,
) -> f64 {
    let middle = f64::from(middle);
    let towards = |corner: f32, from_corner: f64| {
        let change = f64::from(corner) - middle;
        let ramp = ramp(change.abs()).min(0.5 * length);
        if ramp <= 0.0 {
            return 0.0;
        }
        change * (1.0 - smootherstep(from_corner / ramp))
    };
    middle + towards(start, along) + towards(end, length - along)
}

impl EdgeTarget {
    /// Returns true if every part of the target is valid.
    pub(crate) fn is_valid(&self) -> bool {
        self.parts().iter().all(|t| t.is_valid())
    }

    /// The middle, start and end targets.
    fn parts(&self) -> [&BorderTarget; 3] {
        [&self.middle, &self.start, &self.end]
    }

    /// Every region with a blend share somewhere along the edge, sorted.
    #[cfg(test)]
    fn blend_regions(&self) -> Vec<RegionId> {
        let mut regions: Vec<RegionId> = self
            .parts()
            .iter()
            .flat_map(|t| t.blend.iter().map(|(r, _)| *r))
            .collect();
        regions.sort();
        regions.dedup();
        regions
    }

    /// The relief amplitude at `along` metres from the start of an edge
    /// `length` metres long.
    pub(crate) fn amplitude_at(&self, along: f64, length: f64) -> f64 {
        let values = self.parts().map(|t| t.amplitude);
        along_edge(values, along, length, |change| change / FADE_RATE)
    }

    /// The road grades at `along` metres from the start of an edge
    /// `length` metres long.
    pub(crate) fn roads_at(&self, along: f64, length: f64) -> RoadGrades {
        let at = |grade: fn(&RoadGrades) -> f32| {
            let values = self.parts().map(|t| grade(&t.roads));
            along_edge(values, along, length, |_| CORNER_RAMP) as f32
        };
        RoadGrades {
            grade: at(|r| r.grade),
            max_grade: at(|r| r.max_grade),
        }
    }

    #[cfg(test)]
    fn blend_at(&self, regions: &[RegionId], along: f64, length: f64) -> Vec<f64> {
        regions
            .iter()
            .map(|region| {
                let values = self.parts().map(|t| {
                    t.blend
                        .iter()
                        .find(|(r, _)| r == region)
                        .map_or(0.0, |(_, s)| *s)
                });
                along_edge(values, along, length, |_| CORNER_RAMP)
            })
            .collect()
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
pub(crate) fn level_for(spreads: LayerSpreads, target: f64) -> f64 {
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
pub(crate) fn scales_at(level: f64) -> LayerScales {
    if level >= UNCHANGED {
        return [(level - 2.0) as f32; 3];
    }
    [0.0, 1.0, 2.0].map(|start| step(level - start) as f32)
}

/// The fastest a terrain's mean height changes with distance from a border
/// as it meets the border's profile, in metres per metre: about as fast as
/// scaling changes it (a mountain range's mean falls about 400 m over
/// 1.6 km next to plains).
pub(crate) const BLEND_RATE: f64 = 0.25;

/// The widest a terrain's layer scales fade from its edges' targets, in
/// metres, for a terrain whose layers spread `spreads`, with `targets` on
/// its edges and edge settings asking for at least `margin` metres.
pub(crate) fn fade_width(spreads: LayerSpreads, targets: &[EdgeTarget], margin: f64) -> f64 {
    let total = f64::from(total_spread(spreads));
    targets
        .iter()
        .flat_map(|t| t.parts())
        .map(|part| (f64::from(part.amplitude) - total).abs() / FADE_RATE)
        .fold(margin, f64::max)
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

    /// The roughness of a mix of unrelated grounds with `amplitudes`, in
    /// `shares`.
    fn mixed(amplitudes: &[f32], shares: &[f32]) -> f32 {
        amplitudes
            .iter()
            .zip(shares)
            .map(|(a, s)| (a * s).powi(2))
            .sum::<f32>()
            .sqrt()
    }

    #[test]
    fn blend_weights_follow_the_meeting_exponent() {
        let pair = [3.1, 282.0];
        // Only the smoother side, at negative infinity.
        assert_eq!(blend_weights(&pair, f32::NEG_INFINITY), vec![1.0, 0.0]);
        // Contrasting terrains: the mix is as rough as the target, and the
        // smoother side's share falls as the exponent rises.
        let mut last = 1.0;
        for meeting in [-1.0, 0.0, 1.0] {
            let w = blend_weights(&pair, meeting);
            let target = relief_target(&pair, meeting);
            assert!((w[0] + w[1] - 1.0).abs() < 1e-6);
            assert!(w[0] < last && w[0] > 0.4, "{meeting}: {w:?}");
            last = w[0];
            let rough = mixed(&pair, &w);
            assert!(
                (rough - target).abs() < 1e-3 * target,
                "{meeting}: {rough} vs {target}"
            );
        }
        // Three terrains at a corner too.
        let three = [3.1, 30.0, 282.0];
        for meeting in [-1.0, 0.0, 1.0] {
            let w = blend_weights(&three, meeting);
            let target = relief_target(&three, meeting);
            assert!((mixed(&three, &w) - target).abs() < 1e-3 * target);
        }
    }

    #[test]
    fn blend_weights_are_even_for_alike_terrains_and_handle_flat_ones() {
        assert_eq!(blend_weights(&[20.0, 20.0], -1.0), vec![0.5, 0.5]);
        assert_eq!(blend_weights(&[7.0], 1.0), vec![1.0]);
        // Similar amplitudes can't reach the target; the shares are as even
        // as the rule allows.
        let w = blend_weights(&[30.0, 32.0], 1.0);
        assert!((w[0] - 0.5).abs() < 0.02, "{w:?}");
        // Flat terrains: with p ≤ 0 the flat side takes everything.
        assert_eq!(blend_weights(&[0.0, 5.0], -1.0), vec![1.0, 0.0]);
        assert_eq!(blend_weights(&[0.0, 0.0], 1.0), vec![0.5, 0.5]);
        let w = blend_weights(&[0.0, 5.0], 1.0);
        assert!((mixed(&[0.0, 5.0], &w) - 2.5).abs() < 1e-3, "{w:?}");
    }

    #[test]
    fn blend_weights_ignore_order() {
        let amplitudes = [3.1f32, 282.0, 14.6, 0.5];
        for meeting in [f32::NEG_INFINITY, -2.5, -1.0, 0.0, 0.5, 1.0] {
            let expected = blend_weights(&amplitudes, meeting);
            for rotation in 0..amplitudes.len() {
                let mut rotated = amplitudes;
                rotated.rotate_left(rotation);
                let mut got = blend_weights(&rotated, meeting);
                got.rotate_right(rotation);
                assert_eq!(got, expected, "meeting {meeting}, rotation {rotation}");
            }
        }
    }

    #[test]
    fn edge_targets_reach_their_corners() {
        let border = |amplitude: f32, grade: f32| BorderTarget {
            amplitude,
            blend: vec![(RegionId(1), 0.5), (RegionId(2), 0.5)],
            roads: RoadGrades {
                grade,
                max_grade: 2.0 * grade,
            },
        };
        let mut start = border(50.0, 0.1);
        start.blend = vec![(RegionId(1), 0.2), (RegionId(2), 0.3), (RegionId(3), 0.5)];
        let target = EdgeTarget {
            edge: EdgeId(0),
            middle: border(20.0, 0.06),
            start,
            end: border(20.0, 0.06),
        };
        assert!(target.is_valid());
        let regions = target.blend_regions();
        assert_eq!(regions, vec![RegionId(1), RegionId(2), RegionId(3)]);
        let corner = [0.2f32, 0.3, 0.5].map(f64::from).to_vec();
        assert_eq!(target.blend_at(&regions, 0.0, 1000.0), corner);
        assert_eq!(
            target.blend_at(&regions, 500.0, 1000.0),
            vec![0.5, 0.5, 0.0]
        );
        let shares = target.blend_at(&regions, 200.0, 1000.0);
        assert!(
            (shares.iter().sum::<f64>() - 1.0).abs() < 1e-6,
            "{shares:?}"
        );
        assert_eq!(target.amplitude_at(0.0, 1000.0), 50.0);
        assert_eq!(target.amplitude_at(1000.0, 1000.0), 20.0);
        assert_eq!(target.amplitude_at(500.0, 1000.0), 20.0);
        let near = target.amplitude_at(60.0, 1000.0);
        assert!(near > 20.0 && near < 50.0);
        assert_eq!(target.roads_at(0.0, 1000.0), border(0.0, 0.1).roads);
        assert_eq!(target.roads_at(500.0, 1000.0), border(0.0, 0.06).roads);
        let near = target.roads_at(250.0, 1000.0).grade;
        assert!(near > 0.06 && near < 0.1);
    }
}
