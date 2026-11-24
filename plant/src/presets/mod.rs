//! Ready-made plants and fields to use directly or customise.
//!
//! [`PlantPreset::all`] and [`PlantScatterPreset::all`] list built-in presets in
//! group order; `named(key)` looks one up by its exact key. Loaded assets
//! are separate and do not appear in these catalogues.
//!
//! Each group lives in its own file (`general.rs`, `seasons.rs`, ...). To add a
//! Scatter preset, add one entry to the block for its group:
//!
//! ```text
//! MY_SCATTER { name: "My scatter", description: "What it looks like." } => PlantScatter { /* ... */ };
//! ```
//!
//! A plant preset is the same, with a `PlantParams` after the arrow, in a
//! `plant_preset_group!` block. A preset's key is its constant's name in lower
//! case (`MY_SCATTER` is `my_scatter`). Then add its shipped file to
//! `assets/plants` or `assets/scatters`, written with `to_yaml(None)`: a test
//! fails until every constant has a file that reads back equal to it. The
//! group arrays, `all()`, `named()` and the embedded preset assets all come
//! from these blocks, so no other file needs a preset list.

use std::borrow::Cow;

use bevy::asset::Asset;
use bevy::color::Color;
use bevy::math::Vec2;
use bevy::reflect::Reflect;

use crate::api::{PlantGround, PlantScatter};
use crate::look::{
    GroundTint, PlantAccent, PlantArrangement, PlantBranches, PlantParams, PlantSegment,
    PlantVariation, ScatterPlant,
};

/// A named plant preset, ready to add to a field.
///
/// Every built-in plant is a constant here and on [`PlantParams`], with the same
/// name: `PlantPreset::FERN.plant == PlantParams::FERN`. Mix them into a field
/// with [`ScatterPlant::new`]. A `.plant.yml` file loads as one of these, keyed
/// by its filename without the `.plant.yml` extension.
#[derive(Asset, Debug, Clone, PartialEq, Reflect)]
#[reflect(Debug, PartialEq)]
pub struct PlantPreset {
    /// The constant's name in lower case, such as `"fern"`, or a loaded
    /// file's name without its extension.
    pub key: Cow<'static, str>,
    /// A short display name, such as `"Fern"`.
    pub name: Cow<'static, str>,
    /// What the plant looks like, in a sentence or two.
    pub description: Cow<'static, str>,
    /// The group it is listed under, such as `"Ground cover"`, or `"File"` for
    /// a loaded file.
    pub group: Cow<'static, str>,
    /// The plant itself.
    pub plant: PlantParams,
}

/// A named, ready-made [`PlantScatter`]: its plants, density, variation and
/// colours.
///
/// Spawn one with [`PlantScatter::from_preset`], which gives it a size. The
/// built-in preset's Scatter has size 0, flat ground and no density map. A
/// `.plant_scatter.yml` file loads as one of these, keyed by its filename
/// without that extension.
#[derive(Asset, Debug, Clone, PartialEq, Reflect)]
#[reflect(Debug, PartialEq)]
pub struct PlantScatterPreset {
    /// The constant's name in lower case, such as `"meadow"`, or a loaded
    /// file's name without its extension.
    pub key: Cow<'static, str>,
    /// A short display name, such as `"Meadow"`.
    pub name: Cow<'static, str>,
    /// What the Scatter looks like, in a sentence or two.
    pub description: Cow<'static, str>,
    /// The group it is listed under, such as `"General"`, or `"File"` for a
    /// loaded file.
    pub group: Cow<'static, str>,
    /// The Scatter, with size 0.
    pub plant_scatter: PlantScatter,
}

/// The ASCII lower-case bytes of `name`, which must be `N` bytes long: a
/// preset constant's name made into its key.
const fn lower_case<const N: usize>(name: &str) -> [u8; N] {
    let bytes = name.as_bytes();
    let mut lower = [0; N];
    let mut i = 0;
    while i < N {
        lower[i] = bytes[i].to_ascii_lowercase();
        i += 1;
    }
    lower
}

/// The key of the preset constant named `$name`: the name in lower case.
macro_rules! preset_key {
    ($name:ident) => {{
        const KEY: [u8; stringify!($name).len()] = crate::presets::lower_case(stringify!($name));
        match std::str::from_utf8(&KEY) {
            Ok(key) => key,
            Err(_) => panic!("a constant's name is ASCII"),
        }
    }};
}

