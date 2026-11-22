//! Ready-made clump settings.

use crate::options::{ClumpOptions, Direction, MaskOptions, MeshOptions, ShapeOptions};

/// A ready-made kind of clump. Start from [`options`](Self::options) and
/// change what you need, or pass a preset to
/// [`Clump::from_preset`](crate::Clump::from_preset).
///
/// Sizes suit rocks about one to three metres across. Change
/// [`ClumpOptions::scale`] for smaller or larger inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ClumpPreset {
    /// A snow cap about 12 cm deep on upward-facing surfaces that are open to
    /// the sky. On rough rocks it breaks into patches and flecks.
    SnowCap,
    /// Thin moss, about 3 cm deep, in irregular patches over tops and upper
    /// sides. Ignores how open the sky is.
    MossPatches,
}

impl ClumpPreset {
    /// Every preset, in declaration order.
    pub const ALL: [ClumpPreset; 2] = [ClumpPreset::SnowCap, ClumpPreset::MossPatches];

    /// Returns the preset's name in snake case, such as `"snow_cap"`.
    pub fn name(self) -> &'static str {
        match self {
            Self::SnowCap => "snow_cap",
            Self::MossPatches => "moss_patches",
        }
    }

    /// Returns the preset with the given [`name`](Self::name), if any.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == name)
    }

    /// Returns the preset's settings.
    pub fn options(self) -> ClumpOptions {
        match self {
            Self::SnowCap => ClumpOptions {
                seed: 0,
                directions: vec![Direction::DOWN],
                scale: 1.0,
                mask: MaskOptions { facing_min: 0.3, facing_full: 0.7, sky_rays: 64, sky_power: 1.0 },
                shape: ShapeOptions {
                    at: None,
                    radius: None,
                    edge_width: 0.3,
                    thickness: 0.12,
                    sink: 0.03,
                    max_slope: 1.0,
                    lumpiness: 0.3,
                    lump_size: 0.25,
                },
                mesh: MeshOptions { edge_length: 0.04, closed: true },
            },
            Self::MossPatches => ClumpOptions {
                seed: 3,
                directions: vec![Direction::DOWN],
                scale: 1.0,
                mask: MaskOptions { facing_min: 0.0, facing_full: 0.6, sky_rays: 64, sky_power: 0.0 },
                shape: ShapeOptions {
                    at: None,
                    radius: None,
                    edge_width: 0.3,
                    thickness: 0.03,
                    sink: 0.03,
                    max_slope: 2.0,
                    lumpiness: 1.0,
                    lump_size: 0.2,
                },
                mesh: MeshOptions { edge_length: 0.02, closed: true },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_valid_and_named() {
        for preset in ClumpPreset::ALL {
            preset.options().validate().unwrap();
            assert_eq!(ClumpPreset::from_name(preset.name()), Some(preset));
        }
        assert_eq!(ClumpPreset::from_name("glacier"), None);
    }

    #[test]
    fn default_options_are_the_snow_cap() {
        assert_eq!(ClumpOptions::default(), ClumpPreset::SnowCap.options());
    }
}
