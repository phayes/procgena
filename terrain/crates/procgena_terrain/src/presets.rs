//! Named starting points for [`TerrainOptions`].

use crate::options::{
    BaseOptions, EdgeOptions, FeatureOptions, MountainLayer, NoiseLayer, RiverOptions, RoadOptions,
    TerrainOptions,
};

/// A named terrain type with tuned settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TerrainPreset {
    /// Nearly flat land with slow swells and small bumps.
    Plains,
    /// Hills a few tens of metres high, with worn slopes.
    RollingHills,
    /// Ridged mountain ranges up to about a kilometre above their foothills.
    Mountains,
}

impl TerrainPreset {
    /// Every preset, in display order.
    pub const ALL: [TerrainPreset; 3] = [Self::Plains, Self::RollingHills, Self::Mountains];

    /// The preset's display name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Plains => "Plains",
            Self::RollingHills => "Rolling hills",
            Self::Mountains => "Mountains",
        }
    }

    /// The preset's settings, with seed 0.
    pub fn options(self) -> TerrainOptions {
        let none = MountainLayer {
            height: 0.0,
            wavelength: 1500.0,
            coverage: 0.0,
            octaves: 6,
        };
        match self {
            Self::Plains => TerrainOptions {
                seed: 0,
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
                    mountains: none,
                    erosion: 0.3,
                },
                edges: EdgeOptions {
                    stiffness: 0.5,
                    margin: 250.0,
                    roughness: 0.5,
                },
                features: features(0.3, 8.0, 600.0, 0.06),
            },
            Self::RollingHills => TerrainOptions {
                seed: 0,
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
                    mountains: none,
                    erosion: 1.0,
                },
                edges: EdgeOptions {
                    stiffness: 1.0,
                    margin: 250.0,
                    roughness: 2.0,
                },
                features: features(0.5, 5.0, 400.0, 0.1),
            },
            Self::Mountains => TerrainOptions {
                seed: 0,
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
            },
        }
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
    use super::*;

    #[test]
    fn presets_are_valid() {
        for preset in TerrainPreset::ALL {
            assert_eq!(preset.options().validate(), Ok(()), "{}", preset.name());
        }
    }
}
