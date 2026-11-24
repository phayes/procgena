//! Mesh quality settings and levels of detail.
//!
//! A "level of detail" (LOD) is a simpler version of a mesh drawn when the
//! object is far from the camera, where fine detail can't be seen anyway.
//! [`DeadwoodDetail`] says how detailed one mesh is, and [`DeadwoodLod`]
//! says which detail to draw at which camera distance.

use procgena_core::{FarForm, LevelForm, Lod, Quality};
use serde::{Deserialize, Serialize};

use crate::options::{
    DeadwoodOptions, MAX_SEGMENTS, OptionsError, int_range, non_negative, within,
};

/// Size, in metres, that [`default_lod`]'s distances are written for: its
/// [`reference_size`](Lod::reference_size). Pieces are measured with
/// [`lod_size`].
pub const DEFAULT_LOD_SIZE: f32 = 0.45;

/// How detailed one mesh of a piece is.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// main stem stays round unless [`strip_all`](Self::strip_all) is set.
    /// Must be zero or more; zero draws everything round.
    pub ribbon_radius: f32,
    /// Draws every stem kept, the main stem included, as a flat strip, with
    /// no ends. For the farthest level, where a piece is only a few pixels
    /// across. Defaults to false in a file.
    #[serde(default)]
    pub strip_all: bool,
    /// Share of the piece's leaf sprays (see [`crate::LeafOptions`]) to
    /// draw, from 0 (none) through 1 (all). Lower shares keep a subset of
    /// the sprays a higher share keeps. Defaults to 1 in a file.
    #[serde(default = "all_leaves")]
    pub leaf_share: f32,
}

/// Default for [`DeadwoodDetail::leaf_share`] in a file.
fn all_leaves() -> f32 {
    1.0
}

impl DeadwoodDetail {
    /// Close-up detail: smooth stems, jagged breaks and every twig.
    pub const FULL: Self = Self {
        sides: 32,
        segments_per_diameter: 3.0,
        cap_rings: 8,
        min_radius: 0.0,
        ribbon_radius: 0.0,
        strip_all: false,
        leaf_share: 1.0,
    };
    /// Mid-distance detail: about a quarter of the triangles of
    /// [`FULL`](Self::FULL). Thin twigs become strips, and 60% of leaf
    /// sprays are kept.
    pub const HIGH: Self = Self {
        sides: 16,
        segments_per_diameter: 1.2,
        cap_rings: 3,
        min_radius: 0.002,
        ribbon_radius: 0.008,
        strip_all: false,
        leaf_share: 0.6,
    };
    /// Distant detail: an eight-sided stem, flat ends, only thick branches
    /// and a quarter of the leaf sprays.
    pub const LOW: Self = Self {
        sides: 8,
        segments_per_diameter: 0.4,
        cap_rings: 1,
        min_radius: 0.015,
        ribbon_radius: 0.03,
        strip_all: false,
        leaf_share: 0.25,
    };
    /// Farthest detail: every stem a flat strip turned toward the camera,
    /// with only the thickest branches kept and no leaves. A few triangles
    /// per stem.
    pub const STRIP: Self = Self {
        sides: 3,
        segments_per_diameter: 0.25,
        cap_rings: 1,
        min_radius: 0.03,
        ribbon_radius: 0.0,
        strip_all: true,
        leaf_share: 0.0,
    };

