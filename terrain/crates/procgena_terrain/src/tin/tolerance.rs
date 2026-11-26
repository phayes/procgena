//! How closely a canonical mesh follows the ground.

/// How closely a canonical mesh follows the ground, and how many vertices
/// it may spend doing so.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TinTolerance {
    /// The most the mesh may differ from the ground in height at a probe,
    /// in metres. Positive.
    pub height: f64,
    /// The longest a triangle edge may be, in metres. Positive.
    pub max_edge: f64,
    /// The most vertices one chunk may have, border included. Where it runs
    /// out, the chunk records the places it misses `height`. The border
    /// is always kept, even past the budget.
    pub vertex_budget: usize,
}

impl TinTolerance {
    /// Open land: half a metre in height, edges up to 32 m, at most 4,096
    /// vertices a chunk.
    pub const GENERAL: Self = Self {
        height: 0.5,
        max_edge: 32.0,
        vertex_budget: 4096,
    };

    /// Whether every value is in range.
    pub fn is_valid(&self) -> bool {
        self.height.is_finite()
            && self.height > 0.0
            && self.max_edge.is_finite()
            && self.max_edge > 0.0
            && self.vertex_budget >= 4
    }
}

impl Default for TinTolerance {
    fn default() -> Self {
        Self::GENERAL
    }
}
