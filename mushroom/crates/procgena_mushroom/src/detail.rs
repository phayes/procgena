//! How finely a mushroom is meshed: its game-level detail, and the coarser
//! detail a renderer draws when the mushroom is small on screen.

use procgena_core::chain::{Lod, LowerDetail};
use serde::{Deserialize, Serialize};

use crate::options::{MAX_SEGMENTS, OptionsError, int_range};

/// How the cap and stem surfaces are built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyShape {
    /// Closed ("watertight") surfaces with no holes: the cap has a top,
    /// rim and underside, and the stem is closed at the top and, with
    /// [`BulbOptions::closed`](crate::BulbOptions::closed), at the bottom.
    /// Back faces are never seen, so it can be drawn one-sided. Use it up
    /// close, or when another generator needs to tell inside from outside.
    #[default]
    Solid,
    /// Open surfaces: the cap's top, rim and underside without the disc that
    /// seals it against the stem, and the stem's side with its rounded
    /// bottom but no top. Draw it two-sided, because its inside can be seen
    /// through the openings.
    Shell,
}

/// How detailed one mushroom mesh is.
///
/// A preset's [`MushroomOptions::game_level`](crate::MushroomOptions::game_level)
/// is one of these: the mushroom's real shape, which the game uses for
/// picking, for growing other things on it, and for its impostor. Renderers
/// also draw a coarser mesh when the mushroom is small on screen;
/// [`lod`](Self::lod) gives both.
///
/// "Sides" count segments around a circle and must be from 3 through
/// [`MAX_SEGMENTS`]; "segments" count steps along a length and must be from
/// 1 through [`MAX_SEGMENTS`]. More of either gives a smoother mesh with more
/// triangles. Start from one of the named details, such as
/// [`HIGH`](Self::HIGH), the default.
///
/// ```
/// use procgena_core::chain::Step;
/// use procgena_mushroom::{BodyShape, MushroomDetail};
///
/// let lod = MushroomDetail::HIGH.lod();
/// assert_eq!(lod.game, MushroomDetail::HIGH);
/// // The coarser mesh is open, with half the sides and segments.
/// let lower = lod.detail(Step::Lower).unwrap();
/// assert_eq!(lower.shape, BodyShape::Shell);
/// assert_eq!(lower.cap_sides, 12);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MushroomDetail {
    /// How the surfaces are built.
    pub shape: BodyShape,
    /// Segments around the cap.
    pub cap_sides: u32,
    /// Segments along the cap's outline, from rim to top. The underside
    /// gets half as many, and at least one.
    pub cap_segments: u32,
    /// Segments around the stem.
    pub stem_sides: u32,
    /// Segments along the stem.
    pub stem_segments: u32,
    /// Whether to build the annulus, if the mushroom has one.
    pub annulus: bool,
    /// Whether to paint the mushroom's warts into the cap's vertex colours.
    ///
    /// Leave it on for renderers that don't draw warts themselves, such as
    /// 3D tools reading exported files. Turn it off when the renderer draws
    /// them, as `procgena_mushroom_bevy` does, or they show twice. Painted
    /// warts are only as sharp as the mesh, so coarse details show soft
    /// blotches.
    pub paint_warts: bool,
}

impl MushroomDetail {
    /// Close-up quality: smooth solid surfaces with every feature.
    pub const FULL: Self = Self {
        shape: BodyShape::Solid,
        cap_sides: 48,
        cap_segments: 16,
        stem_sides: 24,
        stem_segments: 12,
        annulus: true,
        paint_warts: true,
    };
    /// Solid surfaces at about a quarter of [`FULL`](Self::FULL)'s
    /// triangles. The default.
    pub const HIGH: Self = Self {
        shape: BodyShape::Solid,
        cap_sides: 24,
        cap_segments: 8,
        stem_sides: 12,
        stem_segments: 6,
        annulus: true,
        paint_warts: true,
    };
    /// Open surfaces.
    pub const MEDIUM: Self = Self {
        shape: BodyShape::Shell,
        cap_sides: 12,
        cap_segments: 4,
        stem_sides: 8,
        stem_segments: 4,
        annulus: true,
        paint_warts: true,
    };
    /// Coarse open surfaces without an annulus.
    pub const LOW: Self = Self {
        shape: BodyShape::Shell,
        cap_sides: 8,
        cap_segments: 3,
        stem_sides: 6,
        stem_segments: 3,
        annulus: false,
        paint_warts: true,
    };

