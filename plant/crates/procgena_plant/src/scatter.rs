//! A Scatter's settings: what grows, how densely, and the look the plants
//! share, without the ground they grow on.

use std::borrow::Cow;

#[cfg(feature = "reflect")]
use bevy_reflect::{Reflect, std_traits::ReflectDefault};
use serde::{Deserialize, Serialize};

use crate::files::PlantFileError;
use crate::look::{GroundTint, MAX_PLANTS, PlantVariation, ScatterPlant};
use crate::presets::PlantScatterPreset;

/// The settings of one plant Scatter: the plants that grow, their density,
/// and the look they share.
///
/// A Scatter's size, ground and density mask are not settings: whatever
/// places it gives it those. In Bevy, `procgena_plant_bevy`'s `PlantScatter`
/// component holds these settings with a size and a ground.
///
/// Start from a preset, such as `PlantScatterPreset::MEADOW.plant_scatter`,
/// or from [`PlantScatterOptions::default`], which is the meadow.
///
/// With serde, the settings read and write the same text as the
/// `plant_scatter:` part of a `.plant_scatter.yml` file. Settings that are
/// missing come from [`PlantScatterOptions::default`], and a value out of
/// range is an error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "reflect",
    derive(Reflect),
    reflect(Default, Debug, PartialEq)
)]
#[serde(
    try_from = "crate::files::PlantScatterFile",
    into = "crate::files::PlantScatterFile"
)]
pub struct PlantScatterOptions {
    /// The seed used when this Scatter is built on its own, rather than
    /// inside a larger level that gives it a seed.
    ///
    /// Not used yet: plants are placed from their position alone, so
    /// changing it changes nothing that is drawn.
    pub default_seed: u32,
    /// Scales wind movement for every plant: 0 is still, 0.3 is sheltered,
    /// and values above 1 give stronger movement. Negative values act as 0.
    /// Gusts are shared, so neighbouring Scatters sway in step even with
    /// different strengths.
    pub wind_scale: f32,
    /// The plants that grow here, each at its own density times its
    /// [`density_scale`](ScatterPlant::density_scale). See [`ScatterPlant`] for
    /// how they mix. An empty list, or one whose densities are all 0, draws
    /// nothing. Only the first [`MAX_PLANTS`] entries are used.
    ///
    /// Build a list with [`PlantScatterOptions::with_plants`], or in a
    /// constant with `Cow::Borrowed(&[...])`.
    pub plants: Cow<'static, [ScatterPlant]>,
    /// Multiplies every plant entry's density, keeping the same proportions of plants.
    ///
    /// Use 1 to keep each entry's [`ScatterPlant::density`], 0.5 for half as
    /// many plants, or 0 for an empty Scatter. Negative values act as 0.
    /// Each entry's own scale still applies. See
    /// [`PlantScatterOptions::density`] for the total plants per square metre.
    pub density_scale: f32,
    /// How the Scatter varies across its area.
    pub variation: PlantVariation,
    /// Colour blended into the base of the plants to match the terrain.
    /// This changes the plants' shading, not the ground material.
    pub ground_tint: GroundTint,
    /// Flat toon shading with this many bands of light; 0 shades physically,
    /// like the rest of the scene. Forward rendering only: a deferred view
    /// shades every Scatter physically.
    pub toon_bands: u32,
}

impl Default for PlantScatterOptions {
    /// The [`PlantScatterPreset::MEADOW`] Scatter.
    fn default() -> Self {
        PlantScatterPreset::MEADOW.plant_scatter
    }
}

impl PlantScatterOptions {
    /// Returns the total plants per square metre before masks and thinning.
    ///
    /// Includes each entry's density scale and the Scatter's
    /// [`density_scale`](Self::density_scale). Only the first [`MAX_PLANTS`]
    /// entries contribute. Density masks, edits and distance can reduce
    /// the visible count. A plant with several stems counts once.
    #[must_use]
    pub fn density(&self) -> f32 {
        self.unscaled_density() * self.density_scale.max(0.0)
    }

    /// Returns approximate spacing between plant roots.
    ///
    /// The result is in metres: `1 / sqrt(density)`, using
    /// [`PlantScatterOptions::density`] before masks and thinning. Individual
    /// gaps vary with tufting and density masks. An empty Scatter returns a
    /// very large finite value rather than infinity.
    #[must_use]
    pub fn spacing(&self) -> f32 {
        1.0 / self.density().max(f32::MIN_POSITIVE).sqrt()
    }

    /// Sets approximate spacing between plant roots.
    ///
    /// Pass a positive distance in metres. For example, 0.5 metres gives
    /// four plants per square metre before masks and thinning. Plant
    /// proportions stay the same. A Scatter with no positive-density entries
    /// is unchanged. Negative distances act like their positive magnitude;
    /// zero requests a very large finite density.
    #[must_use]
    pub fn with_spacing(self, metres: f32) -> Self {
        self.with_density(1.0 / (metres * metres).max(f32::MIN_POSITIVE))
    }

    /// Sets the total density while keeping the same proportions of plants.
    ///
    /// Pass the desired plants per square metre before masks and thinning.
    /// This adjusts [`density_scale`](Self::density_scale); each entry's
    /// settings stay unchanged. Negative values become 0. If none of the
    /// first [`MAX_PLANTS`] entries has a positive density, the Scatter is
    /// returned unchanged.
    #[must_use]
    pub fn with_density(self, plants_per_m2: f32) -> Self {
        let unscaled = self.unscaled_density();
        if unscaled <= 0.0 {
            return self;
        }
        Self {
            density_scale: plants_per_m2.max(0.0) / unscaled,
            ..self
        }
    }

    /// Replaces the plants that grow in this Scatter.
    ///
    /// See [`ScatterPlant`] for how their densities mix.
    #[must_use]
    pub fn with_plants(self, plants: impl Into<Cow<'static, [ScatterPlant]>>) -> Self {
        Self {
            plants: plants.into(),
            ..self
        }
    }

    /// Checks every setting, including each plant's, as a
    /// `.plant_scatter.yml` file's are checked.
    ///
    /// # Errors
    ///
    /// Returns [`PlantFileError::Invalid`] naming the first value out of
    /// range, or a Scatter with more than [`MAX_PLANTS`] plants or none with
    /// a density above 0.
    pub fn validate(&self) -> Result<(), PlantFileError> {
        crate::files::validate_scatter(self)
    }

    /// The summed density of the first [`MAX_PLANTS`] entries, before the
    /// Scatter's own scale.
    fn unscaled_density(&self) -> f32 {
        self.plants
            .iter()
            .take(MAX_PLANTS)
            .map(ScatterPlant::density)
            .sum()
    }
}
