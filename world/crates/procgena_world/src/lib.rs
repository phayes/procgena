//! # procgena_world
//!
//! A World is the whole map: the plane split into regions, each region
//! filled by its own Biome, all standing on land that joins without seams
//! at the borders. One World seed fixes everything in it.
//!
//! [`WorldOptions`] set the World's seed, its scale, how it is split into
//! regions, the world elevation E, and how each region's Biome is chosen:
//! smooth selector fields (ruggedness and moisture) read with E at each
//! region's site, rules that turn those values into Biomes, and pins that
//! set a region's Biome by hand. [`WorldPreset`] has ready-made settings for
//! each scale class, from small Minecraft-like worlds to real-life ones.
//!
//! This is an early version: Biomes are bare land for now, so a World is
//! land with nothing growing on it.
//!
//! ```
//! use glam::DVec2;
//! use procgena_world::{World, WorldPreset};
//!
//! let world = World::new(WorldPreset::medium().world.clone())?;
//! let p = DVec2::new(500.0, 2500.0);
//! let biome = world.biome_at(p);
//! println!("{} (seed {}) on E = {} m", biome.preset.name, biome.seed, biome.fields.elevation);
//! // `world.terrain()` builds the land of a region when it is first asked for.
//! # Ok::<(), procgena_world::WorldError>(())
//! ```

mod options;
mod presets;
mod selection;
mod world;

pub use options::{
    BIOME_SELECTION_VERSION, BiomeCompatibility, BiomePin, BiomeRule, BiomeRules,
    BiomeSelectionOptions, BiomeSuitability, CompatibilityPair, FieldPreference, FieldRange,
    MAX_ITERATIONS, SelectionError, SelectorField, SelectorFields, WorldOptions, WorldOptionsError,
    WorldScale,
};
pub use presets::WorldPreset;

/// The text of `world.meta.yml`: each World setting's label, description
/// and range, for tools that edit World presets. Read it with
/// [`KindMeta::parse`](procgena_core::meta::KindMeta::parse).
pub const WORLD_META: &str = include_str!("../presets/world.meta.yml");
pub use world::{FieldValues, RegionBiome, World, WorldError};
