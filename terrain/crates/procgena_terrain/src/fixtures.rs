//! Test fixtures: the medium-scale (Skyrim-like) settings that were the
//! defaults before the shipped presets became Valheim-sized.
//!
//! Tests and examples use them so that their recorded figures keep the
//! meaning they were measured with. They are not presets: they are not in
//! [`TerrainPreset::all`] and the studio doesn't list them. They may be
//! removed once no test needs them.

use std::borrow::Cow;
use std::sync::LazyLock;

use glam::DVec2;

use crate::elevation::ElevationOptions;
use crate::graph::{MIN_EDGE, RegionGraphOptions};
use crate::options::{BaseOptions, EdgeOptions, MountainLayer, NoiseLayer, TerrainOptions};
use crate::presets::{TerrainPreset, features, no_mountains};
use crate::scale::WorldScale;
use crate::warp::BorderWarp;
use crate::world::WorldTerrainOptions;

/// The medium-scale plains, rolling hills and mountains, in that order.
static PRESETS: LazyLock<[TerrainPreset; 3]> = LazyLock::new(|| {
    let preset = |key, name, description, terrain| TerrainPreset {
        key: Cow::Borrowed(key),
        name: Cow::Borrowed(name),
        description: Cow::Borrowed(description),
        group: Cow::Borrowed("Test fixtures"),
        terrain,
    };
    [
        preset(
            "plains_medium",
            "Plains (medium)",
            "Nearly flat land with slow swells and small bumps, at medium scale.",
            plains(),
        ),
        preset(
            "rolling_hills_medium",
            "Rolling hills (medium)",
            "Hills a few tens of metres high, with worn slopes, at medium scale.",
            rolling_hills(),
        ),
        preset(
            "mountains_medium",
            "Mountains (medium)",
            "Ridged mountain ranges up to about a kilometre above their foothills, \
                at medium scale.",
            mountains(),
        ),
    ]
});

/// Every fixture preset: plains, rolling hills and mountains at medium
/// scale.
pub fn all() -> impl Iterator<Item = &'static TerrainPreset> {
    PRESETS.iter()
}

/// Plains at medium scale: 20 m up, 6 m swells 2 km apart.
pub fn plains_medium() -> &'static TerrainPreset {
    &PRESETS[0]
}

/// Rolling hills at medium scale: 25 m hills 450 m apart.
pub fn rolling_hills_medium() -> &'static TerrainPreset {
    &PRESETS[1]
}

/// Mountains at medium scale: 900 m ridges 1.8 km apart.
pub fn mountains_medium() -> &'static TerrainPreset {
    &PRESETS[2]
}

/// Medium-scale region layout: 4 km regions with borders warped by up to
/// 100 m over 800 m wiggles.
pub fn regions_medium() -> RegionGraphOptions {
    RegionGraphOptions {
        size: 4000.0,
        jitter: 0.6,
        min_edge: MIN_EDGE,
        warp: warp_medium(),
    }
}

/// Medium-scale border warp: up to 100 m over 800 m wiggles.
pub fn warp_medium() -> BorderWarp {
    BorderWarp {
        amplitude: 100.0,
        wavelength: 800.0,
    }
}

/// Medium-scale world elevation: 400 m continents 150 km apart and 120 m
/// uplands 40 km apart, no tilt, no bumps.
pub fn elevation_medium() -> ElevationOptions {
    ElevationOptions {
        continents: NoiseLayer {
            height: 400.0,
            wavelength: 150_000.0,
            octaves: 2,
            roughness: 0.4,
        },
        uplift: NoiseLayer {
            height: 120.0,
            wavelength: 40_000.0,
            octaves: 2,
            roughness: 0.4,
        },
        tilt: DVec2::ZERO,
        max_slope: 0.05,
        bumps: Vec::new(),
    }
}

/// A medium-scale [`WorldTerrainOptions`]: [`regions_medium`] and
/// [`elevation_medium`] at the medium [`WorldScale`] of 2 × 2, with every
/// other setting at its default.
pub fn world_medium() -> WorldTerrainOptions {
    WorldTerrainOptions {
        elevation: elevation_medium(),
        regions: regions_medium(),
        scale: WorldScale {
            horizontal: 2.0,
            vertical: 2.0,
        },
        ..WorldTerrainOptions::default()
    }
}

fn plains() -> TerrainOptions {
    TerrainOptions {
        default_seed: 0,
        base: BaseOptions {
            elevation: 20.0,
            swells: NoiseLayer {
                height: 6.0,
                wavelength: 2000.0,
                octaves: 3,
                roughness: 0.4,
            },
            hills: NoiseLayer {
                height: 1.5,
                wavelength: 250.0,
                octaves: 5,
                roughness: 0.5,
            },
            mountains: no_mountains(1500.0),
            erosion: 0.3,
        },
        edges: EdgeOptions {
            stiffness: 0.5,
            margin: 250.0,
        },
        features: features(0.3, 8.0, 500.0, 600.0, 0.06),
    }
}

fn rolling_hills() -> TerrainOptions {
    TerrainOptions {
        default_seed: 0,
        base: BaseOptions {
            elevation: 50.0,
            swells: NoiseLayer {
                height: 15.0,
                wavelength: 1500.0,
                octaves: 3,
                roughness: 0.45,
            },
            hills: NoiseLayer {
                height: 25.0,
                wavelength: 450.0,
                octaves: 6,
                roughness: 0.5,
            },
            mountains: no_mountains(1500.0),
            erosion: 1.0,
        },
        edges: EdgeOptions {
            stiffness: 1.0,
            margin: 250.0,
        },
        features: features(0.5, 5.0, 500.0, 400.0, 0.1),
    }
}

fn mountains() -> TerrainOptions {
    TerrainOptions {
        default_seed: 0,
        base: BaseOptions {
            elevation: 120.0,
            swells: NoiseLayer {
                height: 40.0,
                wavelength: 3000.0,
                octaves: 3,
                roughness: 0.45,
            },
            hills: NoiseLayer {
                height: 30.0,
                wavelength: 500.0,
                octaves: 6,
                roughness: 0.5,
            },
            mountains: MountainLayer {
                height: 900.0,
                wavelength: 1800.0,
                coverage: 0.65,
                octaves: 7,
            },
            erosion: 2.0,
        },
        edges: EdgeOptions {
            stiffness: 3.0,
            margin: 500.0,
        },
        features: features(0.6, 4.0, 500.0, 300.0, 0.14),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_are_valid_and_unlisted() {
        for fixture in all() {
            assert_eq!(fixture.terrain.validate(), Ok(()), "{}", fixture.key);
            assert!(TerrainPreset::named(&fixture.key).is_none());
        }
        assert_eq!(regions_medium().validate(), Ok(()));
        assert_eq!(elevation_medium().validate(), Ok(()));
    }
}
