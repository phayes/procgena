//! Controls for choosing a rock preset and customising its shape and appearance.

pub mod editor;
pub mod params;

use procgena_stone::StonePreset;

/// The initial rock preset displayed by the editor.
pub const START_PRESET: StonePreset = StonePreset::Boulder;
