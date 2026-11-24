//! Named starting points for [`TerrainOptions`].
//!
//! [`TerrainPreset::all`] lists the built-in presets in display order, and
//! [`TerrainPreset::named`] looks one up by its key. A `.terrain.yml` file
//! loads as a preset with [`TerrainPreset::from_yaml`].

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
            description: "Hills a few tens of metres high, with worn slopes.",
        } => rolling_hills();
        mountains {
            name: "Mountains",
            description: "Ridged mountain ranges up to about a kilometre above their foothills.",
        } => mountains();
    }
}

/// A mountain layer that adds nothing.
fn no_mountains() -> MountainLayer {
    MountainLayer {
        height: 0.0,
        wavelength: 1500.0,
        coverage: 0.0,
        octaves: 6,
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
            mountains: no_mountains(),
            erosion: 0.3,
        },
        edges: EdgeOptions {
            stiffness: 0.5,
            margin: 250.0,
            roughness: 0.5,
        },
        features: features(0.3, 8.0, 600.0, 0.06),
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
            mountains: no_mountains(),
            erosion: 1.0,
        },
        edges: EdgeOptions {
            stiffness: 1.0,
            margin: 250.0,
            roughness: 2.0,
        },
        features: features(0.5, 5.0, 400.0, 0.1),
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
            roughness: 5.0,
        },
        features: features(0.6, 4.0, 300.0, 0.14),
    }
}

/// River and road settings: sources per km², river width (m), shortest
/// river kept (m), and the steepest comfortable road grade. Roads may
/// climb up to twice that grade, but never more than 0.25.
fn features(density: f32, width: f32, min_length: f32, grade: f32) -> FeatureOptions {
    FeatureOptions {
        rivers: RiverOptions {
            density,
            width,
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
