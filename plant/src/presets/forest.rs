use super::{
    Color, Cow, FIELD_BASE, FieldPlant, FieldPreset, GrassField, GrassPlant, GrassSegment,
    GrassVariation, GroundTint,
};

preset_group! {
/// Forest-floor fields: moss, fern clumps, a mixed floor and autumn bracken.
FOREST "Forest" {
    FOREST_MOSS {
        name: "Forest moss",
        description: "A dense carpet of very short, wide moss, in dark patches.",
    } => GrassField {
        plants: Cow::Borrowed(&[FieldPlant::new(GrassPlant::MOSS)]),
        density_scale: 1.1,
        variation: GrassVariation {
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
        ..FIELD_BASE
    };
    FOREST_FERNS {
        name: "Forest ferns",
        description: "Arching fern crowns above broad leaves and clover.\n\n\
                      Sparse ferns leave space for the lower ground cover.",
    } => GrassField {
        plants: Cow::Borrowed(&[
            // About 1.7 ferns per square metre before thinning.
            FieldPlant::new(GrassPlant::FERN)
                .scaled(0.84)
                .clustered(0.5),
            FieldPlant::new(GrassPlant::CLOVER)
                .scaled(0.056)
                .clustered(0.8),
            FieldPlant::new(GrassPlant {
                density: 20.72,
                tufts: 0.3,
                tuft_size: 0.6,
                ..GrassPlant::BROAD_LEAF
            }),
        ]),
        variation: GrassVariation {
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
        ..FIELD_BASE
    };
    FOREST_FLOOR {
        name: "Forest floor",
        description: "The whole floor as one field: moss, clumps of ferns, fallen leaves, \
                      clover and broad leaves among them.",
    } => GrassField {
        plants: Cow::Borrowed(&[
            // About 1.4 ferns per square metre, grouped in patches.
            FieldPlant::new(GrassPlant::FERN)
                .scaled(0.7)
                .clustered(0.55),
            FieldPlant::new(GrassPlant::LEAF_LITTER)
                .scaled(0.35)
                .clustered(0.35),
            FieldPlant::new(GrassPlant::CLOVER)
                .scaled(0.084)
                .clustered(0.7),
            FieldPlant::new(GrassPlant::BROAD_LEAF)
                .scaled(0.35)
                .clustered(0.6),
            FieldPlant::new(GrassPlant {
                density: 109.2,
                tufts: 0.35,
                tuft_size: 0.45,
                ..GrassPlant::MOSS
            }),
        ]),
        density_scale: 1.0,
        variation: GrassVariation {
            patch_color: Color::srgb(0.22, 0.34, 0.14),
            patch_amount: 0.45,
            bare_share: 0.08,
            bare_patch_size: 1.5,
            ..FieldPreset::FOREST_MOSS.field.variation
        },
        ground_tint: GroundTint {
            amount: 0.4,
            ..FieldPreset::FOREST_MOSS.field.ground_tint
        },
        ..FieldPreset::FOREST_MOSS.field
    };
    FOREST_BRACKEN {
        name: "Forest bracken",
        description: "Bracken in its autumn colour: rusty fern fronds over fallen leaves.",
    } => GrassField {
        plants: Cow::Borrowed(&[
            // About 1.7 bracken plants per square metre before thinning.
            FieldPlant::new(GrassPlant::BRACKEN)
                .scaled(0.42)
                .clustered(0.5),
            FieldPlant::new(GrassPlant {
                translucency: 0.3,
                segment: GrassSegment {
                    root_color: Color::srgb(0.14, 0.08, 0.03),
                    tip_color: Color::srgb(0.46, 0.27, 0.12),
                    ..GrassPlant::BROAD_LEAF.segment
                },
                ..GrassPlant::BROAD_LEAF
            })
            .scaled(0.144)
            .clustered(0.6),
            FieldPlant::new(GrassPlant {
                density: 19.44,
                tufts: 0.45,
                tuft_size: 0.7,
                ..GrassPlant::LEAF_LITTER
            }),
        ]),
        variation: GrassVariation {
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
        ..FIELD_BASE
    };
}
}
