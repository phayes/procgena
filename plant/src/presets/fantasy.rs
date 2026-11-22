use super::{
    Color, Cow, FIELD_BASE, FieldPlant, FieldPreset, GrassAccent, GrassBranches, GrassField,
    GrassPlant, GrassSegment, GrassVariation, GroundTint, Vec2,
};

/// A crystal facet: stiff, glassy blue, folded into a sharp V.
const CRYSTAL: GrassSegment = GrassSegment {
    profile: 0.0,
    stiffness: 1.0,
    root_color: Color::srgb(0.05, 0.15, 0.40),
    tip_color: Color::srgb(0.55, 0.90, 1.00),
    twist: 0.0,
    droop: 0.0,
    fold: 1.0,
};

/// A kelp frond: soft, translucent and twisting.
const KELP: GrassSegment = GrassSegment {
    profile: 0.0,
    stiffness: 0.0,
    root_color: Color::srgb(0.10, 0.08, 0.02),
    tip_color: Color::srgb(0.45, 0.40, 0.10),
    twist: 0.35,
    droop: 0.25,
    fold: 0.0,
};

preset_group! {
/// Plants that do not grow on Earth: crystal, alien ribbons, kelp and a
/// corrupted field.
FANTASY "Fantasy" {
    CRYSTAL_SPIRES {
        name: "Crystal spires",
        description: "Clusters of glassy crystal shards that barely move in the wind: \
                      folded into sharp V-shaped facets, fully translucent and glossy.",
    } => GrassField {
        plants: Cow::Borrowed(&[FieldPlant::new(
            GrassPlant {
                density: 30.0,
                height: Vec2::new(0.3, 1.0),
                width: 0.12,
                lean: 0.2,
                root_occlusion: 0.5,
                translucency: 1.0,
                specular: 1.0,
                segment: CRYSTAL,
                tufts: 0.9,
                tuft_size: 0.8,
                // Taller accent shards share the smaller crystals' tuft centres.
                accent: GrassAccent {
                    share: 0.06,
                    height: Vec2::new(1.1, 1.7),
                    width: 0.2,
                    lean: 0.1,
                    translucency: 1.0,
                    segment: CRYSTAL,
                    head_length: 0.0,
                    head_width: 0.03,
                    head_color: Color::srgb(0.85, 0.80, 0.60),
                },
                ..GrassPlant::MEADOW_GRASS
            },
        )]),
        variation: GrassVariation {
            patch_color: Color::srgb(0.85, 0.55, 1.00),
            patch_amount: 0.5,
            patch_size: 6.0,
            dry_tips: 0.0,
            dead_share: 0.0,
            bare_share: 0.3,
            bare_patch_size: 2.0,
        },
        ground_tint: GroundTint {
            color: Color::srgb(0.10, 0.10, 0.14),
            amount: 0.4,
        },
        ..FIELD_BASE
    };
    CORKSCREW_ALIEN {
        name: "Corkscrew alien",
        description: "Wide violet-to-cyan ribbons, each twisted two full turns, with magenta \
                      seed pods among them. Flat two-band toon shading.",
    } => GrassField {
        plants: Cow::Borrowed(&[
            FieldPlant::new(
                GrassPlant {
                    density: 1.12,
                    height: Vec2::new(0.5, 1.2),
                    width: 0.02,
                    lean: 0.1,
                    root_occlusion: 0.5,
                    translucency: 0.6,
                    specular: 0.3,
                    head_length: 0.25,
                    head_width: 0.08,
                    head_color: Color::srgb(1.00, 0.35, 0.75),
                    stems_per_plant: 1,
                    segment: GrassSegment {
                        profile: 0.0,
                        stiffness: 0.3,
                        root_color: Color::srgb(0.18, 0.03, 0.30),
                        tip_color: Color::srgb(0.30, 0.80, 0.80),
                        twist: 1.0,
                        droop: 0.0,
                        fold: 0.0,
                    },
                    levels: [GrassBranches::NONE, GrassBranches::NONE],
                    tufts: 0.0,
                    tuft_size: 0.6,
                    accent: GrassAccent::NONE,
                },
            )
            .clustered(0.7),
            FieldPlant::new(
                GrassPlant {
                    density: 12.88,
                    height: Vec2::new(0.6, 1.3),
                    width: 0.12,
                    lean: 0.15,
                    root_occlusion: 0.5,
                    translucency: 0.8,
                    specular: 0.3,
                    segment: GrassSegment {
                        stiffness: 0.2,
                        root_color: Color::srgb(0.18, 0.03, 0.30),
                        tip_color: Color::srgb(0.10, 0.95, 0.85),
                        twist: 2.0,
                        droop: 0.1,
                        fold: 0.0,
                        ..GrassPlant::MEADOW_GRASS.segment
                    },
                    ..GrassPlant::MEADOW_GRASS
                },
            ),
        ]),
        variation: GrassVariation {
            patch_color: Color::srgb(0.55, 0.20, 0.95),
            patch_amount: 0.4,
            patch_size: 8.0,
            ..FIELD_BASE.variation
        },
        ground_tint: GroundTint {
            color: Color::srgb(0.10, 0.05, 0.14),
            amount: 0.4,
        },
        toon_bands: 2,
        ..FIELD_BASE
    };
    KELP_BED {
        name: "Kelp bed",
        description: "A kelp bed: long, soft, translucent ribbons rising from holdfasts on \
                      the sand, some with a float, and seagrass between them. Give the level \
                      a slow wind with lively sway, and it moves like a current.",
    } => GrassField {
        plants: Cow::Borrowed(&[
            // Seagrass.
            FieldPlant::new(
                GrassPlant {
                    density: 4.9,
                    height: Vec2::new(0.2, 0.5),
                    width: 0.015,
                    lean: 0.2,
                    root_occlusion: 0.5,
                    translucency: 0.8,
                    specular: 0.3,
                    head_length: 0.0,
                    head_width: 0.03,
                    head_color: Color::srgb(0.85, 0.80, 0.60),
                    stems_per_plant: 1,
                    segment: GrassSegment {
                        profile: 0.0,
                        stiffness: 0.1,
                        root_color: Color::srgb(0.04, 0.12, 0.06),
                        tip_color: Color::srgb(0.18, 0.42, 0.22),
                        twist: 0.2,
                        droop: 0.2,
                        fold: 0.0,
                    },
                    levels: [GrassBranches::NONE, GrassBranches::NONE],
                    tufts: 0.0,
                    tuft_size: 0.6,
                    accent: GrassAccent::NONE,
                },
            )
            .clustered(0.6),
            // Kelp occupies about 65% of the planting positions; 15% use float accents.
            FieldPlant::new(
                GrassPlant {
                    density: 9.1,
                    height: Vec2::new(1.4, 3.0),
                    width: 0.14,
                    lean: 0.1,
                    root_occlusion: 0.6,
                    translucency: 1.0,
                    specular: 0.5,
                    segment: KELP,
                    tufts: 0.6,
                    tuft_size: 1.2,
                    // Fronds with a gas float at the top.
                    accent: GrassAccent {
                        share: 0.15 / 0.65,
                        height: Vec2::new(1.4, 3.0),
                        width: 0.14,
                        lean: 0.1,
                        translucency: 1.0,
                        segment: KELP,
                        head_length: 0.06,
                        head_width: 0.07,
                        head_color: Color::srgb(0.50, 0.42, 0.12),
                    },
                    ..GrassPlant::MEADOW_GRASS
                },
            ),
        ]),
        variation: GrassVariation {
            patch_color: Color::srgb(0.40, 0.30, 0.10),
            patch_amount: 0.3,
            patch_size: 6.0,
            dry_tips: 0.0,
            dead_share: 0.0,
            bare_share: 0.25,
            bare_patch_size: 3.0,
        },
        ground_tint: GroundTint {
            color: Color::srgb(0.42, 0.40, 0.30),
            amount: 0.4,
        },
        ..FIELD_BASE
    };
    BLIGHTED {
        name: "Blighted",
        description: "Corrupted land: bruised violet blades that twist and hang, sickly green \
                      patches, dead stalks, and pale bone-white seed heads.",
    } => GrassField {
        plants: Cow::Borrowed(&[FieldPlant::new(
            GrassPlant {
                density: 45.0,
                height: Vec2::new(0.4, 1.0),
                width: 0.03,
                lean: 0.3,
                root_occlusion: 0.8,
                translucency: 0.3,
                specular: 0.4,
                segment: GrassSegment {
                    stiffness: 0.2,
                    root_color: Color::srgb(0.08, 0.02, 0.10),
                    tip_color: Color::srgb(0.50, 0.24, 0.60),
                    twist: 0.5,
                    droop: 0.8,
                    fold: 0.4,
                    ..GrassPlant::MEADOW_GRASS.segment
                },
                tufts: 0.4,
                tuft_size: 0.6,
                // Dead stalks with pale bone-white seed heads.
                accent: GrassAccent {
                    share: 0.06,
                    height: Vec2::new(0.6, 1.2),
                    width: 0.01,
                    lean: 0.15,
                    translucency: 0.2,
                    segment: GrassSegment {
                        profile: 0.0,
                        stiffness: 0.8,
                        root_color: Color::srgb(0.25, 0.22, 0.20),
                        tip_color: Color::srgb(0.80, 0.78, 0.70),
                        twist: 0.0,
                        droop: 0.0,
                        fold: 0.0,
                    },
                    head_length: 0.1,
                    head_width: 0.025,
                    head_color: Color::srgb(0.92, 0.90, 0.82),
                },
                ..GrassPlant::MEADOW_GRASS
            },
        )]),
        variation: GrassVariation {
            patch_color: Color::srgb(0.45, 0.62, 0.12),
            patch_amount: 0.5,
            patch_size: 6.0,
            dry_tips: 0.25,
            dead_share: 0.25,
            bare_share: 0.15,
            bare_patch_size: 2.5,
        },
        ground_tint: GroundTint {
            color: Color::srgb(0.08, 0.06, 0.08),
            amount: 0.5,
        },
        ..FIELD_BASE
    };
}
}
