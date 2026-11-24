//! Named starting points for [`WorldOptions`], one per scale class.
//!
//! [`WorldPreset::all`] lists the built-in presets from the smallest class
//! to the largest, and [`WorldPreset::named`] looks one up by its key. A
//! `.world.yml` file loads as a preset with [`WorldPreset::from_yaml`].

use std::borrow::Cow;

use procgena_terrain::RegionGraphOptions;

use crate::options::{WorldOptions, WorldScale};

/// A named World with tuned settings.
///
/// The built-in presets are the scale classes, from Minecraft-like small
/// worlds to real-life ones. They share the same elevation, selector
/// fields and Biome rules at reference scale, and differ in their
/// [`scale`](WorldOptions::scale) and region size. Their numbers are first
/// drafts, to be tuned. Built-in presets use seed 0.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldPreset {
    /// The preset's key, such as `"medium"`, or a loaded file's name.
    pub key: Cow<'static, str>,
    /// A short display name, such as `"Medium"`.
    pub name: Cow<'static, str>,
    /// What the World is like, in a sentence or two.
    pub description: Cow<'static, str>,
    /// The group it is listed under, such as `"Scale classes"`, or
    /// `"File"` for a loaded file.
    pub group: Cow<'static, str>,
    /// The World's settings.
    pub world: WorldOptions,
}

procgena_core::catalogue! {
    WorldPreset {
        world: WorldOptions,
        check: |options| options.validate().map_err(|error| error.to_string()),
    }
    "Scale classes" {
        small {
            name: "Small",
            description: "Minecraft-like: land at a quarter of reference scale, \
                with the smallest regions today's terrain allows (2 km).",
        } => class(0.25, 0.25, 2000.0);
        medium_small {
            name: "Medium-small",
            description: "Valheim-like: land at half of reference scale, with 2 km regions.",
        } => class(0.5, 0.5, 2000.0);
        medium {
            name: "Medium",
            description: "Skyrim-like: reference scale, with 4 km regions.",
        } => class(1.0, 1.0, 4000.0);
        large {
            name: "Large",
            description: "The largest open worlds: three times as wide and twice as high \
                as reference scale, with 8 km regions.",
        } => class(3.0, 2.0, 8000.0);
        real_life {
            name: "Real life",
            description: "Earth-like: ten times as wide and four times as high as reference \
                scale, with 16 km regions. A stretch goal: rivers don't grow across regions yet.",
        } => class(10.0, 4.0, 16_000.0);
    }
}

/// The default World at scale `horizontal` × `vertical`, with regions
/// `size` metres across.
fn class(horizontal: f64, vertical: f64, size: f64) -> WorldOptions {
    WorldOptions {
        scale: WorldScale {
            horizontal,
            vertical,
        },
        regions: RegionGraphOptions {
            size,
            ..RegionGraphOptions::default()
        },
        ..WorldOptions::default()
    }
}
