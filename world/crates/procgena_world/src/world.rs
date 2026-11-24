//! The World: its regions, the Biome chosen for each, and the land.

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::sync::Arc;

use glam::DVec2;
use procgena_biome::{BiomeError, BiomePreset, biome_seed};
use procgena_core::hash::seed_for;
use procgena_terrain::{
    DEFAULT_CACHE, Elevation, NoiseLayer, RegionId, TerrainOptions, WorldTerrain,
    WorldTerrainOptions, region_local_seed,
};

use crate::options::{BiomeRule, WorldOptions, WorldOptionsError};

// Seed keys, named `<crate>.<area>.<role>`. They are frozen: renaming one
// changes every World built from it. Add a new key for a new role, and list
// it in `KEYS`.

/// The ruggedness selector field.
const FIELD_RUGGEDNESS: &str = "world.field.ruggedness";
/// The moisture selector field.
const FIELD_MOISTURE: &str = "world.field.moisture";

/// Every seed key in this crate, for the test that they are all different.
#[cfg(test)]
pub(crate) const KEYS: &[&str] = &[FIELD_RUGGEDNESS, FIELD_MOISTURE];

/// The values of the World's fields at one point, as Biome rules read
/// them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FieldValues {
    /// The world elevation E, in metres above sea level, at the World's
    /// scale.
    pub elevation: f32,
    /// The ruggedness field, about 95 % of it from −1 to 1.
    pub ruggedness: f32,
    /// The moisture field, about 95 % of it from −1 to 1.
    pub moisture: f32,
}

/// Why a [`World`] could not be made.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum WorldError {
    /// The settings aren't valid.
    Options(WorldOptionsError),
    /// A rule, the fallback or a pin names a Biome the World wasn't given.
    /// Holds the unknown key.
    UnknownBiome(String),
    /// Two of the Biomes the World was given share this key.
    DuplicateBiome(String),
    /// The Terrain of the Biome with this key can't be resolved, or its
    /// settings aren't valid.
    Biome(String, BiomeError),
    /// The land could not be set up.
    Terrain(procgena_terrain::WorldError),
}

impl fmt::Display for WorldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Options(e) => write!(f, "invalid World settings: {e}"),
            Self::UnknownBiome(key) => write!(f, "no Biome is named {key:?}"),
            Self::DuplicateBiome(key) => write!(f, "two Biomes are named {key:?}"),
            Self::Biome(key, e) => write!(f, "Biome {key:?}: {e}"),
            Self::Terrain(e) => write!(f, "land: {e}"),
        }
    }
}

impl std::error::Error for WorldError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Options(e) => Some(e),
            Self::Biome(_, e) => Some(e),
            Self::Terrain(e) => Some(e),
            Self::UnknownBiome(_) | Self::DuplicateBiome(_) => None,
        }
    }
}

/// The Biome instance filling one region of a [`World`].
#[derive(Debug, Clone, Copy)]
pub struct RegionBiome<'a> {
    /// The region.
    pub region: RegionId,
    /// The Biome preset the region uses. Neighbouring regions may use the
    /// same one.
    pub preset: &'a BiomePreset,
    /// This region's own seed for its Biome (see [`World::biome_seed`]):
    /// two regions on the same preset differ through it.
    pub seed: u32,
    /// Whether a pin chose the Biome, rather than the rules.
    pub pinned: bool,
    /// The field values at the region's site, which the rules read.
    pub fields: FieldValues,
}

/// A World of regions, each filled by its own Biome instance, standing on
/// land built when it is first asked for.
///
/// The plane is split into regions by the region graph. Each region's
/// Biome comes from a pin, or else from the first rule that matches the
/// field values at the region's site (E, ruggedness and moisture), or else
/// from the fallback. Neighbouring regions may use the same Biome; each
/// region still has its own instance with its own seed. The land is the
/// [`WorldTerrain`] the World makes, whose regions take their Terrain from
/// their Biome.
///
/// Everything is a pure function of the settings: asking for regions in
/// any order, on any number of threads, gives the same Biomes and the same
/// land.
///
/// ```
/// use glam::DVec2;
/// use procgena_terrain::HeightField;
/// use procgena_world::{World, WorldOptions};
///
/// let world = World::new(WorldOptions::default())?;
/// let p = DVec2::new(1200.0, -300.0);
/// let biome = world.biome(world.region_at(p));
/// println!("{} at {p}", biome.preset.name);
/// assert!(world.terrain().height(p).is_finite());
/// # Ok::<(), procgena_world::WorldError>(())
/// ```
pub struct World {
    options: WorldOptions,
    choice: Arc<Choice>,
    terrain: WorldTerrain,
}

