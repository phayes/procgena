//! Fade bands in metres: where one step of a copy is drawn, and how it
//! cross-fades with its neighbours.
//!
//! A [`DistanceChain`](crate::chain::DistanceChain) works out each step's
//! [`LodRange`] for a copy's size and the camera, and
//! [`LodRange::dither_level`] gives the fade amount at any distance, as the
//! shared dither shader in `procgena_core_bevy` uses it.

use std::ops::Range;

use glam::Vec4;

/// The named, cheap form drawn in place of far specimens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FarForm {
    /// No geometry: the specimens are drawn as a colour in the ground's
    /// material, as grass far away is.
    GroundMaterial,
}

/// Where one level of detail is drawn, in metres from the viewer: it fades
/// in over `fade_in`, is fully drawn between the two bands, and fades out
/// over `fade_out`.
///
/// The first level's `fade_in` is `0.0..0.0`. A level that never fades out
/// has a `fade_out` of `f32::INFINITY..f32::INFINITY`.
#[derive(Debug, Clone, PartialEq)]
pub struct LodRange {
    /// Distances over which the level fades in.
    pub fade_in: Range<f32>,
    /// Distances over which the level fades out.
    pub fade_out: Range<f32>,
}

impl LodRange {
    /// A range drawn fully at every distance.
    pub const ALWAYS: Self = Self {
        fade_in: 0.0..0.0,
        fade_out: f32::INFINITY..f32::INFINITY,
    };

    /// How faded the level is `distance` metres away, as a dither level
    /// from -16 to 16: 0 is fully drawn, -16 to 0 fading in, 0 to 16 fading
    /// out. -16 before the level starts and 16 after it ends mean it isn't
    /// drawn at all.
    ///
    /// The same numbers as Bevy's visibility-range dithering and the
    /// `dither_level` function of `procgena_core_bevy`'s dither shader, so a
    /// level drawn by Bevy and one drawn by a custom renderer cross-fade
    /// pixel for pixel.
    pub fn dither_level(&self, distance: f32) -> i32 {
        if distance < self.fade_in.start {
            return -16;
        }
        if distance >= self.fade_out.end {
            return 16;
        }
        if distance < self.fade_in.end {
            return -16 + band_level(&self.fade_in, distance);
        }
        if distance < self.fade_out.start {
            return 0;
        }
        band_level(&self.fade_out, distance)
    }

    /// Whether any part of the level is drawn `distance` metres away.
    pub fn is_drawn(&self, distance: f32) -> bool {
        (-15..=15).contains(&self.dither_level(distance))
    }

    /// Whether the level is fully drawn at every distance, so a renderer
    /// can skip fading it.
    pub fn is_always(&self) -> bool {
        self.fade_in.end <= 0.0 && self.fade_out.start == f32::INFINITY
    }

    /// The range packed for a GPU buffer as `(fade_in.start, fade_in.end,
    /// fade_out.start, fade_out.end)`, the layout the dither shader reads.
    /// Infinite distances become `f32::MAX`.
    pub fn to_vec4(&self) -> Vec4 {
        Vec4::new(
            self.fade_in.start,
            self.fade_in.end,
            self.fade_out.start,
            self.fade_out.end,
        )
        .min(Vec4::splat(f32::MAX))
    }
}

/// How far `distance` is through `band`, from 0 to 16, rounded as WGSL's
/// `round` does. A band of zero width counts as passed.
fn band_level(band: &Range<f32>, distance: f32) -> i32 {
    let width = band.end - band.start;
    if width <= 0.0 {
        return 16;
    }
    (((distance - band.start) / width * 16.0).round_ties_even() as i32).clamp(0, 16)
}
