//! Gradient noise and its sums (fBm), evaluated at f64 positions.
//!
//! Positions are split into an integer lattice cell and an f32 fraction, so
//! the noise is equally detailed and repeatable at any distance from the
//! origin.
//!
//! Each octave's seed is `derive_seed(seed, i)` for octave `i` of the
//! layer's own seed: a second level under the layer's keyed seed (see
//! `hash::KEYS`), so these small numbers never clash with a role's salt.

use glam::{DVec2, I64Vec2, Vec2};

use crate::calibration::{NOISE_SLOPE_MAX, NOISE_STD, RIDGED_MEAN, RIDGED_STD};
use crate::hash::{derive_seed, hash2};

/// The largest number of octaves a noise sum accepts.
pub const MAX_OCTAVES: u32 = 12;

/// Lattice gradients: 16 unit directions, 22.5° apart.
const GRADIENTS: [Vec2; 16] = [
    Vec2::new(1.0, 0.0),
    Vec2::new(0.923_879_5, 0.382_683_43),
    Vec2::new(0.707_106_77, 0.707_106_77),
    Vec2::new(0.382_683_43, 0.923_879_5),
    Vec2::new(0.0, 1.0),
    Vec2::new(-0.382_683_43, 0.923_879_5),
    Vec2::new(-0.707_106_77, 0.707_106_77),
    Vec2::new(-0.923_879_5, 0.382_683_43),
    Vec2::new(-1.0, 0.0),
    Vec2::new(-0.923_879_5, -0.382_683_43),
    Vec2::new(-0.707_106_77, -0.707_106_77),
    Vec2::new(-0.382_683_43, -0.923_879_5),
    Vec2::new(0.0, -1.0),
    Vec2::new(0.382_683_43, -0.923_879_5),
    Vec2::new(0.707_106_77, -0.707_106_77),
    Vec2::new(0.923_879_5, -0.382_683_43),
];

/// Frequency multiplier between octaves. Slightly off 2 so octave lattices
/// don't line up.
const LACUNARITY: f64 = 2.03;

/// A noise value with its gradient, both in the noise's own input units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NoiseSample {
    /// The noise value, about −0.7 to 0.7.
    pub value: f32,
    /// The rate of change of `value` along x and along the second axis.
    pub gradient: Vec2,
}

/// The gradient assigned to lattice point `cell`.
fn lattice_gradient(cell: I64Vec2, seed: u32) -> Vec2 {
    GRADIENTS[(hash2(cell.x, cell.y, seed) >> 28) as usize]
}

/// Quintic fade `6t⁵ − 15t⁴ + 10t³` and its derivative, per axis.
fn fade(t: Vec2) -> (Vec2, Vec2) {
    let u = t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let du = 30.0 * t * t * (t * (t - 2.0) + 1.0);
    (u, du)
}

/// 2D gradient (Perlin) noise with its analytic gradient.
///
/// Each integer lattice point gets a pseudo-random unit gradient from `seed`.
/// The result is zero on lattice points.
pub(crate) fn gradient_noise(p: DVec2, seed: u32) -> NoiseSample {
    // Split into lattice cell and fraction.
    let floor = p.floor();
    let cell = floor.as_i64vec2();
    let f = (p - floor).as_vec2();

    // The corners' gradients.
    let corners = [
        lattice_gradient(cell, seed),
        lattice_gradient(cell + I64Vec2::new(1, 0), seed),
        lattice_gradient(cell + I64Vec2::new(0, 1), seed),
        lattice_gradient(cell + I64Vec2::new(1, 1), seed),
    ];
    blend_corners(corners, f)
}

/// The noise at fraction `f` of a lattice cell (0 to 1 along each axis)
/// whose corners have gradients `[ga, gb, gc, gd]`: at (0, 0), (1, 0),
/// (0, 1) and (1, 1).
fn blend_corners([ga, gb, gc, gd]: [Vec2; 4], f: Vec2) -> NoiseSample {
    let va = ga.dot(f);
    let vb = gb.dot(f - Vec2::new(1.0, 0.0));
    let vc = gc.dot(f - Vec2::new(0.0, 1.0));
    let vd = gd.dot(f - Vec2::new(1.0, 1.0));

    // Blend, following Inigo Quilez's analytic-derivative formulation.
    let (u, du) = fade(f);
    let k = va - vb - vc + vd;
    let value = va + u.x * (vb - va) + u.y * (vc - va) + u.x * u.y * k;
    let gradient = ga
        + u.x * (gb - ga)
        + u.y * (gc - ga)
        + u.x * u.y * (ga - gb - gc + gd)
        + du * (Vec2::new(u.y, u.x) * k + Vec2::new(vb - va, vc - va));
    NoiseSample { value, gradient }
}

