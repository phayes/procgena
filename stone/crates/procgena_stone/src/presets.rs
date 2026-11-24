//! Ready-to-use rock shapes and materials that can be customised after selection.
//!
//! [`StonePreset::all`] lists the built-in presets in menu order, from larger
//! rocks to smaller ones, and [`StonePreset::named`] looks one up by its key.
//! A `.stone.yml` file loads as a preset with [`StonePreset::from_yaml`].

use std::borrow::Cow;

use crate::options::{
    CarveOptions, MaterialOptions, MeshOptions, Mesher, PlacementOptions, StoneOptions,
    SurfaceOptions,
};

/// A named, ready-made rock, from small pebbles to large cliff fragments.
///
/// Every built-in preset has a function of its own, such as
/// [`StonePreset::pebble`]. Clone its [`stone`](Self::stone) options to
/// customise the size, shape or material, and change
/// [`StoneOptions::default_seed`] to generate a different rock with the same
/// style.
#[derive(Debug, Clone, PartialEq)]
pub struct StonePreset {
    /// The preset's key, such as `"river_stone"`, or a loaded file's name.
    pub key: Cow<'static, str>,
    /// A short display name, such as `"River Stone"`.
    pub name: Cow<'static, str>,
    /// What the rock looks like, in a sentence or two.
    pub description: Cow<'static, str>,
    /// The group it is listed under, such as `"Large"`, or `"File"` for a
    /// loaded file.
    pub group: Cow<'static, str>,
    /// The rock's settings.
    pub stone: StoneOptions,
}

procgena_core::catalogue! {
    StonePreset {
        stone: StoneOptions,
        check: |options| options.validate().map_err(|error| error.to_string()),
    }
    "Large" {
        cliff_chunk {
            name: "Cliff Chunk",
            description: "A large cliff fragment with many flat faces and a 4 m size setting.",
        } => cliff_chunk();
        standing_stone {
            name: "Standing Stone",
            description: "A tall, narrow upright stone with a 3 m size setting.",
        } => standing_stone();
        boulder {
            name: "Boulder",
            description: "A rough, blocky rock with the default 2.5 m size setting.",
        } => boulder();
        lichen_boulder {
            name: "Lichen Boulder",
            description: "A boulder with softer edges and a lichen-covered appearance.",
        } => lichen_boulder();
        flat_slab {
            name: "Flat Slab",
            description: "A broad, shallow slab with sharp edges.",
        } => flat_slab();
        ice_chunk {
            name: "Ice Chunk",
            description: "A broad, flat, polished slab of faintly blue ice with a 2 m size \
                setting, resting on the ground. Change the material's color for a stronger or \
                weaker tint.",
        } => ice_chunk();
    }
    "Medium" {
        ore {
            name: "Ore",
            description: "A slim, glossy lump of blue mineral ore with a 1.65 m size setting, \
                resting on the ground.",
        } => ore();
        shard {
            name: "Shard",
            description: "A slim rock fragment with sharp edges.",
        } => shard();
        hewn_stone {
            name: "Hewn Stone",
            description: "A roughly squared stone block with chipped edges, rough faces and a \
                1 m size setting.",
        } => hewn_stone();
    }
    "Small" {
        river_stone {
            name: "River Stone",
            description: "A flat, rounded stone with a 60 cm size setting, resembling a river \
                rock.",
        } => river_stone();
        pebble {
            name: "Pebble",
            description: "A small, smooth pebble with a 15 cm size setting.",
        } => pebble();
    }
}

/// The default material with a texture set and size.
fn material(texture: &str, texture_size: f32) -> MaterialOptions {
    MaterialOptions {
        texture: texture.to_owned(),
        texture_size,
        ..MaterialOptions::default()
    }
}

fn boulder() -> StoneOptions {
    StoneOptions {
        material: material("rock_boulder_dry", 1.0),
        ..StoneOptions::default()
    }
}

fn lichen_boulder() -> StoneOptions {
    let defaults = StoneOptions::default();
    StoneOptions {
        default_seed: 412,
        carve: CarveOptions {
            steps: 14,
            smoothness: 0.12,
            ..defaults.carve.clone()
        },
        surface: SurfaceOptions {
            displacement_scale: 0.1,
            ..defaults.surface.clone()
        },
        material: material("lichen_rock_02", 1.0),
        ..defaults
    }
}

