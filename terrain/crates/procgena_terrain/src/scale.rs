//! How much a world stretches its landscape against reference scale 1.

use serde::{Deserialize, Serialize};

/// How much a world stretches its landscape against reference scale 1.
///
/// `horizontal` multiplies landscape lengths (the wavelengths of hills,
/// mountains and the world elevation), and `vertical` landscape heights.
/// Slopes change by `vertical / horizontal`. Anything placed by hand, and
/// anything sized for objects and players (road and river widths, chunks,
/// level of detail), stays in real metres.
///
/// Every terrain of one world has the same scale. Today the terrain uses
/// it only for its [`Labels`](crate::Labels); terrain settings are still
/// written at the size they are meant to be drawn.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldScale {
    /// The stretch of landscape lengths. Finite and positive.
    pub horizontal: f64,
    /// The stretch of landscape heights. Finite and positive.
    pub vertical: f64,
}

impl Default for WorldScale {
    /// Scale 1: lengths and heights as written.
    fn default() -> Self {
        Self {
            horizontal: 1.0,
            vertical: 1.0,
        }
    }
}

impl WorldScale {
    /// Whether both stretches are finite and positive.
    pub fn is_valid(&self) -> bool {
        let positive = |v: f64| v.is_finite() && v > 0.0;
        positive(self.horizontal) && positive(self.vertical)
    }
}
