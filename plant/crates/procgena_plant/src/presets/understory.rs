use super::{
    Cow, GroundTint, PlantArrangement, PlantBranches, PlantParams, PlantPreset,
    PlantScatterOptions, PlantScatterPreset, PlantSegment, PlantVariation, ScatterPlant, Srgb,
    Vec2,
};

// Each matching field preset grows the named plant in clumps, with moss
// filling the spaces between them.
plant_preset_group! {
/// Plants that grow beneath temperate rainforest trees, from low shrubs to tall ferns.
UNDERSTORY "Understory" {
    SALAL {
        name: "Salal",
        description: "Salal with arching stems and glossy oval leaves.\n\n\
                      Side shoots carry leathery-looking leaves for dense, low shrub cover.",
    } => Self {
        density: 1.5,
        height: Vec2::new(0.45, 0.85),
        width: 0.005,
        lean: 0.35,
        stems_per_plant: 6,
        root_occlusion: 0.6,
        translucency: 0.2,
        specular: 0.45,
        segment: PlantSegment {
            profile: 0.0,
            stiffness: 0.55,
            droop: 0.35,
            twist: 0.0,
            fold: 0.0,
            root_color: Srgb::new(0.16, 0.08, 0.05),
            tip_color: Srgb::new(0.30, 0.18, 0.10),
        },
        // Spiralling shoots and seven leaves per shoot give the shrub volume.
        levels: [
            PlantBranches {
                count: 5,
                arrangement: PlantArrangement::Alternate,
                attachment_start: 0.3,
                length: 0.4,
                length_envelope: 0.6,
                width: 0.03,
                angle: 0.35,
                spread: 0.24,
                segment: PlantSegment {
                    profile: 0.0,
                    stiffness: 0.6,
                    droop: 0.2,
                    twist: 0.0,
                    fold: 0.0,
                    root_color: Srgb::new(0.18, 0.10, 0.06),
                    tip_color: Srgb::new(0.20, 0.20, 0.08),
                },
            },
            PlantBranches {
                count: 7,
                arrangement: PlantArrangement::Alternate,
                attachment_start: 0.05,
                length: 0.38,
                length_envelope: 0.55,
                width: 0.6,
                angle: 0.25,
                spread: 0.0,
                segment: PlantSegment {
                    profile: 1.0,
                    stiffness: 0.75,
                    droop: 0.15,
                    twist: 0.05,
                    fold: 0.1,
                    root_color: Srgb::new(0.05, 0.14, 0.05),
                    tip_color: Srgb::new(0.13, 0.30, 0.10),
                },
            },
        ],
        ..PlantParams::FERN
    };

    SAPLING {
        name: "Sapling",
        description: "A young broadleaf tree with golden-brown stems.\n\n\
                      Arching stems carry clusters of three leaflets.",
    } => Self {
        density: 0.6,
        height: Vec2::new(0.8, 1.2),
        width: 0.009,
        lean: 0.3,
        stems_per_plant: 4,
        root_occlusion: 0.6,
        translucency: 0.45,
        specular: 0.15,
        segment: PlantSegment {
            profile: 0.0,
            stiffness: 0.5,
            droop: 0.4,
            twist: 0.0,
            fold: 0.0,
            root_color: Srgb::new(0.22, 0.15, 0.07),
            tip_color: Srgb::new(0.36, 0.26, 0.12),
        },
        // Six spiralling leaf stalks carry three leaflets each, forming an open crown.
        levels: [
            PlantBranches {
                count: 6,
                arrangement: PlantArrangement::Alternate,
                attachment_start: 0.35,
                length: 0.25,
                length_envelope: 0.6,
                width: 0.04,
                angle: 0.35,
                spread: 0.24,
                segment: PlantSegment {
                    profile: 0.0,
                    stiffness: 0.5,
                    droop: 0.3,
                    twist: 0.0,
                    fold: 0.0,
                    root_color: Srgb::new(0.12, 0.18, 0.06),
                    tip_color: Srgb::new(0.16, 0.26, 0.08),
                },
            },
            PlantBranches {
                count: 3,
                arrangement: PlantArrangement::Alternate,
                attachment_start: 0.4,
                length: 0.55,
                length_envelope: 0.7,
                width: 0.5,
                angle: 0.45,
                spread: 0.0,
                segment: PlantSegment {
                    profile: 0.6,
                    stiffness: 0.5,
                    droop: 0.2,
                    twist: 0.04,
                    fold: 0.15,
                    root_color: Srgb::new(0.07, 0.18, 0.05),
                    tip_color: Srgb::new(0.20, 0.40, 0.12),
                },
            },
        ],
        ..PlantParams::FERN
    };

    HUCKLEBERRY {
        name: "Huckleberry",
        description: "Red huckleberry with bright green leaves and an airy outline.\n\n\
                      Small oval leaves grow along angular twigs.",
    } => Self {
        density: 0.5,
        height: Vec2::new(0.6, 1.05),
        width: 0.006,
        lean: 0.2,
        stems_per_plant: 4,
        root_occlusion: 0.5,
        translucency: 0.55,
        specular: 0.15,
        segment: PlantSegment {
            profile: 0.0,
            stiffness: 0.7,
            droop: 0.15,
            twist: 0.0,
            fold: 0.0,
            root_color: Srgb::new(0.12, 0.25, 0.07),
            tip_color: Srgb::new(0.25, 0.45, 0.12),
        },
        // Nearly flat sprays of eight twigs, each with eight leaves, keep the crown
        // open.
        levels: [
            PlantBranches {
                count: 8,
                arrangement: PlantArrangement::Alternate,
                attachment_start: 0.25,
                length: 0.45,
                length_envelope: 0.45,
                width: 0.03,
                angle: 0.45,
                spread: 0.1,
                segment: PlantSegment {
                    profile: 0.0,
                    stiffness: 0.55,
                    droop: 0.15,
                    twist: 0.0,
                    fold: 0.0,
                    root_color: Srgb::new(0.14, 0.28, 0.07),
                    tip_color: Srgb::new(0.24, 0.44, 0.12),
                },
            },
            PlantBranches {
                count: 8,
                arrangement: PlantArrangement::Alternate,
                attachment_start: 0.1,
                length: 0.14,
                length_envelope: 0.5,
                width: 0.55,
                angle: 0.3,
                spread: 0.0,
                segment: PlantSegment {
                    profile: 1.0,
                    stiffness: 0.6,
                    droop: 0.05,
                    twist: 0.04,
                    fold: 0.0,
                    root_color: Srgb::new(0.12, 0.30, 0.08),
                    tip_color: Srgb::new(0.30, 0.52, 0.14),
                },
            },
        ],
        ..PlantParams::FERN
    };

    OREGON_GRAPE {
        name: "Oregon grape",
        description: "Low Oregon grape with glossy, holly-like leaflets.\n\n\
                      Paired leaflets follow arching stalks, leaving their bases bare.",
    } => Self {
        density: 2.5,
        height: Vec2::new(0.25, 0.5),
        width: 0.004,
        lean: 0.4,
        stems_per_plant: 5,
        root_occlusion: 0.6,
        translucency: 0.2,
        specular: 0.5,
        segment: PlantSegment {
            profile: 0.0,
            stiffness: 0.6,
            droop: 0.3,
            twist: 0.0,
            fold: 0.0,
            root_color: Srgb::new(0.18, 0.08, 0.05),
            tip_color: Srgb::new(0.12, 0.20, 0.06),
        },
        // Seven pairs of stiff, creased leaflets give the stalk a holly-like outline.
        levels: [
            PlantBranches {
                count: 14,
                arrangement: PlantArrangement::Opposite,
                attachment_start: 0.3,
                length: 0.2,
                length_envelope: 0.45,
                width: 0.42,
                angle: 0.25,
                spread: 0.0,
                segment: PlantSegment {
                    profile: 0.75,
                    stiffness: 0.75,
                    droop: 0.1,
                    twist: 0.02,
                    fold: 0.3,
                    root_color: Srgb::new(0.04, 0.12, 0.05),
                    tip_color: Srgb::new(0.10, 0.26, 0.10),
                },
            },
            PlantBranches::NONE,
        ],
        ..PlantParams::FERN
    };

    NETTLE {
        name: "Nettle",
        description: "Stinging nettle with upright stems and drooping, pointed leaves.\n\n\
                      Each opposite pair turns a quarter-turn from the previous pair.",
    } => Self {
        density: 5.0,
        height: Vec2::new(0.6, 1.0),
        width: 0.006,
        lean: 0.08,
        stems_per_plant: 4,
        root_occlusion: 0.6,
        translucency: 0.4,
        specular: 0.1,
        segment: PlantSegment {
            profile: 0.0,
            stiffness: 0.7,
            droop: 0.05,
            twist: 0.0,
            fold: 0.0,
            root_color: Srgb::new(0.08, 0.16, 0.05),
            tip_color: Srgb::new(0.16, 0.30, 0.09),
        },
        // Eight pairs, longest near the base, make the plant taper upwards.
        levels: [
            PlantBranches {
                count: 16,
                arrangement: PlantArrangement::Opposite,
                attachment_start: 0.12,
                length: 0.2,
                length_envelope: 0.35,
                width: 0.55,
                angle: 0.25,
                spread: 0.5,
                segment: PlantSegment {
                    profile: 0.6,
                    stiffness: 0.55,
                    droop: 0.45,
                    twist: 0.03,
                    fold: 0.1,
                    root_color: Srgb::new(0.05, 0.13, 0.04),
                    tip_color: Srgb::new(0.14, 0.30, 0.08),
                },
            },
            PlantBranches::NONE,
        ],
        ..PlantParams::FERN
    };

    BRAMBLE {
        name: "Bramble",
        description: "Bramble canes that arch over and root their tips back in the ground.\n\n\
                      Thick brown canes leave the crown low, and stubby side shoots carry \
                      clusters of large, dark leaves.",
    } => Self {
        density: 0.4,
        height: Vec2::new(1.6, 3.0),
        width: 0.02,
        lean: 0.4,
        stems_per_plant: 4,
        root_occlusion: 0.3,
        translucency: 0.3,
        specular: 0.3,
        // Droop 2 brings every cane's tip down to the ground; fully stiff canes keep
        // the arch steady in the wind.
        segment: PlantSegment {
            profile: 0.0,
            stiffness: 1.0,
            droop: 2.0,
            twist: 0.0,
            fold: 0.0,
            root_color: Srgb::new(0.22, 0.10, 0.07),
            tip_color: Srgb::new(0.34, 0.15, 0.10),
        },
        // Eight stubby side shoots per cane, each with a cluster of eight large
        // leaves. The counts match huckleberry's, so the two share one mesh.
        levels: [
            PlantBranches {
                count: 8,
                arrangement: PlantArrangement::Alternate,
                attachment_start: 0.12,
                length: 0.07,
                length_envelope: 0.5,
                width: 0.09,
                angle: 0.35,
                spread: 0.0,
                segment: PlantSegment {
                    profile: 0.0,
                    stiffness: 0.8,
                    droop: 0.1,
                    twist: 0.0,
                    fold: 0.0,
                    root_color: Srgb::new(0.24, 0.11, 0.07),
                    tip_color: Srgb::new(0.20, 0.16, 0.07),
                },
            },
            // Leaves longer than their stubby shoot, bunched from a third of the way
            // along it.
            PlantBranches {
                count: 8,
                arrangement: PlantArrangement::Alternate,
                attachment_start: 0.3,
                length: 1.1,
                length_envelope: 0.6,
                width: 0.6,
                angle: 0.3,
                spread: 0.24,
                segment: PlantSegment {
                    profile: 0.8,
                    stiffness: 0.6,
                    droop: 0.2,
                    twist: 0.03,
                    fold: 0.2,
                    root_color: Srgb::new(0.03, 0.08, 0.03),
                    tip_color: Srgb::new(0.06, 0.15, 0.04),
                },
            },
        ],
        ..PlantParams::FERN
    };

    MATURE_BRACKEN {
        name: "Mature bracken",
        description: "Head-high western bracken with broad, triangular fronds.\n\n\
                      Tall, bare stalks arch under two levels of branches and leaflets.",
    } => Self {
        density: 1.0,
        height: Vec2::new(1.6, 2.4),
        width: 0.012,
        lean: 0.12,
        stems_per_plant: 3,
        root_occlusion: 0.5,
        translucency: 0.45,
        specular: 0.15,
        // Strong droop makes the stalk arch under the broad frond.
        segment: PlantSegment {
            profile: 0.0,
            stiffness: 0.4,
            droop: 0.8,
            twist: 0.02,
            fold: 0.0,
            root_color: Srgb::new(0.22, 0.18, 0.08),
            tip_color: Srgb::new(0.24, 0.36, 0.10),
        },
        // Five branch pairs shorten towards the tip to form a triangular frond. Six
        // leaflet pairs on each branch add the finer outline.
        levels: [
            PlantBranches {
                count: 10,
                arrangement: PlantArrangement::Opposite,
                attachment_start: 0.5,
                length: 0.36,
                length_envelope: 0.12,
                width: 0.02,
                angle: 0.2,
                spread: 0.0,
                segment: PlantSegment {
                    profile: 0.0,
                    stiffness: 0.45,
                    droop: 0.35,
                    twist: 0.0,
                    fold: 0.1,
                    root_color: Srgb::new(0.16, 0.30, 0.07),
                    tip_color: Srgb::new(0.22, 0.42, 0.10),
                },
            },
            PlantBranches {
                count: 12,
                arrangement: PlantArrangement::Opposite,
                attachment_start: 0.08,
                length: 0.28,
                length_envelope: 0.3,
                width: 0.5,
                angle: 0.25,
                spread: 0.0,
                segment: PlantSegment {
                    profile: 0.3,
                    stiffness: 0.5,
                    droop: 0.15,
                    twist: 0.02,
                    fold: 0.15,
                    root_color: Srgb::new(0.12, 0.30, 0.06),
                    tip_color: Srgb::new(0.32, 0.58, 0.12),
                },
            },
        ],
        ..PlantParams::FERN
    };
}
}

