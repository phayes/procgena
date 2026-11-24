//! Ready-made tree settings covering several species and sizes: the presets
//! from ez-tree, plus trees that grow fruit or flowers.
//!
//! Use [`TreePreset::options`] to get settings that can be customised, for example
//! `TreePreset::OakSmall.options()`. Missing settings in the embedded preset JSON
//! are filled from [`TreeOptions::default()`].

use crate::options::TreeOptions;

/// Steepest ground shrub and sapling presets grow on in a grove, in degrees.
const SHRUB_MAX_SLOPE: f32 = 45.0;

/// Built-in starting points for tree settings, including several sizes and species.
///
/// Use [`options`](Self::options) to get editable settings, for example
/// `TreePreset::OakSmall.options()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TreePreset {
    AshSmall,
    AshMedium,
    AshLarge,
    AspenSmall,
    AspenMedium,
    AspenLarge,
    Bush1,
    Bush2,
    Bush3,
    OakSmall,
    OakMedium,
    OakLarge,
    PineSmall,
    PineMedium,
    PineLarge,
    /// A broadleaf tree configured to grow along a grid of guide wires.
    Trellis,
    /// A single-trunked bigleaf maple of moderate height with a broad crown
    /// of large autumn-coloured leaves.
    BigLeafMapleMedium,
    /// A tall, many-stemmed bigleaf maple with a broad crown of large
    /// autumn-coloured leaves.
    BigLeafMapleLarge,
    /// A short, spreading apple tree hung with ripe apples.
    Apple,
    /// A low-forking flowering dogwood in full bloom, covered in white,
    /// four-bracted flowers that face the sky.
    FloweringDogwood,
    /// A many-stemmed pussy willow shrub in early spring: bare upright stems
    /// studded with fuzzy, upward-pointing catkins.
    SpringPussyWillow,
    /// A dense, many-stemmed willow thicket in winter: bare upright stems
    /// with no leaves, fruit or flowers.
    WinterThicket,
    /// A tall, narrow cedar with a single straight trunk and layered branches
    /// carrying flat, horizontal sprays of green foliage.
    Cedar,
}

impl TreePreset {
    /// Every preset, in an order suitable for a selection menu: the general
    /// species and sizes first, followed by the specialty trees such as fruit,
    /// flowering and winter trees.
    pub const ALL: [TreePreset; 23] = [
        Self::AshSmall,
        Self::AshMedium,
        Self::AshLarge,
        Self::AspenSmall,
        Self::AspenMedium,
        Self::AspenLarge,
        Self::Bush1,
        Self::Bush2,
        Self::Bush3,
        Self::OakSmall,
        Self::OakMedium,
        Self::OakLarge,
        Self::PineSmall,
        Self::PineMedium,
        Self::PineLarge,
        Self::Trellis,
        Self::BigLeafMapleMedium,
        Self::BigLeafMapleLarge,
        Self::Apple,
        Self::FloweringDogwood,
        Self::SpringPussyWillow,
        Self::WinterThicket,
        Self::Cedar,
    ];

    /// The 13 presets whose settings match ez-tree's own presets, in ez-tree
    /// display order. They grow no fruit or flowers. Their shapes differ from
    /// ez-tree's, since this crate draws its own random numbers.
    pub const EZ_TREE: [TreePreset; 13] = [
        Self::AshSmall,
        Self::AshMedium,
        Self::AshLarge,
        Self::AspenSmall,
        Self::AspenMedium,
        Self::AspenLarge,
        Self::Bush1,
        Self::Bush2,
        Self::Bush3,
        Self::PineSmall,
        Self::PineMedium,
        Self::PineLarge,
        Self::Trellis,
    ];

