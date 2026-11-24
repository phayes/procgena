//! Fruit and flower documents: each edits a fruit or flower preset's
//! settings, saves and loads `.fruit.yml` or `.flower.yml` files, and
//! exports the cluster as a model.

pub mod editor;
pub mod params;

#[cfg(test)]
use procgena_fruit::{FruitOptions, FruitPreset};

/// The key of the fruit preset opened when fruit is the kind opened first.
pub const START_FRUIT: &str = "apple";
/// The key of the flower preset opened when flowers are the kind opened
/// first.
pub const START_FLOWER: &str = "dogwood_bloom";

/// The settings of [`START_FRUIT`].
#[cfg(test)]
pub fn start_options() -> FruitOptions {
    FruitPreset::named(START_FRUIT)
        .expect("the start preset is built in")
        .fruit
        .clone()
}
