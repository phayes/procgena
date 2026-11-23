//! Measured statistics of this crate's noise, so that settings can be given
//! in metres and percentages.
//!
//! The values are measured by the test `calibration_matches_measurement`.
//! Change them only together with the noise functions.

/// Standard deviation of [`gradient_noise`](crate::noise::gradient_noise)
/// over the plane.
pub(crate) const NOISE_STD: f32 = 0.215_715;

/// The largest gradient [`gradient_noise`](crate::noise::gradient_noise)
/// ever has, per unit of its input, with a little to spare: measured as
/// 2.067 over every choice of corner gradients by the ignored test
/// `measure_steepest_noise`.
pub(crate) const NOISE_SLOPE_MAX: f32 = 2.1;

/// Mean of the raw ridged sum (before scaling), at [`RIDGED_OCTAVES`].
pub(crate) const RIDGED_MEAN: f32 = 0.541_666;

/// Standard deviation of the raw ridged sum (before scaling), at
/// [`RIDGED_OCTAVES`].
pub(crate) const RIDGED_STD: f32 = 0.156_966;

/// The octaves the ridged statistics were measured with. Other octave counts
/// and gains are close, but not exact.
pub(crate) const RIDGED_OCTAVES: crate::noise::Octaves = crate::noise::Octaves {
    count: 6,
    gain: 0.5,
};

/// The octaves the mask quantiles were measured with.
pub(crate) const MASK_OCTAVES: crate::noise::Octaves = crate::noise::Octaves {
    count: 3,
    gain: 0.5,
};

/// Quantiles of [`fbm`](crate::noise::fbm) at [`MASK_OCTAVES`] without
/// erosion: entry `i` is the value below which `i·5 %` of the plane lies.
pub(crate) const MASK_QUANTILES: [f32; 21] = [
    -3.726, -1.668, -1.313, -1.071, -0.871, -0.699, -0.545, -0.401, -0.264, -0.129, 0.002, 0.134,
    0.268, 0.405, 0.547, 0.699, 0.870, 1.063, 1.310, 1.655, 3.558,
];

/// Returns the fBm value below which `fraction` (0 to 1) of the plane lies,
/// for noise at [`MASK_OCTAVES`]. Use it to turn an area share into a
/// threshold.
pub(crate) fn mask_threshold(fraction: f32) -> f32 {
    let t = fraction.clamp(0.0, 1.0) * 20.0;
    let i = (t.floor() as usize).min(19);
    let f = t - i as f32;
    MASK_QUANTILES[i] + (MASK_QUANTILES[i + 1] - MASK_QUANTILES[i]) * f
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::noise::{Octaves, fbm, gradient_noise, ridged};
    use glam::DVec2;

    /// Sample positions spread over a large area, off any lattice.
    fn samples() -> impl Iterator<Item = DVec2> {
        (0..400_000u32).map(|i| {
            let i = f64::from(i);
            // Golden-ratio spacing in a 1000 × 1000 cell area.
            DVec2::new((i * 0.618_033_988_75) % 1000.0, (i * 0.002_5) % 1000.0) + 0.123
        })
    }

    fn mean_std(values: &[f32]) -> (f32, f32) {
        let n = values.len() as f64;
        let mean = values.iter().map(|&v| f64::from(v)).sum::<f64>() / n;
        let var = values
            .iter()
            .map(|&v| (f64::from(v) - mean).powi(2))
            .sum::<f64>()
            / n;
        (mean as f32, var.sqrt() as f32)
    }

    #[test]
    fn calibration_matches_measurement() {
        // Raw noise spread.
        let noise: Vec<f32> = samples().map(|p| gradient_noise(p, 11).value).collect();
        let (_, noise_std) = mean_std(&noise);

        // Raw ridged sum: undo the scaling applied with the current constants.
        let ridge: Vec<f32> = samples()
            .map(|p| ridged(p * 0.05, 12, RIDGED_OCTAVES) * RIDGED_STD + RIDGED_MEAN)
            .collect();
        let (ridged_mean, ridged_std) = mean_std(&ridge);

        // Mask quantiles, measured on fBm scaled with the measured noise spread.
        let mut mask: Vec<f32> = samples()
            .map(|p| fbm(p * 0.05, 13, MASK_OCTAVES, 0.0) * NOISE_STD / noise_std)
            .collect();
        mask.sort_by(f32::total_cmp);
        let quantiles: Vec<f32> = (0..=20)
            .map(|i| mask[((i as f64 / 20.0) * (mask.len() - 1) as f64).round() as usize])
            .collect();

        eprintln!("NOISE_STD = {noise_std}");
        eprintln!("RIDGED_MEAN = {ridged_mean}, RIDGED_STD = {ridged_std}");
        eprintln!("MASK_QUANTILES = {quantiles:?}");

        assert!((noise_std - NOISE_STD).abs() < 0.01 * NOISE_STD);
        assert!((ridged_mean - RIDGED_MEAN).abs() < 0.01);
        assert!((ridged_std - RIDGED_STD).abs() < 0.01 * RIDGED_STD);
        // The extremes depend on the sample set, so only the inner ones are checked.
        for i in 1..20 {
            assert!(
                (quantiles[i] - MASK_QUANTILES[i]).abs() < 0.03,
                "quantile {i}"
            );
        }
    }

    #[test]
    fn fbm_has_about_unit_spread_for_any_octaves() {
        for (count, gain) in [(1, 0.5), (4, 0.5), (6, 0.35), (8, 0.65)] {
            let values: Vec<f32> = samples()
                .take(100_000)
                .map(|p| fbm(p * 0.05, 3, Octaves { count, gain }, 0.0))
                .collect();
            let (mean, std) = mean_std(&values);
            assert!(mean.abs() < 0.05, "{count} {gain}: mean {mean}");
            assert!((std - 1.0).abs() < 0.15, "{count} {gain}: std {std}");
        }
    }

    #[test]
    fn mask_threshold_is_monotonic() {
        let mut last = f32::NEG_INFINITY;
        for i in 0..=100 {
            let t = mask_threshold(i as f32 / 100.0);
            assert!(t >= last);
            last = t;
        }
    }
}
