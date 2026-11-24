//! Ready-made fruit clusters and flower clusters.
//!
//! [`FruitPreset::all`] and [`FlowerPreset::all`] list the built-in presets
//! in menu order, and `named` looks one up by its key. Each built-in
//! preset is a file in the crate's `presets` folder, embedded in the crate:
//! a `.fruit.yml` file for fruit, a `.flower.yml` file for flowers. A file
//! of your own loads with [`FruitPreset::from_yaml`] or
//! [`FlowerPreset::from_yaml`].

use std::borrow::Cow;

use crate::options::FruitOptions;

/// A named, ready-made cluster of fruit, such as apples or catkins.
///
/// Every built-in preset has a function of its own, such as
/// [`FruitPreset::apple`]. Clone its [`fruit`](Self::fruit) options to
/// change them. Its items are always fruit, never flowers.
#[derive(Debug, Clone, PartialEq)]
pub struct FruitPreset {
    /// The preset's key, such as `"apple"`, or a loaded file's name.
    pub key: Cow<'static, str>,
    /// A short display name, such as `"Apple"`.
    pub name: Cow<'static, str>,
    /// What the cluster looks like, in a sentence or two.
    pub description: Cow<'static, str>,
    /// The group it is listed under: `"Fruit"` for a built-in preset, or
    /// `"File"` for a loaded file.
    pub group: Cow<'static, str>,
    /// The cluster's settings. They are always valid for a built-in preset.
    pub fruit: FruitOptions,
}

/// A named, ready-made cluster of flowers, such as a dogwood bloom.
///
/// Every built-in preset has a function of its own, such as
/// [`FlowerPreset::dogwood_bloom`]. Clone its [`flower`](Self::flower)
/// options to change them. Its items are always flowers, never fruit.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowerPreset {
    /// The preset's key, such as `"dogwood_bloom"`, or a loaded file's name.
    pub key: Cow<'static, str>,
    /// A short display name, such as `"Dogwood Bloom"`.
    pub name: Cow<'static, str>,
    /// What the cluster looks like, in a sentence or two.
    pub description: Cow<'static, str>,
    /// The group it is listed under: `"Flowers"` for a built-in preset, or
    /// `"File"` for a loaded file.
    pub group: Cow<'static, str>,
    /// The cluster's settings. They are always valid for a built-in preset.
    pub flower: FruitOptions,
}

/// The options of an embedded `.fruit.yml` file, which names no `base`.
///
/// # Panics
///
/// Panics if the file can't be read; a test checks every embedded file.
fn embedded_fruit(key: &str, text: &str) -> FruitOptions {
    procgena_core::preset::read::<FruitPreset>(key, text)
        .unwrap_or_else(|error| panic!("built-in preset {key} is not a valid fruit file: {error}"))
        .fruit
}

/// The options of an embedded `.flower.yml` file, which names no `base`.
///
/// # Panics
///
/// Panics if the file can't be read; a test checks every embedded file.
fn embedded_flower(key: &str, text: &str) -> FruitOptions {
    procgena_core::preset::read::<FlowerPreset>(key, text)
        .unwrap_or_else(|error| panic!("built-in preset {key} is not a valid flower file: {error}"))
        .flower
}

/// Reads the embedded file of fruit preset `$key` from `presets/`.
macro_rules! fruit_file {
    ($key:ident) => {
        embedded_fruit(
            stringify!($key),
            include_str!(concat!("../presets/", stringify!($key), ".fruit.yml")),
        )
    };
}

/// Reads the embedded file of flower preset `$key` from `presets/`.
macro_rules! flower_file {
    ($key:ident) => {
        embedded_flower(
            stringify!($key),
            include_str!(concat!("../presets/", stringify!($key), ".flower.yml")),
        )
    };
}

/// Checks that `options` are valid, and that their items are fruit when
/// `fruit` is true and flowers when it is false.
fn check_items(options: &FruitOptions, fruit: bool) -> Result<(), String> {
    options.validate().map_err(|error| error.to_string())?;
    match (options.item.is_fruit(), fruit) {
        (true, false) => Err("a flower preset's items must be flowers, not fruit".into()),
        (false, true) => Err("a fruit preset's items must be fruit, not flowers".into()),
        _ => Ok(()),
    }
}

