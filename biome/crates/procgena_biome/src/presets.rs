//! Ready-made Biomes to use directly or customise.
//!
//! [`BiomePreset::all`] lists the built-in presets in group order, and
//! [`BiomePreset::named`] looks one up by its exact key.

use std::borrow::Cow;

use procgena_terrain::TerrainPreset;

use crate::biome::{Biome, BiomeTerrain};

/// A named, ready-made Biome.
///
/// Every built-in preset is a constant here, such as
/// [`BiomePreset::PLAINS`].
#[derive(Debug, Clone, PartialEq)]
pub struct BiomePreset {
    /// The constant's name, such as `"PLAINS"`.
    pub key: Cow<'static, str>,
    /// A short display name, such as `"Plains"`.
    pub name: Cow<'static, str>,
    /// What the Biome looks like, in a sentence or two.
    pub description: Cow<'static, str>,
    /// The group it is listed under, such as `"Bare land"`.
    pub group: Cow<'static, str>,
    /// The Biome itself.
    pub biome: Biome,
    /// The World seed to build the Biome with when it is built on its own.
    /// Ignored once the Biome is part of a World, which passes its own seed.
    pub default_seed: u32,
}

/// The default empty Biome on the Terrain preset `terrain`.
const fn bare_preset(
    key: &'static str,
    name: &'static str,
    description: &'static str,
    terrain: TerrainPreset,
) -> BiomePreset {
    BiomePreset {
        key: Cow::Borrowed(key),
        name: Cow::Borrowed(name),
        description: Cow::Borrowed(description),
        group: Cow::Borrowed("Bare land"),
        biome: Biome::empty(BiomeTerrain::Preset(terrain)),
        default_seed: 0,
    }
}

impl BiomePreset {
    /// Bare plains: nearly flat land with slow swells, and no cover.
    pub const PLAINS: Self = bare_preset(
        "PLAINS",
        "Plains",
        "Bare plains: nearly flat land with slow swells, and no cover.",
        TerrainPreset::Plains,
    );

    /// Bare rolling hills a few tens of metres high, and no cover.
    pub const ROLLING_HILLS: Self = bare_preset(
        "ROLLING_HILLS",
        "Rolling hills",
        "Bare rolling hills a few tens of metres high, and no cover.",
        TerrainPreset::RollingHills,
    );

    /// Bare ridged mountain ranges, and no cover.
    pub const MOUNTAINS: Self = bare_preset(
        "MOUNTAINS",
        "Mountains",
        "Bare ridged mountain ranges, and no cover.",
        TerrainPreset::Mountains,
    );

    /// The default empty Biome for each Terrain preset, in the Terrain
    /// presets' order.
    pub const BARE_LAND: [Self; 3] = [Self::PLAINS, Self::ROLLING_HILLS, Self::MOUNTAINS];

    /// Built-in presets grouped for browsing.
    ///
    /// Groups and their presets appear in the same order as [`Self::all`].
    pub const GROUPS: &'static [&'static [Self]] = &[&Self::BARE_LAND];

    /// Lists every built-in preset in group order.
    pub fn all() -> impl Iterator<Item = &'static Self> {
        Self::GROUPS.iter().flat_map(|group| group.iter())
    }

    /// Finds a built-in preset by its exact key.
    ///
    /// For example, `"PLAINS"` matches; a display name or lower-case
    /// spelling does not. Returns `None` for unknown keys.
    #[must_use]
    pub fn named(key: &str) -> Option<&'static Self> {
        Self::all().find(|preset| preset.key == key)
    }

    /// Finds the default empty Biome for the Terrain preset `terrain`.
    ///
    /// Returns `None` only if a Terrain preset has no default Biome yet.
    #[must_use]
    pub fn bare(terrain: TerrainPreset) -> Option<&'static Self> {
        Self::all().find(|preset| preset.biome.terrain == BiomeTerrain::Preset(terrain))
    }
}
