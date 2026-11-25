use super::{
    Cow, GroundTint, PlantAccent, PlantBranches, PlantParams, PlantScatterOptions,
    PlantScatterPreset, PlantSegment, PlantVariation, SCATTER_BASE, ScatterPlant, Srgb, Vec2,
};

/// A flower for the toon fields: a short stem and a big, flat-coloured head,
/// 20 to the square metre on its own.
const fn toon_flower(height: Vec2, head_color: Srgb) -> PlantParams {
    PlantParams {
        default_seed: 0,
        density: 20.0,
        height,
        width: 0.015,
        lean: 0.1,
        stems_per_plant: 1,
        segment: PlantSegment {
            profile: 0.0,
            stiffness: 0.4,
            droop: 0.0,
            twist: 0.0,
            fold: 0.0,
            root_color: Srgb::new(0.05, 0.30, 0.20),
            tip_color: Srgb::new(0.35, 0.70, 0.30),
        },
        levels: [PlantBranches::NONE, PlantBranches::NONE],
        root_occlusion: 0.3,
        translucency: 0.3,
        specular: 0.0,
        head_length: 0.2,
        head_width: 0.07,
        head_color,
        tufts: 0.0,
        tuft_size: 0.6,
        accent: PlantAccent::NONE,
    }
}

preset_group! {
/// Toon-shaded fields, where the shapes are stylised as well as the light.
TOON "Toon" {
    CEL_FIELD {
        name: "Cel field",
        description: "A cel-shaded field in the manner of open-world adventure games: wide, \
                      saturated blades gathered into chunky tufts, two bands of light, and \
                      red and white flowers in clumps.",
    } => PlantScatterOptions {
        plants: Cow::Borrowed(&[
            ScatterPlant::new(toon_flower(Vec2::new(0.5, 0.9), Srgb::new(0.95, 0.25, 0.25)))
                .scaled(0.055)
                .clustered(0.7),
            ScatterPlant::new(toon_flower(Vec2::new(0.5, 0.9), Srgb::new(0.98, 0.97, 0.92)))
                .scaled(0.055)
                .clustered(0.7),
            ScatterPlant::new(
                PlantParams {
                    density: 19.8,
                    height: Vec2::new(0.35, 0.75),
                    width: 0.09,
                    lean: 0.35,
                    root_occlusion: 0.35,
                    translucency: 0.4,
                    specular: 0.0,
                    segment: PlantSegment {
                        stiffness: 0.3,
                        root_color: Srgb::new(0.05, 0.30, 0.30),
                        tip_color: Srgb::new(0.62, 0.92, 0.30),
                        twist: 0.0,
                        droop: 0.35,
                        fold: 0.0,
                        ..PlantParams::MEADOW_GRASS.segment
                    },
                    tufts: 0.7,
                    tuft_size: 0.55,
                    ..PlantParams::MEADOW_GRASS
                },
            ),
        ]),
        variation: PlantVariation {
            patch_color: Srgb::new(0.75, 0.95, 0.35),
            patch_amount: 0.3,
            patch_size: 10.0,
            ..SCATTER_BASE.variation
        },
        ground_tint: GroundTint {
            color: Srgb::new(0.30, 0.45, 0.20),
            amount: 0.4,
        },
        toon_bands: 2,
        ..SCATTER_BASE
    };
    COZY_PASTEL {
        name: "Cosy pastel",
        description: "Short, soft, floppy mint grass with pastel flowers and four soft bands \
                      of light: cosy and casual games.",
    } => PlantScatterOptions {
        plants: Cow::Borrowed(&[
            ScatterPlant::new(toon_flower(Vec2::new(0.25, 0.45), Srgb::new(0.98, 0.62, 0.78)))
                .scaled(0.15)
                .clustered(0.7),
            ScatterPlant::new(toon_flower(Vec2::new(0.25, 0.45), Srgb::new(0.72, 0.62, 0.98)))
                .scaled(0.12)
                .clustered(0.7),
            ScatterPlant::new(toon_flower(Vec2::new(0.25, 0.45), Srgb::new(1.00, 0.88, 0.45)))
                .scaled(0.12)
                .clustered(0.7),
            ScatterPlant::new(
                PlantParams {
                    density: 52.2,
                    height: Vec2::new(0.15, 0.35),
                    width: 0.05,
                    lean: 0.35,
                    root_occlusion: 0.3,
                    translucency: 0.3,
                    specular: 0.0,
                    segment: PlantSegment {
                        stiffness: 0.35,
                        root_color: Srgb::new(0.18, 0.48, 0.42),
                        tip_color: Srgb::new(0.66, 0.90, 0.60),
                        twist: 0.0,
                        droop: 0.4,
                        fold: 0.0,
                        ..PlantParams::MEADOW_GRASS.segment
                    },
                    tufts: 0.5,
                    tuft_size: 0.35,
                    ..PlantParams::MEADOW_GRASS
                },
            ),
        ]),
        variation: PlantVariation {
            patch_color: Srgb::new(0.95, 0.92, 0.70),
            patch_amount: 0.25,
            patch_size: 6.0,
            ..SCATTER_BASE.variation
        },
        ground_tint: GroundTint {
            color: Srgb::new(0.55, 0.72, 0.50),
            amount: 0.3,
        },
        toon_bands: 4,
        ..SCATTER_BASE
    };
}
}
