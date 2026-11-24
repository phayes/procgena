//! The Trees tab: preset, seed, and every tree setting.

pub mod editor;
pub mod params;

use procgena_tree::{TreeOptions, TreePreset};

/// The key of the preset shown when the app starts.
pub const START_PRESET: &str = "oak_medium";

/// The settings of [`START_PRESET`], which the editor shows first.
pub fn start_options() -> TreeOptions {
    TreePreset::named(START_PRESET)
        .expect("the start preset is built in")
        .tree
        .clone()
}
/// How far Jiggle varies each shape setting: 0.1 is up to ±10%.
pub const JIGGLE_AMOUNT: f32 = 0.1;
