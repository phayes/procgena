//! Ready-made plants and fields to use directly or customise.
//!
//! [`PlantPreset::all`] and [`FieldPreset::all`] list built-in presets in
//! group order; `named(key)` looks one up by its exact key. Loaded assets
//! are separate and do not appear in these catalogues.
//!
//! Each group lives in its own file (`general.rs`, `seasons.rs`, ...). To add a
//! field preset, add one entry to the block for its group:
//!
//! ```text
//! MY_FIELD { name: "My field", description: "What it looks like." } => GrassField { /* ... */ };
//! ```
//!
//! A plant preset is the same, with a `GrassPlant` after the arrow, in a
//! `plant_preset_group!` block. Then run `BLESS_PRESETS=1 cargo test` to write
//! the shipped files from the constants. The group arrays, `all()`, `named()`
//! and the embedded preset assets all come from these blocks, so no other file
//! needs a preset list.

use std::borrow::Cow;

use bevy::asset::Asset;
use bevy::color::Color;
use bevy::math::Vec2;
use bevy::reflect::Reflect;

use crate::api::{GrassField, GrassGround};
use crate::look::{
    FieldPlant, GrassAccent, GrassArrangement, GrassBranches, GrassPlant, GrassSegment,
    GrassVariation, GroundTint,
};

/// A named plant preset, ready to add to a field.
///
/// Every built-in plant is a constant here and on [`GrassPlant`], with the same
/// name: `PlantPreset::FERN.plant == GrassPlant::FERN`. Mix them into a field
/// with [`FieldPlant::new`]. A `.plant.ron` file loads as one of these, keyed
/// by its filename without the `.plant.ron` extension.
#[derive(Asset, Debug, Clone, PartialEq, Reflect)]
#[reflect(Debug, PartialEq)]
pub struct PlantPreset {
    /// The constant's name, such as `"FERN"`, or a loaded file's stem.
    pub key: Cow<'static, str>,
    /// A short display name, such as `"Fern"`.
    pub name: Cow<'static, str>,
    /// What the plant looks like, in a sentence or two.
    pub description: Cow<'static, str>,
    /// The group it is listed under, such as `"Ground cover"`, or `"File"` for
    /// a loaded file.
    pub group: Cow<'static, str>,
    /// The plant itself.
    pub plant: GrassPlant,
}

/// A named, ready-made field: its plants, density, variation and colours.
///
/// Spawn one with [`GrassField::from_preset`], which gives it a size. The
/// built-in preset's field has size 0, flat ground and no density map. A
/// `.field.ron` file loads as one of these, keyed by its filename without
/// that extension.
#[derive(Asset, Debug, Clone, PartialEq, Reflect)]
#[reflect(Debug, PartialEq)]
pub struct FieldPreset {
    /// The constant's name, such as `"MEADOW"`, or a loaded file's stem.
    pub key: Cow<'static, str>,
    /// A short display name, such as `"Meadow"`.
    pub name: Cow<'static, str>,
    /// What the field looks like, in a sentence or two.
    pub description: Cow<'static, str>,
    /// The group it is listed under, such as `"General"`, or `"File"` for a
    /// loaded file.
    pub group: Cow<'static, str>,
    /// The field, with size 0.
    pub field: GrassField,
}

/// Declares field presets and their catalogue group from one list.
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
        impl FieldPreset {
            $(
                #[doc = $description]
                pub const $key: Self = Self {
                    key: Cow::Borrowed(stringify!($key)),
                    name: Cow::Borrowed($name),
                    description: Cow::Borrowed($description),
                    group: Cow::Borrowed($group_name),
                    field: $value,
                };
            )*

            $(#[$group_attr])*
            pub const $group: [Self; preset_group!(@count $($key)*)] = [
                $( Self::$key, )*
            ];
        }

        pub(crate) const PRESETS: &[FieldPreset] = &FieldPreset::$group;
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
        impl GrassPlant {
            $(
                #[doc = $description]
                pub const $key: Self = $value;
            )*
        }

        impl PlantPreset {
            $(
                #[doc = $description]
                pub const $key: Self = Self {
                    key: Cow::Borrowed(stringify!($key)),
                    name: Cow::Borrowed($name),
                    description: Cow::Borrowed($description),
                    group: Cow::Borrowed($group_name),
                    plant: GrassPlant::$key,
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

impl FieldPreset {
    /// Built-in field presets grouped for browsing.
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

    /// Lists every built-in field preset in group order.
    ///
    /// Uses [`Self::GROUPS`]. Presets loaded from files are not included.
    pub fn all() -> impl Iterator<Item = &'static Self> {
        Self::GROUPS.iter().flat_map(|group| group.iter())
    }

    /// Finds a built-in field preset by its exact key.
    ///
    /// For example, `"MEADOW"` matches; a display name or lower-case spelling
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
    /// For example, `"FERN"` matches; a display name or lower-case spelling
    /// does not. Returns `None` for unknown keys, including loaded file assets.
    #[must_use]
    pub fn named(key: &str) -> Option<&'static Self> {
        Self::all().find(|preset| preset.key == key)
    }
}

/// A field preset's settings that most presets share: size 0, flat ground, no
/// density map and full wind.
const FIELD_BASE: GrassField = GrassField {
    size: Vec2::ZERO,
    ground: GrassGround::Flat,
    density_map: None,
    wind_scale: 1.0,
    plants: Cow::Borrowed(&[]),
    density_scale: 1.0,
    variation: GrassVariation {
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