    /// Every named detail with a display name, most detailed first, for
    /// menus.
    pub const PRESETS: [(&'static str, Self); 4] = [
        ("Full", Self::FULL),
        ("High", Self::HIGH),
        ("Medium", Self::MEDIUM),
        ("Low", Self::LOW),
    ];

    /// The display name of the named detail equal to this one, or `None`
    /// for a detail of your own.
    pub fn preset_name(&self) -> Option<&'static str> {
        Self::PRESETS
            .iter()
            .find(|(_, detail)| detail == self)
            .map(|(name, _)| *name)
    }

    /// This detail as the game level, and the coarser mesh after it (see
    /// [`LowerDetail`]).
    pub fn lod(&self) -> Lod<Self> {
        Lod::from_game(*self)
    }

    /// Checks every count against the limits described on
    /// [`MushroomDetail`].
    ///
    /// # Errors
    ///
    /// Returns the first count out of range.
    pub fn validate(&self) -> Result<(), OptionsError> {
        int_range("game_level.cap_sides", self.cap_sides, 3, MAX_SEGMENTS)?;
        int_range(
            "game_level.cap_segments",
            self.cap_segments,
            1,
            MAX_SEGMENTS,
        )?;
        int_range("game_level.stem_sides", self.stem_sides, 3, MAX_SEGMENTS)?;
        int_range(
            "game_level.stem_segments",
            self.stem_segments,
            1,
            MAX_SEGMENTS,
        )
    }
}

/// Open surfaces ([`BodyShape::Shell`]) with half of each count (at least 3
/// sides and 1 segment) and no annulus. Warts are painted as in the game
/// level.
impl LowerDetail for MushroomDetail {
    fn lower(&self) -> Self {
        Self {
            shape: BodyShape::Shell,
            cap_sides: (self.cap_sides / 2).max(3),
            cap_segments: (self.cap_segments / 2).max(1),
            stem_sides: (self.stem_sides / 2).max(3),
            stem_segments: (self.stem_segments / 2).max(1),
            annulus: false,
            paint_warts: self.paint_warts,
        }
    }
}

impl Default for MushroomDetail {
    fn default() -> Self {
        Self::HIGH
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lower_mesh_is_open_with_half_the_counts() {
        let lower = MushroomDetail::FULL.lod().lower;
        assert_eq!(
            lower,
            MushroomDetail {
                shape: BodyShape::Shell,
                cap_sides: 24,
                cap_segments: 8,
                stem_sides: 12,
                stem_segments: 6,
                annulus: false,
                paint_warts: true,
            }
        );
    }

    #[test]
    fn the_lower_mesh_keeps_the_smallest_valid_counts() {
        let smallest = MushroomDetail {
            shape: BodyShape::Shell,
            cap_sides: 3,
            cap_segments: 1,
            stem_sides: 3,
            stem_segments: 1,
            annulus: false,
            paint_warts: false,
        };
        assert_eq!(smallest.lod().lower, smallest);
        for (_, detail) in MushroomDetail::PRESETS {
            detail.lod().lower.validate().unwrap();
        }
    }

    #[test]
    fn named_details_are_named_and_others_are_not() {
        assert_eq!(MushroomDetail::HIGH.preset_name(), Some("High"));
        assert_eq!(MushroomDetail::HIGH.lod().lower.preset_name(), None);
    }
}
