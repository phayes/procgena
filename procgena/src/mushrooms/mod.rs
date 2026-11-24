//! Controls for choosing a mushroom preset and customising its shape and
//! colours.

pub mod editor;
pub mod params;

use procgena_mushroom::{MushroomOptions, MushroomPreset};

/// The key of the mushroom preset the editor shows first.
pub const START_PRESET: &str = "fly_agaric";

/// The settings of [`START_PRESET`], which the editor shows first.
pub fn start_options() -> MushroomOptions {
    MushroomPreset::named(START_PRESET)
        .expect("the start preset is built in")
        .mushroom
        .clone()
}
