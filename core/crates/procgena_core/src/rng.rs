//! A small seeded stream of repeatable random numbers.

use crate::hash::{pcg3d, unit_float};

/// A small seeded random number stream. The same seed always gives the same values.
#[derive(Debug, Clone)]
pub struct Rng {
    seed: u32,
    counter: u32,
}

impl Rng {
    /// Starts a stream for `seed`.
    pub fn new(seed: u32) -> Self {
        Self { seed, counter: 0 }
    }

    /// Returns the next value in `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        let h = pcg3d([self.counter, self.seed, 0x5eed]);
        self.counter = self.counter.wrapping_add(1);
        unit_float(h[0])
    }

    /// Returns the next value in `[lo, hi)`.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
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
