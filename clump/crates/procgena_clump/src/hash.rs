//! Repeatable pseudo-random values.

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
}
