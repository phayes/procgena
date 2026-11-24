//! Grass and foliage settings, with no game engine: the plants, the Scatters
//! that mix them, the built-in presets, and their `.plant.yml` and
//! `.plant_scatter.yml` files.
//!
//! A [`PlantScatterOptions`] says what grows in one Scatter: its plants,
//! each a [`PlantParams`] at its own density and scaled by a
//! [`ScatterPlant`], and the look they share. Start from a
//! [`PlantScatterPreset`] or build one from [`PlantPreset`]s:
//!
//! ```
//! use procgena_plant::{PlantParams, PlantScatterOptions, PlantScatterPreset, ScatterPlant};
//!
//! let meadow = PlantScatterPreset::MEADOW.plant_scatter.with_density(40.0);
//! let ferns = PlantScatterOptions::default().with_plants(vec![
//!     ScatterPlant::new(PlantParams::FERN).scaled(0.5).clustered(0.5),
//!     ScatterPlant::new(PlantParams::MOSS),
//! ]);
//! assert!(meadow.validate().is_ok() && ferns.validate().is_ok());
//! ```
//!
//! Grass is drawn by `procgena_plant_bevy`, which places the blades on the
//! GPU. Turn on the `reflect` feature for Bevy's `Reflect` on every
//! settings type, so scenes and inspectors can read and edit them.

mod bake;
mod files;
mod look;
mod presets;
mod scatter;

pub use bake::{MAX_BAKE_TEXELS, PlantBake};
pub use files::{PlantFileError, PlantSettingsError};
pub use look::{
    GroundTint, MAX_BRANCHES, MAX_PLANTS, MAX_STEMS_PER_PLANT, PlantAccent, PlantArrangement,
    PlantBranches, PlantParams, PlantSegment, PlantVariation, ScatterPlant, Srgb,
};
pub use presets::{PlantPreset, PlantScatterPreset};
/// Why a preset file's text could not be read, as held by
/// [`PlantFileError::File`].
pub use procgena_core::preset::PresetFileError;
pub use scatter::PlantScatterOptions;
