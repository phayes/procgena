use super::{
    Color, Cow, GroundTint, PlantAccent, PlantBranches, PlantParams, PlantScatter,
    PlantScatterPreset, PlantSegment, PlantVariation, SCATTER_BASE, ScatterPlant, Vec2,
};

preset_group! {
/// Places with a look of their own: bog, marsh, dunes, farmland, steppe,
/// outback, jungle, frost, a burnt field and lavender. Each one pushes a
/// setting the general presets leave alone.
BIOMES "Biomes" {
    COTTON_BOG {
        name: "Cotton bog",
        description: "A Baltic raised bog: low sedge in tussocks over red and ochre \
                      sphagnum, white tufts of cotton grass, and dark pools where the ground \
                      shows through.",
    } => PlantScatter {
        plants: Cow::Borrowed(&[
            // Sedge in tussocks.
            ScatterPlant::new(
                PlantParams {
                    default_seed: 0,
                    density: 40.0,
                    height: Vec2::new(0.15, 0.4),
                    width: 0.01,
                    lean: 0.2,
                    root_occlusion: 0.6,
                    translucency: 0.5,
                    specular: 0.2,
                    head_length: 0.0,
                    head_width: 0.03,
                    head_color: Color::srgb(0.85, 0.80, 0.60),
                    stems_per_plant: 1,
                    segment: PlantSegment {
                        profile: 0.0,
                        stiffness: 0.4,
                        root_color: Color::srgb(0.10, 0.10, 0.04),
                        tip_color: Color::srgb(0.46, 0.44, 0.18),
                        twist: 0.1,
                        droop: 0.3,
                        fold: 0.4,
                    },
                    levels: [PlantBranches::NONE, PlantBranches::NONE],
                    tufts: 0.45,
                    tuft_size: 0.5,
                    accent: PlantAccent::NONE,
                },
            )
            .clustered(0.6),
            ScatterPlant::new(PlantParams::COTTON_GRASS)
                .scaled(0.3)
                .clustered(0.5),
            // Sphagnum: a carpet of red moss, ochre-green in patches.
            ScatterPlant::new(
                PlantParams {
                    density: 151.0,
                    height: Vec2::new(0.03, 0.08),
                    width: 0.03,
                    segment: PlantSegment {
                        root_color: Color::srgb(0.32, 0.07, 0.04),
                        tip_color: Color::srgb(0.78, 0.26, 0.12),
                        ..PlantParams::MOSS.segment
                    },
                    tufts: 0.45,
                    tuft_size: 0.5,
                    ..PlantParams::MOSS
                },
            ),
        ]),
        variation: PlantVariation {
            patch_color: Color::srgb(0.52, 0.46, 0.16),
            patch_amount: 0.55,
            patch_size: 3.0,
            dry_tips: 0.1,
            dead_share: 0.03,
            bare_share: 0.2,
            bare_patch_size: 2.0,
        },
        ground_tint: GroundTint {
            color: Color::srgb(0.10, 0.08, 0.05),
            amount: 0.3,
        },
        ..SCATTER_BASE
    };
    REED_MARSH {
        name: "Reed marsh",
        description: "Reeds taller than a person in dense clumps, with cattails among them \
                      and open water between the clumps.",
    } => PlantScatter {
        plants: Cow::Borrowed(&[
            ScatterPlant::new(PlantParams::CATTAIL)
                .scaled(0.33)
                .clustered(0.7),
            ScatterPlant::new(
                PlantParams {
                    density: 35.1,
                    height: Vec2::new(1.2, 2.0),
                    width: 0.025,
                    lean: 0.15,
                    root_occlusion: 0.7,
                    translucency: 0.55,
                    specular: 0.35,
                    segment: PlantSegment {
                        stiffness: 0.6,
                        root_color: Color::srgb(0.08, 0.12, 0.05),
                        tip_color: Color::srgb(0.42, 0.50, 0.24),
                        twist: 0.1,
                        droop: 0.3,
                        fold: 0.3,
                        ..PlantParams::MEADOW_GRASS.segment
                    },
                    tufts: 0.7,
                    tuft_size: 0.8,
                    ..PlantParams::MEADOW_GRASS
                },
            ),
        ]),
        variation: PlantVariation {
            patch_color: Color::srgb(0.55, 0.50, 0.28),
            patch_amount: 0.35,
            patch_size: 8.0,
            dry_tips: 0.25,
            dead_share: 0.08,
            bare_share: 0.15,
            bare_patch_size: 3.0,
        },
        ground_tint: GroundTint {
            color: Color::srgb(0.12, 0.11, 0.07),
            amount: 0.3,
        },
        ..SCATTER_BASE
    };
    DUNE_MARRAM {
        name: "Dune marram",
        description: "Coastal dune grass: blue-grey, rolled, waxy blades in separate clumps \
                      on open sand, like marram on the Curonian Spit.",
    } => PlantScatter {
        plants: Cow::Borrowed(&[ScatterPlant::new(PlantParams::MARRAM)]),
        variation: PlantVariation {
            patch_color: Color::srgb(0.70, 0.66, 0.48),
            patch_amount: 0.25,
            patch_size: 6.0,
            dry_tips: 0.35,
            dead_share: 0.1,
            bare_share: 0.4,
            bare_patch_size: 4.0,
        },
        ground_tint: GroundTint {
            color: Color::srgb(0.78, 0.70, 0.52),
            amount: 0.55,
        },
        ..SCATTER_BASE
    };
    RYE_AND_CORNFLOWERS {
        name: "Rye and cornflowers",
        description: "Ripening rye, green-gold with seed heads, with blue cornflowers and \
                      red poppies growing through it.",
    } => PlantScatter {
        plants: Cow::Borrowed(&[
            ScatterPlant::new(PlantParams::CORNFLOWER)
                .scaled(0.24)
                .clustered(0.45),
            ScatterPlant::new(PlantParams::POPPY)
                .scaled(0.14)
                .clustered(0.55),
            ScatterPlant::new(
                PlantParams {
                    density: 72.4,
                    height: Vec2::new(1.1, 1.6),
                    width: 0.012,
                    lean: 0.1,
                    root_occlusion: 0.6,
                    translucency: 0.5,
                    specular: 0.45,
                    head_length: 0.15,
                    head_width: 0.018,
                    head_color: Color::srgb(0.72, 0.64, 0.40),
                    segment: PlantSegment {
                        stiffness: 0.55,
                        root_color: Color::srgb(0.18, 0.22, 0.08),
                        tip_color: Color::srgb(0.70, 0.68, 0.36),
                        droop: 0.15,
                        ..PlantParams::MEADOW_GRASS.segment
                    },
                    ..PlantParams::MEADOW_GRASS
                },
            ),
        ]),
        variation: PlantVariation {
            patch_color: Color::srgb(0.55, 0.62, 0.30),
            patch_amount: 0.3,
            patch_size: 10.0,
            dry_tips: 0.2,
            ..SCATTER_BASE.variation
        },
        ground_tint: GroundTint {
            color: Color::srgb(0.30, 0.24, 0.15),
            amount: 0.2,
        },
        ..SCATTER_BASE
    };
    PAMPAS {
        name: "Pampas",
        description: "Pampas grass: every tuft a fountain of long arching leaves, with tall \
                      cream plumes standing out of it.",
    } => PlantScatter {
        plants: Cow::Borrowed(&[ScatterPlant::new(
            PlantParams {
                density: 70.0,
                height: Vec2::new(1.6, 2.6),
                width: 0.018,
                lean: 0.6,
                root_occlusion: 0.75,
                translucency: 0.5,
                specular: 0.4,
                segment: PlantSegment {
                    stiffness: 0.3,
                    root_color: Color::srgb(0.10, 0.14, 0.06),
                    tip_color: Color::srgb(0.48, 0.54, 0.34),
                    twist: 0.1,
                    droop: 0.9,
                    fold: 0.4,
                    ..PlantParams::MEADOW_GRASS.segment
                },
                tufts: 1.0,
                tuft_size: 1.8,
                // Cream plumes standing out of each tussock.
                accent: PlantAccent {
                    share: 0.06,
                    height: Vec2::new(2.2, 3.0),
                    width: 0.01,
                    lean: 0.12,
                    translucency: 0.7,
                    segment: PlantSegment {
                        profile: 0.0,
                        stiffness: 0.45,
                        root_color: Color::srgb(0.30, 0.30, 0.18),
                        tip_color: Color::srgb(0.70, 0.66, 0.50),
                        twist: 0.0,
                        droop: 0.2,
                        fold: 0.0,
                    },
                    head_length: 0.3,
                    head_width: 0.045,
                    head_color: Color::srgb(0.94, 0.88, 0.74),
                },
                ..PlantParams::MEADOW_GRASS
            },
        )]),
        variation: PlantVariation {
            patch_color: Color::srgb(0.55, 0.56, 0.36),
            patch_amount: 0.2,
            patch_size: 8.0,
            dry_tips: 0.3,
            dead_share: 0.06,
            bare_share: 0.15,
            bare_patch_size: 3.0,
        },
        ground_tint: GroundTint {
            color: Color::srgb(0.22, 0.17, 0.10),
            amount: 0.3,
        },
        ..SCATTER_BASE
    };
    SPINIFEX {
        name: "Spinifex",
        description: "Spinifex: stiff, needle-thin blades splayed out of round hummocks on \
                      bare red earth. Outback, desert and wasteland.",
    } => PlantScatter {
        plants: Cow::Borrowed(&[ScatterPlant::new(
            PlantParams {
                density: 130.0,
                height: Vec2::new(0.4, 0.7),
                width: 0.01,
                lean: 0.85,
                root_occlusion: 0.45,
                translucency: 0.3,
                specular: 0.35,
                segment: PlantSegment {
                    stiffness: 0.9,
                    root_color: Color::srgb(0.32, 0.30, 0.18),
                    tip_color: Color::srgb(0.76, 0.70, 0.42),
                    twist: 0.0,
                    droop: 0.0,
                    fold: 0.9,
                    ..PlantParams::MEADOW_GRASS.segment
                },
                tufts: 1.0,
                tuft_size: 0.9,
                ..PlantParams::MEADOW_GRASS
            },
        )]),
        variation: PlantVariation {
            patch_color: Color::srgb(0.62, 0.62, 0.40),
            patch_amount: 0.3,
            patch_size: 6.0,
            dry_tips: 0.4,
            dead_share: 0.15,
            bare_share: 0.3,
            bare_patch_size: 3.0,
        },
        ground_tint: GroundTint {
            color: Color::srgb(0.55, 0.28, 0.14),
            amount: 0.4,
        },
        ..SCATTER_BASE
    };
    ELEPHANT_GRASS {
        name: "Elephant grass",
        description: "Elephant grass: a wall of broad, arching blades twice as tall as a \
                      person, with sunlight glowing through it. Something to hide in.",
    } => PlantScatter {
        plants: Cow::Borrowed(&[
            ScatterPlant::new(
                PlantParams {
                    height: Vec2::new(0.4, 0.9),
                    width: 0.09,
                    ..PlantParams::BROAD_LEAF
                },
            )
            .scaled(0.16)
            .clustered(0.6),
            ScatterPlant::new(
                PlantParams {
                    density: 36.8,
                    height: Vec2::new(2.4, 3.6),
                    width: 0.045,
                    lean: 0.25,
                    root_occlusion: 0.8,
                    translucency: 0.85,
                    specular: 0.35,
                    segment: PlantSegment {
                        stiffness: 0.3,
                        root_color: Color::srgb(0.04, 0.09, 0.02),
                        tip_color: Color::srgb(0.30, 0.52, 0.12),
                        twist: 0.2,
                        droop: 0.75,
                        fold: 0.45,
                        ..PlantParams::MEADOW_GRASS.segment
                    },
                    tufts: 0.6,
                    tuft_size: 0.9,
                    ..PlantParams::MEADOW_GRASS
                },
            ),
        ]),
        variation: PlantVariation {
            patch_color: Color::srgb(0.45, 0.55, 0.16),
            patch_amount: 0.3,
            patch_size: 8.0,
            dry_tips: 0.15,
            dead_share: 0.05,
            bare_share: 0.0,
            bare_patch_size: 3.0,
        },
        ground_tint: GroundTint {
            color: Color::srgb(0.12, 0.09, 0.05),
            amount: 0.3,
        },
        ..SCATTER_BASE
    };
    FROSTED_MEADOW {
        name: "Frosted meadow",
        description: "A meadow on a frosty morning: stiff, glinting, near-white blades \
                      growing out of snow, with straw stalks and frosted seed heads, and \
                      green where the frost has melted.",
    } => PlantScatter {
        plants: Cow::Borrowed(&[ScatterPlant::new(
            PlantParams {
                density: 130.0,
                height: Vec2::new(0.12, 0.32),
                width: 0.022,
                lean: 0.2,
                root_occlusion: 0.5,
                translucency: 0.15,
                specular: 0.9,
                segment: PlantSegment {
                    stiffness: 0.95,
                    root_color: Color::srgb(0.40, 0.48, 0.44),
                    tip_color: Color::srgb(0.94, 0.97, 1.00),
                    twist: 0.05,
                    droop: 0.2,
                    fold: 0.3,
                    ..PlantParams::MEADOW_GRASS.segment
                },
                tufts: 0.3,
                tuft_size: 0.6,
                // Straw stalks with frosted seed heads.
                accent: PlantAccent {
                    share: 0.06,
                    height: Vec2::new(0.35, 0.7),
                    width: PlantParams::SEED_HEADS.width,
                    lean: PlantParams::SEED_HEADS.lean,
                    translucency: 0.15,
                    segment: PlantSegment {
                        stiffness: 0.95,
                        root_color: Color::srgb(0.30, 0.28, 0.20),
                        tip_color: Color::srgb(0.85, 0.88, 0.90),
                        ..PlantParams::SEED_HEADS.segment
                    },
                    head_length: PlantParams::SEED_HEADS.head_length,
                    head_width: PlantParams::SEED_HEADS.head_width,
                    head_color: Color::srgb(0.90, 0.92, 0.94),
                },
                ..PlantParams::MEADOW_GRASS
            },
        )]),
        variation: PlantVariation {
            patch_color: Color::srgb(0.35, 0.50, 0.38),
            patch_amount: 0.35,
            patch_size: 6.0,
            dry_tips: 0.0,
            dead_share: 0.1,
            bare_share: 0.12,
            bare_patch_size: 3.0,
        },
        ground_tint: GroundTint {
            color: Color::srgb(0.90, 0.93, 0.96),
            amount: 0.6,
        },
        ..SCATTER_BASE
    };
    BURNT_REGROWTH {
        name: "Burnt regrowth",
        description: "After a grass fire: charcoal stubble in grey ash, scorched stalks, and \
                      bright new shoots coming through in patches.",
    } => PlantScatter {
        plants: Cow::Borrowed(&[
            // New shoots.
            ScatterPlant::new(
                PlantParams {
                    default_seed: 0,
                    density: 19.8,
                    height: Vec2::new(0.08, 0.28),
                    width: 0.012,
                    lean: 0.12,
                    root_occlusion: 0.3,
                    translucency: 0.8,
                    specular: 0.2,
                    head_length: 0.0,
                    head_width: 0.03,
                    head_color: Color::srgb(0.85, 0.80, 0.60),
                    stems_per_plant: 1,
                    segment: PlantSegment {
                        profile: 0.0,
                        stiffness: 0.5,
                        root_color: Color::srgb(0.10, 0.25, 0.05),
                        tip_color: Color::srgb(0.42, 0.78, 0.18),
                        twist: 0.0,
                        droop: 0.0,
                        fold: 0.0,
                    },
                    levels: [PlantBranches::NONE, PlantBranches::NONE],
                    tufts: 0.0,
                    tuft_size: 0.6,
                    accent: PlantAccent::NONE,
                },
            )
            .clustered(0.85),
            // Scorched stalks still standing.
            ScatterPlant::new(
                PlantParams {
                    default_seed: 0,
                    density: 5.4,
                    height: Vec2::new(0.5, 0.9),
                    width: 0.008,
                    lean: 0.3,
                    root_occlusion: 0.3,
                    translucency: 0.0,
                    specular: 0.05,
                    head_length: 0.0,
                    head_width: 0.03,
                    head_color: Color::srgb(0.85, 0.80, 0.60),
                    stems_per_plant: 1,
                    segment: PlantSegment {
                        profile: 0.0,
                        stiffness: 0.9,
                        root_color: Color::srgb(0.03, 0.03, 0.03),
                        tip_color: Color::srgb(0.20, 0.14, 0.08),
                        twist: 0.0,
                        droop: 0.0,
                        fold: 0.0,
                    },
                    levels: [PlantBranches::NONE, PlantBranches::NONE],
                    tufts: 0.0,
                    tuft_size: 0.6,
                    accent: PlantAccent::NONE,
                },
            )
            .clustered(0.3),
            // Charcoal stubble.
            ScatterPlant::new(
                PlantParams {
                    density: 64.8,
                    height: Vec2::new(0.08, 0.3),
                    width: 0.015,
                    lean: 0.25,
                    root_occlusion: 0.3,
                    translucency: 0.0,
                    specular: 0.05,
                    segment: PlantSegment {
                        stiffness: 0.8,
                        root_color: Color::srgb(0.02, 0.02, 0.02),
                        tip_color: Color::srgb(0.10, 0.09, 0.08),
                        twist: 0.2,
                        droop: 0.3,
                        fold: 0.2,
                        ..PlantParams::MEADOW_GRASS.segment
                    },
                    tufts: 0.2,
                    tuft_size: 0.6,
                    ..PlantParams::MEADOW_GRASS
                },
            ),
        ]),
        variation: PlantVariation {
            patch_color: Color::srgb(0.40, 0.39, 0.37),
            patch_amount: 0.35,
            patch_size: 5.0,
            dry_tips: 0.0,
            dead_share: 0.15,
            bare_share: 0.3,
            bare_patch_size: 2.0,
        },
        ground_tint: GroundTint {
            color: Color::srgb(0.16, 0.15, 0.14),
            amount: 0.5,
        },
        ..SCATTER_BASE
    };
    LAVENDER {
        name: "Lavender",
        description: "Lavender: grey-green bushes of stiff stems with long purple heads. \
                      A density mask with stripes plants it in rows.",
    } => PlantScatter {
        plants: Cow::Borrowed(&[ScatterPlant::new(
            PlantParams {
                density: 160.0,
                height: Vec2::new(0.35, 0.6),
                width: 0.008,
                lean: 0.25,
                root_occlusion: 0.6,
                translucency: 0.3,
                specular: 0.2,
                head_length: 0.4,
                head_width: 0.022,
                head_color: Color::srgb(0.52, 0.38, 0.78),
                segment: PlantSegment {
                    stiffness: 0.75,
                    root_color: Color::srgb(0.22, 0.28, 0.20),
                    tip_color: Color::srgb(0.48, 0.55, 0.45),
                    ..PlantParams::MEADOW_GRASS.segment
                },
                tufts: 1.0,
                tuft_size: 0.45,
                ..PlantParams::MEADOW_GRASS
            },
        )]),
        variation: PlantVariation {
            patch_color: Color::srgb(0.46, 0.52, 0.42),
            patch_amount: 0.0,
            patch_size: 12.0,
            dry_tips: 0.0,
            dead_share: 0.02,
            bare_share: 0.0,
            bare_patch_size: 3.0,
        },
        ground_tint: GroundTint {
            color: Color::srgb(0.40, 0.32, 0.22),
            amount: 0.2,
        },
        ..SCATTER_BASE
    };
}
}
