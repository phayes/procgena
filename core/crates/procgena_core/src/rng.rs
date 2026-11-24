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

    /// Returns the next whole number, any `u32` equally likely. Advances the
    /// stream by one step, as [`next_f32`](Self::next_f32) does.
    pub fn next_u32(&mut self) -> u32 {
        let h = pcg3d([self.counter, self.seed, 0x5eed]);
        self.counter = self.counter.wrapping_add(1);
        h[0]
    }

    /// Returns the next value in `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        unit_float(self.next_u32())
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

    /// Checks fixed outputs against reference values calculated with a
    /// Python copy of `pcg3d`, for both kinds of draw. The `f32` values pin
    /// `next_f32` to what it gave before `next_u32` existed.
    #[test]
    fn draws_match_reference_values() {
        let mut r = Rng::new(7);
        let whole: Vec<u32> = (0..4).map(|_| r.next_u32()).collect();
        assert_eq!(
            whole,
            [3_148_222_404, 2_733_663_051, 2_288_609_043, 4_094_185_524]
        );

        let mut r = Rng::new(7);
        let unit: Vec<f32> = (0..4).map(|_| r.next_f32()).collect();
        assert_eq!(unit, [0.733_002_6, 0.636_480_5, 0.532_858_3, 0.953_251_84]);
    }

    /// Each draw, of either kind, advances the stream by one step.
    #[test]
    fn both_draws_share_one_stream() {
        let mut mixed = Rng::new(11);
        let mut whole = Rng::new(11);
        let first = mixed.next_u32();
        assert_eq!(first, whole.next_u32());
        assert_eq!(mixed.next_f32(), unit_float(whole.next_u32()));
    }
}
