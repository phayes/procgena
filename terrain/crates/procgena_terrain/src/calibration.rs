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

/// How much erosion lowers the spread of an eroded noise layer, as
/// (erosion, spread with erosion over spread without) pairs, rising in
/// erosion; between them, interpolate linearly (see [`erosion_damping`]).
///
/// Measured by the ignored test `measure_erosion_damping` on fBm at
/// roughness 0.5 and 6 octaves over 400 000 samples. The octave count
/// makes no difference, and roughness from 0.4 to 0.6 changes the
/// damping by less than 1.5 %.
pub(crate) const EROSION_DAMPING: [(f32, f32); 12] = [
    (0.0, 1.0),
    (0.25, 0.867),
    (0.5, 0.782),
    (0.75, 0.722),
    (1.0, 0.674),
    (1.5, 0.605),
    (2.0, 0.555),
    (3.0, 0.485),
    (4.0, 0.437),
    (6.0, 0.374),
    (8.0, 0.333),
    (10.0, 0.303),
];

/// The share of its spread an eroded noise layer keeps at `erosion` (0 to
/// [`MAX_EROSION`](crate::MAX_EROSION)), from [`EROSION_DAMPING`].
pub(crate) fn erosion_damping(erosion: f32) -> f32 {
    let table = &EROSION_DAMPING;
    let i = table
        .iter()
        .position(|&(e, _)| e >= erosion)
        .unwrap_or(table.len() - 1)
        .max(1);
    let ((e0, d0), (e1, d1)) = (table[i - 1], table[i]);
    let t = ((erosion - e0) / (e1 - e0)).clamp(0.0, 1.0);
    d0 + (d1 - d0) * t
}

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

    /// How much erosion lowers the spread of eroded fBm, against the same
    /// noise without erosion, over erosion strengths, octave counts and
    /// roughness. Run with `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn measure_erosion_damping() {
        let points: Vec<DVec2> = samples().step_by(10).collect();
        for gain in [0.4f32, 0.5, 0.6] {
            for count in [3u32, 5, 6, 8] {
                let octaves = crate::noise::Octaves { count, gain };
                let spread = |erosion: f32| {
                    let v: Vec<f32> = points
                        .iter()
                        .map(|&p| crate::noise::fbm(p * 0.1, 21, octaves, erosion))
                        .collect();
                    mean_std(&v).1
                };
                let raw = spread(0.0);
                let ratios: Vec<String> = [0.3f32, 0.5, 1.0, 2.0, 4.0, 10.0]
                    .iter()
                    .map(|&e| format!("{e}: {:.3}", spread(e) / raw))
                    .collect();
                println!("DAMP gain {gain}, {count} octaves: {}", ratios.join(", "));
            }
        }

        // The table, at roughness 0.5 and 6 octaves.
        let octaves = crate::noise::Octaves {
            count: 6,
            gain: 0.5,
        };
        let spread = |erosion: f32| {
            let v: Vec<f32> = samples()
                .map(|p| crate::noise::fbm(p * 0.1, 21, octaves, erosion))
                .collect();
            mean_std(&v).1
        };
        let raw = spread(0.0);
        let table: Vec<String> = EROSION_DAMPING
            .iter()
            .map(|&(e, _)| format!("({e:.2}, {:.3})", spread(e) / raw))
            .collect();
        println!("TABLE {}", table.join(", "));
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