/// The rotation applied to octave `i`, which hides lattice alignment.
///
/// The angles follow Game-Heightmap-Generator (MIT): `0.37 + 0.7231·i`
/// radians.
fn octave_rotation(i: u32) -> DVec2 {
    DVec2::from_angle(0.37 + 0.7231 * f64::from(i))
}

/// Settings for one noise sum.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Octaves {
    /// Number of noise layers, 1 through [`MAX_OCTAVES`].
    pub count: u32,
    /// Amplitude multiplier between layers, 0 to 1.
    pub gain: f32,
}

impl Octaves {
    /// The amplitudes of the layers' sum: `(sum of squares).sqrt()`.
    fn rms_amplitude(self) -> f32 {
        let mut sum = 0.0;
        let mut a = 1.0;
        for _ in 0..self.count {
            sum += a * a;
            a *= self.gain;
        }
        f32::sqrt(sum)
    }
}

/// Fractal Brownian motion: a sum of rotated gradient-noise octaves, scaled
/// so its standard deviation is about 1 for any octave count and gain.
///
/// `erosion` (0 or more) damps each octave where the octaves below it are
/// steep, following Inigo Quilez's derivative-damped fBm. Slopes then stay
/// smooth while valleys and crests keep their detail, which looks worn.
/// Damping lowers the overall amplitude somewhat.
pub(crate) fn fbm(p: DVec2, seed: u32, octaves: Octaves, erosion: f32) -> f32 {
    let mut sum = 0.0;
    let mut slope = Vec2::ZERO;
    let mut amplitude = 1.0;
    let mut frequency = 1.0;
    for i in 0..octaves.count {
        let q = octave_rotation(i).rotate(p) * frequency;
        let n = gradient_noise(q, derive_seed(seed, i));
        slope += n.gradient * amplitude;
        sum += amplitude * n.value / (1.0 + erosion * slope.length_squared());
        amplitude *= octaves.gain;
        frequency *= LACUNARITY;
    }
    sum / (NOISE_STD * octaves.rms_amplitude())
}

/// [`fbm`] without erosion, with its gradient (the rate of change along x
/// and z, per unit of `p`).
pub(crate) fn fbm_gradient(p: DVec2, seed: u32, octaves: Octaves) -> (f32, DVec2) {
    let mut sum = 0.0;
    let mut gradient = DVec2::ZERO;
    let mut amplitude = 1.0;
    let mut frequency = 1.0;
    for i in 0..octaves.count {
        let rotation = octave_rotation(i);
        let q = rotation.rotate(p) * frequency;
        let n = gradient_noise(q, derive_seed(seed, i));
        sum += amplitude * n.value;
        // Back from the octave's rotated, scaled input to `p`.
        let back = DVec2::new(rotation.x, -rotation.y).rotate(n.gradient.as_dvec2());
        gradient += back * f64::from(amplitude) * frequency;
        amplitude *= octaves.gain;
        frequency *= LACUNARITY;
    }
    let norm = NOISE_STD * octaves.rms_amplitude();
    (sum / norm, gradient / f64::from(norm))
}

/// The largest gradient [`fbm`] without erosion can have, per unit of its
/// input.
pub(crate) fn fbm_slope_bound(octaves: Octaves) -> f64 {
    let mut sum = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = 1.0;
    for _ in 0..octaves.count {
        sum += amplitude * frequency;
        amplitude *= f64::from(octaves.gain);
        frequency *= LACUNARITY;
    }
    f64::from(NOISE_SLOPE_MAX) * sum / f64::from(NOISE_STD * octaves.rms_amplitude())
}

