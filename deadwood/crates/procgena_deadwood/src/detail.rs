//! Mesh quality settings and levels of detail.
//!
//! A "level of detail" (LOD) is a simpler version of a mesh drawn when the
//! object is far from the camera, where fine detail can't be seen anyway.
//! [`DeadwoodDetail`] says how detailed one mesh is, and [`DeadwoodLod`]
//! says which detail to draw at which camera distance.

use serde::{Deserialize, Serialize};

use crate::options::{
    DeadwoodOptions, MAX_SEGMENTS, OptionsError, int_range, non_negative, positive, within,
};

/// Size, in metres, that [`DeadwoodLod::default`]'s distances are written
/// for. [`DeadwoodLod::for_options`] scales them by a piece's own size.
pub const DEFAULT_LOD_SIZE: f32 = 0.45;

/// How detailed one mesh of a piece is.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DeadwoodDetail {
    /// Number of sides around the main stem, from 3 up to [`MAX_SEGMENTS`].
    /// Thinner branches, stubs and roots get fewer, but at least 3.
    pub sides: u32,
    /// How many rings of vertices to place along a stem for each diameter of
    /// its length, from 0.1 through 16. Every stem gets at least one
    /// segment, however short.
    pub segments_per_diameter: f32,
    /// Number of rings of vertices across a cut or broken end, from 1 (a
    /// flat fan of triangles) up to 32. More rings show splinters and jagged
    /// breaks better.
    pub cap_rings: u32,
    /// Branches, stubs, twigs and roots thinner than this radius, in metres,
    /// are left out. The main stem is always kept. Must be zero or more;
    /// zero keeps everything.
    pub min_radius: f32,
    /// Branches, stubs, twigs and roots thinner than this radius, in metres,
    /// are drawn as flat strips that a renderer turns to face the camera (see
    /// [`crate::Ribbons`]) instead of as round tubes. From a distance a
    /// strip looks like a thin round twig but costs far fewer triangles. The
    /// main stem is always round. Must be zero or more; zero draws
    /// everything round.
    pub ribbon_radius: f32,
}

impl DeadwoodDetail {
    /// Close-up detail: smooth stems, jagged breaks and every twig.
    pub const FULL: Self = Self {
        sides: 32,
        segments_per_diameter: 3.0,
        cap_rings: 8,
        min_radius: 0.0,
        ribbon_radius: 0.0,
    };
    /// Mid-distance detail: about a quarter of the triangles of
    /// [`FULL`](Self::FULL). Thin twigs become strips.
    pub const HIGH: Self = Self {
        sides: 16,
        segments_per_diameter: 1.2,
        cap_rings: 3,
        min_radius: 0.002,
        ribbon_radius: 0.008,
    };
    /// Distant detail: an eight-sided stem, flat ends, only thick branches.
    pub const LOW: Self = Self {
        sides: 8,
        segments_per_diameter: 0.4,
        cap_rings: 1,
        min_radius: 0.015,
        ribbon_radius: 0.03,
    };

    /// The named detail levels, for menus.
    pub const PRESETS: [(&'static str, Self); 3] = [
        ("Full", Self::FULL),
        ("High", Self::HIGH),
        ("Low", Self::LOW),
    ];

    /// Checks every setting and returns the first one out of range.
    ///
    /// # Errors
    ///
    /// Returns an [`OptionsError`] naming the first invalid setting.
    pub fn validate(&self) -> Result<(), OptionsError> {
        int_range("detail.sides", self.sides, 3, MAX_SEGMENTS)?;
        within(
            "detail.segments_per_diameter",
            self.segments_per_diameter,
            0.1,
            16.0,
        )?;
        int_range("detail.cap_rings", self.cap_rings, 1, 32)?;
        non_negative("detail.min_radius", self.min_radius)?;
        non_negative("detail.ribbon_radius", self.ribbon_radius)
    }
}

impl Default for DeadwoodDetail {
    fn default() -> Self {
        Self::FULL
    }
}

/// One level of detail: the detail to draw from a camera distance onward.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LodLevel {
    /// Camera distance in metres where this level starts. The first level
    /// starts at 0.
    pub distance: f32,
    /// Fraction of `distance`, from 0 through 1, by which a renderer should
    /// overlap this level with the one before, so that a camera hovering at
    /// the boundary doesn't make the mesh flicker between them.
    pub hysteresis: f32,
    /// The mesh detail at this level.
    pub detail: DeadwoodDetail,
}