    /// The named detail levels, for menus.
    pub const PRESETS: [(&'static str, Self); 4] = [
        ("Full", Self::FULL),
        ("High", Self::HIGH),
        ("Low", Self::LOW),
        ("Strip", Self::STRIP),
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
        non_negative("detail.ribbon_radius", self.ribbon_radius)?;
        within("detail.leaf_share", self.leaf_share, 0.0, 1.0)
    }
}

impl Default for DeadwoodDetail {
    fn default() -> Self {
        Self::FULL
    }
}

/// Which detail to draw at which camera distance, and where to stop
/// drawing: the shared [`Lod`] type with a [`DeadwoodDetail`] per level.
///
/// Its distances are written for a piece [`reference_size`](Lod::reference_size)
/// metres across, as measured by [`lod_size`]. Pass it to
/// [`crate::Piece::lod_meshes`] to build one mesh per level, and scale its
/// distances for each piece with [`Lod::factor`] and [`Lod::ranges`].
pub type DeadwoodLod = Lod<DeadwoodDetail>;

/// One level of a [`DeadwoodLod`].
pub type LodLevel = procgena_core::LodLevel<DeadwoodDetail>;

/// The standard levels, for a piece [`DEFAULT_LOD_SIZE`] across: full detail
/// up close, then [`HIGH`](DeadwoodDetail::HIGH) from 8 m,
/// [`LOW`](DeadwoodDetail::LOW) from 25 m and camera-facing strips
/// ([`FarForm::Strips`], casting no shadows) from 45 m, with nothing drawn
/// past 90 m.
pub fn default_lod() -> DeadwoodLod {
    let level = |distance, detail| LodLevel::new(distance, 0.1, detail);
    Lod::new(
        DEFAULT_LOD_SIZE,
        vec![
            level(0.0, DeadwoodDetail::FULL),
            level(8.0, DeadwoodDetail::HIGH),
            level(25.0, DeadwoodDetail::LOW),
            level(45.0, DeadwoodDetail::STRIP)
                .with_form(LevelForm::Far(FarForm::Strips))
                .without_shadows(),
        ],
    )
    .with_cull(90.0)
}

/// Returns a piece's size for choosing LOD distances: its diameter, or 15%
/// of its length if that is larger, since long thin pieces stay visible
/// further away. Pass it to [`Lod::factor`].
pub fn lod_size(options: &DeadwoodOptions) -> f32 {
    options.diameter.max(0.15 * options.length)
}

/// Returns the factor by which `lod`'s distances grow for a piece made from
/// `options`, at [`Quality::High`]: its [`lod_size`] over `lod`'s
/// reference size. [`crate::Piece::lod_meshes`] scales stem radii by it;
/// multiply in [`Quality::distance_scale`] for another quality.
pub fn size_factor(lod: &DeadwoodLod, options: &DeadwoodOptions) -> f32 {
    lod.factor(lod_size(options), Quality::High)
}

/// Checks `lod` against the shared level-of-detail rules (see
/// [`Lod::validate`]) and checks each level's detail.
///
/// # Errors
///
/// Returns [`OptionsError::Lod`] if the levels break a shared rule, or the
/// first invalid setting of a level's detail.
pub fn validate_lod(lod: &DeadwoodLod) -> Result<(), OptionsError> {
    lod.validate().map_err(OptionsError::Lod)?;
    lod.levels
        .iter()
        .try_for_each(|level| level.detail.validate())
}

/// Returns `lod`'s levels scaled for a piece made from `options`, as drawn
/// at [`Quality::High`]: each distance, and each detail's
/// [`min_radius`](DeadwoodDetail::min_radius) and
/// [`ribbon_radius`](DeadwoodDetail::ribbon_radius), multiplied by the
/// piece's size factor. A piece twice as large then drops or flattens
/// stems twice as thick, so each level looks the same on screen whatever
/// the piece's size. The quality setting never changes these radii, so it
/// never changes the meshes.
pub(crate) fn sized_levels(lod: &DeadwoodLod, options: &DeadwoodOptions) -> Vec<LodLevel> {
    let factor = size_factor(lod, options);
    lod.levels
        .iter()
        .map(|level| {
            let mut level = *level;
            level.distance *= factor;
            level.detail.min_radius *= factor;
            level.detail.ribbon_radius *= factor;
            level
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_lod_is_valid_and_scales_with_size() {
        let lod = default_lod();
        validate_lod(&lod).unwrap();
        let o = DeadwoodOptions {
            diameter: DEFAULT_LOD_SIZE * 0.5,
            length: 1.0,
            ..DeadwoodOptions::default()
        };
        let half = sized_levels(&lod, &o);
        assert_eq!(half[1].distance, lod.levels[1].distance * 0.5);
        // Stem thresholds scale with the distances.
        let (low, half_low) = (lod.levels[2].detail, half[2].detail);
        assert_eq!(half_low.min_radius, low.min_radius * 0.5);
        assert_eq!(half_low.ribbon_radius, low.ribbon_radius * 0.5);
    }

    #[test]
    fn default_lod_ends_with_shadowless_strips() {
        let lod = default_lod();
        let last = lod.levels.last().unwrap();
        assert!(last.detail.strip_all);
        assert_eq!(last.form, LevelForm::Far(FarForm::Strips));
        assert!(!last.casts_shadows);
        assert!(
            lod.levels[..lod.levels.len() - 1]
                .iter()
                .all(|l| !l.detail.strip_all && l.form == LevelForm::Mesh && l.casts_shadows)
        );
    }

    #[test]
    fn presets_are_valid() {
        for (_, d) in DeadwoodDetail::PRESETS {
            d.validate().unwrap();
        }
    }
}
