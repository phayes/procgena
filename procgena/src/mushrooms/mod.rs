//! Controls for choosing a mushroom preset and customising its shape and
//! colours.

pub mod editor;
pub mod params;

use procgena_mushroom::MushroomPreset;

/// The mushroom preset the editor shows first.
pub const START_PRESET: MushroomPreset = MushroomPreset::FlyAgaric;
