//! Repeatable pseudo-random values and smooth noise.

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

/// A small seeded random number stream. The same seed always gives the same values.
#[derive(Debug, Clone)]
pub(crate) struct Rng {
    seed: u32,
    counter: u32,
}

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "scatter seeding uses it in phase 3")
)]
impl Rng {
    /// Starts a stream for `seed`.
    pub(crate) fn new(seed: u32) -> Self {
        Self { seed, counter: 0 }
    }

    /// Returns the next value in `[0, 1)`.
    pub(crate) fn next_f32(&mut self) -> f32 {
        let h = pcg3d([self.counter, self.seed, 0x5eed]);
        self.counter = self.counter.wrapping_add(1);
        unit_float(h[0])
    }

    /// Returns the next value in `[lo, hi)`.
    pub(crate) fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }
}

/// Smooth 3D value noise in `[0, 1)` at `p`, with features about one unit apart.
pub(crate) fn value_noise(p: Vec3, seed: u32) -> f32 {
    let ip = p.floor();
    let f = p - ip;
    let u = f * f * (Vec3::splat(3.0) - 2.0 * f);

    // Hash the eight cell corners. The casts keep the bit pattern of negative coordinates.
    let [x, y, z] = [ip.x as i32 as u32, ip.y as i32 as u32, ip.z as i32 as u32];
    let corner = |dx: u32, dy: u32, dz: u32| {
        let h = pcg3d([
            x.wrapping_add(dx),
            y.wrapping_add(dy),
            z.wrapping_add(dz) ^ seed.rotate_left(16),
        ]);
        unit_float(h[0] ^ seed)
    };

    // Blend along x, then y, then z.
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = lerp(corner(0, 0, 0), corner(1, 0, 0), u.x);
    let x10 = lerp(corner(0, 1, 0), corner(1, 1, 0), u.x);
    let x01 = lerp(corner(0, 0, 1), corner(1, 0, 1), u.x);
    let x11 = lerp(corner(0, 1, 1), corner(1, 1, 1), u.x);
    lerp(lerp(x00, x10, u.y), lerp(x01, x11, u.y), u.z)
}

/// Layered value noise in `[0, 1)`: `octaves` layers, each twice as fine and half as strong.
/// The result is rescaled so its range doesn't depend on the layer count.
/// Zero octaves returns 0.5.
pub(crate) fn fbm(p: Vec3, seed: u32, octaves: u32) -> f32 {
    if octaves == 0 {
        return 0.5;
    }
    let mut sum = 0.0;
    let mut weight = 0.5;
    let mut total = 0.0;
    let mut q = p;
    for octave in 0..octaves {
        sum += weight * value_noise(q, seed.wrapping_add(octave));
        total += weight;
        weight *= 0.5;
        // Offset each layer so their lattices don't line up.
        q = q * 2.0 + Vec3::new(17.3, -9.1, 5.7);
    }
    sum / total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_repeats_for_a_seed() {
        let a: Vec<f32> = {
            let mut r = Rng::new(3);
            (0..8).map(|_| r.next_f32()).collect()
        };
        let b: Vec<f32> = {
            let mut r = Rng::new(3);
            (0..8).map(|_| r.next_f32()).collect()
        };
        let c: Vec<f32> = {
            let mut r = Rng::new(4);
            (0..8).map(|_| r.next_f32()).collect()
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.iter().all(|v| (0.0..1.0).contains(v)));
    }

    #[test]
    fn noise_is_in_range_and_continuous() {
        for i in 0..500 {
            let p = Vec3::new(i as f32 * 0.137, i as f32 * -0.071, i as f32 * 0.053);
            let v = fbm(p, 9, 4);
            assert!((0.0..1.0).contains(&v));
            let near = fbm(p + Vec3::splat(1e-4), 9, 4);
            assert!((v - near).abs() < 1e-2);
        }
    }
}
