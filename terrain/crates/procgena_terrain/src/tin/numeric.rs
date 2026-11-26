//! Canonical numbers: whole centimetres for positions and heights.

/// The numeric rules a canonical mesh was built under. Meshes built under
/// the same profile from the same ground are identical bit for bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NumericProfile {
    /// Positions and heights in whole centimetres, heights sampled with the
    /// platform's own maths library. Identical on one platform (operating
    /// system, CPU architecture and build); other platforms may differ by
    /// a centimetre here and there.
    SamePlatformV2,
}

impl NumericProfile {
    /// The profile meshes are built under today.
    pub const CURRENT: Self = Self::SamePlatformV2;

    /// A stable number for the profile, for hashing.
    pub fn code(self) -> u32 {
        match self {
            Self::SamePlatformV2 => 2,
        }
    }
}

/// Centimetres in a metre.
pub const CM_PER_M: f64 = 100.0;

/// The largest horizontal coordinate, in centimetres, either way from the
/// world's origin: 2^53, so every coordinate and every difference between
/// two coordinates in one chunk is exact as an `f64`.
pub const MAX_CM: i64 = 1 << 53;

/// A horizontal world coordinate (x or z) in whole centimetres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Cm(pub i64);

impl Cm {
    /// The nearest whole centimetre to `metres`, halves rounded away from
    /// zero. `None` if `metres` is not finite or is beyond [`MAX_CM`].
    pub fn from_m(metres: f64) -> Option<Self> {
        let cm = (metres * CM_PER_M).round();
        (cm.is_finite() && cm.abs() <= MAX_CM as f64).then_some(Self(cm as i64))
    }

    /// The coordinate in metres.
    pub fn to_m(self) -> f64 {
        self.0 as f64 / CM_PER_M
    }
}

/// A height in whole centimetres, up to about 21,474 km either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct CmHeight(pub i32);

impl CmHeight {
    /// The nearest whole centimetre to `metres`, halves rounded away from
    /// zero. `None` if `metres` is not finite or doesn't fit.
    pub fn from_m(metres: f64) -> Option<Self> {
        let cm = (metres * CM_PER_M).round();
        (cm.is_finite() && cm.abs() <= f64::from(i32::MAX)).then_some(Self(cm as i32))
    }

    /// The height in metres.
    pub fn to_m(self) -> f64 {
        f64::from(self.0) / CM_PER_M
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metres_round_to_the_nearest_centimetre_away_from_zero() {
        assert_eq!(Cm::from_m(1.2344), Some(Cm(123)));
        assert_eq!(Cm::from_m(0.005), Some(Cm(1)));
        assert_eq!(Cm::from_m(-0.005), Some(Cm(-1)));
        assert_eq!(Cm::from_m(-64.0), Some(Cm(-6_400)));
        assert_eq!(Cm::from_m(f64::NAN), None);
        assert_eq!(Cm::from_m(1e15), None);
        assert_eq!(CmHeight::from_m(-12.346), Some(CmHeight(-1_235)));
        assert_eq!(CmHeight::from_m(f64::INFINITY), None);
        assert_eq!(CmHeight::from_m(3e7), None);
        assert_eq!(Cm(-150).to_m(), -1.5);
    }
}
