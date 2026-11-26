//! Canonical numbers: whole millimetres for positions and heights.

/// The numeric rules a canonical mesh was built under. Meshes built under
/// the same profile from the same ground are identical bit for bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NumericProfile {
    /// Positions and heights in whole millimetres, heights sampled with the
    /// platform's own maths library. Identical on one platform (operating
    /// system, CPU architecture and build); other platforms may differ by
    /// a millimetre here and there.
    SamePlatformV1,
}

impl NumericProfile {
    /// The profile meshes are built under today.
    pub const CURRENT: Self = Self::SamePlatformV1;

    /// A stable number for the profile, for hashing.
    pub fn code(self) -> u32 {
        match self {
            Self::SamePlatformV1 => 1,
        }
    }
}

/// Millimetres in a metre.
pub const MM_PER_M: f64 = 1000.0;

/// The largest horizontal coordinate, in millimetres, either way from the
/// world's origin: 2^53, so every coordinate and every difference between
/// two coordinates in one chunk is exact as an `f64`.
pub const MAX_MM: i64 = 1 << 53;

/// A horizontal world coordinate (x or z) in whole millimetres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Mm(pub i64);

impl Mm {
    /// The nearest whole millimetre to `metres`, halves rounded away from
    /// zero. `None` if `metres` is not finite or is beyond [`MAX_MM`].
    pub fn from_m(metres: f64) -> Option<Self> {
        let mm = (metres * MM_PER_M).round();
        (mm.is_finite() && mm.abs() <= MAX_MM as f64).then_some(Self(mm as i64))
    }

    /// The coordinate in metres.
    pub fn to_m(self) -> f64 {
        self.0 as f64 / MM_PER_M
    }
}

/// A height in whole millimetres, up to about 2,147 km either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct MmHeight(pub i32);

impl MmHeight {
    /// The nearest whole millimetre to `metres`, halves rounded away from
    /// zero. `None` if `metres` is not finite or doesn't fit.
    pub fn from_m(metres: f64) -> Option<Self> {
        let mm = (metres * MM_PER_M).round();
        (mm.is_finite() && mm.abs() <= f64::from(i32::MAX)).then_some(Self(mm as i32))
    }

    /// The height in metres.
    pub fn to_m(self) -> f64 {
        f64::from(self.0) / MM_PER_M
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metres_round_to_the_nearest_millimetre_away_from_zero() {
        assert_eq!(Mm::from_m(1.2344), Some(Mm(1234)));
        assert_eq!(Mm::from_m(0.0005), Some(Mm(1)));
        assert_eq!(Mm::from_m(-0.0005), Some(Mm(-1)));
        assert_eq!(Mm::from_m(-64.0), Some(Mm(-64_000)));
        assert_eq!(Mm::from_m(f64::NAN), None);
        assert_eq!(Mm::from_m(1e14), None);
        assert_eq!(MmHeight::from_m(-12.3456), Some(MmHeight(-12_346)));
        assert_eq!(MmHeight::from_m(f64::INFINITY), None);
        assert_eq!(MmHeight::from_m(3e6), None);
        assert_eq!(Mm(-1500).to_m(), -1.5);
    }
}
