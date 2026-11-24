//! The Biome type, its Terrain and its Ecotope rules.

use std::borrow::Cow;
use std::fmt;

use procgena_terrain::{
    EdgeAgreement, OptionsError, Region, Terrain, TerrainError, TerrainGenerator, TerrainOptions,
    TerrainPreset,
};

/// One region's climate and character: its Terrain and the rules for which
/// Ecotopes cover it.
///
/// A Biome doesn't store a seed. Pass the World seed each time you resolve
/// its Terrain with [`terrain_options`](Self::terrain_options) or build it
/// with [`build`](Self::build). To build a preset's Biome on its own, use the
/// preset's [`default_seed`](crate::BiomePreset::default_seed).
#[derive(Debug, Clone, PartialEq)]
pub struct Biome {
    /// The ground of the Biome's region.
    pub terrain: BiomeTerrain,
    /// Which Ecotope grows on which kind of ground. Always empty for now,
    /// because Ecotopes don't exist yet: every Biome is bare land.
    pub ecotope_rules: Vec<EcotopeRule>,
}

/// The Terrain of a [`Biome`]: a Terrain preset, or settings given inline.
#[derive(Debug, Clone, PartialEq)]
pub enum BiomeTerrain {
    /// The key of a built-in Terrain preset, such as `"rolling_hills"` (see
    /// [`TerrainPreset::named`]). Many Biomes can share one. An unknown key
    /// is an error when the Terrain is resolved.
    Preset(Cow<'static, str>),
    /// Terrain settings given in full. Their `default_seed` is ignored: the
    /// Biome replaces it with the seed from [`terrain_seed`].
    Inline(TerrainOptions),
}

/// One rule saying which Ecotope grows on which kind of ground.
///
/// Ecotopes don't exist yet, so this type has no values and every Biome's
/// list of rules is empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EcotopeRule {}

/// A Biome whose land could not be built.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum BiomeError {
    /// [`BiomeTerrain::Preset`] names no built-in Terrain preset. Holds the
    /// unknown key.
    UnknownTerrainPreset(String),
    /// The Terrain's settings are invalid.
    Options(OptionsError),
    /// The Terrain could not be bound to its region, for example because a
    /// border agreement is missing.
    Terrain(TerrainError),
}

impl fmt::Display for BiomeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTerrainPreset(key) => write!(f, "no terrain preset is named {key:?}"),
            Self::Options(e) => write!(f, "invalid terrain settings: {e}"),
            Self::Terrain(e) => write!(f, "terrain could not be built: {e}"),
        }
    }
}

impl std::error::Error for BiomeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::UnknownTerrainPreset(_) => None,
            Self::Options(e) => Some(e),
            Self::Terrain(e) => Some(e),
        }
    }
}

impl From<OptionsError> for BiomeError {
    fn from(e: OptionsError) -> Self {
        Self::Options(e)
    }
}

impl From<TerrainError> for BiomeError {
    fn from(e: TerrainError) -> Self {
        Self::Terrain(e)
    }
}

/// The Terrain seed a Biome uses in a World with seed `world_seed`.
///
/// Today this returns `world_seed` unchanged, so every Terrain in a World
/// shares the same noise and neighbouring regions join naturally. It is
/// provisional: how a Terrain's seed comes from the World seed is still to
/// be decided, and this function may then change.
pub const fn terrain_seed(world_seed: u32) -> u32 {
    world_seed
}

impl Biome {
    /// An empty Biome: `terrain` with no Ecotope rules, which builds bare
    /// land.
    pub const fn empty(terrain: BiomeTerrain) -> Self {
        Self {
            terrain,
            ecotope_rules: Vec::new(),
        }
    }

    /// Returns true if the Biome has no Ecotope rules, so it builds bare
    /// land.
    pub fn is_empty(&self) -> bool {
        self.ecotope_rules.is_empty()
    }

    /// The settings of this Biome's Terrain in a World with seed
    /// `world_seed`.
    ///
    /// The `default_seed` in the result comes from [`terrain_seed`], also for
    /// inline settings. The settings are not checked; [`build`](Self::build)
    /// checks them.
    ///
    /// # Errors
    ///
    /// [`BiomeError::UnknownTerrainPreset`] if the Biome names a Terrain
    /// preset that doesn't exist.
    pub fn terrain_options(&self, world_seed: u32) -> Result<TerrainOptions, BiomeError> {
        let mut options = match &self.terrain {
            BiomeTerrain::Preset(key) => TerrainPreset::named(key)
                .ok_or_else(|| BiomeError::UnknownTerrainPreset(key.to_string()))?
                .terrain
                .clone(),
            BiomeTerrain::Inline(options) => options.clone(),
        };
        options.default_seed = terrain_seed(world_seed);
        Ok(options)
    }

    /// Builds this Biome's land on `region`, bent to meet `agreements`, in a
    /// World with seed `world_seed`.
    ///
    /// The result is the same [`Terrain`] that building the settings from
    /// [`terrain_options`](Self::terrain_options) on their own gives. There
    /// must be exactly one agreement for every edge of `region` that has a
    /// neighbour; a region with no neighbours takes none.
    ///
    /// # Errors
    ///
    /// [`BiomeError::UnknownTerrainPreset`] if the Biome names a Terrain
    /// preset that doesn't exist, [`BiomeError::Options`] if the Terrain's
    /// settings are invalid, and [`BiomeError::Terrain`] if the agreements
    /// don't fit `region`.
    pub fn build(
        &self,
        world_seed: u32,
        region: Region,
        agreements: &[EdgeAgreement],
    ) -> Result<Terrain, BiomeError> {
        let generator = TerrainGenerator::new(self.terrain_options(world_seed)?)?;
        Ok(generator.build(region, agreements)?)
    }
}
