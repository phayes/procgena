//! Ready-made clump settings.

use procgena_mesh::{TagFilter, tags};

use crate::material::MaterialOptions;
use crate::options::{
    ClumpOptions, Direction, MaskOptions, MeshOptions, Placement, PlacementOptions, ShapeOptions,
};

/// A ready-made kind of clump. Start from [`options`](Self::options) and
/// change what you need, or pass a preset to
/// [`Clump::from_preset`](crate::Clump::from_preset).
///
/// Sizes suit rocks about one to three metres across. Change
/// [`ClumpOptions::scale`] for smaller or larger inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ClumpPreset {
    /// A freshly fallen snow cap about 12 cm deep on upward-facing surfaces
    /// that are open to the sky, plain pure white with no texture. On rough
    /// rocks it breaks into patches and flecks.
    FreshSnowCap,
    /// The same snow cap as [`FreshSnowCap`](Self::FreshSnowCap), but with a
    /// weathered, grimy snow texture.
    DirtySnowCap,
    /// A thin moss carpet, about 3 cm deep with small lumps, over tops and
    /// upper sides. Ignores how sheltered the surface is.
    MossPatches,
    /// A dozen rounded moss cushions, about 15 cm across, on tops and upper
    /// sides.
    MossCushions,
    /// A single knobbly burl, about 35 cm across, bulging out of the thick
    /// part of a trunk or rock on any side.
    Burl,
}

impl ClumpPreset {
    /// Every preset, in declaration order.
    pub const ALL: [ClumpPreset; 5] = [
        ClumpPreset::FreshSnowCap,
        ClumpPreset::DirtySnowCap,
        ClumpPreset::MossPatches,
        ClumpPreset::MossCushions,
        ClumpPreset::Burl,
    ];

