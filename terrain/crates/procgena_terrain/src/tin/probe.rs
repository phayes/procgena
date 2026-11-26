//! Sampling the ground at canonical points.

use std::fmt;

use glam::DVec2;

use crate::chunk::ChunkSource;
use crate::feature::FeatureTier;

use super::numeric::{Mm, MmHeight};

/// A point where the ground is sampled for a canonical mesh: a world
/// position in whole millimetres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProbeAddr {
    pub x: Mm,
    pub z: Mm,
}

impl ProbeAddr {
    /// The probe at `x`, `z` millimetres.
    pub fn new(x: Mm, z: Mm) -> Self {
        Self { x, z }
    }

    /// The probe's position in metres.
    pub fn to_m(self) -> DVec2 {
        DVec2::new(self.x.to_m(), self.z.to_m())
    }
}

/// Why canonical ground couldn't be sampled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TinError {
    /// The source can't give the ground at the
    /// [`Fine`](FeatureTier::Fine) tier, only at this one.
    NotFine(FeatureTier),
    /// There is no ground at this probe (for example, outside every
    /// region).
    NoGround(ProbeAddr),
    /// The ground at this probe is too high or too low for a millimetre
    /// height.
    OutOfRange(ProbeAddr),
}

impl fmt::Display for TinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFine(tier) => write!(f, "the ground is only available at the {tier:?} tier"),
            Self::NoGround(p) => write!(f, "no ground at ({} m, {} m)", p.x.to_m(), p.z.to_m()),
            Self::OutOfRange(p) => write!(
                f,
                "the ground at ({} m, {} m) is out of range",
                p.x.to_m(),
                p.z.to_m()
            ),
        }
    }
}

impl std::error::Error for TinError {}

/// The ground height at each of `probes`, from `source` at the
/// [`Fine`](FeatureTier::Fine) tier, rounded to the nearest millimetre.
/// The heights are the same in any order and on any number of threads.
///
/// Fails if `source` can't give the fine tier, or if a probe has no
/// ground or a height out of range.
pub fn sample_heights(
    source: &dyn ChunkSource,
    probes: &[ProbeAddr],
) -> Result<Vec<MmHeight>, TinError> {
    let tier = source.tier(FeatureTier::Fine);
    if tier != FeatureTier::Fine {
        return Err(TinError::NotFine(tier));
    }
    let points: Vec<DVec2> = probes.iter().map(|p| p.to_m()).collect();
    source
        .surfaces(&points, FeatureTier::Fine)
        .iter()
        .zip(probes)
        .map(|(s, &p)| {
            if s.height.is_nan() {
                return Err(TinError::NoGround(p));
            }
            MmHeight::from_m(f64::from(s.height)).ok_or(TinError::OutOfRange(p))
        })
        .collect()
}