/// Clusters a plant among the moss, at `density_scale` of its own density.
const fn over_moss(plant: &PlantParams, density_scale: f32) -> ScatterPlant {
    ScatterPlant::new(*plant)
        .scaled(density_scale)
        .clustered(0.6)
}

/// Moss under shrubs: 120 plants per square metre, sparser than a moss
/// carpet, clustered to leave gaps for the plant listed after it.
const fn moss_around(clustering: f32) -> ScatterPlant {
    ScatterPlant::new(PlantParams::MOSS)
        .scaled(0.6)
        .clustered(clustering)
}

/// A moss base with bare patches, shared by the understory presets, leaving
/// room for larger plants.
const UNDER_MOSS: PlantScatterOptions = PlantScatterOptions {
    density_scale: 1.0,
    variation: PlantVariation {
        bare_share: 0.12,
        ..PlantScatterPreset::FOREST_MOSS.plant_scatter.variation
    },
    ground_tint: GroundTint {
        amount: 0.4,
        ..PlantScatterPreset::FOREST_MOSS.plant_scatter.ground_tint
    },
    ..PlantScatterPreset::FOREST_MOSS.plant_scatter
};

preset_group! {
/// Forest shrubs and ferns growing above a moss carpet.
///
/// Choose a single dominant plant or combine them with `FOREST_UNDERSTORY`.
UNDERSTORY "Understory" {
    SALAL {
        name: "Salal",
        description: "Salal thickets above moss.\n\n\
                      Glossy leaves form dense clumps among the ground cover.",
    } => PlantScatterOptions {
        // Moss fills the gaps between salal clumps.
        plants: Cow::Borrowed(&[
            moss_around(0.6),
            ScatterPlant::new(PlantParams::SALAL),
        ]),
        ..UNDER_MOSS
    };
    SAPLING {
        name: "Saplings",
        description: "Scattered young trees above moss.\n\n\
                      Sparse sapling clumps leave the ground cover visible.",
    } => PlantScatterOptions {
        // Mostly moss, with small groups of young trees.
        plants: Cow::Borrowed(&[
            moss_around(0.6),
            ScatterPlant::new(PlantParams::SAPLING),
        ]),
        ..UNDER_MOSS
    };
    HUCKLEBERRY {
        name: "Huckleberry",
        description: "Airy red huckleberry bushes above moss.\n\n\
                      Loose clumps add taller foliage without hiding all the ground cover.",
    } => PlantScatterOptions {
        // Moss leaves room for sparse groups of huckleberry bushes.
        plants: Cow::Borrowed(&[
            moss_around(0.6),
            ScatterPlant::new(PlantParams::HUCKLEBERRY),
        ]),
        ..UNDER_MOSS
    };
    OREGON_GRAPE {
        name: "Oregon grape",
        description: "Low Oregon grape patches above moss.\n\n\
                      Glossy, paired leaflets form broad clumps close to the ground.",
    } => PlantScatterOptions {
        // Moss grows between patches of low Oregon grape shrubs.
        plants: Cow::Borrowed(&[
            moss_around(0.6),
            ScatterPlant::new(PlantParams::OREGON_GRAPE),
        ]),
        ..UNDER_MOSS
    };
    NETTLE {
        name: "Nettles",
        description: "Dense nettle beds above moss.\n\n\
                      Strong clustering leaves gaps between groups of upright stems.",
    } => PlantScatterOptions {
        // Strong clustering gathers nettles into beds, with moss between them.
        plants: Cow::Borrowed(&[
            moss_around(0.85),
            ScatterPlant::new(PlantParams::NETTLE),
        ]),
        ..UNDER_MOSS
    };
    MATURE_BRACKEN {
        name: "Mature bracken",
        description: "Loose stands of head-high bracken above moss.\n\n\
                      Broad fronds provide a tall layer of forest ground cover.",
    } => PlantScatterOptions {
        // Moss surrounds loose groups of tall bracken ferns.
        plants: Cow::Borrowed(&[
            moss_around(0.5),
            ScatterPlant::new(PlantParams::MATURE_BRACKEN),
        ]),
        ..UNDER_MOSS
    };
    BRAMBLE {
        name: "Bramble",
        description: "Bramble thickets of arching canes above moss.\n\n\
                      Canes loop out from each crown and back into the ground, in tangled \
                      clumps.",
    } => PlantScatterOptions {
        // Widely spaced bramble plants spread their arching stems over the moss.
        plants: Cow::Borrowed(&[
            moss_around(0.75),
            ScatterPlant::new(PlantParams::BRAMBLE),
        ]),
        ..UNDER_MOSS
    };
    FOREST_UNDERSTORY {
        name: "Forest understory",
        description: "Mixed forest shrubs and ferns above moss and fallen leaves.\n\n\
                      Combines all understory plants in clumps, with leaf litter between \
                      them.",
    } => PlantScatterOptions {
        plants: Cow::Borrowed(&[
            // Together about 3 shrubs and ferns per square metre before thinning.
            over_moss(&PlantParams::FERN, 0.3),
            over_moss(&PlantParams::SALAL, 0.4),
            over_moss(&PlantParams::OREGON_GRAPE, 0.25),
            over_moss(&PlantParams::HUCKLEBERRY, 0.5),
            over_moss(&PlantParams::SAPLING, 0.4),
            over_moss(&PlantParams::MATURE_BRACKEN, 0.1),
            over_moss(&PlantParams::NETTLE, 0.1),
            ScatterPlant::new(PlantParams::LEAF_LITTER)
                .scaled(0.25)
                .clustered(0.35),
            ScatterPlant::new(PlantParams::MOSS).scaled(0.55),
        ]),
        ..UNDER_MOSS
    };
}
}