fn pebble() -> StoneOptions {
    let defaults = StoneOptions::default();
    StoneOptions {
        default_seed: 7,
        scale: 0.15,
        stretch: [1.0, 0.6, 0.8],
        carve: CarveOptions {
            steps: 6,
            smoothness: 0.25,
            ..defaults.carve.clone()
        },
        surface: SurfaceOptions {
            displacement_scale: 0.03,
            displacement_spread: 6.0,
            ..defaults.surface.clone()
        },
        material: material("rock_2", 0.1),
        ..defaults
    }
}

fn river_stone() -> StoneOptions {
    let defaults = StoneOptions::default();
    StoneOptions {
        default_seed: 31,
        scale: 0.6,
        stretch: [1.0, 0.45, 0.75],
        carve: CarveOptions {
            steps: 4,
            smoothness: 0.3,
            ..defaults.carve.clone()
        },
        surface: SurfaceOptions {
            displacement_scale: 0.03,
            displacement_spread: 6.0,
            ..defaults.surface.clone()
        },
        material: material("seaside_rock", 0.3),
        ..defaults
    }
}

fn flat_slab() -> StoneOptions {
    let defaults = StoneOptions::default();
    StoneOptions {
        default_seed: 95,
        scale: 2.0,
        stretch: [1.0, 0.3, 0.8],
        carve: CarveOptions {
            steps: 24,
            smoothness: 0.03,
            ..defaults.carve.clone()
        },
        surface: SurfaceOptions {
            displacement_scale: 0.12,
            ..defaults.surface.clone()
        },
        material: material("rock_face_03", 0.8),
        ..defaults
    }
}

fn cliff_chunk() -> StoneOptions {
    let defaults = StoneOptions::default();
    StoneOptions {
        default_seed: 1203,
        scale: 4.0,
        stretch: [1.0, 0.9, 0.8],
        carve: CarveOptions {
            steps: 30,
            smoothness: 0.02,
            radius_min: 6.0,
            radius_jitter: 1.5,
            depth: 0.9,
            ..defaults.carve.clone()
        },
        surface: SurfaceOptions {
            displacement_scale: 0.2,
            ..defaults.surface.clone()
        },
        material: material("rock_face", 1.6),
        ..defaults
    }
}

fn shard() -> StoneOptions {
    let defaults = StoneOptions::default();
    StoneOptions {
        default_seed: 58,
        scale: 1.5,
        stretch: [0.5, 1.0, 0.45],
        placement: PlacementOptions {
            settle: false,
            ..PlacementOptions::default()
        },
        carve: CarveOptions {
            steps: 22,
            smoothness: 0.01,
            radius_min: 4.0,
            depth: 0.86,
            ..defaults.carve.clone()
        },
        surface: SurfaceOptions {
            displacement_scale: 0.08,
            ..defaults.surface.clone()
        },
        material: material("rock_05", 0.6),
        ..defaults
    }
}

fn standing_stone() -> StoneOptions {
    let defaults = StoneOptions::default();
    StoneOptions {
        default_seed: 266,
        scale: 3.0,
        stretch: [0.45, 1.0, 0.35],
        placement: PlacementOptions {
            settle: false,
            max_embed_fraction: 0.3,
            ..PlacementOptions::default()
        },
        carve: CarveOptions {
            steps: 18,
            smoothness: 0.06,
            ..defaults.carve.clone()
        },
        surface: SurfaceOptions {
            displacement_scale: 0.12,
            ..defaults.surface.clone()
        },
        material: material("lichen_rock", 1.2),
        ..defaults
    }
}

fn hewn_stone() -> StoneOptions {
    let defaults = StoneOptions::default();
    StoneOptions {
        default_seed: 17,
        scale: 1.0,
        stretch: [1.0, 0.55, 0.7],
        carve: CarveOptions {
            steps: 30,
            smoothness: 0.03,
            base_radius: 1.0,
            radius_min: 5.0,
            radius_jitter: 1.0,
            depth: 0.9,
            blockiness: 0.9,
        },
        surface: SurfaceOptions {
            displacement_scale: 0.08,
            displacement_spread: 8.0,
            ..defaults.surface.clone()
        },
        mesh: MeshOptions {
            mesher: Mesher::DualContouring,
            ..defaults.mesh.clone()
        },
        material: material("rock_01", 0.8),
        ..defaults
    }
}

