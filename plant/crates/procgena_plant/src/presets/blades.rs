use super::{
    Cow, PlantArrangement, PlantBranches, PlantParams, PlantPreset, PlantSegment, Srgb, Vec2,
};

plant_preset_group! {
/// Plants that grow among grass: flowers, clover, moss, ferns and leaves.
GROUND_COVER "Ground cover" {
    SEED_HEADS {
        name: "Seed heads",
        description: "Thin stems with pale seed heads, like timothy or foxtail.",
    } => Self {
        density: 40.0,
        height: Vec2::new(0.8, 1.4),
        width: 0.012,
        lean: 0.1,
        translucency: 0.5,
        head_length: 0.2,
        head_width: 0.022,
        head_color: Srgb::new(0.62, 0.56, 0.36),
        segment: PlantSegment {
            stiffness: 0.45,
            root_color: Srgb::new(0.07, 0.12, 0.04),
            tip_color: Srgb::new(0.40, 0.50, 0.20),
            ..PlantParams::SUMMER_TALL_GRASS.segment
        },
        ..PlantParams::SUMMER_TALL_GRASS
    };
    WHITE_FLOWERS {
        name: "White flowers",
        description: "White flowers on slender stems, like oxeye daisies.",
    } => Self {
        density: 30.0,
        height: Vec2::new(0.35, 0.7),
        width: 0.01,
        lean: 0.08,
        translucency: 0.4,
        head_length: 0.12,
        head_width: 0.05,
        head_color: Srgb::new(0.92, 0.92, 0.86),
        segment: PlantSegment {
            stiffness: 0.5,
            root_color: Srgb::new(0.05, 0.11, 0.03),
            tip_color: Srgb::new(0.24, 0.42, 0.12),
            ..PlantParams::SUMMER_TALL_GRASS.segment
        },
        ..PlantParams::SUMMER_TALL_GRASS
    };
    YELLOW_FLOWERS {
        name: "Yellow flowers",
        description: "Small yellow flowers, like buttercups.",
    } => Self {
        density: 40.0,
        height: Vec2::new(0.3, 0.6),
        head_length: 0.1,
        head_width: 0.035,
        head_color: Srgb::new(0.95, 0.78, 0.12),
        ..PlantParams::WHITE_FLOWERS
    };
    CLOVER {
        name: "Clover",
        description: "Low, broad, dark leaves lying over, like clover.",
    } => Self {
        density: 100.0,
        height: Vec2::new(0.08, 0.2),
        width: 0.035,
        lean: 0.35,
        root_occlusion: 0.5,
        translucency: 0.4,
        segment: PlantSegment {
            stiffness: 0.5,
            root_color: Srgb::new(0.04, 0.10, 0.04),
            tip_color: Srgb::new(0.16, 0.34, 0.12),
            droop: 0.7,
            ..PlantParams::SUMMER_TALL_GRASS.segment
        },
        ..PlantParams::SUMMER_TALL_GRASS
    };
    BROAD_LEAF {
        name: "Broad leaf",
        description: "Wide, folded leaves that arch over, like plantain or dock.",
    } => Self {
        density: 20.0,
        height: Vec2::new(0.2, 0.45),
        width: 0.06,
        lean: 0.3,
        root_occlusion: 0.55,
        translucency: 0.5,
        segment: PlantSegment {
            stiffness: 0.4,
            root_color: Srgb::new(0.04, 0.09, 0.03),
            tip_color: Srgb::new(0.20, 0.38, 0.10),
            twist: 0.1,
            droop: 0.6,
            fold: 0.5,
            ..PlantParams::SUMMER_TALL_GRASS.segment
        },
        ..PlantParams::SUMMER_TALL_GRASS
    };
    MOSS {
        name: "Moss",
        description: "Very short, wide and folded: a moss or lichen carpet, gathered into \
                      small cushions.",
    } => Self {
        density: 200.0,
        height: Vec2::new(0.02, 0.07),
        width: 0.025,
        lean: 0.25,
        root_occlusion: 0.55,
        translucency: 0.35,
        specular: 0.15,
        segment: PlantSegment {
            stiffness: 0.45,
            root_color: Srgb::new(0.03, 0.07, 0.03),
            tip_color: Srgb::new(0.14, 0.30, 0.13),
            droop: 0.45,
            fold: 0.35,
            ..PlantParams::MEADOW_GRASS.segment
        },
        tufts: 0.25,
        tuft_size: 0.35,
        ..PlantParams::MEADOW_GRASS
    };
    FERN {
        name: "Fern",
        description: "A fern with six arching fronds and fourteen leaflet pairs per frond.\n\n\
                      Grows two plants per square metre on its own. Most forest floors look \
                      right with one or two, so scale it down under other plants.",
    } => Self {
        density: 2.0,
        height: Vec2::new(0.5, 0.95),
        // Keep the stalk narrow; leaflets supply the frond's visible width.
        width: 0.007,
        lean: 0.45,
        stems_per_plant: 6,
        root_occlusion: 0.6,
        translucency: 0.5,
        specular: 0.2,
        segment: PlantSegment {
            profile: 0.0,
            stiffness: 0.3,
            droop: 0.55,
            twist: 0.04,
            fold: 0.25,
            root_color: Srgb::new(0.09, 0.12, 0.04),
            tip_color: Srgb::new(0.20, 0.34, 0.10),
        },
        // Leave the bottom quarter bare and place the longest leaflets near the base of
        // the leafy region for a fern-like outline.
        levels: [
            PlantBranches {
                count: 28,
                arrangement: PlantArrangement::Opposite,
                attachment_start: 0.25,
                length: 0.3,
                length_envelope: 0.3,
                width: 0.22,
                angle: 0.22,
                spread: 0.0,
                segment: PlantSegment {
                    profile: 0.3,
                    stiffness: 0.45,
                    droop: 0.25,
                    twist: 0.0,
                    fold: 0.15,
                    root_color: Srgb::new(0.07, 0.16, 0.05),
                    tip_color: Srgb::new(0.22, 0.44, 0.14),
                },
            },
            PlantBranches::NONE,
        ],
        ..PlantParams::MEADOW_GRASS
    };
    BRACKEN {
        name: "Bracken",
        description: "Rust-coloured bracken with tall, upright fronds.\n\n\
                      The longest leaflets sit near the base, giving the dying fern a broad \
                      lower outline.",
    } => Self {
        density: 4.0,
        height: Vec2::new(0.6, 1.1),
        width: 0.009,
        lean: 0.35,
        stems_per_plant: 4,
        translucency: 0.35,
        segment: PlantSegment {
            stiffness: 0.4,
            droop: 0.45,
            root_color: Srgb::new(0.20, 0.11, 0.05),
            tip_color: Srgb::new(0.42, 0.24, 0.10),
            ..Self::FERN.segment
        },
        levels: [
            PlantBranches {
                count: 24,
                attachment_start: 0.3,
                length: 0.34,
                length_envelope: 0.15,
                width: 0.2,
                angle: 0.3,
                segment: PlantSegment {
                    stiffness: 0.5,
                    droop: 0.35,
                    root_color: Srgb::new(0.30, 0.15, 0.06),
                    tip_color: Srgb::new(0.58, 0.32, 0.12),
                    ..Self::FERN.levels[0].segment
                },
                ..Self::FERN.levels[0]
            },
            PlantBranches::NONE,
        ],
        ..Self::FERN
    };
    LEAF_LITTER {
        name: "Leaf litter",
        description: "Wide, flat, drooping: fallen leaves lying over the ground.",
    } => Self {
        density: 40.0,
        height: Vec2::new(0.05, 0.15),
        width: 0.05,
        lean: 0.5,
        root_occlusion: 0.5,
        translucency: 0.25,
        specular: 0.1,
        segment: PlantSegment {
            stiffness: 0.3,
            root_color: Srgb::new(0.12, 0.08, 0.04),
            tip_color: Srgb::new(0.40, 0.28, 0.14),
            twist: 0.15,
            droop: 0.7,
            fold: 0.3,
            ..PlantParams::MEADOW_GRASS.segment
        },
        ..PlantParams::MEADOW_GRASS
    };
    COTTON_GRASS {
        name: "Cotton grass",
        description: "Slender stems with a white tuft of cotton: bog cotton grass.",
    } => Self {
        density: 30.0,
        height: Vec2::new(0.3, 0.55),
        width: 0.008,
        lean: 0.06,
        translucency: 0.4,
        head_length: 0.14,
        head_width: 0.035,
        head_color: Srgb::new(0.96, 0.96, 0.93),
        segment: PlantSegment {
            stiffness: 0.4,
            root_color: Srgb::new(0.08, 0.10, 0.04),
            tip_color: Srgb::new(0.32, 0.36, 0.16),
            ..PlantParams::SUMMER_TALL_GRASS.segment
        },
        ..PlantParams::SUMMER_TALL_GRASS
    };
    CATTAIL {
        name: "Cattail",
        description: "Tall, stiff stems with a long dark-brown head: cattail.",
    } => Self {
        density: 30.0,
        height: Vec2::new(1.6, 2.4),
        width: 0.012,
        lean: 0.04,
        root_occlusion: 0.7,
        translucency: 0.35,
        specular: 0.3,
        head_length: 0.16,
        head_width: 0.035,
        head_color: Srgb::new(0.30, 0.17, 0.08),
        segment: PlantSegment {
            stiffness: 0.85,
            root_color: Srgb::new(0.10, 0.14, 0.05),
            tip_color: Srgb::new(0.30, 0.38, 0.14),
            ..PlantParams::SUMMER_TALL_GRASS.segment
        },
        ..PlantParams::SUMMER_TALL_GRASS
    };
    CORNFLOWER {
        name: "Cornflower",
        description: "Blue flowers on tall stems, like cornflowers in a rye field.",
    } => Self {
        density: 20.0,
        height: Vec2::new(0.7, 1.1),
        width: 0.01,
        head_length: 0.1,
        head_width: 0.05,
        head_color: Srgb::new(0.22, 0.38, 0.92),
        ..PlantParams::WHITE_FLOWERS
    };
    POPPY {
        name: "Poppy",
        description: "Red flowers on tall stems, like field poppies.",
    } => Self {
        density: 20.0,
        height: Vec2::new(0.8, 1.25),
        width: 0.01,
        head_length: 0.1,
        head_width: 0.065,
        head_color: Srgb::new(0.86, 0.10, 0.06),
        ..PlantParams::WHITE_FLOWERS
    };
}
}
