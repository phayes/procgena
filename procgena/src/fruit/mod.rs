//! The Fruit and Flower tabs: each picks a fruit or flower preset, edits
//! its settings, saves and loads `.fruit.yml` files, and exports the
//! cluster as a model.

pub mod editor;
pub mod params;

use procgena_fruit::{FruitOptions, FruitPreset};

/// The key of the preset the Fruit tab shows first.
pub const START_FRUIT: &str = "apple";
/// The key of the preset the Flower tab shows first.
pub const START_FLOWER: &str = "dogwood_bloom";

/// The settings the Fruit tab (`fruit` true) or the Flower tab shows first.
pub fn start_options(fruit: bool) -> FruitOptions {
    let key = if fruit { START_FRUIT } else { START_FLOWER };
    FruitPreset::named(key)
        .expect("the start preset is built in")
        .fruit
        .clone()
}

/// The presets the Fruit tab (`fruit` true) or the Flower tab lists, in
/// catalogue order.
pub fn presets(fruit: bool) -> Vec<&'static FruitPreset> {
    FruitPreset::of_kind(fruit).collect()
}
