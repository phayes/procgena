//! Repeatable random numbers and smooth noise.
//!
//! Everything here is "pseudo-random": it looks random but is worked out
//! from a seed, so the same seed always gives the same mushroom.

use glam::Vec3;

/// Multiplier applied to each input before mixing in [`pcg3d`].
const PCG_MULTIPLIER: u32 = 1_664_525;
/// Increment added to each input before mixing in [`pcg3d`].
const PCG_INCREMENT: u32 = 1_013_904_223;

/// Turns three integers into three repeatable pseudo-random integers.
///
/// Uses the PCG3D hash from Jarzynski and Olano, "Hash Functions for GPU
/// Rendering" (2020), with wrapping 32-bit arithmetic.
pub(crate) fn pcg3d(v: [u32; 3]) -> [u32; 3] {
    let [mut x, mut y, mut z] =
        v.map(|c| c.wrapping_mul(PCG_MULTIPLIER).wrapping_add(PCG_INCREMENT));

    x = x.wrapping_add(y.wrapping_mul(z));
    y = y.wrapping_add(z.wrapping_mul(x));
    z = z.wrapping_add(x.wrapping_mul(y));

    x ^= x >> 16;
    y ^= y >> 16;
    z ^= z >> 16;

    x = x.wrapping_add(y.wrapping_mul(z));
    y = y.wrapping_add(z.wrapping_mul(x));
    z = z.wrapping_add(x.wrapping_mul(y));

    [x, y, z]
}

/// Converts a hash to a number from 0 (inclusive) to 1 (exclusive), using its
/// upper 24 bits so the result is exact in an `f32`.
pub(crate) fn unit_float(h: u32) -> f32 {
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

/// A small seeded random number stream. The same seed and stream id always
/// give the same values.
#[derive(Debug, Clone)]
pub(crate) struct Rng {
    seed: u32,
    stream: u32,
    counter: u32,
}

impl Rng {
    /// Starts stream `stream` for `seed`. Different streams of one seed are
    /// independent, so adding draws to one never shifts another.
    pub(crate) fn new(seed: u32, stream: u32) -> Self {
        Self {
            seed,
            stream,
            counter: 0,
        }
    }

    /// Returns the next value in `[0, 1)`.
    pub(crate) fn next_f32(&mut self) -> f32 {
        let h = pcg3d([self.counter, self.seed, self.stream]);
        self.counter = self.counter.wrapping_add(1);
        unit_float(h[0])
    }

    /// Returns the next value in `[lo, hi)`.
    pub(crate) fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }

    /// Returns the next value in `[1 - amount, 1 + amount)`.
    pub(crate) fn jitter(&mut self, amount: f32) -> f32 {
        self.range(1.0 - amount, 1.0 + amount)
    }

    /// Returns the next whole number in `lo..=hi`.
    pub(crate) fn int(&mut self, lo: u32, hi: u32) -> u32 {
        let span = (hi - lo + 1) as f32;
        lo + ((self.next_f32() * span) as u32).min(hi - lo)
    }
}

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
        total += value_noise(q, seed.wrapping_add(octave.wrapping_mul(0x9e37_79b9))) * weight;
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
    use super::*;

    #[test]
    fn streams_repeat_and_differ() {
        let draw = |seed, stream| {
            let mut r = Rng::new(seed, stream);
            (0..8).map(|_| r.next_f32()).collect::<Vec<_>>()
        };
        assert_eq!(draw(3, 1), draw(3, 1));
        assert_ne!(draw(3, 1), draw(3, 2));
        assert_ne!(draw(3, 1), draw(4, 1));
    }

    #[test]
    fn int_stays_in_range() {
        let mut r = Rng::new(9, 0);
        for _ in 0..1000 {
            let v = r.int(2, 5);
            assert!((2..=5).contains(&v));
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
