//! Mesh quality settings and levels of detail.
//!
//! A "level of detail" (LOD) is a simpler version of a mesh drawn when the
//! object is far from the camera, where fine detail can't be seen anyway.
//! [`MushroomDetail`] says how detailed one mesh is, and a
//! [`MushroomLod`] says which detail to draw at which camera distance.

use procgena_core::{FarForm, LevelForm, Lod, LodLevel};
use serde::{Deserialize, Serialize};

use crate::options::{MAX_SEGMENTS, OptionsError, int_range};

/// The cap diameter, in metres, that the distances in [`default_lod`] are
/// chosen for (8 cm): its [`Lod::reference_size`].
pub const DEFAULT_LOD_SIZE: f32 = 0.08;

/// A mushroom's levels of detail: which [`MushroomDetail`] to draw at which
/// camera distance, written for a cap of
/// [`reference_size`](Lod::reference_size) metres. Start from
/// [`default_lod`], and check your own with [`validate_lod`].
///
/// Use [`Lod::factor`] with a mushroom's cap diameter to scale the
/// distances to its size, so a mushroom twice as big keeps its detail twice
/// as far away.
pub type MushroomLod = Lod<MushroomDetail>;

/// One level of a [`MushroomLod`]: the detail to draw from its distance
/// onward, until the next level starts.
pub type MushroomLodLevel = LodLevel<MushroomDetail>;

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
    /// For distant or very numerous mushrooms: an open cap like
    /// [`Shell`](Self::Shell) and a flat strip for the stem. The renderer
    /// keeps the strip turned toward the camera, so it looks like a stem
    /// from every side, as distant trees do with their branches. See
    /// [`crate::BillboardMesh`].
    Flat,
    /// The cheapest form, for the farthest distances: one triangle for the
    /// stem and one for the cap, both turned toward the camera like the
    /// stem of [`Flat`](Self::Flat). Renderers draw it with vertex colours
    /// only, without textures, patterns, gills or warts. See
    /// [`crate::BillboardMesh`].
    Billboard,
}

/// How detailed one mushroom mesh is.
///
/// "Sides" count segments around a circle and must be from 3 through
/// [`MAX_SEGMENTS`]; "segments" count steps along a length and must be from
/// 1 through [`MAX_SEGMENTS`]. More of either gives a smoother mesh with more
/// triangles. Start from one of the named levels, such as
/// [`FULL`](Self::FULL).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MushroomDetail {
    /// How the surfaces are built.
    pub shape: BodyShape,
    /// Segments around the cap.
    pub cap_sides: u32,
    /// Segments along the cap's outline, from rim to top. The underside
    /// gets half as many, and at least one.
    pub cap_segments: u32,
    /// Segments around the stem. Not used by [`BodyShape::Flat`] or
    /// [`BodyShape::Billboard`].
    pub stem_sides: u32,
    /// Segments along the stem. Not used by [`BodyShape::Billboard`].
    pub stem_segments: u32,
    /// Whether to build the annulus, if the mushroom has one.
    pub annulus: bool,
    /// Whether to paint the mushroom's warts into the cap's vertex colours.
    ///
    /// Leave it on for renderers that don't draw warts themselves, such as
    /// 3D tools reading exported files. Turn it off when the renderer draws
    /// them, as `procgena_mushroom_bevy` does, or they show twice. Painted
    /// warts are only as sharp as the mesh, so coarse levels show soft
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
    /// Solid surfaces at about a quarter of [`FULL`](Self::FULL)'s triangles.
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
    /// An open six-sided cap and a stem strip that turns toward the camera.
    /// About 40 triangles; suited to far distances and large crowds.
    pub const FLAT: Self = Self {
        shape: BodyShape::Flat,
        cap_sides: 6,
        cap_segments: 2,
        stem_sides: 3,
        stem_segments: 3,
        annulus: false,
        paint_warts: true,
    };
    /// The cheapest form: two triangles, one for the stem and one for the
    /// cap, turned toward the camera and drawn with vertex colours only.
    /// For the farthest distances.
    pub const BILLBOARD: Self = Self {
        shape: BodyShape::Billboard,
        cap_sides: 3,
        cap_segments: 1,
        stem_sides: 3,
        stem_segments: 1,
        annulus: false,
        paint_warts: false,
    };

    /// Every named level with a display name, most detailed first, for
    /// menus.
    pub const PRESETS: [(&'static str, Self); 6] = [
        ("Full", Self::FULL),
        ("High", Self::HIGH),
        ("Medium", Self::MEDIUM),
        ("Low", Self::LOW),
        ("Flat", Self::FLAT),
        ("Billboard", Self::BILLBOARD),
    ];

    /// Checks every count against the limits described on
    /// [`MushroomDetail`].
    ///
    /// # Errors
    ///
    /// Returns the first count out of range.
    pub fn validate(&self) -> Result<(), OptionsError> {
        int_range("detail.cap_sides", self.cap_sides, 3, MAX_SEGMENTS)?;
        int_range("detail.cap_segments", self.cap_segments, 1, MAX_SEGMENTS)?;
        int_range("detail.stem_sides", self.stem_sides, 3, MAX_SEGMENTS)?;
        int_range("detail.stem_segments", self.stem_segments, 1, MAX_SEGMENTS)
    }
}

impl Default for MushroomDetail {
    fn default() -> Self {
        Self::FULL
    }
}

/// Returns the default levels for an 8 cm cap ([`DEFAULT_LOD_SIZE`]):
/// [`FULL`](MushroomDetail::FULL) up to 1.5 m,
/// [`HIGH`](MushroomDetail::HIGH) to 4 m,
/// [`MEDIUM`](MushroomDetail::MEDIUM) to 10 m,
/// [`FLAT`](MushroomDetail::FLAT) to 18 m and
/// [`BILLBOARD`](MushroomDetail::BILLBOARD), a camera-facing
/// [`FarForm::Card`] that casts no shadow, to 40 m, with nothing drawn
/// beyond. Each switch blends over 10% of its distance either side.
pub fn default_lod() -> MushroomLod {
    Lod::new(
        DEFAULT_LOD_SIZE,
        vec![
            LodLevel::new(0.0, 0.1, MushroomDetail::FULL),
            LodLevel::new(1.5, 0.1, MushroomDetail::HIGH),
            LodLevel::new(4.0, 0.1, MushroomDetail::MEDIUM),
            LodLevel::new(10.0, 0.1, MushroomDetail::FLAT),
            LodLevel::new(18.0, 0.1, MushroomDetail::BILLBOARD)
                .with_form(LevelForm::Far(FarForm::Card))
                .without_shadows(),
        ],
    )
    .with_cull(40.0)
}

/// Checks `lod`'s levels, their order and every level's detail.
///
/// # Errors
///
/// Returns [`OptionsError::Lod`] if the levels themselves are invalid (see
/// [`Lod::validate`]), or the first count out of range in a level's
/// [`MushroomDetail`].
pub fn validate_lod(lod: &MushroomLod) -> Result<(), OptionsError> {
    lod.validate().map_err(OptionsError::Lod)?;
    lod.levels
        .iter()
        .try_for_each(|level| level.detail.validate())
}
