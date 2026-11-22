//! Repeatable pseudo-random values from integer coordinates.

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

/// Folds a 64-bit lattice coordinate into 32 bits, keeping nearby
/// coordinates distinct.
fn fold(v: i64) -> u32 {
    let bits = v as u64;
    (bits as u32) ^ ((bits >> 32) as u32).wrapping_mul(0x9e37_79b9)
}

/// Hashes a 2D lattice cell and a seed.
pub(crate) fn hash2(x: i64, z: i64, seed: u32) -> u32 {
    pcg3d([fold(x), fold(z), seed])[0]
}

/// Derives a seed for one use (`salt`) from a parent seed, so separate noise
/// layers don't repeat each other.
pub(crate) fn derive_seed(seed: u32, salt: u32) -> u32 {
    pcg3d([seed, salt, 0x7e44_a1d5])[0]
}

/// Mixes a 64-bit value into a 64-bit hash, for building identifiers.
pub(crate) fn mix64(state: u64, value: u64) -> u64 {
    // SplitMix64 finaliser over the combined value.
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

    #[test]
    fn hashes_repeat_and_differ() {
        assert_eq!(hash2(3, -7, 1), hash2(3, -7, 1));
        assert_ne!(hash2(3, -7, 1), hash2(3, -7, 2));
        assert_ne!(hash2(3, -7, 1), hash2(-7, 3, 1));
        // Coordinates differing only in their upper 32 bits stay distinct.
        assert_ne!(hash2(1 << 33, 0, 1), hash2(0, 0, 1));
        assert_ne!(derive_seed(1, 2), derive_seed(1, 3));
        assert_ne!(mix64(1, 2), mix64(2, 1));
    }
}
