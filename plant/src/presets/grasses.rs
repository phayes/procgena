use super::{Color, Cow, GrassAccent, GrassBranches, GrassPlant, GrassSegment, PlantPreset, Vec2};

plant_preset_group! {
/// Grasses to grow alone or mix with flowers and other plants.
GRASSES "Grasses" {
    MEADOW_GRASS {
        name: "Meadow grass",
        description: "Fresh green, soft blades of mixed heights. The default plant.",
    } => GrassPlant {
        density: 60.0,
        height: Vec2::new(0.3, 0.6),
        width: 0.02,
        lean: 0.15,
        root_occlusion: 0.6,
        translucency: 0.6,
        specular: 0.3,
        head_length: 0.0,
        head_width: 0.03,
        head_color: Color::srgb(0.85, 0.80, 0.60),
        stems_per_plant: 1,
        segment: GrassSegment {
            profile: 0.0,
            stiffness: 0.3,
            root_color: Color::srgb(0.06, 0.13, 0.03),
            tip_color: Color::srgb(0.38, 0.55, 0.16),
            twist: 0.0,
            droop: 0.0,
            fold: 0.0,
        },
        levels: [GrassBranches::NONE, GrassBranches::NONE],
        tufts: 0.0,
        tuft_size: 0.6,
        accent: GrassAccent::NONE,
    };
    SUMMER_TALL_GRASS {
        name: "Summer tall grass",
        description: "Tall, wide, green summer blades, very translucent.",
    } => GrassPlant {
        density: 40.0,
        height: Vec2::new(0.7, 1.3),
        width: 0.04,
        lean: 0.2,
        root_occlusion: 0.65,
        translucency: 0.6,
        specular: 0.0,
        segment: GrassSegment {
            stiffness: 0.25,
            root_color: Color::srgb(0.05, 0.11, 0.03),
            tip_color: Color::srgb(0.34, 0.56, 0.14),
            ..GrassPlant::MEADOW_GRASS.segment
        },
        ..GrassPlant::MEADOW_GRASS
    };
    LAWN_GRASS {
        name: "Lawn grass",
        description: "Short, fine, stiff blades for a mown lawn.",
    } => GrassPlant {
        density: 250.0,
        height: Vec2::new(0.05, 0.12),
        width: 0.008,
        lean: 0.1,
        root_occlusion: 0.4,
        translucency: 0.4,
        specular: 0.2,
        segment: GrassSegment {
            stiffness: 0.7,
            root_color: Color::srgb(0.04, 0.10, 0.03),
            tip_color: Color::srgb(0.22, 0.42, 0.10),
            ..GrassPlant::MEADOW_GRASS.segment
        },
        ..GrassPlant::MEADOW_GRASS
    };
    WHEAT {
        name: "Wheat",
        description: "Golden, stiff stems with a sheen.",
    } => GrassPlant {
        density: 90.0,
        height: Vec2::new(0.9, 1.2),
        width: 0.015,
        lean: 0.08,
        root_occlusion: 0.5,
        translucency: 0.5,
        specular: 0.5,
        segment: GrassSegment {
            stiffness: 0.6,
            root_color: Color::srgb(0.30, 0.27, 0.11),
            tip_color: Color::srgb(0.86, 0.70, 0.32),
            ..GrassPlant::MEADOW_GRASS.segment
        },
        ..GrassPlant::MEADOW_GRASS
    };
    MARRAM {
        name: "Marram",
        description: "Blue-grey, rolled, waxy dune grass, gathered in separate clumps.",
    } => GrassPlant {
        density: 45.0,
        height: Vec2::new(0.5, 0.95),
        width: 0.012,
        lean: 0.45,
        root_occlusion: 0.4,
        translucency: 0.35,
        specular: 0.6,
        segment: GrassSegment {
            stiffness: 0.55,
            root_color: Color::srgb(0.30, 0.30, 0.22),
            tip_color: Color::srgb(0.58, 0.64, 0.50),
            twist: 0.15,
            droop: 0.55,
            fold: 0.8,
            ..GrassPlant::MEADOW_GRASS.segment
        },
        tufts: 1.0,
        tuft_size: 1.1,
        ..GrassPlant::MEADOW_GRASS
    };
}
}