impl fmt::Debug for World {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("World")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

/// Everything the Biome choice needs, shared by the World and the terrain
/// source it hands its [`WorldTerrain`].
struct Choice {
    /// The Biomes the World may use, with their Terrain settings resolved
    /// for the World seed.
    biomes: Vec<(BiomePreset, TerrainOptions)>,
    /// Each rule, with the index of its Biome.
    rules: Vec<(BiomeRule, usize)>,
    /// The index of the fallback Biome.
    fallback: usize,
    /// The index of each pinned region's Biome.
    pins: HashMap<RegionId, usize>,
    /// The scale's vertical stretch, which multiplies the rules' elevation
    /// bounds.
    vertical: f64,
    ruggedness: (NoiseLayer, u32),
    moisture: (NoiseLayer, u32),
}

impl Choice {
    /// The field values at `p`.
    fn fields(&self, p: DVec2, elevation: &Elevation) -> FieldValues {
        FieldValues {
            elevation: elevation.height(p),
            ruggedness: self.ruggedness.0.sample(p, self.ruggedness.1),
            moisture: self.moisture.0.sample(p, self.moisture.1),
        }
    }

    /// The index of the Biome for `region`, whose site is `site`, whether
    /// a pin chose it, and the field values at the site.
    fn choose(
        &self,
        region: RegionId,
        site: DVec2,
        elevation: &Elevation,
    ) -> (usize, bool, FieldValues) {
        let fields = self.fields(site, elevation);
        if let Some(&biome) = self.pins.get(&region) {
            return (biome, true, fields);
        }
        let biome = self
            .rules
            .iter()
            .find(|(rule, _)| {
                rule.elevation.holds(fields.elevation, self.vertical)
                    && rule.ruggedness.holds(fields.ruggedness, 1.0)
                    && rule.moisture.holds(fields.moisture, 1.0)
            })
            .map_or(self.fallback, |&(_, biome)| biome);
        (biome, false, fields)
    }
}

impl World {
    /// The World for `options`, choosing among the built-in Biome presets
    /// ([`BiomePreset::all`]). Builds no land yet.
    ///
    /// # Errors
    ///
    /// See [`with_biomes`](Self::with_biomes).
    pub fn new(options: WorldOptions) -> Result<Self, WorldError> {
        Self::with_biomes(options, BiomePreset::all().cloned().collect())
    }

