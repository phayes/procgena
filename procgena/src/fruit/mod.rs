//! Fruit and flower documents: each edits a fruit or flower preset's
//! settings, saves and loads `.fruit.yml` files, and exports the cluster as
//! a model.

pub mod editor;
pub mod params;

#[cfg(test)]
use procgena_fruit::{FruitOptions, FruitPreset};

/// The key of the fruit preset opened when fruit is the kind opened first.
pub const START_FRUIT: &str = "apple";
/// The key of the flower preset opened when flowers are the kind opened
/// first.
pub const START_FLOWER: &str = "dogwood_bloom";

/// The settings of [`START_FRUIT`] (`fruit` true) or [`START_FLOWER`].
#[cfg(test)]
pub fn start_options(fruit: bool) -> FruitOptions {
    let key = if fruit { START_FRUIT } else { START_FLOWER };
    FruitPreset::named(key)
        .expect("the start preset is built in")
        .fruit
        .clone()
}
