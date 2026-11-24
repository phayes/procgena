//! Repeatable random numbers, smooth noise and the crate's seed keys.
//!
//! Everything here is "pseudo-random": it looks random but is worked out
//! from a seed, so the same seed always gives the same mushroom. The random
//! number stream itself is [`procgena_core::Rng`]; each use gets its own
//! seed from a named seed key (see [`KEYS`]), so adding draws to one use
//! never shifts another.

use glam::Vec3;
use procgena_core::Rng;
use procgena_core::hash::{derive_seed, pcg3d, unit_float};

// --------------------------------------------------------------------------
// Seed keys
//
// Each key names one use of random numbers. Keys are frozen: renaming one
// changes every mushroom built with it. To add a use, add a key here and
// list it in `KEYS`.

/// The rim waves, lopsidedness and tilt of a cap.
pub(crate) const SHAPE_CAP: &str = "mushroom.shape.cap";
/// Wart placement on a cap.
pub(crate) const SHAPE_WARTS: &str = "mushroom.shape.warts";
/// The surface-noise pattern of a cap, stem and annulus.
pub(crate) const SHAPE_NOISE: &str = "mushroom.shape.noise";
/// The seed of each single mushroom (body) of a specimen.
pub(crate) const CLUSTER_BODY: &str = "mushroom.cluster.body";
/// The small differences between the bodies of one specimen.
pub(crate) const CLUSTER_VARIATION: &str = "mushroom.cluster.variation";
/// Member sizes and placement in a cluster.
pub(crate) const CLUSTER_LAYOUT: &str = "mushroom.cluster.layout";
/// Growing spots, sizes and turns in a scatter.
pub(crate) const SCATTER_SITES: &str = "mushroom.scatter.sites";
/// The layout of the cluster grown at each spot of a scatter.
pub(crate) const SCATTER_CLUSTER: &str = "mushroom.scatter.cluster";

/// Every seed key of this crate, plus the generator kind's key.
#[cfg(test)]
pub(crate) const KEYS: &[&str] = &[
    crate::specimen_id::GENERATOR_KIND_KEY,
    SHAPE_CAP,
    SHAPE_WARTS,
    SHAPE_NOISE,
    CLUSTER_BODY,
    CLUSTER_VARIATION,
    CLUSTER_LAYOUT,
    SCATTER_SITES,
    SCATTER_CLUSTER,
];

// --------------------------------------------------------------------------
// Random numbers

/// Draws this crate needs beyond [`Rng::next_f32`] and [`Rng::range`].
pub(crate) trait RngExt {
    /// Returns the next value in `[1 - amount, 1 + amount)`.
    fn jitter(&mut self, amount: f32) -> f32;
    /// Returns the next whole number in `lo..=hi`. `lo` must not be above
    /// `hi`.
    fn int(&mut self, lo: u32, hi: u32) -> u32;
}

impl RngExt for Rng {
    fn jitter(&mut self, amount: f32) -> f32 {
        self.range(1.0 - amount, 1.0 + amount)
    }

    fn int(&mut self, lo: u32, hi: u32) -> u32 {
        let span = (hi - lo) as f32 + 1.0;
        lo + ((self.next_f32() * span) as u32).min(hi - lo)
    }
}

// --------------------------------------------------------------------------
// Noise

/// Returns smooth random noise at point `p`, from -1 to 1: a value that
/// changes gradually as `p` moves, with features about one unit apart.
///
/// `octaves` layers are added, each with features half the size and half
/// the strength of the one before, for finer detail. `seed` picks an
/// independent pattern.
pub(crate) fn fbm(p: Vec3, seed: u32, octaves: u32) -> f32 {
    let mut total = 0.0;
    let mut weight = 1.0;
    let mut norm = 0.0;
    let mut q = p;
    for octave in 0..octaves {
        total += value_noise(q, derive_seed(seed, octave)) * weight;
        norm += weight;
        weight *= 0.5;
        q *= 2.0;
    }
    if norm > 0.0 { total / norm } else { 0.0 }
}

/// Returns one layer of noise, from -1 to 1: random values at whole-number
/// grid points, blended smoothly in between.
fn value_noise(p: Vec3, seed: u32) -> f32 {
    let cell = p.floor();
    let f = p - cell;
    let w = f * f * (Vec3::splat(3.0) - 2.0 * f);
    let [cx, cy, cz] = [cell.x as i32, cell.y as i32, cell.z as i32];

    // Blend the random values at the eight corners of the grid cell around
    // `p`.
    let corner = |dx: i32, dy: i32, dz: i32| {
        let h = pcg3d([
            (cx + dx) as u32 ^ seed,
            (cy + dy) as u32,
            ((cz + dz) as u32).wrapping_add(seed.rotate_left(13)),
        ]);
        unit_float(h[0]) * 2.0 - 1.0
    };
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = lerp(corner(0, 0, 0), corner(1, 0, 0), w.x);
    let x10 = lerp(corner(0, 1, 0), corner(1, 1, 0), w.x);
    let x01 = lerp(corner(0, 0, 1), corner(1, 0, 1), w.x);
    let x11 = lerp(corner(0, 1, 1), corner(1, 1, 1), w.x);
    lerp(lerp(x00, x10, w.y), lerp(x01, x11, w.y), w.z)
}

/// Returns 0 below `edge0`, 1 above `edge1`, and a smooth S-curve between.
pub(crate) fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use procgena_core::hash::salt;

    use super::*;

    #[test]
    fn seed_keys_and_their_salts_are_all_different() {
        for (i, a) in KEYS.iter().enumerate() {
            assert!(a.starts_with("mushroom."), "{a}");
            assert_eq!(a.split('.').count(), 3, "{a} isn't <crate>.<area>.<role>");
            for b in &KEYS[i + 1..] {
                assert_ne!(a, b);
                assert_ne!(salt(a), salt(b), "{a} and {b} share a salt");
            }
        }
    }

    #[test]
    fn int_stays_in_range() {
        let mut r = Rng::new(9);
        for _ in 0..1000 {
            let v = r.int(2, 5);
            assert!((2..=5).contains(&v));
        }
        let mut all = Rng::new(9);
        for _ in 0..1000 {
            all.int(0, u32::MAX);
        }
    }

    #[test]
    fn noise_is_bounded_and_continuous() {
        for i in 0..200 {
            let p = Vec3::new(i as f32 * 0.137, i as f32 * -0.071, i as f32 * 0.29);
            let a = fbm(p, 5, 3);
            let b = fbm(p + Vec3::splat(1e-4), 5, 3);
            assert!((-1.0..=1.0).contains(&a));
            assert!((a - b).abs() < 1e-2);
        }
    }
}