    /// The World for `options`, choosing among `biomes` only: the rules,
    /// the fallback and the pins name Biomes by their keys in this list.
    /// Builds no land yet.
    ///
    /// # Errors
    ///
    /// [`WorldError::Options`] if the settings aren't valid,
    /// [`WorldError::DuplicateBiome`] if two of `biomes` share a key,
    /// [`WorldError::UnknownBiome`] if a rule, the fallback or a pin names
    /// a key not in `biomes`, and [`WorldError::Biome`] if one of `biomes`
    /// has a Terrain that can't be resolved or isn't valid.
    pub fn with_biomes(
        options: WorldOptions,
        biomes: Vec<BiomePreset>,
    ) -> Result<Self, WorldError> {
        options.validate().map_err(WorldError::Options)?;

        // Resolve every Biome's Terrain up front, so that choosing a Biome
        // for a region can't fail.
        let mut keys = BTreeSet::new();
        let mut resolved = Vec::with_capacity(biomes.len());
        for preset in biomes {
            if !keys.insert(preset.key.to_string()) {
                return Err(WorldError::DuplicateBiome(preset.key.to_string()));
            }
            let terrain = preset
                .biome
                .terrain_options(options.seed)
                .and_then(|terrain| terrain.validate().map(|()| terrain).map_err(Into::into))
                .map_err(|e| WorldError::Biome(preset.key.to_string(), e))?;
            resolved.push((preset, terrain));
        }
        let index = |key: &str| {
            resolved
                .iter()
                .position(|(preset, _)| preset.key == key)
                .ok_or_else(|| WorldError::UnknownBiome(key.to_owned()))
        };

        // The rules, the fallback and the pins, by index.
        let rules = options
            .biomes
            .rules
            .iter()
            .map(|rule| Ok((rule.clone(), index(&rule.biome)?)))
            .collect::<Result<Vec<_>, WorldError>>()?;
        let fallback = index(&options.biomes.fallback)?;
        let pins = options
            .pins
            .iter()
            .map(|pin| Ok((RegionId(pin.region), index(&pin.biome)?)))
            .collect::<Result<HashMap<_, _>, WorldError>>()?;

        let horizontal = options.scale.horizontal;
        let choice = Arc::new(Choice {
            biomes: resolved,
            rules,
            fallback,
            pins,
            vertical: options.scale.vertical,
            ruggedness: (
                options.fields.ruggedness.layer(horizontal),
                seed_for(options.seed, FIELD_RUGGEDNESS),
            ),
            moisture: (
                options.fields.moisture.layer(horizontal),
                seed_for(options.seed, FIELD_MOISTURE),
            ),
        });

        // The land: each region takes the Terrain of the Biome chosen for
        // it.
        let source = {
            let choice = Arc::clone(&choice);
            move |region: RegionId, site: DVec2, elevation: &Elevation| {
                let (biome, _, _) = choice.choose(region, site, elevation);
                choice.biomes[biome].1.clone()
            }
        };
        let terrain = WorldTerrain::new(
            WorldTerrainOptions {
                seed: options.seed,
                elevation: options.scaled_elevation(),
                regions: options.regions,
                convergence: options.convergence,
                cache: DEFAULT_CACHE,
            },
            Arc::new(source),
        )
        .map_err(WorldError::Terrain)?;

        Ok(Self {
            options,
            choice,
            terrain,
        })
    }

    /// The settings.
    pub fn options(&self) -> &WorldOptions {
        &self.options
    }

    /// The land: the region graph, the world elevation E and every
    /// region's terrain, built on demand.
    pub fn terrain(&self) -> &WorldTerrain {
        &self.terrain
    }

    /// The region holding world position `p` (x and z, metres).
    pub fn region_at(&self, p: DVec2) -> RegionId {
        self.terrain.region_at(p)
    }

    /// The Biome instance filling `region`.
    pub fn biome(&self, region: RegionId) -> RegionBiome<'_> {
        let site = self.terrain.site(region);
        let (biome, pinned, fields) = self.choice.choose(region, site, self.terrain.elevation());
        RegionBiome {
            region,
            preset: &self.choice.biomes[biome].0,
            seed: self.biome_seed(region),
            pinned,
            fields,
        }
    }

    /// The Biome instance filling the region that holds `p`.
    pub fn biome_at(&self, p: DVec2) -> RegionBiome<'_> {
        self.biome(self.region_at(p))
    }

    /// The values of the World's fields at `p`.
    pub fn fields(&self, p: DVec2) -> FieldValues {
        self.choice.fields(p, self.terrain.elevation())
    }

    /// The seed of `region`'s Biome instance (see
    /// [`procgena_biome::biome_seed`]): the root of the seeds of everything
    /// that grows in the region.
    pub fn biome_seed(&self, region: RegionId) -> u32 {
        biome_seed(self.options.seed, region)
    }

    /// The seed `region`'s Terrain uses for its own per-region choices
    /// (see [`procgena_terrain::region_local_seed`]). It doesn't depend on
    /// the region's Biome.
    pub fn terrain_local_seed(&self, region: RegionId) -> u32 {
        region_local_seed(self.options.seed, region)
    }
}

#[cfg(test)]
mod tests {
    use procgena_core::hash::salt;

    use super::KEYS;

    #[test]
    fn seed_keys_and_their_salts_are_all_different() {
        for (i, a) in KEYS.iter().enumerate() {
            for b in &KEYS[i + 1..] {
                assert_ne!(a, b);
                assert_ne!(salt(a), salt(b), "{a} and {b} share a salt");
            }
        }
    }
}
