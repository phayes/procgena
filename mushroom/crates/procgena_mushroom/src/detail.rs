//! Mesh quality settings and levels of detail.
//!
//! A "level of detail" (LOD) is a simpler version of a mesh drawn when the
//! object is far from the camera, where fine detail can't be seen anyway.
//! [`MushroomDetail`] says how detailed one mesh is, and [`MushroomLod`]
//! says which detail to draw at which camera distance.

use serde::{Deserialize, Serialize};

use crate::options::{MAX_SEGMENTS, OptionsError, int_range, non_negative, positive, within};

/// The cap diameter, in metres, that the distances in
/// [`MushroomLod::default`] are chosen for (8 cm). Use
/// [`MushroomLod::for_size`] for other sizes.
pub const DEFAULT_LOD_SIZE: f32 = 0.08;

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

/// One step of a [`MushroomLod`]: the detail to draw from `distance`
/// onward, until the next level starts.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LodLevel {
    /// Camera distance in metres where this level starts. The first level
    /// must start at 0.
    pub distance: f32,
    /// Width of the blend with the previous level, as a fraction of
    /// `distance`, from 0 through 1. Around the switch, renderers fade one
    /// level out as the other fades in, so the change doesn't pop. For
    /// example, 0.1 at 10 m blends from 9.5 m to 10.5 m.
    pub hysteresis: f32,
    /// Mesh quality for this level.
    pub detail: MushroomDetail,
}

/// Which detail to draw at which camera distance, and where to stop
/// drawing the mushroom altogether.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MushroomLod {
    /// Levels from nearest to farthest. The first starts at 0, and each
    /// starts no nearer than the one before.
    pub levels: Vec<LodLevel>,
    /// Distance in metres beyond which the mushroom is not drawn at all.
    /// Must be at least the last level's distance.
    pub cull_distance: f32,
}

impl Default for MushroomLod {
    /// Five levels for an 8 cm cap: [`FULL`](MushroomDetail::FULL) up to
    /// 1.5 m, [`HIGH`](MushroomDetail::HIGH) to 4 m,
    /// [`MEDIUM`](MushroomDetail::MEDIUM) to 10 m,
    /// [`FLAT`](MushroomDetail::FLAT) to 18 m and
    /// [`BILLBOARD`](MushroomDetail::BILLBOARD) to 40 m, with nothing drawn
    /// beyond.
    fn default() -> Self {
        let level = |distance, detail| LodLevel {
            distance,
            hysteresis: 0.1,
            detail,
        };
        Self {
            levels: vec![
                level(0.0, MushroomDetail::FULL),
                level(1.5, MushroomDetail::HIGH),
                level(4.0, MushroomDetail::MEDIUM),
                level(10.0, MushroomDetail::FLAT),
                level(18.0, MushroomDetail::BILLBOARD),
            ],
            cull_distance: 40.0,
        }
    }
}

impl MushroomLod {
    /// Returns the default levels with their distances scaled for a cap of
    /// diameter `size` metres, so a mushroom twice as big keeps its detail
    /// twice as far away.
    pub fn for_size(size: f32) -> Self {
        Self::default().scaled(size / DEFAULT_LOD_SIZE)
    }

    /// Returns these levels with every distance, including the cull
    /// distance, multiplied by `factor`.
    pub fn scaled(mut self, factor: f32) -> Self {
        for level in &mut self.levels {
            level.distance *= factor;
        }
        self.cull_distance *= factor;
        self
    }

    /// Checks the levels and their order.
    ///
    /// # Errors
    ///
    /// Returns an error if there are no levels, the first does not start at
    /// 0, a level starts nearer than the one before, a hysteresis is out of
    /// range, a level's detail is invalid, or the cull distance is not
    /// positive or is nearer than the last level.
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
            non_negative("lod.levels.distance", level.distance)?;
            within("lod.levels.distance", level.distance, previous, f32::MAX)?;
            within("lod.levels.hysteresis", level.hysteresis, 0.0, 1.0)?;
            level.detail.validate()?;
            previous = level.distance;
        }
        positive("lod.cull_distance", self.cull_distance)?;
        within("lod.cull_distance", self.cull_distance, previous, f32::MAX)
    }
}
