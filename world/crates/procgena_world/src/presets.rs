//! Named starting points for [`WorldOptions`], one per scale class.
//!
//! [`WorldPreset::all`] lists the built-in presets from the smallest class
//! to the largest, and [`WorldPreset::named`] looks one up by its key. A
//! `.world.yml` file loads as a preset with [`WorldPreset::from_yaml`].

use std::borrow::Cow;

use procgena_terrain::{BorderWarp, RegionGraphOptions};

use crate::options::{WorldOptions, WorldScale};

/// A named World with tuned settings.
///
/// The built-in presets are the scale classes, from Minecraft-like small
/// worlds to real-life ones. They share the same elevation, selector
/// fields and Biome rules at reference scale, and differ in their
/// [`scale`](WorldOptions::scale), region size and border warp. Reference
/// scale 1 is the Valheim-like medium-small class, which is also
/// [`WorldOptions::default`]. Terrain presets are not stretched by the
/// scale yet, so the larger classes build Valheim-sized relief on their
/// larger regions. Their numbers are first drafts, to be tuned. Built-in
/// presets use seed 0.
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
            description: "Minecraft-like: land at half of reference scale, \
                with the smallest regions today's terrain allows (2 km).",
        } => class(0.5, 0.5, 2000.0, BorderWarp::default());
        medium_small {
            name: "Medium-small",
            description: "Valheim-like: reference scale, with 2 km regions. The default.",
        } => class(1.0, 1.0, 2000.0, BorderWarp::default());
        medium {
            name: "Medium",
            description: "Skyrim-like: twice reference scale, with 4 km regions.",
        } => class(2.0, 2.0, 4000.0, WIDE_WARP);
        large {
            name: "Large",
            description: "The largest open worlds: six times as wide and four times as high \
                as reference scale, with 8 km regions.",
        } => class(6.0, 4.0, 8000.0, WIDE_WARP);
        real_life {
            name: "Real life",
            description: "Earth-like: twenty times as wide and eight times as high as reference \
                scale, with 16 km regions. A stretch goal: rivers don't grow across regions yet.",
        } => class(20.0, 8.0, 16_000.0, WIDE_WARP);
    }
}

/// The border warp of the classes with regions of 4 km and more: up to
/// 100 m over 800 m wiggles.
const WIDE_WARP: BorderWarp = BorderWarp {
    amplitude: 100.0,
    wavelength: 800.0,
};

/// The default World at scale `horizontal` × `vertical`, with regions
/// `size` metres across whose borders are warped by `warp`.
fn class(horizontal: f64, vertical: f64, size: f64, warp: BorderWarp) -> WorldOptions {
    WorldOptions {
        scale: WorldScale {
            horizontal,
            vertical,
        },
        regions: RegionGraphOptions {
            size,
            warp,
            ..RegionGraphOptions::default()
        },
        ..WorldOptions::default()
    }
}