    /// Returns the preset's name in snake case, such as `"fresh_snow_cap"`.
    pub fn name(self) -> &'static str {
        match self {
            Self::FreshSnowCap => "fresh_snow_cap",
            Self::DirtySnowCap => "dirty_snow_cap",
            Self::MossPatches => "moss_patches",
            Self::MossCushions => "moss_cushions",
            Self::Burl => "burl",
        }
    }

    /// Returns the preset with the given [`name`](Self::name), if any.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == name)
    }

    /// Returns the preset's settings.
    pub fn options(self) -> ClumpOptions {
        match self {
            Self::FreshSnowCap => ClumpOptions {
                seed: 0,
                directions: vec![Direction::DOWN],
                scale: 1.0,
                mask: MaskOptions {
                    any_facing: false,
                    facing_min: 0.3,
                    facing_full: 0.7,
                    exposure_rays: 64,
                    exposure_power: 1.0,
                },
                placement: PlacementOptions::default(),
                shape: ShapeOptions {
                    thickness: 0.12,
                    sink: 0.03,
                    max_slope: 1.0,
                    min_support: 0.0,
                    lumpiness: 0.3,
                    lump_size: 0.25,
                    neck: 0.0,
                },
                mesh: MeshOptions {
                    edge_length: 0.04,
                    closed: true,
                    strict: false,
                },
                grows_on: TagFilter::any_of([
                    tags::ROCK,
                    tags::BARK,
                    tags::WOOD,
                    tags::GROUND,
                    tags::MOSS,
                ])
                .excluding([tags::CARD]),
                tags: [tags::CLUMP, tags::SNOW].into(),
                material: MaterialOptions {
                    texture: None,
                    color: [1.0; 3],
                    roughness: 1.0,
                    texture_size: 1.0,
                },
            },
            Self::DirtySnowCap => {
                let mut options = Self::FreshSnowCap.options();
                options.material.texture = Some("snow_02".to_owned());
                options
            }
            Self::MossPatches => ClumpOptions {
                seed: 3,
                directions: vec![Direction::DOWN],
                scale: 1.0,
                mask: MaskOptions {
                    any_facing: false,
                    facing_min: 0.0,
                    facing_full: 0.6,
                    exposure_rays: 64,
                    exposure_power: 0.0,
                },
                placement: PlacementOptions::default(),
                shape: ShapeOptions {
                    thickness: 0.03,
                    sink: 0.03,
                    max_slope: 2.0,
                    min_support: 0.0,
                    lumpiness: 0.4,
                    lump_size: 0.08,
                    neck: 0.0,
                },
                mesh: MeshOptions {
                    edge_length: 0.02,
                    closed: true,
                    strict: false,
                },
                grows_on: TagFilter::any_of([tags::ROCK, tags::BARK, tags::WOOD, tags::GROUND])
                    .excluding([tags::CARD]),
                tags: [tags::CLUMP, tags::MOSS].into(),
                material: MaterialOptions {
                    texture: Some("moss_002".to_owned()),
                    color: [1.0; 3],
                    roughness: 1.0,
                    texture_size: 0.5,
                },
            },
            Self::MossCushions => ClumpOptions {
                seed: 5,
                directions: vec![Direction::DOWN],
                scale: 1.0,
                mask: MaskOptions {
                    any_facing: false,
                    facing_min: 0.0,
                    facing_full: 0.6,
                    exposure_rays: 0,
                    exposure_power: 0.0,
                },
                placement: PlacementOptions {
                    mode: Placement::Scatter,
                    count: 12,
                    radius: 0.08,
                    size_variation: 0.4,
                    min_spacing: 0.12,
                },
                shape: ShapeOptions {
                    thickness: 0.07,
                    sink: 0.02,
                    max_slope: 2.0,
                    min_support: 0.0,
                    lumpiness: 0.25,
                    lump_size: 0.03,
                    neck: 0.15,
                },
                mesh: MeshOptions {
                    edge_length: 0.02,
                    closed: true,
                    strict: false,
                },
                grows_on: TagFilter::any_of([tags::ROCK, tags::BARK, tags::WOOD, tags::GROUND])
                    .excluding([tags::CARD]),
                tags: [tags::CLUMP, tags::MOSS].into(),
                material: MaterialOptions {
                    texture: Some("moss_004".to_owned()),
                    color: [1.0; 3],
                    roughness: 1.0,
                    texture_size: 0.5,
                },
            },
            Self::Burl => ClumpOptions {
                seed: 1,
                directions: vec![Direction::DOWN],
                scale: 1.0,
                mask: MaskOptions {
                    any_facing: true,
                    facing_min: 0.3,
                    facing_full: 0.7,
                    exposure_rays: 0,
                    exposure_power: 0.0,
                },
                placement: PlacementOptions {
                    mode: Placement::Scatter,
                    count: 1,
                    radius: 0.18,
                    size_variation: 0.2,
                    min_spacing: 0.5,
                },
                shape: ShapeOptions {
                    thickness: 0.18,
                    sink: 0.03,
                    max_slope: 2.0,
                    min_support: 0.15,
                    lumpiness: 0.6,
                    lump_size: 0.06,
                    neck: 0.35,
                },
                mesh: MeshOptions {
                    edge_length: 0.03,
                    closed: true,
                    strict: false,
                },
                // A burl swells out of wood or stone thick enough to carry it.
                grows_on: TagFilter::any_of([tags::TRUNK, tags::LIMB, tags::ROCK]),
                tags: [tags::CLUMP, tags::BURL].into(),
                material: MaterialOptions {
                    texture: None,
                    color: [0.25, 0.17, 0.1],
                    roughness: 0.9,
                    texture_size: 1.0,
                },
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
    fn default_options_are_the_fresh_snow_cap() {
        assert_eq!(ClumpOptions::default(), ClumpPreset::FreshSnowCap.options());
    }

    #[test]
    fn fresh_snow_is_untextured_white_dirty_snow() {
        let fresh = ClumpPreset::FreshSnowCap.options();
        assert_eq!(fresh.material.texture, None);
        assert_eq!(fresh.material.color, [1.0; 3]);
        let mut dirty = ClumpPreset::DirtySnowCap.options();
        dirty.material.texture = None;
        assert_eq!(fresh, dirty);
    }
}