    /// Returns the display name, such as `"Ash Small"`.
    pub fn name(self) -> &'static str {
        match self {
            Self::AshSmall => "Ash Small",
            Self::AshMedium => "Ash Medium",
            Self::AshLarge => "Ash Large",
            Self::AspenSmall => "Aspen Small",
            Self::AspenMedium => "Aspen Medium",
            Self::AspenLarge => "Aspen Large",
            Self::Bush1 => "Bush 1",
            Self::Bush2 => "Bush 2",
            Self::Bush3 => "Bush 3",
            Self::OakSmall => "Oak Small",
            Self::OakMedium => "Oak Medium",
            Self::OakLarge => "Oak Large",
            Self::PineSmall => "Pine Small",
            Self::PineMedium => "Pine Medium",
            Self::PineLarge => "Pine Large",
            Self::Trellis => "Trellis",
            Self::BigLeafMapleMedium => "Big Leaf Maple Medium",
            Self::BigLeafMapleLarge => "Big Leaf Maple Large",
            Self::Apple => "Apple",
            Self::FloweringDogwood => "Flowering Dogwood",
            Self::SpringPussyWillow => "Spring Pussy Willow",
            Self::WinterThicket => "Winter Thicket",
            Self::Cedar => "Cedar",
        }
    }

    /// Looks up an exact display name from [`name`](Self::name).
    ///
    /// Matching is case-sensitive and includes spaces: `"Ash Small"` matches,
    /// while `"ash_small"` does not. Returns `None` for an unrecognised name.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|preset| preset.name() == name)
    }

    /// Returns the embedded preset JSON. It may omit settings; use
    /// [`TreeOptions::from_json`] or [`options`](Self::options) to fill in defaults.
    pub fn json(self) -> &'static str {
        match self {
            Self::AshSmall => include_str!("../presets/ash_small.json"),
            Self::AshMedium => include_str!("../presets/ash_medium.json"),
            Self::AshLarge => include_str!("../presets/ash_large.json"),
            Self::AspenSmall => include_str!("../presets/aspen_small.json"),
            Self::AspenMedium => include_str!("../presets/aspen_medium.json"),
            Self::AspenLarge => include_str!("../presets/aspen_large.json"),
            Self::Bush1 => include_str!("../presets/bush_1.json"),
            Self::Bush2 => include_str!("../presets/bush_2.json"),
            Self::Bush3 => include_str!("../presets/bush_3.json"),
            Self::OakSmall => include_str!("../presets/oak_small.json"),
            Self::OakMedium => include_str!("../presets/oak_medium.json"),
            Self::OakLarge => include_str!("../presets/oak_large.json"),
            Self::PineSmall => include_str!("../presets/pine_small.json"),
            Self::PineMedium => include_str!("../presets/pine_medium.json"),
            Self::PineLarge => include_str!("../presets/pine_large.json"),
            Self::Trellis => include_str!("../presets/trellis.json"),
            Self::BigLeafMapleMedium => include_str!("../presets/big_leaf_maple_medium.json"),
            Self::BigLeafMapleLarge => include_str!("../presets/big_leaf_maple_large.json"),
            Self::Apple => include_str!("../presets/apple.json"),
            Self::FloweringDogwood => include_str!("../presets/flowering_dogwood.json"),
            Self::SpringPussyWillow => include_str!("../presets/spring_pussy_willow.json"),
            Self::WinterThicket => include_str!("../presets/winter_thicket.json"),
            Self::Cedar => include_str!("../presets/cedar.json"),
        }
    }

    /// Returns a fresh set of options with preset values applied over the defaults.
    /// The returned settings can be edited without changing the preset.
    pub fn options(self) -> TreeOptions {
        TreeOptions::from_json(self.json()).expect("built-in presets are valid tree options")
    }

    /// Returns the preset's key, such as `"oak_small"`: lower case with
    /// underscores, as used in grove files.
    pub fn key(self) -> &'static str {
        match self {
            Self::AshSmall => "ash_small",
            Self::AshMedium => "ash_medium",
            Self::AshLarge => "ash_large",
            Self::AspenSmall => "aspen_small",
            Self::AspenMedium => "aspen_medium",
            Self::AspenLarge => "aspen_large",
            Self::Bush1 => "bush_1",
            Self::Bush2 => "bush_2",
            Self::Bush3 => "bush_3",
            Self::OakSmall => "oak_small",
            Self::OakMedium => "oak_medium",
            Self::OakLarge => "oak_large",
            Self::PineSmall => "pine_small",
            Self::PineMedium => "pine_medium",
            Self::PineLarge => "pine_large",
            Self::Trellis => "trellis",
            Self::BigLeafMapleMedium => "big_leaf_maple_medium",
            Self::BigLeafMapleLarge => "big_leaf_maple_large",
            Self::Apple => "apple",
            Self::FloweringDogwood => "flowering_dogwood",
            Self::SpringPussyWillow => "spring_pussy_willow",
            Self::WinterThicket => "winter_thicket",
            Self::Cedar => "cedar",
        }
    }

    /// Looks up a preset by its exact [`key`](Self::key), such as
    /// `"oak_small"`. Returns `None` for an unrecognised key.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|preset| preset.key() == key)
    }

    /// Trees per hectare (10,000 m²) that suit this preset growing in a
    /// grove of its own kind: about one tree for each crown's area, so the
    /// canopy closes. Spacing then leaves out a few of these, about one in
    /// six.
    ///
    /// [`Trellis`](Self::Trellis) is a garden support rather than a grove
    /// tree, so its density is 0; set one yourself to grow it in a grove.
    pub fn grove_density(self) -> f32 {
        self.grove_stand().0
    }

    /// The smallest distance between this preset's trunk and a neighbour's
    /// that suits it in a grove, in metres: about half its crown's radius,
    /// so neighbouring crowns can interlock but trunks never crowd.
    pub fn grove_spacing(self) -> f32 {
        self.grove_stand().1
    }

    /// The steepest ground this preset grows on in a grove, in degrees, when
    /// it tolerates steeper ground than the grove's own
    /// [`max_slope`](crate::GroveVariation::max_slope): 45° for shrubs and
    /// saplings, `None` for everything else.
    pub fn grove_max_slope(self) -> Option<f32> {
        match self {
            Self::Bush1
            | Self::Bush2
            | Self::Bush3
            | Self::AspenSmall
            | Self::PineSmall
            | Self::SpringPussyWillow => Some(SHRUB_MAX_SLOPE),
            _ => None,
        }
    }

    /// Grove density in trees per hectare, and grove spacing in metres, from
    /// each preset's widest branch or leaf `r` from the trunk: a density of
    /// 10,000 / (π r²) and a spacing of 0.55 r.
    fn grove_stand(self) -> (f32, f32) {
        match self {
            Self::AshSmall => (510.0, 1.4),
            Self::AshMedium => (260.0, 1.9),
            Self::AshLarge => (160.0, 2.5),
            Self::AspenSmall => (2600.0, 0.6),
            Self::AspenMedium => (980.0, 1.0),
            Self::AspenLarge => (550.0, 1.3),
            Self::Bush1 => (880.0, 1.05),
            Self::Bush2 => (1100.0, 0.95),
            Self::Bush3 => (800.0, 1.1),
            Self::OakSmall => (720.0, 1.15),
            Self::OakMedium => (400.0, 1.55),
            Self::OakLarge => (130.0, 2.7),
            Self::PineSmall => (2600.0, 0.6),
            Self::PineMedium => (1100.0, 0.95),
            Self::PineLarge => (550.0, 1.3),
            Self::Trellis => (0.0, 0.8),
            Self::BigLeafMapleMedium => (310.0, 1.75),
            Self::BigLeafMapleLarge => (140.0, 2.6),
            Self::Apple => (650.0, 1.2),
            Self::FloweringDogwood => (470.0, 1.45),
            Self::SpringPussyWillow => (2600.0, 0.6),
            Self::WinterThicket => (1250.0, 0.9),
            Self::Cedar => (480.0, 1.4),
        }
    }
}
