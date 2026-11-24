//! Ready-made groves, each stored as an embedded `.grove.json` file.

use super::TreeGrove;

/// Built-in groves: woodlands, conifer stands, an orchard and seasonal mixes.
///
/// Use [`grove`](Self::grove) to get an editable [`TreeGrove`], for example
/// `GrovePreset::PineStand.grove()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GrovePreset {
    /// Oaks of two sizes, with ash in small stands and a few bushes.
    OakWoodland,
    /// Aspens of three sizes, young ones gathered in clumps.
    AspenStand,
    /// Maple, ash, aspen and oak mixed, each in loose stands.
    MixedDeciduous,
    /// Pines of three sizes, young ones gathered in clumps.
    PineStand,
    /// Cedars with a few pines and evergreen bushes.
    CedarGrove,
    /// Apple trees, evenly spread, nearly upright and of similar size.
    Orchard,
    /// Flowering dogwoods with clumps of pussy willow in catkin.
    SpringBlossom,
    /// Bare winter thickets with a few pussy willows.
    WinterCopse,
}

impl GrovePreset {
    /// Every preset, in display order.
    pub const ALL: [GrovePreset; 8] = [
        Self::OakWoodland,
        Self::AspenStand,
        Self::MixedDeciduous,
        Self::PineStand,
        Self::CedarGrove,
        Self::Orchard,
        Self::SpringBlossom,
        Self::WinterCopse,
    ];

    /// The preset's key, such as `"oak_woodland"`: lower case with
    /// underscores, as used for a grove file's `base`.
    pub fn key(self) -> &'static str {
        match self {
            Self::OakWoodland => "oak_woodland",
            Self::AspenStand => "aspen_stand",
            Self::MixedDeciduous => "mixed_deciduous",
            Self::PineStand => "pine_stand",
            Self::CedarGrove => "cedar_grove",
            Self::Orchard => "orchard",
            Self::SpringBlossom => "spring_blossom",
            Self::WinterCopse => "winter_copse",
        }
    }

    /// Looks up a preset by its exact [`key`](Self::key). Returns `None` for
    /// an unrecognised key.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|preset| preset.key() == key)
    }

    /// The display name, such as `"Oak Woodland"`.
    pub fn name(self) -> &'static str {
        match self {
            Self::OakWoodland => "Oak Woodland",
            Self::AspenStand => "Aspen Stand",
            Self::MixedDeciduous => "Mixed Deciduous",
            Self::PineStand => "Pine Stand",
            Self::CedarGrove => "Cedar Grove",
            Self::Orchard => "Orchard",
            Self::SpringBlossom => "Spring Blossom",
            Self::WinterCopse => "Winter Copse",
        }
    }

    /// One sentence describing the grove.
    pub fn description(self) -> &'static str {
        match self {
            Self::OakWoodland => {
                "Medium oaks with a few large ones, small stands of ash and scattered bushes."
            }
            Self::AspenStand => "Aspens of three sizes, the young ones in dense clumps.",
            Self::MixedDeciduous => "Big-leaf maple, ash, aspen and oak, each in loose stands.",
            Self::PineStand => "Pines of three sizes, the young ones in dense clumps.",
            Self::CedarGrove => "Cedars with a few pines and clumps of evergreen bushes.",
            Self::Orchard => "Apple trees spread evenly, standing straight and of similar size.",
            Self::SpringBlossom => "Flowering dogwoods with clumps of pussy willow in catkin.",
            Self::WinterCopse => "Leafless winter thickets with a few pussy willows.",
        }
    }

    /// The group the preset is listed under: `"Woodland"`, `"Conifer"`,
    /// `"Cultivated"` or `"Seasonal"`.
    pub fn group(self) -> &'static str {
        match self {
            Self::OakWoodland | Self::AspenStand | Self::MixedDeciduous => "Woodland",
            Self::PineStand | Self::CedarGrove => "Conifer",
            Self::Orchard => "Cultivated",
            Self::SpringBlossom | Self::WinterCopse => "Seasonal",
        }
    }

    /// The preset's `.grove.json` file, as embedded in the crate.
    pub fn json(self) -> &'static str {
        match self {
            Self::OakWoodland => include_str!("../../groves/oak_woodland.grove.json"),
            Self::AspenStand => include_str!("../../groves/aspen_stand.grove.json"),
            Self::MixedDeciduous => include_str!("../../groves/mixed_deciduous.grove.json"),
            Self::PineStand => include_str!("../../groves/pine_stand.grove.json"),
            Self::CedarGrove => include_str!("../../groves/cedar_grove.grove.json"),
            Self::Orchard => include_str!("../../groves/orchard.grove.json"),
            Self::SpringBlossom => include_str!("../../groves/spring_blossom.grove.json"),
            Self::WinterCopse => include_str!("../../groves/winter_copse.grove.json"),
        }
    }

    /// A fresh copy of the preset's grove, which can be edited without
    /// changing the preset.
    pub fn grove(self) -> TreeGrove {
        TreeGrove::from_json(self.json()).expect("built-in grove presets are valid grove files")
    }
}
