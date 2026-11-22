//! Ready-to-use rock shapes and materials that can be customised after selection.

use crate::options::{
    CarveOptions, MaterialOptions, MeshOptions, Mesher, PlacementOptions, StoneOptions,
    SurfaceOptions,
};

/// A choice of ready-made rocks, from small pebbles to large cliff fragments.
///
/// Call [`options`](Self::options), such as `StonePreset::Pebble.options()`,
/// to customise the size, shape or material. Change [`StoneOptions::seed`] to
/// generate a different rock with the same style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StonePreset {
    /// A rough, blocky rock with the default 2.5 m size setting.
    Boulder,
    /// A boulder with softer edges and a lichen-covered appearance.
    LichenBoulder,
    /// A small, smooth pebble with a 15 cm size setting.
    Pebble,
    /// A flat, rounded stone with a 60 cm size setting, resembling a river rock.
    RiverStone,
    /// A broad, shallow slab with sharp edges.
    FlatSlab,
    /// A large cliff fragment with many flat faces and a 4 m size setting.
    CliffChunk,
    /// A slim rock fragment with sharp edges.
    Shard,
    /// A tall, narrow upright stone with a 3 m size setting.
    StandingStone,
    /// A roughly squared stone block with chipped edges, rough faces and a 1 m size setting.
    HewnStone,
    /// A slim, glossy lump of blue mineral ore with a 1.65 m size setting,
    /// resting on the ground.
    Ore,
    /// A broad, flat, polished slab of faintly blue ice with a 2 m size
    /// setting, resting on the ground. Change [`MaterialOptions::color`] for
    /// a stronger or weaker tint.
    IceChunk,
}

impl StonePreset {
    /// All presets in menu order, from larger rocks to smaller ones.
    pub const ALL: [StonePreset; 11] = [
        Self::CliffChunk,
        Self::StandingStone,
        Self::Boulder,
        Self::LichenBoulder,
        Self::FlatSlab,
        Self::IceChunk,
        Self::Ore,
        Self::Shard,
        Self::HewnStone,
        Self::RiverStone,
        Self::Pebble,
    ];

    /// Returns a readable label for this preset, such as `"River Stone"`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Boulder => "Boulder",
            Self::LichenBoulder => "Lichen Boulder",
            Self::Pebble => "Pebble",
            Self::RiverStone => "River Stone",
            Self::FlatSlab => "Flat Slab",
            Self::CliffChunk => "Cliff Chunk",
            Self::Shard => "Shard",
            Self::StandingStone => "Standing Stone",
            Self::HewnStone => "Hewn Stone",
            Self::Ore => "Ore",
            Self::IceChunk => "Ice Chunk",
        }
    }

    /// Returns editable rock settings for this preset. Changes to the returned
    /// settings do not affect future calls or other rocks.
    pub fn options(self) -> StoneOptions {
        // Unspecified settings retain their default values.
        let defaults = StoneOptions::default();
        let carve = defaults.carve.clone();
        let surface = defaults.surface.clone();
        let material = |texture: &str, texture_size: f32| MaterialOptions {
            texture: texture.to_owned(),
            texture_size,
            ..MaterialOptions::default()
        };

        match self {
            Self::Boulder => StoneOptions {
                material: material("rock_boulder_dry", 1.0),
                ..defaults
            },
            Self::LichenBoulder => StoneOptions {
                seed: 412,
                carve: CarveOptions {
                    steps: 14,
                    smoothness: 0.12,
                    ..carve
                },
                surface: SurfaceOptions {
                    displacement_scale: 0.1,
                    ..surface
                },
                material: material("lichen_rock_02", 1.0),
                ..defaults
            },
            Self::Pebble => StoneOptions {
                seed: 7,
                scale: 0.15,
                stretch: [1.0, 0.6, 0.8],
                carve: CarveOptions {
                    steps: 6,
                    smoothness: 0.25,
                    ..carve
                },
                surface: SurfaceOptions {
                    displacement_scale: 0.03,
                    displacement_spread: 6.0,
                    ..surface
                },
                material: material("rock_2", 0.1),
                ..defaults
            },
            Self::RiverStone => StoneOptions {
                seed: 31,
                scale: 0.6,
                stretch: [1.0, 0.45, 0.75],
                carve: CarveOptions {
                    steps: 4,
                    smoothness: 0.3,
                    ..carve
                },
                surface: SurfaceOptions {
                    displacement_scale: 0.03,
                    displacement_spread: 6.0,
                    ..surface
                },
                material: material("seaside_rock", 0.3),
                ..defaults
            },
            Self::FlatSlab => StoneOptions {
                seed: 95,
                scale: 2.0,
                stretch: [1.0, 0.3, 0.8],
                carve: CarveOptions {
                    steps: 24,
                    smoothness: 0.03,
                    ..carve
                },
                surface: SurfaceOptions {
                    displacement_scale: 0.12,
                    ..surface
                },
                material: material("rock_face_03", 0.8),
                ..defaults
            },
            Self::CliffChunk => StoneOptions {
                seed: 1203,
                scale: 4.0,
                stretch: [1.0, 0.9, 0.8],
                carve: CarveOptions {
                    steps: 30,
                    smoothness: 0.02,
                    radius_min: 6.0,
                    radius_jitter: 1.5,
                    depth: 0.9,
                    ..carve
                },
                surface: SurfaceOptions {
                    displacement_scale: 0.2,
                    ..surface
                },
                material: material("rock_face", 1.6),
                ..defaults
            },
            Self::Shard => StoneOptions {
                seed: 58,
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
                    ..carve
                },
                surface: SurfaceOptions {
                    displacement_scale: 0.08,
                    ..surface
                },
                material: material("rock_05", 0.6),
                ..defaults
            },
            Self::StandingStone => StoneOptions {
                seed: 266,
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
                    ..carve
                },
                surface: SurfaceOptions {
                    displacement_scale: 0.12,
                    ..surface
                },
                material: material("lichen_rock", 1.2),
                ..defaults
            },
            Self::HewnStone => StoneOptions {
                seed: 17,
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
                    ..surface
                },
                mesh: MeshOptions {
                    mesher: Mesher::DualContouring,
                    ..defaults.mesh.clone()
                },
                material: material("rock_01", 0.8),
                ..defaults
            },
            Self::Ore => StoneOptions {
                seed: 1312,
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
            },
            Self::IceChunk => StoneOptions {
                seed: 15523,
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
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::{Stone, TEXTURE_SETS};

    /// Checks that menu entries have unique names, valid settings and known textures.
    #[test]
    fn every_preset_is_valid_and_listed_once() {
        let mut names = HashSet::new();
        for preset in StonePreset::ALL {
            let options = preset.options();
            assert_eq!(options.validate(), Ok(()), "{}", preset.name());
            assert!(
                names.insert(preset.name()),
                "{} is listed twice",
                preset.name()
            );
            assert!(
                TEXTURE_SETS.contains(&options.material.texture.as_str()),
                "{} uses an unknown texture set",
                preset.name()
            );
        }
    }

    /// Checks that every preset produces a nonempty rock at reduced test detail.
    #[test]
    fn every_preset_makes_a_rock() {
        for preset in StonePreset::ALL {
            let mut options = preset.options();
            options.mesh.detail = crate::StoneDetail::single(32, crate::TextureSize::Size512);
            let stone = Stone::new(options).unwrap();
            assert!(
                stone.mesh().triangle_count() > 100,
                "{} is empty",
                preset.name()
            );
        }
    }
}
