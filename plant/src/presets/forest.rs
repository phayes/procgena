use super::{
    Color, Cow, GroundTint, PlantParams, PlantScatter, PlantScatterPreset, PlantSegment,
    PlantVariation, SCATTER_BASE, ScatterPlant,
};

preset_group! {
/// Forest-floor fields: moss, fern clumps, a mixed floor and autumn bracken.
FOREST "Forest" {
    FOREST_MOSS {
        name: "Forest moss",
        description: "A dense carpet of very short, wide moss, in dark patches.",
    } => PlantScatter {
        plants: Cow::Borrowed(&[ScatterPlant::new(PlantParams::MOSS)]),
        density_scale: 1.1,
        variation: PlantVariation {
            patch_color: Color::srgb(0.20, 0.34, 0.15),
            patch_amount: 0.35,
            patch_size: 3.0,
            dry_tips: 0.05,
            dead_share: 0.02,
            bare_share: 0.05,
            bare_patch_size: 1.2,
        },
        ground_tint: GroundTint {
            color: Color::srgb(0.12, 0.09, 0.05),
            amount: 0.35,
        },
        ..SCATTER_BASE
    };
    FOREST_FERNS {
        name: "Forest ferns",
        description: "Arching fern crowns above broad leaves and clover.\n\n\
                      Sparse ferns leave space for the lower ground cover.",
    } => PlantScatter {
        plants: Cow::Borrowed(&[
            // About 1.7 ferns per square metre before thinning.
            ScatterPlant::new(PlantParams::FERN)
                .scaled(0.84)
                .clustered(0.5),
            ScatterPlant::new(PlantParams::CLOVER)
                .scaled(0.056)
                .clustered(0.8),
            ScatterPlant::new(PlantParams {
                density: 20.72,
                tufts: 0.3,
                tuft_size: 0.6,
                ..PlantParams::BROAD_LEAF
            }),
        ]),
        variation: PlantVariation {
            patch_color: Color::srgb(0.20, 0.36, 0.13),
            patch_amount: 0.35,
            patch_size: 4.0,
            dry_tips: 0.12,
            dead_share: 0.02,
            bare_share: 0.12,
            bare_patch_size: 1.8,
        },
        ground_tint: GroundTint {
            color: Color::srgb(0.12, 0.09, 0.05),
            amount: 0.3,
        },
        ..SCATTER_BASE
    };
    FOREST_FLOOR {
        name: "Forest floor",
        description: "The whole floor as one field: moss, clumps of ferns, fallen leaves, \
                      clover and broad leaves among them.",
    } => PlantScatter {
        plants: Cow::Borrowed(&[
            // About 1.4 ferns per square metre, grouped in patches.
            ScatterPlant::new(PlantParams::FERN)
                .scaled(0.7)
                .clustered(0.55),
            ScatterPlant::new(PlantParams::LEAF_LITTER)
                .scaled(0.35)
                .clustered(0.35),
            ScatterPlant::new(PlantParams::CLOVER)
                .scaled(0.084)
                .clustered(0.7),
            ScatterPlant::new(PlantParams::BROAD_LEAF)
                .scaled(0.35)
                .clustered(0.6),
            ScatterPlant::new(PlantParams {
                density: 109.2,
                tufts: 0.35,
                tuft_size: 0.45,
                ..PlantParams::MOSS
            }),
        ]),
        density_scale: 1.0,
        variation: PlantVariation {
            patch_color: Color::srgb(0.22, 0.34, 0.14),
            patch_amount: 0.45,
            bare_share: 0.08,
            bare_patch_size: 1.5,
            ..PlantScatterPreset::FOREST_MOSS.plant_scatter.variation
        },
        ground_tint: GroundTint {
            amount: 0.4,
            ..PlantScatterPreset::FOREST_MOSS.plant_scatter.ground_tint
        },
        ..PlantScatterPreset::FOREST_MOSS.plant_scatter
    };
    FOREST_BRACKEN {
        name: "Forest bracken",
        description: "Bracken in its autumn colour: rusty fern fronds over fallen leaves.",
    } => PlantScatter {
        plants: Cow::Borrowed(&[
            // About 1.7 bracken plants per square metre before thinning.
            ScatterPlant::new(PlantParams::BRACKEN)
                .scaled(0.42)
                .clustered(0.5),
            ScatterPlant::new(PlantParams {
                translucency: 0.3,
                segment: PlantSegment {
                    root_color: Color::srgb(0.14, 0.08, 0.03),
                    tip_color: Color::srgb(0.46, 0.27, 0.12),
                    ..PlantParams::BROAD_LEAF.segment
                },
                ..PlantParams::BROAD_LEAF
            })
            .scaled(0.144)
            .clustered(0.6),
            ScatterPlant::new(PlantParams {
                density: 19.44,
                tufts: 0.45,
                tuft_size: 0.7,
                ..PlantParams::LEAF_LITTER
            }),
        ]),
        variation: PlantVariation {
            patch_color: Color::srgb(0.45, 0.28, 0.12),
            patch_amount: 0.4,
            patch_size: 5.0,
            dry_tips: 0.25,
            dead_share: 0.08,
            bare_share: 0.1,
            bare_patch_size: 2.0,
        },
        ground_tint: GroundTint {
            color: Color::srgb(0.15, 0.10, 0.06),
            amount: 0.35,
        },
        ..SCATTER_BASE
    };
}
}
