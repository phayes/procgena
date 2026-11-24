//! Controls for choosing a rock preset and customising its shape and appearance.

pub mod editor;
pub mod params;

use procgena_stone::{StoneOptions, StonePreset};

/// The key of the initial rock preset displayed by the editor.
pub const START_PRESET: &str = "boulder";

/// The settings of [`START_PRESET`], which the editor shows first.
pub fn start_options() -> StoneOptions {
    StonePreset::named(START_PRESET)
        .expect("the start preset is built in")
        .stone
        .clone()
}