/// Which detail to draw at which camera distance, and where to stop drawing.
///
/// Pass it to [`crate::Piece::lod_meshes`] to build one mesh per level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeadwoodLod {
    /// Levels in order of increasing distance; the first starts at 0.
    pub levels: Vec<LodLevel>,
    /// Camera distance in metres beyond which the piece is not drawn.
    pub cull_distance: f32,
}

impl Default for DeadwoodLod {
    /// Levels for a log about [`DEFAULT_LOD_SIZE`] across.
    fn default() -> Self {
        let level = |distance, detail| LodLevel {
            distance,
            hysteresis: 0.1,
            detail,
        };
        Self {
            levels: vec![
                level(0.0, DeadwoodDetail::FULL),
                level(8.0, DeadwoodDetail::HIGH),
                level(25.0, DeadwoodDetail::LOW),
            ],
            cull_distance: 80.0,
        }
    }
}

impl DeadwoodLod {
    /// Returns the default levels with distances scaled to suit a piece
    /// made from `options`: further for large logs, nearer for sticks.
    ///
    /// The piece's size is its diameter, or 15% of its length if that is
    /// larger, since long thin pieces stay visible further away.
    pub fn for_options(options: &DeadwoodOptions) -> Self {
        Self::default().scaled(lod_size(options) / DEFAULT_LOD_SIZE)
    }

    /// Returns these levels with every distance multiplied by `factor`.
    pub fn scaled(mut self, factor: f32) -> Self {
        for level in &mut self.levels {
            level.distance *= factor;
        }
        self.cull_distance *= factor;
        self
    }

    /// Checks the levels and returns the first problem found.
    ///
    /// # Errors
    ///
    /// Returns an [`OptionsError`] if there are no levels, the first level
    /// doesn't start at 0, a level's distance is less than the one before,
    /// a hysteresis is outside 0 through 1, or a level's detail is invalid.
    /// Also returns one if the cull distance is not positive, or is less
    /// than the last level's distance.
    pub fn validate(&self) -> Result<(), OptionsError> {
        let Some(first) = self.levels.first() else {
            return Err(OptionsError::IntOutOfRange {
                field: "lod.levels",
                value: 0,
                min: 1,
                max: u32::MAX,
            });
        };
        within("lod.levels[0].distance", first.distance, 0.0, 0.0)?;
        let mut previous = 0.0;
        for level in &self.levels {
            within("lod.levels.distance", level.distance, previous, f32::MAX)?;
            within("lod.levels.hysteresis", level.hysteresis, 0.0, 1.0)?;
            level.detail.validate()?;
            previous = level.distance;
        }
        positive("lod.cull_distance", self.cull_distance)?;
        within("lod.cull_distance", self.cull_distance, previous, f32::MAX)
    }
}

/// Returns a piece's size for choosing LOD distances: its diameter, or 15%
/// of its length if that is larger, since long thin pieces stay visible
/// further away.
fn lod_size(options: &DeadwoodOptions) -> f32 {
    options.diameter.max(0.15 * options.length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_lod_is_valid_and_scales() {
        let lod = DeadwoodLod::default();
        lod.validate().unwrap();
        let half = lod.clone().scaled(0.5);
        assert_eq!(half.levels[1].distance, lod.levels[1].distance * 0.5);
        half.validate().unwrap();
    }

    #[test]
    fn presets_are_valid() {
        for (_, d) in DeadwoodDetail::PRESETS {
            d.validate().unwrap();
        }
    }
}
