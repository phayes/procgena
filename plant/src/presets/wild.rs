use super::{
    Color, Cow, FieldPlant, FieldPreset, GrassField, GrassPlant, GrassSegment, GrassVariation,
    seasons::AUTUMN_TALL_GRASS,
};

preset_group! {
/// Mixed fields with flowers, seed heads and uneven growth.
WILD "Wild" {
    SUMMER_WILD {
        name: "Summer wild",
        description: "Summer grass gone wild: tufts and bare spots, blades that twist and \
                      droop, yellower patches, dry tips, seed heads, broad leaves and a few \
                      flowers.",
    } => GrassField {
        plants: Cow::Borrowed(&[
            FieldPlant::new(GrassPlant::SEED_HEADS)
                .scaled(0.1)
                .clustered(0.3),
            FieldPlant::new(GrassPlant::BROAD_LEAF)
                .scaled(0.12)
                .clustered(0.6),
            FieldPlant::new(GrassPlant::WHITE_FLOWERS)
                .scaled(0.04)
                .clustered(0.8),
            FieldPlant::new(GrassPlant::YELLOW_FLOWERS)
                .scaled(0.02)
                .clustered(0.8),
            FieldPlant::new(GrassPlant {
                density: 31.6,
                segment: GrassSegment {
                    twist: 0.15,
                    droop: 0.25,
                    fold: 0.3,
                    ..GrassPlant::SUMMER_TALL_GRASS.segment
                },
                tufts: 0.35,
                ..GrassPlant::SUMMER_TALL_GRASS
            }),
        ]),
        variation: GrassVariation {
            patch_color: Color::srgb(0.56, 0.58, 0.22),
            patch_amount: 0.45,
            dry_tips: 0.2,
            dead_share: 0.04,
            bare_share: 0.06,
            ..FieldPreset::SUMMER_TALL.field.variation
        },
        ..FieldPreset::SUMMER_TALL.field
    };
    AUTUMN_WILD {
        name: "Autumn wild",
        description: "Autumn grass gone wild: drier, more dead blades and beaten-down tufts, \
                      rusty patches, and seed heads and leaves gone brown.",
    } => GrassField {
        plants: Cow::Borrowed(&[
            FieldPlant::new(GrassPlant {
                translucency: 0.4,
                head_color: Color::srgb(0.48, 0.36, 0.20),
                segment: GrassSegment {
                    root_color: Color::srgb(0.16, 0.12, 0.06),
                    tip_color: Color::srgb(0.50, 0.40, 0.22),
                    ..GrassPlant::SEED_HEADS.segment
                },
                ..GrassPlant::SEED_HEADS
            })
            .scaled(0.12)
            .clustered(0.3),
            FieldPlant::new(GrassPlant {
                translucency: 0.3,
                segment: GrassSegment {
                    root_color: Color::srgb(0.12, 0.07, 0.03),
                    tip_color: Color::srgb(0.42, 0.24, 0.10),
                    ..GrassPlant::BROAD_LEAF.segment
                },
                ..GrassPlant::BROAD_LEAF
            })
            .scaled(0.12)
            .clustered(0.6),
            FieldPlant::new(GrassPlant {
                density: 32.8,
                segment: GrassSegment {
                    twist: 0.15,
                    droop: 0.35,
                    fold: 0.3,
                    ..AUTUMN_TALL_GRASS.segment
                },
                tufts: 0.35,
                ..AUTUMN_TALL_GRASS
            }),
        ]),
        variation: GrassVariation {
            patch_color: Color::srgb(0.50, 0.30, 0.14),
            patch_amount: 0.4,
            dry_tips: 0.35,
            dead_share: 0.12,
            bare_share: 0.08,
            ..FieldPreset::AUTUMN_TALL.field.variation
        },
        ..FieldPreset::AUTUMN_TALL.field
    };
    WILDFLOWER_MEADOW {
        name: "Wildflower meadow",
        description: "A short meadow full of flowers and clover.",
    } => GrassField {
        plants: Cow::Borrowed(&[
            FieldPlant::new(GrassPlant::CLOVER)
                .scaled(0.09)
                .clustered(0.7),
            FieldPlant::new(GrassPlant::WHITE_FLOWERS)
                .scaled(0.12)
                .clustered(0.6),
            FieldPlant::new(GrassPlant::YELLOW_FLOWERS)
                .scaled(0.09)
                .clustered(0.6),
            FieldPlant::new(GrassPlant {
                density: 43.8,
                segment: GrassSegment {
                    twist: 0.1,
                    droop: 0.15,
                    fold: 0.2,
                    ..GrassPlant::MEADOW_GRASS.segment
                },
                tufts: 0.2,
                ..GrassPlant::MEADOW_GRASS
            }),
        ]),
        variation: GrassVariation {
            patch_color: Color::srgb(0.30, 0.55, 0.14),
            patch_amount: 0.4,
            dry_tips: 0.1,
            ..FieldPreset::MEADOW.field.variation
        },
        ..FieldPreset::MEADOW.field
    };
}
}
