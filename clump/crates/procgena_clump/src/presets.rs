//! Ready-made clump settings.
//!
//! [`ClumpPreset::all`] lists the built-in presets in menu order, and
//! [`ClumpPreset::named`] looks one up by its key. A `.clump.yml` file loads
//! as a preset with [`ClumpPreset::from_yaml`].

use std::borrow::Cow;

use procgena_specimen::{TagFilter, tags};

use crate::material::MaterialOptions;
use crate::options::{
    ClumpOptions, Direction, MaskOptions, MeshOptions, Placement, PlacementOptions, ShapeOptions,
};

/// A named, ready-made kind of clump, such as a snow cap or moss cushions.
///
/// Every built-in preset has a function of its own, such as
/// [`ClumpPreset::moss_cushions`]. Clone its [`clump`](Self::clump) options
/// and change what you need, or pass the preset to
/// [`Clump::from_preset`](crate::Clump::from_preset).
///
/// Sizes suit rocks about one to three metres across. Change
/// [`ClumpOptions::scale`] for smaller or larger inputs.
#[derive(Debug, Clone, PartialEq)]
pub struct ClumpPreset {
    /// The preset's key, such as `"fresh_snow_cap"`, or a loaded file's name.
    pub key: Cow<'static, str>,
    /// A short display name, such as `"Fresh Snow Cap"`.
    pub name: Cow<'static, str>,
    /// What the clump looks like, in a sentence or two.
    pub description: Cow<'static, str>,
    /// The group it is listed under, such as `"Snow"`, or `"File"` for a
    /// loaded file.
    pub group: Cow<'static, str>,
    /// The clump's settings.
    pub clump: ClumpOptions,
}

procgena_core::catalogue! {
    ClumpPreset {
        clump: ClumpOptions,
        check: |options| options.validate().map_err(|error| error.to_string()),
    }
    "Snow" {
        fresh_snow_cap {
            name: "Fresh Snow Cap",
            description: "A freshly fallen snow cap about 12 cm deep on upward-facing surfaces \
                that are open to the sky, plain pure white with no texture. On rough rocks it \
                breaks into patches and flecks.",
        } => fresh_snow_cap();
        dirty_snow_cap {
            name: "Dirty Snow Cap",
            description: "The same snow cap as Fresh Snow Cap, but with a weathered, grimy \
                snow texture.",
        } => dirty_snow_cap();
    }
    "Moss" {
        moss_patches {
            name: "Moss Patches",
            description: "A thin moss carpet, about 3 cm deep with small lumps, over tops and \
                upper sides. Ignores how sheltered the surface is.",
        } => moss_patches();
        moss_cushions {
            name: "Moss Cushions",
            description: "A dozen rounded moss cushions, about 15 cm across, on tops and upper \
                sides.",
        } => moss_cushions();
    }
    "Burl" {
        burl {
            name: "Burl",
            description: "A single knobbly burl, about 35 cm across, bulging out of the thick \
                part of a trunk or rock on any side.",
        } => burl();
    }
}

fn fresh_snow_cap() -> ClumpOptions {
    ClumpOptions {
        default_seed: 0,
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
        grows_on: TagFilter::any_of([tags::ROCK, tags::BARK, tags::WOOD, tags::GROUND, tags::MOSS])
            .excluding([tags::CARD]),
        tags: [tags::CLUMP, tags::SNOW].into(),
        material: MaterialOptions {
            texture: None,
            color: [1.0; 3],
            roughness: 1.0,
            texture_size: 1.0,
        },
    }
}

fn dirty_snow_cap() -> ClumpOptions {
    let mut options = fresh_snow_cap();
    options.material.texture = Some("snow_02".to_owned());
    options
}

fn moss_patches() -> ClumpOptions {
    ClumpOptions {
        default_seed: 3,
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
    }
}

fn moss_cushions() -> ClumpOptions {
    ClumpOptions {
        default_seed: 5,
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
    }
}

fn burl() -> ClumpOptions {
    ClumpOptions {
        default_seed: 1,
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_valid_and_named() {
        let keys: Vec<&str> = ClumpPreset::all().map(|p| p.key.as_ref()).collect();
        assert_eq!(
            keys,
            [
                "fresh_snow_cap",
                "dirty_snow_cap",
                "moss_patches",
                "moss_cushions",
                "burl"
            ]
        );
        for preset in ClumpPreset::all() {
            preset.clump.validate().unwrap();
            assert_eq!(ClumpPreset::named(&preset.key), Some(preset));
        }
        assert_eq!(ClumpPreset::named("glacier"), None);
    }

    #[test]
    fn default_options_are_the_fresh_snow_cap() {
        assert_eq!(ClumpOptions::default(), ClumpPreset::fresh_snow_cap().clump);
    }

    #[test]
    fn fresh_snow_is_untextured_white_dirty_snow() {
        let fresh = &ClumpPreset::fresh_snow_cap().clump;
        assert_eq!(fresh.material.texture, None);
        assert_eq!(fresh.material.color, [1.0; 3]);
        let mut dirty = ClumpPreset::dirty_snow_cap().clump.clone();
        dirty.material.texture = None;
        assert_eq!(*fresh, dirty);
    }

    #[test]
    fn every_preset_reads_back_from_its_file() {
        let other = ClumpPreset::burl();
        for preset in ClumpPreset::all() {
            let text = preset.to_yaml(None);
            assert_eq!(
                ClumpPreset::from_yaml(&preset.key, &text).unwrap().clump,
                preset.clump,
                "{}: {text}",
                preset.key
            );
            let text = preset.to_yaml(Some(other));
            assert_eq!(
                ClumpPreset::from_yaml(&preset.key, &text).unwrap().clump,
                preset.clump,
                "{} over {}: {text}",
                preset.key,
                other.key
            );
        }
    }
}
