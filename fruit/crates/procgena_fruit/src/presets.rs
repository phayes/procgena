//! Ready-made fruit and flower clusters.
//!
//! [`FruitPreset::all`] lists the built-in presets in menu order, and
//! [`FruitPreset::named`] looks one up by its key. Each built-in preset is a
//! `.fruit.yml` file in the crate's `presets` folder, embedded in the
//! crate; a file of your own loads with [`FruitPreset::from_yaml`].

use std::borrow::Cow;

use crate::options::FruitOptions;

/// A named, ready-made fruit or flower cluster.
///
/// Every built-in preset has a function of its own, such as
/// [`FruitPreset::apple`]. Clone its [`fruit`](Self::fruit) options to
/// change them.
#[derive(Debug, Clone, PartialEq)]
pub struct FruitPreset {
    /// The preset's key, such as `"apple"`, or a loaded file's name.
    pub key: Cow<'static, str>,
    /// A short display name, such as `"Apple"`.
    pub name: Cow<'static, str>,
    /// What the cluster looks like, in a sentence or two.
    pub description: Cow<'static, str>,
    /// The group it is listed under: `"Fruit"` or `"Flowers"` for a built-in
    /// preset, or `"File"` for a loaded file.
    pub group: Cow<'static, str>,
    /// The cluster's settings. They are always valid for a built-in preset.
    pub fruit: FruitOptions,
}

/// The options of an embedded preset file, which names no `base`.
///
/// # Panics
///
/// Panics if the file can't be read; a test checks every embedded file.
fn embedded(key: &str, text: &str) -> FruitOptions {
    procgena_core::preset::read::<FruitPreset>(key, text)
        .unwrap_or_else(|error| panic!("built-in preset {key} is not a valid fruit file: {error}"))
        .fruit
}

/// Reads the embedded file of preset `$key` from `presets/`.
macro_rules! fruit_file {
    ($key:ident) => {
        embedded(
            stringify!($key),
            include_str!(concat!("../presets/", stringify!($key), ".fruit.yml")),
        )
    };
}

procgena_core::catalogue! {
    FruitPreset {
        fruit: FruitOptions,
        check: |options| options.validate().map_err(|error| error.to_string()),
    }
    "Fruit" {
        apple {
            name: "Apple",
            description: "A ripe apple hanging straight from the branch, flushed red on its \
                sunny side.",
        } => fruit_file!(apple);
        pussy_willow_catkin {
            name: "Pussy Willow Catkin",
            description: "A small upright spray of three fuzzy, silver-grey pussy willow \
                catkins.",
        } => fruit_file!(pussy_willow_catkin);
    }
    "Flowers" {
        dogwood_bloom {
            name: "Dogwood Bloom",
            description: "A flowering dogwood bloom of four broad, notched white bracts \
                around a green-gold centre.",
        } => fruit_file!(dogwood_bloom);
    }
}

impl FruitPreset {
    /// Every built-in preset whose items are fruit (`true`) or flowers
    /// (`false`), in catalogue order.
    pub fn of_kind(fruit: bool) -> impl Iterator<Item = &'static Self> {
        Self::all().filter(move |preset| preset.fruit.item.is_fruit() == fruit)
    }

    /// The key of the built-in preset whose options equal `options`, if any.
    pub fn key_of(options: &FruitOptions) -> Option<&'static str> {
        Self::all()
            .find(|preset| &preset.fruit == options)
            .map(|preset| preset.key.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_preset_is_valid() {
        for preset in FruitPreset::all() {
            preset.fruit.validate().unwrap_or_else(|e| panic!("{}: {e}", preset.key));
        }
    }

    #[test]
    fn presets_round_trip_through_their_files() {
        for preset in FruitPreset::all() {
            let text = preset.to_yaml(None);
            let read = FruitPreset::from_yaml(&preset.key, &text).unwrap();
            assert_eq!(read.fruit, preset.fruit, "{}", preset.key);
            let text = preset.to_yaml(Some(FruitPreset::apple()));
            let read = FruitPreset::from_yaml(&preset.key, &text).unwrap();
            assert_eq!(read.fruit, preset.fruit, "{} over apple", preset.key);
        }
    }

    #[test]
    fn presets_split_into_fruit_and_flowers() {
        let fruit: Vec<_> = FruitPreset::of_kind(true).map(|p| p.key.as_ref()).collect();
        let flowers: Vec<_> = FruitPreset::of_kind(false).map(|p| p.key.as_ref()).collect();
        assert_eq!(fruit, ["apple", "pussy_willow_catkin"]);
        assert_eq!(flowers, ["dogwood_bloom"]);
        assert_eq!(FruitPreset::key_of(&FruitPreset::apple().fruit), Some("apple"));
    }
}