/// Ridged multifractal noise, scaled so its mean is about 0 and its standard
/// deviation about 1. Sharp crests are positive and broad valleys negative.
///
/// Each octave is `(1 − |n|)²`, weighted by the octave below it (`0.55 +
/// 0.45·previous`), so crests keep detail and valleys stay smoother. This
/// softened weighting follows Game-Heightmap-Generator (MIT).
pub(crate) fn ridged(p: DVec2, seed: u32, octaves: Octaves) -> f32 {
    let mut sum = 0.0;
    let mut weight = 1.0;
    let mut amplitude = 1.0;
    let mut total = 0.0;
    let mut frequency = 1.0;
    for i in 0..octaves.count {
        let q = octave_rotation(i).rotate(p) * frequency;
        let n = gradient_noise(q, derive_seed(seed, i)).value;
        let crest = (1.0 - (n * std::f32::consts::SQRT_2).abs()).max(0.0);
        let r = crest * crest;
        sum += amplitude * r * weight;
        total += amplitude;
        weight = 0.55 + 0.45 * r;
        amplitude *= octaves.gain;
        frequency *= LACUNARITY;
    }
    (sum / total - RIDGED_MEAN) / RIDGED_STD
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The steepest the noise gets: every choice of corner gradients, on a
    /// fine grid over the cell. Run with `--ignored`; it takes seconds in
    /// release builds.
    #[test]
    #[ignore]
    fn measure_steepest_noise() {
        let steps = 64;
        let mut steepest = 0.0f32;
        for combo in 0..GRADIENTS.len().pow(4) {
            let corners =
                [0, 1, 2, 3].map(|k| GRADIENTS[(combo >> (4 * k)) & (GRADIENTS.len() - 1)]);
            for i in 0..=steps {
                for j in 0..=steps {
                    let f = Vec2::new(i as f32, j as f32) / steps as f32;
                    steepest = steepest.max(blend_corners(corners, f).gradient.length());
                }
            }
        }
        println!("steepest noise gradient: {steepest}");
    }

    #[test]
    fn fbm_gradient_matches_fbm_and_stays_within_its_bound() {
        let octaves = Octaves {
            count: 5,
            gain: 0.5,
        };
        let bound = fbm_slope_bound(octaves);
        let h = 1e-5;
        let mut steepest = 0.0f64;
        for i in 0..20_000 {
            let p = DVec2::new(f64::from(i) * 0.0731 - 400.0, f64::from(i) * 0.0377 + 3.0);
            let (value, gradient) = fbm_gradient(p, 9, octaves);
            assert_eq!(value, fbm(p, 9, octaves, 0.0));
            if i % 100 == 0 {
                let d = |e: DVec2| {
                    f64::from(fbm(p + e * h, 9, octaves, 0.0) - fbm(p - e * h, 9, octaves, 0.0))
                        / (2.0 * h)
                };
                let numeric = DVec2::new(d(DVec2::X), d(DVec2::Y));
                assert!(
                    gradient.distance(numeric) < 0.05 * bound,
                    "{p}: {gradient} vs {numeric}"
                );
            }
            steepest = steepest.max(gradient.length());
        }
        assert!(steepest <= bound, "{steepest} over {bound}");
        // The bound isn't wildly loose either.
        assert!(steepest > 0.2 * bound, "{steepest} against {bound}");
    }

    #[test]
    fn gradient_matches_finite_differences() {
        let h = 1e-4;
        for i in 0..200 {
            let p = DVec2::new(f64::from(i) * 0.731 - 40.0, f64::from(i) * 0.377 + 3.0);
            let n = gradient_noise(p, 7);
            let dx = (gradient_noise(p + DVec2::X * h, 7).value
                - gradient_noise(p - DVec2::X * h, 7).value)
                / (2.0 * h as f32);
            let dz = (gradient_noise(p + DVec2::Y * h, 7).value
                - gradient_noise(p - DVec2::Y * h, 7).value)
                / (2.0 * h as f32);
            assert!(
                (n.gradient.x - dx).abs() < 2e-2,
                "{p}: {} vs {dx}",
                n.gradient.x
            );
            assert!(
                (n.gradient.y - dz).abs() < 2e-2,
                "{p}: {} vs {dz}",
                n.gradient.y
            );
        }
    }

    #[test]
    fn noise_is_zero_on_lattice_points_and_repeats() {
        assert_eq!(gradient_noise(DVec2::new(3.0, -9.0), 1).value, 0.0);
        let p = DVec2::new(12.25, -0.6);
        assert_eq!(gradient_noise(p, 4), gradient_noise(p, 4));
        assert_ne!(gradient_noise(p, 4), gradient_noise(p, 5));
    }

    #[test]
    fn noise_stays_detailed_far_from_the_origin() {
        // The fraction within a lattice cell is exact however large the cell
        // index is, so the same offsets give values of the same spread.
        let far = DVec2::splat(1.0e9);
        let spread = |base: DVec2| {
            (0..64)
                .map(|i| {
                    gradient_noise(base + DVec2::splat(f64::from(i) * 0.13), 3)
                        .value
                        .abs()
                })
                .fold(0.0f32, f32::max)
        };
        assert!(spread(far) > 0.1);
        assert!(spread(DVec2::ZERO) > 0.1);
    }
}
