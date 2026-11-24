use super::{
    Color, Cow, GroundTint, PlantParams, PlantScatter, PlantScatterPreset, PlantSegment,
    SCATTER_BASE, ScatterPlant,
};

/// The summer grass gone brown: stiffer and less translucent.
pub(super) const AUTUMN_TALL_GRASS: PlantParams = PlantParams {
    lean: 0.25,
    translucency: 0.4,
    segment: PlantSegment {
        stiffness: 0.35,
        root_color: Color::srgb(0.13, 0.09, 0.04),
        tip_color: Color::srgb(0.52, 0.36, 0.17),
        ..PlantParams::SUMMER_TALL_GRASS.segment
    },
    ..PlantParams::SUMMER_TALL_GRASS
};

preset_group! {
/// Tall grass in summer and autumn.
///
/// Use the two presets as seasonal starting points for the same field.
SEASONS "Seasons" {
    SUMMER_TALL {
        name: "Summer tall",
        description: "Tall green summer grass.",
    } => PlantScatter {
        // Sheen and ground tint stay off to keep the preset's look; games can
        // turn either on.
        plants: Cow::Borrowed(&[ScatterPlant::new(PlantParams::SUMMER_TALL_GRASS)]),
        ground_tint: GroundTint {
            amount: 0.0,
            ..SCATTER_BASE.ground_tint
        },
        ..SCATTER_BASE
    };
    AUTUMN_TALL {
        name: "Autumn tall",
        description: "Tall brown autumn grass: the summer field gone dry, so stiffer and \
                      less translucent.",
    } => PlantScatter {
        plants: Cow::Borrowed(&[ScatterPlant::new(AUTUMN_TALL_GRASS)]),
        ..Self::SUMMER_TALL.plant_scatter
    };
}
}
