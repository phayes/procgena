//! Named starting points for [`TerrainOptions`].
//!
//! [`TerrainPreset::all`] lists the built-in presets in display order, and
//! [`TerrainPreset::named`] looks one up by its key. A `.terrain.yml` file
//! loads as a preset with [`TerrainPreset::from_yaml`].
//!
//! The built-in presets have no rivers and no roads: their
//! `features.rivers.density` is 0 and `features.roads.enabled` is off.
//! Their other river and road settings are tuned, so switching those two
//! back on gives rivers and roads that suit the land.

use std::borrow::Cow;

use crate::options::{
    BaseOptions, EdgeOptions, FeatureOptions, MountainLayer, NoiseLayer, RiverOptions, RoadOptions,
    TerrainOptions,
};

/// A named terrain type with tuned settings.
///
/// Every built-in preset has a function of its own, such as
/// [`TerrainPreset::plains`]. Clone its [`terrain`](Self::terrain) options
/// to customise them. Built-in presets use
/// [`default_seed`](TerrainOptions::default_seed) 0.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainPreset {
    /// The preset's key, such as `"rolling_hills"`, or a loaded file's name.
    pub key: Cow<'static, str>,
    /// A short display name, such as `"Rolling hills"`.
    pub name: Cow<'static, str>,
    /// What the land looks like, in a sentence or two.
    pub description: Cow<'static, str>,
    /// The group it is listed under, such as `"Land"`, or `"File"` for a
    /// loaded file.
    pub group: Cow<'static, str>,
    /// The terrain's settings.
    pub terrain: TerrainOptions,
}

procgena_core::catalogue! {
    TerrainPreset {
        terrain: TerrainOptions,
        check: |options| options.validate().map_err(|error| error.to_string()),
    }
    "Land" {
        plains {
            name: "Plains",
            description: "Nearly flat land with slow swells and small bumps.",
        } => plains();
        rolling_hills {
            name: "Rolling hills",
            description: "Hills ten to twenty metres high, with worn slopes.",
        } => rolling_hills();
        mountains {
            name: "Mountains",
            description: "Ridged mountain ranges up to about 500 metres above their foothills.",
        } => mountains();
    }
}

/// A mountain layer that adds nothing, with ridges `wavelength` metres
/// apart should its height be raised.
pub(crate) fn no_mountains(wavelength: f32) -> MountainLayer {
    MountainLayer {
        height: 0.0,
        wavelength,
        coverage: 0.0,
        octaves: 6,
    }
}

fn plains() -> TerrainOptions {
    TerrainOptions {
        default_seed: 0,
        base: BaseOptions {
            elevation: 10.0,
            swells: NoiseLayer {
                height: 3.0,
                wavelength: 1000.0,
                octaves: 3,
                roughness: 0.4,
            },
            hills: NoiseLayer {
                height: 0.75,
                wavelength: 125.0,
                octaves: 5,
                roughness: 0.5,
            },
            mountains: no_mountains(750.0),
            erosion: 0.3,
        },
        edges: EdgeOptions {
            stiffness: 0.5,
            margin: 125.0,
            roughness: 0.25,
        },
        features: switched_off(features(0.6, 8.0, 250.0, 300.0, 0.06)),
    }
}

fn rolling_hills() -> TerrainOptions {
    TerrainOptions {
        default_seed: 0,
        base: BaseOptions {
            elevation: 25.0,
            swells: NoiseLayer {
                height: 7.5,
                wavelength: 750.0,
                octaves: 3,
                roughness: 0.45,
            },
            hills: NoiseLayer {
                height: 12.5,
                wavelength: 225.0,
                octaves: 6,
                roughness: 0.5,
            },
            mountains: no_mountains(750.0),
            erosion: 1.0,
        },
        edges: EdgeOptions {
            stiffness: 1.0,
            margin: 125.0,
            roughness: 1.0,
        },
        features: switched_off(features(1.0, 5.0, 250.0, 200.0, 0.1)),
    }
}

fn mountains() -> TerrainOptions {
    TerrainOptions {
        default_seed: 0,
        base: BaseOptions {
            elevation: 60.0,
            swells: NoiseLayer {
                height: 20.0,
                wavelength: 1500.0,
                octaves: 3,
                roughness: 0.45,
            },
            hills: NoiseLayer {
                height: 15.0,
                wavelength: 250.0,
                octaves: 6,
                roughness: 0.5,
            },
            mountains: MountainLayer {
                height: 350.0,
                wavelength: 900.0,
                coverage: 0.65,
                octaves: 7,
            },
            erosion: 2.0,
        },
        edges: EdgeOptions {
            stiffness: 3.0,
            margin: 250.0,
            roughness: 2.5,
        },
        features: switched_off(features(1.2, 4.0, 250.0, 150.0, 0.14)),
    }
}

/// River and road settings: sources per km², river width (m), least
/// distance between sources (m), shortest river kept (m), and the steepest
/// comfortable road grade. Roads may climb up to twice that grade, but
/// never more than 0.25.
pub(crate) fn features(
    density: f32,
    width: f32,
    spacing: f32,
    min_length: f32,
    grade: f32,
) -> FeatureOptions {
    FeatureOptions {
        rivers: RiverOptions {
            density,
            width,
            spacing,
            min_length,
            ..RiverOptions::default()
        },
        roads: RoadOptions {
            grade,
            max_grade: (2.0 * grade).min(0.25).max(grade),
            ..RoadOptions::default()
        },
    }
}

/// `features` with no rivers starting in the region and no roads. The other
/// river and road settings are kept, so raising
/// `features.rivers.density` and setting `features.roads.enabled` brings
/// them back as tuned.
fn switched_off(mut features: FeatureOptions) -> FeatureOptions {
    features.rivers.density = 0.0;
    features.roads.enabled = false;
    features
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn presets_are_valid_and_listed_once() {
        let mut keys = HashSet::new();
        for preset in TerrainPreset::all() {
            assert_eq!(preset.terrain.validate(), Ok(()), "{}", preset.name);
            assert!(keys.insert(&preset.key), "{} is listed twice", preset.key);
        }
        let keys: Vec<_> = TerrainPreset::all().map(|p| p.key.as_ref()).collect();
        assert_eq!(keys, ["plains", "rolling_hills", "mountains"]);
    }

    /// Checks that every preset survives a trip through a `.terrain.yml`
    /// file, written against no base and against another preset.
    #[test]
    fn every_preset_reads_back_from_its_file() {
        for preset in TerrainPreset::all() {
            let text = preset.to_yaml(None);
            let read = TerrainPreset::from_yaml(&preset.key, &text).unwrap();
            assert_eq!(read.terrain, preset.terrain, "{}", preset.key);
            assert_eq!(read.name, preset.name);
            let text = preset.to_yaml(Some(TerrainPreset::mountains()));
            let read = TerrainPreset::from_yaml(&preset.key, &text).unwrap();
            assert_eq!(read.terrain, preset.terrain, "{}", preset.key);
        }
    }
}
