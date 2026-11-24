//! Repeatable random numbers and smooth noise.
//!
//! Everything here is "pseudo-random": it looks random but is worked out
//! from a seed, so the same seed always gives the same piece of wood.
//! Random number streams are [`procgena_core::Rng`], each started from a
//! seed for one use (see [`crate::seed_keys`]).

use glam::Vec3;
use procgena_core::Rng;
use procgena_core::hash::{pcg3d, seed_for, unit_float};

/// Starts the random number stream for the use named by seed key `key`
/// under `seed`.
pub(crate) fn stream(seed: u32, key: &str) -> Rng {
    Rng::new(seed_for(seed, key))
}

/// Draws that this crate needs beyond [`Rng`]'s own.
pub(crate) trait RngExt {
    /// Returns the next value in `[1 - amount, 1 + amount)`.
    fn jitter(&mut self, amount: f32) -> f32;

    /// Returns true with probability `p`.
    fn chance(&mut self, p: f32) -> bool;
}

impl RngExt for Rng {
    fn jitter(&mut self, amount: f32) -> f32 {
        self.range(1.0 - amount, 1.0 + amount)
    }

    fn chance(&mut self, p: f32) -> bool {
        self.next_f32() < p
    }
}

/// Returns smooth random noise at point `p`, from -1 to 1: a value that
/// changes gradually as `p` moves, with features about one unit apart.
///
/// `octaves` layers are added, each with features half the size and half
/// the strength of the one before, for finer detail. (The name stands for
/// "fractal Brownian motion", the usual term for noise layered this way.)
/// `seed` picks an independent pattern. Zero octaves gives 0.
pub(crate) fn fbm(p: Vec3, seed: u32, octaves: u32) -> f32 {
    let mut total = 0.0;
    let mut weight = 1.0;
    let mut norm = 0.0;
    let mut q = p;
    for octave in 0..octaves {
        total += value_noise(q, seed.wrapping_add(octave.wrapping_mul(0x9e37_79b9))) * weight;
        norm += weight;
        weight *= 0.5;
        q *= 2.0;
    }
    if norm > 0.0 { total / norm } else { 0.0 }
}

/// Returns one layer of noise, from -1 to 1: random values at whole-number
/// grid points, blended smoothly in between.
pub(crate) fn value_noise(p: Vec3, seed: u32) -> f32 {
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

/// Returns how far `p` is from the nearest border between random cells.
///
/// Space is split into cells around randomly placed centres, one centre per
/// unit cube. The result is the gap between the distances to the nearest
/// and second-nearest centre. It is 0 on a border and grows toward a cell's
/// middle, so `1 - smoothstep(0, width, ..)` draws thin cracks between
/// plates.
pub(crate) fn cell_border(p: Vec3, seed: u32) -> f32 {
    let cell = p.floor();
    let [cx, cy, cz] = [cell.x as i32, cell.y as i32, cell.z as i32];
    let (mut f1, mut f2) = (f32::MAX, f32::MAX);

    // Check the jittered centres of the 27 cells around `p`.
    for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let h = pcg3d([
                    (cx + dx) as u32 ^ seed,
                    (cy + dy) as u32,
                    ((cz + dz) as u32).wrapping_add(seed.rotate_left(7)),
                ]);
                let centre = Vec3::new(
                    (cx + dx) as f32 + unit_float(h[0]),
                    (cy + dy) as f32 + unit_float(h[1]),
                    (cz + dz) as f32 + unit_float(h[2]),
                );
                let d = centre.distance(p);
                if d < f1 {
                    f2 = f1;
                    f1 = d;
                } else if d < f2 {
                    f2 = d;
                }
            }
        }
    }
    f2 - f1
}

/// Returns 0 below `edge0`, 1 above `edge1`, and a smooth S-curve between.
pub(crate) fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_repeat_and_differ() {
        let draw = |seed, key| {
            let mut r = stream(seed, key);
            (0..8).map(|_| r.next_f32()).collect::<Vec<_>>()
        };
        let (a, b) = (crate::seed_keys::STEM_SHAPE, crate::seed_keys::STUB_SHAPE);
        assert_eq!(draw(3, a), draw(3, a));
        assert_ne!(draw(3, a), draw(3, b));
        assert_ne!(draw(3, a), draw(4, a));
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

    #[test]
    fn cell_border_is_non_negative() {
        for i in 0..200 {
            let p = Vec3::new(i as f32 * 0.31, i as f32 * 0.17, i as f32 * -0.23);
            assert!(cell_border(p, 9) >= 0.0);
        }
    }
}