fn ore() -> StoneOptions {
    let defaults = StoneOptions::default();
    StoneOptions {
        default_seed: 1312,
        scale: 1.65,
        stretch: [0.33, 0.55, 0.29],
        placement: PlacementOptions {
            settle: true,
            max_embed_fraction: 0.05,
            min_embed_fraction: 0.0,
        },
        carve: CarveOptions {
            steps: 21,
            smoothness: 0.32,
            base_radius: 0.94,
            radius_min: 4.4,
            radius_jitter: 0.0,
            depth: 0.9,
            blockiness: 0.0,
        },
        surface: SurfaceOptions {
            displacement_scale: 0.21,
            displacement_spread: 27.0,
            octaves: 8,
        },
        mesh: MeshOptions {
            mesher: Mesher::DualContouring,
            ..defaults.mesh.clone()
        },
        material: MaterialOptions {
            color: [0.98, 0.89, 0.94],
            roughness: 0.13,
            ..material("rock_ore_002", 1.1)
        },
    }
}

fn ice_chunk() -> StoneOptions {
    let defaults = StoneOptions::default();
    StoneOptions {
        default_seed: 15523,
        scale: 2.0,
        stretch: [1.0, 0.3, 0.8],
        placement: PlacementOptions {
            settle: true,
            max_embed_fraction: 0.05,
            min_embed_fraction: 0.0,
        },
        carve: CarveOptions {
            steps: 24,
            smoothness: 0.03,
            base_radius: 1.0,
            radius_min: 2.5,
            radius_jitter: 1.0,
            depth: 0.81,
            blockiness: 0.45,
        },
        surface: SurfaceOptions {
            displacement_scale: 0.12,
            displacement_spread: 10.0,
            octaves: 6,
        },
        mesh: MeshOptions {
            mesher: Mesher::MarchingCubes,
            ..defaults.mesh.clone()
        },
        material: MaterialOptions {
            color: [0.9, 0.95, 1.0],
            roughness: 0.0,
            ..material("ice", 0.8)
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::{Stone, TEXTURE_SETS};

    /// Checks that menu entries have unique keys and names, valid settings
    /// and known textures.
    #[test]
    fn every_preset_is_valid_and_listed_once() {
        let mut keys = HashSet::new();
        let mut names = HashSet::new();
        for preset in StonePreset::all() {
            let options = &preset.stone;
            assert_eq!(options.validate(), Ok(()), "{}", preset.name);
            assert!(keys.insert(&preset.key), "{} is listed twice", preset.key);
            assert!(
                names.insert(&preset.name),
                "{} is listed twice",
                preset.name
            );
            assert!(
                TEXTURE_SETS.contains(&options.material.texture.as_str()),
                "{} uses an unknown texture set",
                preset.name
            );
        }
    }

    /// Checks that every preset produces a nonempty rock at reduced test detail.
    #[test]
    fn every_preset_makes_a_rock() {
        for preset in StonePreset::all() {
            let mut options = preset.stone.clone();
            options.mesh.detail = crate::StoneDetail::single(32, crate::TextureSize::Size512);
            let stone = Stone::new(options).unwrap();
            assert!(
                stone.mesh().triangle_count() > 100,
                "{} is empty",
                preset.name
            );
        }
    }

    /// Checks that every preset survives a trip through a `.stone.yml` file,
    /// written against no base and against another preset.
    #[test]
    fn every_preset_reads_back_from_its_file() {
        for preset in StonePreset::all() {
            let text = preset.to_yaml(None);
            let read = StonePreset::from_yaml(&preset.key, &text).unwrap();
            assert_eq!(read.stone, preset.stone, "{}", preset.key);
            assert_eq!(read.name, preset.name);
            let text = preset.to_yaml(Some(StonePreset::boulder()));
            let read = StonePreset::from_yaml(&preset.key, &text).unwrap();
            assert_eq!(read.stone, preset.stone, "{}", preset.key);
        }
    }
}
