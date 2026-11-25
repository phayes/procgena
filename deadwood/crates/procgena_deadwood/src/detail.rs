//! How finely a piece is meshed: its game-level detail, and the coarser
//! mesh a renderer draws when the piece is small on screen.
//!
//! [`DeadwoodDetail`] says how detailed one mesh is. A piece's
//! [`game_level`](crate::DeadwoodOptions::game_level) is its real shape;
//! [`DeadwoodDetail::lod`] adds the coarser mesh derived from it, and a
//! renderer draws an impostor (a baked picture) after that.

use procgena_core::chain::{Lod, LowerDetail};
use serde::{Deserialize, Serialize};

use crate::options::{
    DeadwoodOptions, MAX_SEGMENTS, OptionsError, int_range, non_negative, within,
};

/// Size, in metres, that a [`DeadwoodDetail`]'s stem radii are written
/// for, as measured by [`detail_size`]. A piece of another size scales
/// them (see [`DeadwoodDetail::sized_for`]).
pub const DETAIL_REFERENCE_SIZE: f32 = 0.45;

/// How detailed one mesh of a piece is.
///
/// Pick one of the named details ([`FULL`](Self::FULL),
/// [`HIGH`](Self::HIGH), [`LOW`](Self::LOW)) or set your own. The default
/// is [`HIGH`](Self::HIGH).
///
/// ```
/// use procgena_core::chain::Step;
/// use procgena_deadwood::DeadwoodDetail;
///
/// let lod = DeadwoodDetail::HIGH.lod();
/// assert_eq!(lod.game, DeadwoodDetail::HIGH);
/// // The coarser mesh has half the sides.
/// assert_eq!(lod.detail(Step::Lower).unwrap().sides, 8);
/// ```
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
    /// main stem always stays round. Must be zero or more; zero draws
    /// everything round.
    pub ribbon_radius: f32,
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
        leaf_share: 1.0,
    };
    /// The default: about a quarter of the triangles of
    /// [`FULL`](Self::FULL). Thin twigs become strips, and 60% of leaf
    /// sprays are kept.
    pub const HIGH: Self = Self {
        sides: 16,
        segments_per_diameter: 1.2,
        cap_rings: 3,
        min_radius: 0.002,
        ribbon_radius: 0.008,
        leaf_share: 0.6,
    };
    /// Coarse detail: an eight-sided stem, flat ends, only thick branches
    /// and a quarter of the leaf sprays.
    pub const LOW: Self = Self {
        sides: 8,
        segments_per_diameter: 0.4,
        cap_rings: 1,
        min_radius: 0.015,
        ribbon_radius: 0.03,
        leaf_share: 0.25,
    };

    /// The named details, most detailed first, for menus.
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
        int_range("game_level.sides", self.sides, 3, MAX_SEGMENTS)?;
        within(
            "game_level.segments_per_diameter",
            self.segments_per_diameter,
            0.1,
            16.0,
        )?;
        int_range("game_level.cap_rings", self.cap_rings, 1, 32)?;
        non_negative("game_level.min_radius", self.min_radius)?;
        non_negative("game_level.ribbon_radius", self.ribbon_radius)?;
        within("game_level.leaf_share", self.leaf_share, 0.0, 1.0)
    }

    /// This detail as the game level, and the coarser mesh after it (see
    /// [`LowerDetail`]).
    pub fn lod(&self) -> DeadwoodLod {
        Lod::from_game(*self)
    }

    /// This detail as used for a piece made from `options`: its
    /// [`min_radius`](Self::min_radius) and
    /// [`ribbon_radius`](Self::ribbon_radius) multiplied by the piece's
    /// [`detail_size`] over [`DETAIL_REFERENCE_SIZE`]. A piece twice as
    /// large then drops or flattens stems twice as thick, so a small stick
    /// keeps its twigs round as a large log keeps its branches round.
    pub fn sized_for(&self, options: &DeadwoodOptions) -> Self {
        let factor = detail_size(options) / DETAIL_REFERENCE_SIZE;
        Self {
            min_radius: self.min_radius * factor,
            ribbon_radius: self.ribbon_radius * factor,
            ..*self
        }
    }
}

/// Half the sides (at least 4), a third of the rings along each stem (at
/// least 0.1 per diameter), flat ends, four times the radii below which
/// stems are left out or flattened (at least those of
/// [`HIGH`](DeadwoodDetail::HIGH) times four), and half the leaf sprays.
impl LowerDetail for DeadwoodDetail {
    fn lower(&self) -> Self {
        let high = Self::HIGH;
        Self {
            sides: (self.sides / 2).max(4).min(self.sides),
            segments_per_diameter: (self.segments_per_diameter / 3.0)
                .max(0.1)
                .min(self.segments_per_diameter),
            cap_rings: 1,
            min_radius: (self.min_radius * 4.0).max(high.min_radius * 4.0),
            ribbon_radius: (self.ribbon_radius * 4.0).max(high.ribbon_radius * 4.0),
            leaf_share: self.leaf_share * 0.5,
        }
    }
}

impl Default for DeadwoodDetail {
    fn default() -> Self {
        Self::HIGH
    }
}

/// A piece's two mesh details: its game level and the coarser mesh after
/// it, from [`DeadwoodDetail::lod`]. Build both meshes with
/// [`crate::Piece::lod_meshes`].
pub type DeadwoodLod = Lod<DeadwoodDetail>;

/// Returns a piece's size for scaling its detail's stem radii: its
/// diameter, or 15% of its length if that is larger, since long thin
/// pieces show their branches like thicker ones do.
pub fn detail_size(options: &DeadwoodOptions) -> f32 {
    options.diameter.max(0.15 * options.length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_high() {
        assert_eq!(DeadwoodDetail::default(), DeadwoodDetail::HIGH);
        assert_eq!(DeadwoodOptions::default().game_level, DeadwoodDetail::HIGH);
    }

    #[test]
    fn the_lower_mesh_is_coarser_and_valid() {
        for (name, detail) in DeadwoodDetail::PRESETS {
            let lower = detail.lod().lower;
            lower.validate().unwrap();
            assert!(lower.sides <= detail.sides, "{name}");
            assert!(
                lower.segments_per_diameter <= detail.segments_per_diameter,
                "{name}"
            );
            assert!(lower.min_radius >= detail.min_radius, "{name}");
            assert!(lower.ribbon_radius >= detail.ribbon_radius, "{name}");
            assert!(lower.leaf_share <= detail.leaf_share, "{name}");
        }
        // The default's lower mesh is close to the old far level.
        let lower = DeadwoodDetail::HIGH.lod().lower;
        assert_eq!(lower.sides, DeadwoodDetail::LOW.sides);
        assert_eq!(lower.cap_rings, 1);
        // Even full detail drops the thinnest twigs from its lower mesh.
        assert!(DeadwoodDetail::FULL.lod().lower.min_radius > 0.0);
    }

    #[test]
    fn radii_scale_with_the_piece() {
        let detail = DeadwoodDetail::HIGH;
        let half = DeadwoodOptions {
            diameter: DETAIL_REFERENCE_SIZE * 0.5,
            length: 1.0,
            ..DeadwoodOptions::default()
        };
        let sized = detail.sized_for(&half);
        assert_eq!(sized.min_radius, detail.min_radius * 0.5);
        assert_eq!(sized.ribbon_radius, detail.ribbon_radius * 0.5);
        assert_eq!(sized.sides, detail.sides);
    }

    #[test]
    fn presets_are_valid() {
        for (_, d) in DeadwoodDetail::PRESETS {
            d.validate().unwrap();
        }
    }
}
