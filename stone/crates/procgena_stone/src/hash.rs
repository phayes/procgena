//! Repeatable pseudo-random values for rock shapes and surface patterns.

/// Multiplier applied to each input before mixing in [`pcg3d`].
pub(crate) const PCG_MULTIPLIER: u32 = 1_664_525;
/// Increment added to each input before mixing in [`pcg3d`].
pub(crate) const PCG_INCREMENT: u32 = 1_013_904_223;

/// Turns three integers into three repeatable pseudo-random integers.
///
/// Uses the PCG3D hash described by Jarzynski and Olano in “Hash Functions for
/// GPU Rendering” (2020). Wrapping arithmetic preserves 32-bit overflow behavior.
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

/// Converts a hash to a number from 0 (inclusive) to 1 (exclusive).
/// Only the upper 24 bits are used, so the conversion fits exactly in an `f32`.
pub(crate) fn unit_float(h: u32) -> f32 {
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

/// Hashes three integers and returns three repeatable values in `[0, 1)`.
pub(crate) fn pcg3d_unit(v: [u32; 3]) -> [f32; 3] {
    pcg3d(v).map(unit_float)
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

    #[test]
    fn unit_float_covers_the_unit_interval() {
        assert_eq!(unit_float(0), 0.0);
        assert_eq!(unit_float(u32::MAX), 16_777_215.0 / 16_777_216.0);
        assert!(unit_float(u32::MAX) < 1.0);
    }
}
