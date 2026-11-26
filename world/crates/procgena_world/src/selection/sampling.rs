//! Repeatable random draws: one uniform number per region and pass, and a
//! categorical draw from scores.

use procgena_core::hash::derive_seed;
use procgena_core::region_seed;
use procgena_terrain::RegionId;

/// The seed key of weighted Biome selection's draws. Frozen: renaming it
/// changes every weighted World.
pub(crate) const SELECTION: &str = "world.biome.selection";

/// The uniform number for `region`'s draw on `pass` (0 for the first draw),
/// in `[0, 1)`.
///
/// It is `derive_seed(region_seed(world, SELECTION, region), pass)` divided
/// by 2³², so every value is a multiple of 2⁻³², 0 is possible and 1 never
/// is. It depends on nothing but its arguments.
pub(crate) fn uniform(world: u32, region: RegionId, pass: u32) -> f64 {
    let h = derive_seed(region_seed(world, SELECTION, region), pass);
    f64::from(h) / 4_294_967_296.0
}

/// The index drawn from `scores` at `temperature` with uniform number `u`
/// in `[0, 1)`.
///
/// Index `i` is drawn with probability `exp(scores[i] / temperature)` over
/// the sum of them all (a softmax). The scores are shifted by their maximum
/// first, so large scores don't overflow; a score far enough below the
/// maximum gets a weight that rounds to 0 and is never drawn.
///
/// The indices are walked in order, adding up their weights, and the first
/// whose running sum is above `u` times the total wins. If rounding leaves
/// none above it, the last index with a weight above 0 wins.
///
/// `scores` must not be empty and must be finite; `temperature` must be
/// finite and above 0.
pub(crate) fn categorical(scores: &[f64], temperature: f64, u: f64) -> usize {
    debug_assert!(!scores.is_empty());

    // Softmax weights, shifted so the largest is exactly 1.
    let max = scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let weight = |s: f64| ((s - max) / temperature).exp();
    let total: f64 = scores.iter().map(|&s| weight(s)).sum();

    // Walk the running sum to the target.
    let target = u * total;
    let mut sum = 0.0;
    let mut last_positive = 0;
    for (i, &s) in scores.iter().enumerate() {
        let w = weight(s);
        if w > 0.0 {
            last_positive = i;
        }
        sum += w;
        if target < sum {
            return i;
        }
    }
    last_positive
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_draws_are_frozen() {
        // Golden values: a change here changes every weighted World.
        let got: Vec<u32> = [
            (0, RegionId(0), 0),
            (0, RegionId(0), 1),
            (7, RegionId(0), 0),
            (0, RegionId((u64::from(-3i32 as u32) << 32) | 5), 2),
            (123, RegionId(u64::MAX), 8),
        ]
        .iter()
        .map(|&(w, r, p)| (uniform(w, r, p) * 4_294_967_296.0) as u32)
        .collect();
        assert_eq!(got, GOLDEN_UNIFORM, "{got:?}");
    }

    const GOLDEN_UNIFORM: [u32; 5] = [227471321, 318897687, 2002306801, 3033459302, 1502289178];

    #[test]
    fn uniform_is_in_the_half_open_unit_interval() {
        for pass in 0..50 {
            for id in 0..200u64 {
                let u = uniform(3, RegionId(id * 0x1_0000_0001), pass);
                assert!((0.0..1.0).contains(&u), "{u}");
            }
        }
    }

    #[test]
    fn the_running_sum_decides_at_its_boundaries() {
        // Equal scores: each index owns a third of [0, 1).
        let scores = [0.0, 0.0, 0.0];
        assert_eq!(categorical(&scores, 1.0, 0.0), 0);
        assert_eq!(categorical(&scores, 1.0, 0.333), 0);
        assert_eq!(categorical(&scores, 1.0, 0.334), 1);
        assert_eq!(categorical(&scores, 1.0, 0.667), 2);
        assert_eq!(categorical(&scores, 1.0, 1.0 - f64::EPSILON), 2);
        // A target that equals a running sum goes to the next index.
        assert_eq!(categorical(&[0.0, 0.0], 1.0, 0.5), 1);
    }

    #[test]
    fn rounding_falls_back_to_the_last_positive_weight() {
        // The last weight underflows to 0, so it can never be drawn, even
        // at the very top of the interval.
        let scores = [0.0, 0.0, -1e6];
        assert_eq!(categorical(&scores, 1.0, 1.0 - f64::EPSILON), 1);
        // u = 1 is outside the contract, but still lands on a drawable
        // index.
        assert_eq!(categorical(&scores, 1.0, 1.0), 1);
    }

    #[test]
    fn low_temperature_picks_the_best_and_high_spreads_out() {
        let scores = [0.0, 1.0, 0.5];
        for i in 0..100 {
            let u = f64::from(i) / 100.0;
            assert_eq!(categorical(&scores, 1e-3, u), 1);
        }
        let mut counts = [0; 3];
        for i in 0..3000 {
            counts[categorical(&scores, 1e3, f64::from(i) / 3000.0)] += 1;
        }
        assert!(
            counts.iter().all(|&c| (950..=1050).contains(&c)),
            "{counts:?}"
        );
    }

    #[test]
    fn huge_scores_stay_finite() {
        let scores = [1e300, -1e300, 1e300];
        assert_eq!(categorical(&scores, 0.1, 0.0), 0);
        assert_eq!(categorical(&scores, 0.1, 0.75), 2);
    }

    #[test]
    fn draws_follow_the_softmax() {
        // Weights e^0 : e^1 = 1 : e; check the share with a fine grid of u.
        let scores = [0.0, 1.0];
        let n = 100_000;
        let ones = (0..n)
            .filter(|&i| categorical(&scores, 1.0, f64::from(i) / f64::from(n)) == 1)
            .count();
        let expected = std::f64::consts::E / (1.0 + std::f64::consts::E);
        assert!((ones as f64 / f64::from(n) - expected).abs() < 1e-4);
    }
}
