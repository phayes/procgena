//! Repeatable pseudo-random values from integer inputs.
//!
//! The same functions exist as WESL shaders in the generators that run on
//! the GPU, and give the same results there.

/// Multiplier applied to each input before mixing in [`pcg3d`].
pub const PCG_MULTIPLIER: u32 = 1_664_525;
/// Increment added to each input before mixing in [`pcg3d`].
pub const PCG_INCREMENT: u32 = 1_013_904_223;

/// Turns three integers into three repeatable pseudo-random integers.
///
/// Uses the PCG3D hash described by Jarzynski and Olano in “Hash Functions for
/// GPU Rendering” (2020). Wrapping arithmetic preserves 32-bit overflow behavior.
pub fn pcg3d(v: [u32; 3]) -> [u32; 3] {
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

/// Turns one integer into a repeatable pseudo-random integer.
///
/// This is the 32-bit PCG hash that Scatter placement uses on the CPU and
/// the GPU (plant's `hash` in its shaders). It uses only wrapping 32-bit
/// arithmetic, so both sides give the same bits.
pub fn pcg(x: u32) -> u32 {
    let state = x.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277_803_737);
    (word >> 22) ^ word
}

/// Turns two integers into one repeatable pseudo-random integer, with
/// [`pcg`]. The order matters: `pcg2(a, b)` differs from `pcg2(b, a)`.
pub fn pcg2(a: u32, b: u32) -> u32 {
    pcg(a ^ pcg(b))
}

/// Converts a hash to a number from 0 (inclusive) to 1 (exclusive).
/// Only the upper 24 bits are used, so the conversion fits exactly in an `f32`.
pub fn unit_float(h: u32) -> f32 {
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

/// Hashes three integers and returns three repeatable values in `[0, 1)`.
pub fn pcg3d_unit(v: [u32; 3]) -> [f32; 3] {
    pcg3d(v).map(unit_float)
}

/// Derives a seed for one use (`salt`) from a parent seed, so that separate
/// streams of random numbers don't repeat each other.
///
/// For a named role, use [`seed_for`] with a seed key instead of picking a
/// salt by hand. A numeric salt suits a second level only, such as each
/// octave of one noise layer.
pub fn derive_seed(seed: u32, salt: u32) -> u32 {
    pcg3d([seed, salt, 0x7e44_a1d5])[0]
}

/// The salt for seed key `key`: the 32-bit FNV-1a hash of its bytes.
///
/// Seed keys are dotted names of the form `<crate>.<area>.<role>`, such as
/// `"terrain.relief.hills"`. They are frozen: renaming a key changes every
/// world built with it. Usable in `const` items.
pub const fn salt(key: &str) -> u32 {
    let bytes = key.as_bytes();
    let mut hash = 0x811c_9dc5u32;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u32;
        hash = hash.wrapping_mul(0x0100_0193);
        i += 1;
    }
    hash
}

/// The seed for the role named by seed key `key`, under parent seed `seed`.
///
/// The same as `derive_seed(seed, salt(key))`.
pub fn seed_for(seed: u32, key: &str) -> u32 {
    derive_seed(seed, salt(key))
}

/// Mixes a 64-bit `value` into a 64-bit hash `state` and returns the new
/// state.
///
/// For a fixed `state`, different values always give different results.
/// The order of mixing matters: `mix64(mix64(s, a), b)` differs from
/// `mix64(mix64(s, b), a)`.
pub fn mix64(state: u64, value: u64) -> u64 {
    // Combine like boost's hash_combine, then apply the SplitMix64 finaliser.
    let mut z = state
        ^ value
            .wrapping_add(0x9e37_79b9_7f4a_7c15)
            .wrapping_add(state << 6);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Checks fixed outputs against reference values calculated with Python integers
    /// and masked to 32 bits to reproduce wrapping arithmetic.
    #[test]
    fn pcg3d_matches_reference_vectors() {
        assert_eq!(pcg3d([0, 0, 0]), REFERENCE[0]);
        assert_eq!(pcg3d([1, 2, 3]), REFERENCE[1]);
        assert_eq!(pcg3d([u32::MAX, 880, 0xdead_beef]), REFERENCE[2]);
    }

    const REFERENCE: [[u32; 3]; 3] = [
        [0x9baf_d7c6, 0xa8e8_8a6b, 0x3f15_482c],
        [0xfa9f_79a6, 0x48f2_f44c, 0x596f_5ab1],
        [0x7a3b_bcd7, 0x4168_5d55, 0xab04_d812],
    ];

    /// Checks seeds against reference values calculated in Python from the
    /// published FNV-1a and PCG3D definitions. The salts and seeds must
    /// match `procgena_terrain`'s, so its worlds keep their shape when it
    /// switches to these functions.
    #[test]
    fn seeds_match_reference_values() {
        // FNV-1a's published value for "a".
        assert_eq!(salt("a"), 0xe40c_292c);
        assert_eq!(salt("terrain.relief.hills"), 0x0454_4a2b);
        assert_eq!(derive_seed(0, 0), 0x4cfe_e1f2);
        assert_eq!(derive_seed(42, 7), 0x6880_6509);
        assert_eq!(seed_for(42, "terrain.relief.hills"), 0x3db9_3fcf);
    }

    #[test]
    fn salt_is_usable_in_const_items() {
        const SALT: u32 = salt("a");
        assert_eq!(SALT, 0xe40c_292c);
    }

    /// `mix64(0, 0)` is SplitMix64's published first output for seed 0.
    #[test]
    fn mix64_matches_reference_values() {
        assert_eq!(mix64(0, 0), 0xe220_a839_7b1d_cdaf);
        assert_eq!(mix64(1, 2), 0x3706_970b_052f_16b1);
        assert_ne!(mix64(1, 2), mix64(2, 1));
    }

    /// Reference values calculated in Python from the PCG hash's
    /// definition, masked to 32 bits.
    #[test]
    fn pcg_matches_reference_values() {
        assert_eq!(pcg(0), 0x07bb_2fe2);
        assert_eq!(pcg(1), 0xa8be_ea3c);
        assert_eq!(pcg(0xdead_beef), 0x6729_9972);
        assert_eq!(pcg2(1, 2), 0x7ded_a5d8);
        assert_ne!(pcg2(1, 2), pcg2(2, 1));
    }

    #[test]
    fn unit_float_covers_the_unit_interval() {
        assert_eq!(unit_float(0), 0.0);
        assert_eq!(unit_float(u32::MAX), 16_777_215.0 / 16_777_216.0);
        assert!(unit_float(u32::MAX) < 1.0);
    }
}
