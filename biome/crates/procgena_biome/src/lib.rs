//! # procgena_biome
//!
//! A Biome is one region of the world with one climate and character, such
//! as "temperate forest" or "alpine". It holds exactly one Terrain, the
//! ground of its region, and rules for which Ecotopes grow on which kind of
//! ground.
//!
//! This is an early version: Ecotopes don't exist yet, so every Biome is an
//! empty Biome, bare land with no cover. Building one gives exactly the same
//! land as building its Terrain on its own. [`BiomePreset`] lists a default
//! empty Biome for every [`TerrainPreset`](procgena_terrain::TerrainPreset).
//!
//! ```
//! use procgena_biome::BiomePreset;
//! use procgena_terrain::{Boundary, HeightField, Region, RegionId};
//! use glam::DVec2;
//!
//! let preset = BiomePreset::named("ROLLING_HILLS").expect("built-in preset");
//!
//! // A square region with no neighbours.
//! let region = Region {
//!     id: RegionId(1),
//!     boundary: Boundary::Polygon(vec![
//!         DVec2::new(0.0, 0.0),
//!         DVec2::new(1000.0, 0.0),
//!         DVec2::new(1000.0, 1000.0),
//!         DVec2::new(0.0, 1000.0),
//!     ]),
//!     edges: Vec::new(),
//!     corners: Vec::new(),
//! };
//! let terrain = preset.biome.build(preset.default_seed, region, &[])?;
//! assert!(terrain.height(DVec2::new(500.0, 500.0)).is_finite());
//! # Ok::<(), procgena_biome::BiomeError>(())
//! ```

mod biome;
mod presets;

pub use biome::{Biome, BiomeError, BiomeTerrain, EcotopeRule, terrain_seed};
pub use presets::BiomePreset;