procgena_core::catalogue! {
    FruitPreset {
        fruit: FruitOptions,
        check: |options| check_items(options, true),
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
}

procgena_core::catalogue! {
    FlowerPreset {
        flower: FruitOptions,
        check: |options| check_items(options, false),
    }
    "Flowers" {
        dogwood_bloom {
            name: "Dogwood Bloom",
            description: "A flowering dogwood bloom of four broad, notched white bracts \
                around a green-gold centre.",
        } => flower_file!(dogwood_bloom);
    }
}

impl FruitPreset {
    /// The key of the built-in preset whose options equal `options`, if any.
    pub fn key_of(options: &FruitOptions) -> Option<&'static str> {
        Self::all()
            .find(|preset| &preset.fruit == options)
            .map(|preset| preset.key.as_ref())
    }
}

impl FlowerPreset {
    /// The key of the built-in preset whose options equal `options`, if any.
    pub fn key_of(options: &FruitOptions) -> Option<&'static str> {
        Self::all()
            .find(|preset| &preset.flower == options)
            .map(|preset| preset.key.as_ref())
    }
}

/// Every built-in fruit preset, then every built-in flower preset, as its
/// key and settings. Keys are unique across both.
pub fn cluster_presets() -> impl Iterator<Item = (&'static str, &'static FruitOptions)> {
    let fruit = FruitPreset::all().map(|preset| (preset.key.as_ref(), &preset.fruit));
    let flowers = FlowerPreset::all().map(|preset| (preset.key.as_ref(), &preset.flower));
    fruit.chain(flowers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_preset_is_valid() {
        for preset in FruitPreset::all() {
            check_items(&preset.fruit, true).unwrap_or_else(|e| panic!("{}: {e}", preset.key));
        }
        for preset in FlowerPreset::all() {
            check_items(&preset.flower, false).unwrap_or_else(|e| panic!("{}: {e}", preset.key));
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
        for preset in FlowerPreset::all() {
            let text = preset.to_yaml(None);
            let read = FlowerPreset::from_yaml(&preset.key, &text).unwrap();
            assert_eq!(read.flower, preset.flower, "{}", preset.key);
        }
    }

    #[test]
    fn fruit_and_flowers_are_separate_catalogues() {
        let fruit: Vec<_> = FruitPreset::all().map(|p| p.key.as_ref()).collect();
        let flowers: Vec<_> = FlowerPreset::all().map(|p| p.key.as_ref()).collect();
        assert_eq!(fruit, ["apple", "pussy_willow_catkin"]);
        assert_eq!(flowers, ["dogwood_bloom"]);
        assert_eq!(
            FruitPreset::key_of(&FruitPreset::apple().fruit),
            Some("apple")
        );
        assert_eq!(
            FlowerPreset::key_of(&FlowerPreset::dogwood_bloom().flower),
            Some("dogwood_bloom")
        );
        assert_eq!(
            FruitPreset::key_of(&FlowerPreset::dogwood_bloom().flower),
            None
        );
        let keys: Vec<_> = cluster_presets().map(|(key, _)| key).collect();
        assert_eq!(keys, ["apple", "pussy_willow_catkin", "dogwood_bloom"]);
    }

    #[test]
    fn each_file_type_refuses_the_other_kind_of_item() {
        let flower_as_fruit = FlowerPreset::dogwood_bloom()
            .to_yaml(None)
            .replace("\nflower:", "\nfruit:");
        let error = FruitPreset::from_yaml("bloom", &flower_as_fruit).unwrap_err();
        assert!(error.to_string().contains("must be fruit"), "{error}");

        let fruit_as_flower = FruitPreset::apple()
            .to_yaml(None)
            .replace("\nfruit:", "\nflower:");
        let error = FlowerPreset::from_yaml("apple", &fruit_as_flower).unwrap_err();
        assert!(error.to_string().contains("must be flowers"), "{error}");
    }
}
