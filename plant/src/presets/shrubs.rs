use super::{
    Color, Cow, FieldPlant, FieldPreset, GrassArrangement, GrassBranches, GrassField, GrassPlant,
    GrassSegment, PlantPreset, Vec2,
};

// Each matching field preset grows the named shrub in clumps, with grass
// filling the spaces between them.
plant_preset_group! {
/// Shrubs for open landscapes such as heath and rough grassland.
SHRUBS "Shrubs" {
    BROOM {
        name: "Broom",
        description: "Common broom: bundles of long, upright green rods on a short woody \
                      base.\n\n\
                      The small leaves are not modelled; the thin twigs stand in for them.",
    } => Self {
        density: 0.7,
        height: Vec2::new(0.35, 0.6),
        width: 0.02,
        lean: 0.18,
        stems_per_plant: 3,
        root_occlusion: 0.5,
        translucency: 0.25,
        specular: 0.25,
        // A stiff, brown woody trunk.
        segment: GrassSegment {
            profile: 0.0,
            stiffness: 0.9,
            droop: 0.0,
            twist: 0.0,
            fold: 0.0,
            root_color: Color::srgb(0.20, 0.13, 0.07),
            tip_color: Color::srgb(0.27, 0.20, 0.10),
        },
        // Six long rods spiral off the upper trunk and point almost straight up, the
        // lowest longest so their tips end at a similar height. Three short twigs on
        // each rod also point upwards, so the bush branches only a little.
        levels: [
            GrassBranches {
                count: 6,
                arrangement: GrassArrangement::Alternate,
                attachment_start: 0.35,
                length: 2.2,
                length_envelope: 0.15,
                width: 0.012,
                angle: 0.85,
                spread: 0.24,
                segment: GrassSegment {
                    profile: 0.0,
                    stiffness: 0.55,
                    droop: 0.08,
                    twist: 0.0,
                    fold: 0.0,
                    root_color: Color::srgb(0.09, 0.16, 0.06),
                    tip_color: Color::srgb(0.15, 0.25, 0.08),
                },
            },
            GrassBranches {
                count: 3,
                arrangement: GrassArrangement::Alternate,
                attachment_start: 0.35,
                length: 0.35,
                length_envelope: 0.3,
                width: 0.025,
                angle: 0.8,
                spread: 0.3,
                segment: GrassSegment {
                    profile: 0.0,
                    stiffness: 0.5,
                    droop: 0.05,
                    twist: 0.0,
                    fold: 0.0,
                    root_color: Color::srgb(0.12, 0.21, 0.07),
                    tip_color: Color::srgb(0.19, 0.31, 0.10),
                },
            },
        ],
        ..GrassPlant::FERN
    };
}
}

preset_group! {
/// Shrubs of heath, scrub and rough grassland, growing in clumps through grass.
SHRUBS "Shrubs" {
    BROOM {
        name: "Broom",
        description: "Broom bushes scattered through meadow grass.\n\n\
                      Clumps of tall green rods stand well above the grass.",
    } => GrassField {
        // Meadow grass fills most planting positions, leaving clumps of broom.
        plants: Cow::Borrowed(&[
            FieldPlant::new(GrassPlant::MEADOW_GRASS).clustered(0.6),
            FieldPlant::new(GrassPlant::BROOM),
        ]),
        ..FieldPreset::MEADOW.field
    };
}
}
