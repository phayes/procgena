//! Render policy for the public `PlantShadows` setting.

impl crate::PlantShadows {
    /// Directional cascades grass is drawn into, counted from the nearest.
    pub(crate) fn cascades(self) -> usize {
        match self {
            crate::PlantShadows::Off => 0,
            crate::PlantShadows::Near => 1,
            crate::PlantShadows::Full => 2,
        }
    }

    /// Buckets drawn into shadow maps, counted from LOD 0. Orthographic views
    /// put every blade in bucket 0, so they always cast all of them.
    pub(crate) fn buckets(self) -> u64 {
        match self {
            crate::PlantShadows::Off | crate::PlantShadows::Near => 1,
            crate::PlantShadows::Full => 2,
        }
    }
}
