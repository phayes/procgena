//! The Trees tab: preset, seed, and every tree setting.

pub mod editor;
pub mod params;

use procgena_tree::TreePreset;

/// The preset shown when the app starts.
pub const START_PRESET: TreePreset = TreePreset::OakMedium;
/// How far Jiggle varies each shape setting: 0.1 is up to ±10%.
pub const JIGGLE_AMOUNT: f32 = 0.1;
