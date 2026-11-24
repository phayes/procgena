//! # procgena_biome
//!
//! A Biome is one region of the world with one climate and character, such
//! as "temperate forest" or "alpine". It holds exactly one Terrain, the
//! ground of its region, and rules for which Ecotopes grow on which kind of
//! ground.
//!
//! This is an early version: Biomes don't hold Ecotopes yet, so every Biome
//! is an empty Biome, bare land with no cover. Building one gives exactly
//! the same land as building its Terrain on its own. [`BiomePreset`] lists a
//! default empty Biome for every
//! [`TerrainPreset`](procgena_terrain::TerrainPreset).
//!
//! # Ecotopes
//!
//! An [`Ecotope`] is one kind of ground cover, such as "pine forest floor":
//! Scatters of trees, stones, dead wood and mushrooms, each under a key and
//! a priority, laid over the same ground. It runs on any
//! [`Ground`](procgena_core::Ground), on its own for now:
//!
//! ```
//! use glam::{DVec2, Vec2};
//! use procgena_biome::EcotopePreset;
//! use procgena_core::FlatGround;
//! use procgena_core::scatter::{ScatterArea, ScatterInput};
//!
//! let preset = EcotopePreset::pine_forest_floor();
//! let ground = FlatGround;
//! let area = ScatterArea::new(DVec2::ZERO, Vec2::splat(20.0));
//! let input = ScatterInput::new(&ground, area, preset.ecotope.default_seed);
//! let placed = preset.ecotope.place(&input)?;
//! for layer in placed.layers() {
//!     println!("{}: {} copies", layer.key(), layer.copies().len());
//! }
//! # Ok::<(), procgena_biome::EcotopeError>(())
//! ```
//!
//! ```
//! use procgena_biome::BiomePreset;
//! use procgena_terrain::{Boundary, HeightField, Region, RegionId};
//! use glam::DVec2;
//!
//! let preset = BiomePreset::named("rolling_hills").expect("built-in preset");
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
mod ecotope;
mod presets;

pub use biome::{Biome, BiomeError, BiomeTerrain, EcotopeRule, terrain_seed};
pub use ecotope::{
    Ecotope, EcotopeError, EcotopePlacement, EcotopePreset, EcotopeScatter, FootprintMask,
    Footprints, PlacedScatter, SCATTER_SEED_KIND, ScatterCopies, ScatterFailure, ScatterKind,
    scatter_seed,
};
pub use ecotope::{EcotopeGround, GROUND_SEED_KIND};
pub use presets::BiomePreset;