/// Declares Scatter presets and their catalogue group from one list.
/// The description also supplies each constant's rustdoc, so it must stay
/// in sync with the shipped preset files.
macro_rules! preset_group {
    (
        $(#[$group_attr:meta])*
        $group:ident $group_name:literal {
            $(
                $key:ident { name: $name:literal, description: $description:literal $(,)? }
                    => $value:expr;
            )*
        }
    ) => {
        impl PlantScatterPreset {
            $(
                #[doc = $description]
                pub const $key: Self = Self {
                    key: Cow::Borrowed(preset_key!($key)),
                    name: Cow::Borrowed($name),
                    description: Cow::Borrowed($description),
                    group: Cow::Borrowed($group_name),
                    plant_scatter: $value,
                };
            )*

            $(#[$group_attr])*
            pub const $group: [Self; preset_group!(@count $($key)*)] = [
                $( Self::$key, )*
            ];
        }

        pub(crate) const PRESETS: &[PlantScatterPreset] = &PlantScatterPreset::$group;
    };
    (@count) => { 0 };
    (@count $head:ident $($tail:ident)*) => {
        1 + preset_group!(@count $($tail)*)
    };
}

/// Declares plant constants, named presets and their catalogue group together.
/// The same description supplies the rustdoc and shipped preset metadata.
macro_rules! plant_preset_group {
    (
        $(#[$group_attr:meta])*
        $group:ident $group_name:literal {
            $(
                $key:ident { name: $name:literal, description: $description:literal $(,)? }
                    => $value:expr;
            )*
        }
    ) => {
        impl PlantParams {
            $(
                #[doc = $description]
                pub const $key: Self = $value;
            )*
        }

        impl PlantPreset {
            $(
                #[doc = $description]
                pub const $key: Self = Self {
                    key: Cow::Borrowed(preset_key!($key)),
                    name: Cow::Borrowed($name),
                    description: Cow::Borrowed($description),
                    group: Cow::Borrowed($group_name),
                    plant: PlantParams::$key,
                };
            )*

            $(#[$group_attr])*
            pub const $group: [Self; preset_group!(@count $($key)*)] = [
                $( Self::$key, )*
            ];
        }

        pub(crate) const PLANTS: &[PlantPreset] = &PlantPreset::$group;
    };
}

mod biomes;
mod blades;
mod fantasy;
mod forest;
mod general;
mod grasses;
mod seasons;
mod shrubs;
mod toon;
mod understory;
mod wild;

impl PlantScatterPreset {
    /// Built-in Scatter presets grouped for browsing.
    ///
    /// Groups and their presets appear in the same order as [`Self::all`].
    pub const GROUPS: &'static [&'static [Self]] = &[
        general::PRESETS,
        seasons::PRESETS,
        wild::PRESETS,
        forest::PRESETS,
        understory::PRESETS,
        shrubs::PRESETS,
        biomes::PRESETS,
        fantasy::PRESETS,
        toon::PRESETS,
    ];

    /// Lists every built-in Scatter preset in group order.
    ///
    /// Uses [`Self::GROUPS`]. Presets loaded from files are not included.
    pub fn all() -> impl Iterator<Item = &'static Self> {
        Self::GROUPS.iter().flat_map(|group| group.iter())
    }

    /// Finds a built-in Scatter preset by its exact key.
    ///
    /// For example, `"meadow"` matches; a display name or upper-case spelling
    /// does not. Returns `None` for unknown keys, including loaded file assets.
    #[must_use]
    pub fn named(key: &str) -> Option<&'static Self> {
        Self::all().find(|preset| preset.key == key)
    }
}

impl PlantPreset {
    /// Built-in plant presets grouped for browsing.
    ///
    /// Groups and their presets appear in the same order as [`Self::all`].
    pub const GROUPS: &'static [&'static [Self]] = &[
        grasses::PLANTS,
        blades::PLANTS,
        understory::PLANTS,
        shrubs::PLANTS,
    ];

    /// Lists every built-in plant preset in group order.
    ///
    /// Uses [`Self::GROUPS`]. Presets loaded from files are not included.
    pub fn all() -> impl Iterator<Item = &'static Self> {
        Self::GROUPS.iter().flat_map(|group| group.iter())
    }

    /// Finds a built-in plant preset by its exact key.
    ///
    /// For example, `"fern"` matches; a display name or upper-case spelling
    /// does not. Returns `None` for unknown keys, including loaded file assets.
    #[must_use]
    pub fn named(key: &str) -> Option<&'static Self> {
        Self::all().find(|preset| preset.key == key)
    }
}

/// A Scatter preset's settings that most presets share: size 0, flat ground,
/// no density map, full wind and seed 0.
const SCATTER_BASE: PlantScatter = PlantScatter {
    size: Vec2::ZERO,
    ground: PlantGround::Flat,
    density_map: None,
    default_seed: 0,
    wind_scale: 1.0,
    plants: Cow::Borrowed(&[]),
    density_scale: 1.0,
    variation: PlantVariation {
        patch_color: Color::srgb(0.55, 0.55, 0.22),
        patch_amount: 0.0,
        patch_size: 12.0,
        dry_tips: 0.0,
        dead_share: 0.0,
        bare_share: 0.0,
        bare_patch_size: 3.0,
    },
    ground_tint: GroundTint {
        color: Color::srgb(0.20, 0.15, 0.09),
        amount: 0.25,
    },
    toon_bands: 0,
};

#[cfg(test)]
mod tests;
