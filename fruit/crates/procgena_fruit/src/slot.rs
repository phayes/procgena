//! Choosing which specimen of a pool a placed copy draws.

use procgena_core::hash::pcg;

/// The largest number of specimens a pool may hold.
pub const MAX_SLOTS: u32 = 256;

/// The slot, from 0 up to `slots`, that the copy named by `key` draws.
///
/// This is rendezvous hashing: each slot scores `hash(key ^ hash(slot))`
/// and the highest score wins, with ties going to the lowest slot. Growing
/// or shrinking the pool moves only the copies whose slot appears or
/// disappears. `slots` is clamped to 1–[`MAX_SLOTS`].
///
/// ```
/// use procgena_fruit::choose_slot;
///
/// let slot = choose_slot(1234, 6);
/// assert!(slot < 6);
/// // A copy whose slot is still in a smaller pool keeps it.
/// if slot < 4 {
///     assert_eq!(choose_slot(1234, 4), slot);
/// }
/// ```
pub fn choose_slot(key: u32, slots: u32) -> u32 {
    let slots = slots.clamp(1, MAX_SLOTS);
    let mut best = (0, pcg(key ^ pcg(0)));
    for slot in 1..slots {
        let score = pcg(key ^ pcg(slot));
        if score > best.1 {
            best = (slot, score);
        }
    }
    best.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_are_spread_evenly() {
        let mut counts = [0u32; 6];
        for key in 0..6000 {
            counts[choose_slot(key, 6) as usize] += 1;
        }
        assert!(counts.iter().all(|&c| (800..1200).contains(&c)), "{counts:?}");
    }

    #[test]
    fn shrinking_the_pool_keeps_the_slots_still_in_it() {
        for key in 0..2000 {
            let wide = choose_slot(key, 8);
            if wide < 5 {
                assert_eq!(choose_slot(key, 5), wide, "key {key}");
            }
        }
    }

    #[test]
    fn a_pool_of_one_always_gives_slot_zero() {
        assert_eq!(choose_slot(77, 1), 0);
        assert_eq!(choose_slot(77, 0), 0);
    }
}
