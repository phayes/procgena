use super::{
    Color, Cow, GroundTint, PlantParams, PlantScatter, PlantScatterPreset, PlantSegment,
    SCATTER_BASE, ScatterPlant, Vec2,
};

preset_group! {
/// The six general-purpose presets.
GENERAL "General" {
    MEADOW {
        name: "Meadow",
        description: "Fresh green, soft, mixed heights. The default for temperate worlds.",
    } => PlantScatter {
        plants: Cow::Borrowed(&[ScatterPlant::new(PlantParams::MEADOW_GRASS)]),
        ..SCATTER_BASE
    };
    TALL_PRAIRIE {
        name: "Tall prairie",
        description: "Deep green to straw tips, loose and flowing. For hiding in grass and \
                      dramatic wind.",
    } => PlantScatter {
        plants: Cow::Borrowed(&[ScatterPlant::new(PlantParams {
            density: 40.0,
            height: Vec2::new(0.8, 1.4),
            width: 0.025,
            lean: 0.25,
            root_occlusion: 0.7,
            translucency: 0.5,
            specular: 0.3,
            segment: PlantSegment {
                stiffness: 0.15,
                root_color: Color::srgb(0.05, 0.10, 0.03),
                tip_color: Color::srgb(0.62, 0.58, 0.30),
                ..PlantParams::MEADOW_GRASS.segment
            },
            ..PlantParams::MEADOW_GRASS
        })]),
        ground_tint: GroundTint {
            amount: 0.2,
            ..SCATTER_BASE.ground_tint
        },
        ..SCATTER_BASE
    };
    WHEAT {
        name: "Wheat",
        description: "Golden, dense, stiff stems with a sheen. Farmland.",
    } => PlantScatter {
        plants: Cow::Borrowed(&[ScatterPlant::new(PlantParams::WHEAT)]),
        ground_tint: GroundTint {
            amount: 0.15,
            ..SCATTER_BASE.ground_tint
        },
        ..SCATTER_BASE
    };
    DRY_SAVANNA {
        name: "Dry savanna",
        description: "Yellow-brown, sparse and stiff. Arid and late-summer biomes.",
    } => PlantScatter {
        plants: Cow::Borrowed(&[ScatterPlant::new(PlantParams {
            density: 25.0,
            height: Vec2::new(0.5, 1.0),
            width: 0.02,
            lean: 0.2,
            root_occlusion: 0.5,
            translucency: 0.35,
            specular: 0.2,
            segment: PlantSegment {
                stiffness: 0.6,
                root_color: Color::srgb(0.25, 0.20, 0.10),
                tip_color: Color::srgb(0.74, 0.62, 0.36),
                ..PlantParams::MEADOW_GRASS.segment
            },
            ..PlantParams::MEADOW_GRASS
        })]),
        ground_tint: GroundTint {
            color: Color::srgb(0.42, 0.33, 0.20),
            amount: 0.3,
        },
        ..SCATTER_BASE
    };
    LAWN {
        name: "Lawn",
        description: "Uniform, short and dense. Gardens, towns, sports fields.",
    } => PlantScatter {
        plants: Cow::Borrowed(&[ScatterPlant::new(PlantParams::LAWN_GRASS)]),
        ground_tint: GroundTint {
            amount: 0.0,
            ..SCATTER_BASE.ground_tint
        },
        ..SCATTER_BASE
    };
    STYLIZED {
        name: "Stylised",
        description: "Flat toon bands, wide blades and no specular, for non-photoreal art.",
    } => PlantScatter {
        plants: Cow::Borrowed(&[ScatterPlant::new(PlantParams {
            density: 30.0,
            height: Vec2::new(0.4, 0.8),
            width: 0.06,
            lean: 0.2,
            root_occlusion: 0.5,
            translucency: 0.3,
            specular: 0.0,
            segment: PlantSegment {
                stiffness: 0.3,
                root_color: Color::srgb(0.10, 0.30, 0.12),
                tip_color: Color::srgb(0.45, 0.75, 0.30),
                ..PlantParams::MEADOW_GRASS.segment
            },
            ..PlantParams::MEADOW_GRASS
        })]),
        ground_tint: GroundTint {
            amount: 0.0,
            ..SCATTER_BASE.ground_tint
        },
        toon_bands: 3,
        ..SCATTER_BASE
    };
}
}
