//! Settings for one fruit or flower cluster: its arrangement, its items'
//! shape and colour, and how much the specimens of one pool differ.
//!
//! Use [`FruitOptions::default()`] or a [`crate::FruitPreset`] as a starting
//! point, or read a `.fruit.yml` file with [`crate::FruitPreset::from_yaml`].

use std::fmt;

use serde::{Deserialize, Serialize};

/// Settings for one fruit or flower cluster, such as an apple on its
/// stalk, a dogwood bloom or a spray of willow catkins.
///
/// A cluster is built in its own frame: it starts at the origin, where it
/// attaches to its host, and local +Y is the direction it faces. Its host
/// (a tree) decides where each copy grows and turns it into place.
/// Lengths are in metres. Colours are sRGB values written as `0xRRGGBB`.
///
/// Deserializing directly with serde requires every field; a `.fruit.yml`
/// preset file read with [`crate::FruitPreset::from_yaml`] gives only what
/// differs from a base.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FruitOptions {
    /// Starting value for repeatable random choices when the cluster is
    /// built on its own, or grown on a host outside a World. Change it for
    /// a different set of clusters with the same settings.
    pub default_seed: u32,
    /// The direction the cluster is shaped to face on its host. Droop bends
    /// stems toward the ground for this facing.
    pub facing: Facing,
    /// The arrangement of items in the cluster and their supporting stalks.
    pub cluster: ClusterShape,
    /// Base size of each fruit or flower in metres, before random variation
    /// and cluster taper. Must be finite and non-negative.
    pub size: f32,
    /// Random size variation from 0 (all the same base size) to 1. For
    /// example, 0.2 allows sizes from 80% to 120% of [`size`](Self::size),
    /// before cluster taper is applied.
    pub size_variance: f32,
    /// Whether the items are fruit or flowers, and their shape and colours.
    pub item: ItemShape,
    /// Ripeness or bloom stage, from 0 to 1. Fruit grow from 30% to 100% of
    /// their size, blend from their unripe to their ripe colour and develop
    /// their blush. Flowers go from a closed bud (0), through fully open
    /// (0.5), to faded (1). Does not move items or change their stalks.
    pub maturity: f32,
    /// How much items differ from each other, so that the specimens of one
    /// pool differ. All zero (the default) makes every item alike apart
    /// from its size.
    pub variation: Variation,
}

/// The direction a cluster faces on its host, before the host's random tilt.
/// Up and down refer to the host's +Y axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Facing {
    /// Downward, as for hanging fruit.
    Hang,
    /// Upward, as for an upright flower cluster or a catkin.
    Up,
    /// Away from the side of the branch at a right angle to it. At a branch
    /// tip, along the direction the branch grows.
    Outward,
}

/// Arrangement and supporting stalks of one fruit or flower cluster.
///
/// An *item* is one fruit or flower. The *axis* is the main stem supporting
/// a cluster. Lengths and radii are in metres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClusterShape {
    /// Pattern used to arrange the items. Files call it `arrangement`.
    #[serde(rename = "arrangement")]
    pub kind: ClusterKind,
    /// Base number of items per cluster; must be at least 1.
    /// [`ClusterKind::Single`] always has exactly one.
    pub items: u32,
    /// Random variation in the item count, from 0 to 1. For example, 0.2
    /// varies the count by up to 20%, rounded to a whole number with a
    /// minimum of one. Ignored by [`ClusterKind::Single`].
    pub items_variance: f32,
    /// Angle in degrees at which stalks fan out from the cluster direction
    /// for an umbel, or from their supporting stem for a raceme or panicle.
    /// For a catkin, sets the angle of the items themselves. Ignored for a
    /// single item. Must be finite and non-negative.
    pub spread: f32,
    /// Length of the main supporting stem for racemes, panicles and catkins.
    /// Excludes a catkin's bare stalk. Must be finite and non-negative.
    pub axis_length: f32,
    /// Downward bending of the main stem and panicle side stems, from 0
    /// (no added downward bend) to 1 (strongest bend). The stem still curves
    /// away from the branch toward the facing direction first. Has no effect
    /// on single items or umbels.
    pub droop: f32,
    /// Reduction in item size toward the main stem's tip, from 0 (no
    /// reduction) to 1 (approaching zero at the tip). Also shortens item
    /// stalks and panicle side stems. Has no effect on single items or umbels.
    pub taper: f32,
    /// Number of side stems for a panicle; must be at least 1. Items are
    /// shared among them. Fewer stems are used if there are fewer items.
    pub side_stalks: u32,
    /// Base length of each panicle side stem as a fraction of
    /// [`axis_length`](Self::axis_length), before taper shortens it.
    /// Must be finite and non-negative; values above 1 are allowed.
    pub side_length: f32,
    /// Length of each item's stalk, before taper. A single item or umbel
    /// grows straight from the host; raceme items grow from the main stem,
    /// and panicle items from side stems. For catkins, this is the bare stem
    /// before the items begin. Must be finite and non-negative; zero
    /// attaches items directly to their host or supporting stem.
    pub stalk_length: f32,
    /// Thickness of all cluster stalks and stems, as a radius. Must be
    /// finite and non-negative.
    pub stalk_radius: f32,
    /// Colour of the supporting stalks and stems, written as `0xRRGGBB`.
    pub stalk_color: u32,
}

/// Ways to arrange fruit or flowers and their supporting stalks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClusterKind {
    /// One fruit or flower, with an optional stalk, such as an apple.
    Single,
    /// An umbrella-like group: stalks fan out from one point, each carrying
    /// one fruit or flower, as in a small group of cherries.
    Umbel,
    /// One main stem with individually stalked items along it, as in a
    /// spray of laburnum flowers.
    Raceme,
    /// A branching cluster: side stems grow from a main stem and each carry
    /// items, as in a bunch of grapes.
    Panicle,
    /// Items attached directly along a main stem, without individual
    /// stalks, forming a narrow cluster like a willow catkin.
    Catkin,
}

/// Whether a cluster's items are fruit or flowers, with their shape.
///
/// Files write it as a map whose `kind` is `fruit` or `flower`, followed by
/// that shape's settings. A file that changes the `kind` replaces the whole
/// shape rather than merging it with its base's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ItemShape {
    /// Fruit, such as apples or catkins drawn as fruit.
    Fruit(FruitShape),
    /// Flowers with petals, or rounded catkin flowers.
    Flower(FlowerShape),
}

/// Shape and colours of a fruit.
///
/// Lengths and radii are relative to each fruit's size after size
/// variation, cluster taper and maturity are applied. Colours are sRGB
/// values written as `0xRRGGBB`. Fruit in catkin clusters have no stems of
/// their own, so their stem settings have no effect. When deserialized,
/// omitted fields use their defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FruitShape {
    /// Height relative to width: 1 is round, below 1 is flattened, and above
    /// 1 is elongated. Must be finite and greater than zero.
    pub elongation: f32,
    /// Depth of the hollow around the stem, like the dip in an apple, from 0
    /// (none) to 1 (deepest).
    pub top_dimple: f32,
    /// Narrowing at the end opposite the stem, from 0 (none) to 1 (strongest).
    pub bottom_taper: f32,
    /// Number of raised ribs around the fruit. Zero describes a smooth fruit.
    pub lobes: u32,
    /// Strength of the ribbed shape, from 0 (smooth) to less than 1.
    pub lobe_depth: f32,
    /// Number of points around the fruit; at least 3. Higher values give a
    /// smoother outline.
    pub segments: u32,
    /// Number of rings of points from stem to bottom; at least 2.
    pub rings: u32,
    /// Length of the fruit's own short stem, relative to fruit size.
    /// Must be finite and non-negative.
    pub stem_length: f32,
    /// Radius of the fruit's own stem, relative to fruit size.
    /// Must be finite and non-negative.
    pub stem_radius: f32,
    /// Bend in the fruit's stem, from 0 (straight) to 1 (strongest).
    pub stem_curve: f32,
    /// Whether to show a calyx: the small remains of the flower at the end
    /// opposite the fruit's stem.
    pub calyx: bool,
    /// Ripe fruit colour, written as `0xRRGGBB`.
    pub color: u32,
    /// Unripe fruit colour, written as `0xRRGGBB`.
    pub unripe_color: u32,
    /// Colour of the blush, a contrasting patch on the fruit's skin.
    pub blush_color: u32,
    /// Strength of the blush colour, from 0 (absent) to 1 (strongest).
    pub blush_amount: f32,
}

/// Shape and colours of a flower.
///
/// Lengths are relative to each flower's size after size variation and
/// cluster taper. Colours are sRGB values written as `0xRRGGBB`. Flowers in
/// catkin clusters are rounded bumps and use only `petal_color`,
/// `bud_color` and `fade_color`. When deserialized, omitted fields use
/// their defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FlowerShape {
    /// Petals in each ring around the flower's centre; must be at least 1.
    pub petal_count: u32,
    /// Number of layers of petals around the centre; must be at least 1.
    pub rings: u32,
    /// Length of a petal relative to flower size; must be finite and
    /// non-negative.
    pub petal_length: f32,
    /// Maximum width of a petal relative to flower size; must be finite
    /// and non-negative.
    pub petal_width: f32,
    /// How strongly petals curve into a cup, from 0 (flat) to 1 (most closed).
    pub cup: f32,
    /// Backward bend at the petal tips, from 0 (none) to 1 (strongest).
    pub curl: f32,
    /// Depth of the indentation in each petal tip, from 0 (none) to 1 (deepest).
    pub notch: f32,
    /// Lengthwise divisions of each petal; at least 1. More divisions give
    /// smoother bends.
    pub petal_segments: u32,
    /// Width of the flower's centre relative to flower size; must be finite
    /// and non-negative.
    pub center_size: f32,
    /// Colour of the central part surrounded by the petals.
    pub center_color: u32,
    /// Main colour of the petals.
    pub petal_color: u32,
    /// Colour at the base of each petal, near the flower's centre.
    pub base_color: u32,
    /// Colour of an unopened flower bud.
    pub bud_color: u32,
    /// Colour of petals as the flower fades.
    pub fade_color: u32,
}

/// How much the items of a cluster differ from each other, each drawn at
/// random per item. Every value runs from 0 (no difference) to 1.
///
/// These are what make the specimens of one pool differ when each cluster
/// has a single item: two apples of one pool then differ in ripeness and
/// shape, not only in size.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Variation {
    /// How far each item's maturity strays from
    /// [`FruitOptions::maturity`], up or down. On fruit this changes size,
    /// colour and blush; on flowers it mixes buds, open flowers and fading
    /// flowers.
    pub maturity_spread: f32,
    /// How far each item's colours turn around the colour wheel, as a
    /// fraction of a quarter turn either way.
    pub hue_spread: f32,
    /// How much each fruit's blush strength strays, as a fraction of
    /// [`FruitShape::blush_amount`].
    pub blush_spread: f32,
    /// Lopsidedness of each item: a fruit's ribs turn and deepen and its
    /// height changes a little; a flower's petals turn and change length.
    pub shape_jitter: f32,
    /// How far each fruit's stem bend turns around the stem, as a fraction
    /// of a half turn either way.
    pub stem_curve_jitter: f32,
}

impl Default for FruitOptions {
    /// A single red fruit hanging on a short stalk.
    fn default() -> Self {
        Self {
            default_seed: 1,
            facing: Facing::Hang,
            cluster: ClusterShape::default(),
            size: 0.12,
            size_variance: 0.15,
            item: ItemShape::Fruit(FruitShape::default()),
            maturity: 1.0,
            variation: Variation::default(),
        }
    }
}

impl Default for ClusterShape {
    /// One item on a short stalk.
    fn default() -> Self {
        Self {
            kind: ClusterKind::Single,
            items: 1,
            items_variance: 0.0,
            spread: 30.0,
            axis_length: 0.2,
            droop: 0.0,
            taper: 0.0,
            side_stalks: 4,
            side_length: 0.4,
            stalk_length: 0.04,
            stalk_radius: 0.004,
            stalk_color: 0x5a6b2e,
        }
    }
}

impl Default for FruitShape {
    /// A round red fruit with a short stem and a calyx.
    fn default() -> Self {
        Self {
            elongation: 1.0,
            top_dimple: 0.0,
            bottom_taper: 0.0,
            lobes: 0,
            lobe_depth: 0.0,
            segments: 12,
            rings: 8,
            stem_length: 0.2,
            stem_radius: 0.04,
            stem_curve: 0.3,
            calyx: true,
            color: 0xb8262a,
            unripe_color: 0x7fa83a,
            blush_color: 0xe0552a,
            blush_amount: 0.0,
        }
    }
}

impl Default for FlowerShape {
    /// A white five-petalled flower with a yellow centre.
    fn default() -> Self {
        Self {
            petal_count: 5,
            rings: 1,
            petal_length: 0.5,
            petal_width: 0.35,
            cup: 0.2,
            curl: 0.0,
            notch: 0.0,
            petal_segments: 4,
            center_size: 0.15,
            center_color: 0xe8c840,
            petal_color: 0xffffff,
            base_color: 0xffd8e4,
            bud_color: 0xe08098,
            fade_color: 0xcbbca4,
        }
    }
}

impl ItemShape {
    /// Whether the items are fruit rather than flowers.
    pub fn is_fruit(&self) -> bool {
        matches!(self, Self::Fruit(_))
    }
}

/// A problem found by [`FruitOptions::validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OptionsError {
    /// A setting is out of range or not finite.
    InvalidSetting {
        /// The setting's name as files write it, such as `"cluster.items"`.
        field: String,
        /// What the setting must be, such as `"at least 1"`.
        requirement: &'static str,
    },
}

impl fmt::Display for OptionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSetting { field, requirement } => {
                write!(f, "fruit setting {field} must be {requirement}")
            }
        }
    }
}

impl std::error::Error for OptionsError {}

impl FruitOptions {
    /// Checks every setting's range, including settings the chosen
    /// arrangement or item kind does not use.
    ///
    /// # Errors
    ///
    /// Returns the first setting that is out of range.
    pub fn validate(&self) -> Result<(), OptionsError> {
        let v = Checker;
        v.unit(self.maturity, "maturity")?;
        v.non_negative(self.size, "size")?;
        v.unit(self.size_variance, "size_variance")?;

        // Item counts and the dimensions and bends of the supporting stalks.
        let c = &self.cluster;
        v.check(c.items >= 1, "cluster.items", "at least 1")?;
        v.unit(c.items_variance, "cluster.items_variance")?;
        v.non_negative(c.spread, "cluster.spread")?;
        v.non_negative(c.axis_length, "cluster.axis_length")?;
        v.unit(c.droop, "cluster.droop")?;
        v.unit(c.taper, "cluster.taper")?;
        v.check(c.side_stalks >= 1, "cluster.side_stalks", "at least 1")?;
        v.non_negative(c.side_length, "cluster.side_length")?;
        v.non_negative(c.stalk_length, "cluster.stalk_length")?;
        v.non_negative(c.stalk_radius, "cluster.stalk_radius")?;

        // The item's own shape.
        match &self.item {
            ItemShape::Fruit(s) => {
                v.check(
                    s.elongation > 0.0 && s.elongation.is_finite(),
                    "item.elongation",
                    "a finite value above 0",
                )?;
                v.unit(s.top_dimple, "item.top_dimple")?;
                v.unit(s.bottom_taper, "item.bottom_taper")?;
                v.check(
                    (0.0..1.0).contains(&s.lobe_depth),
                    "item.lobe_depth",
                    "at least 0 and below 1",
                )?;
                v.check(s.segments >= 3, "item.segments", "at least 3")?;
                v.check(s.rings >= 2, "item.rings", "at least 2")?;
                v.non_negative(s.stem_length, "item.stem_length")?;
                v.non_negative(s.stem_radius, "item.stem_radius")?;
                v.unit(s.stem_curve, "item.stem_curve")?;
                v.unit(s.blush_amount, "item.blush_amount")?;
            }
            ItemShape::Flower(s) => {
                v.check(s.petal_count >= 1, "item.petal_count", "at least 1")?;
                v.check(s.rings >= 1, "item.rings", "at least 1")?;
                v.non_negative(s.petal_length, "item.petal_length")?;
                v.non_negative(s.petal_width, "item.petal_width")?;
                v.unit(s.cup, "item.cup")?;
                v.unit(s.curl, "item.curl")?;
                v.unit(s.notch, "item.notch")?;
                v.check(s.petal_segments >= 1, "item.petal_segments", "at least 1")?;
                v.non_negative(s.center_size, "item.center_size")?;
            }
        }

        // How much items differ.
        let s = &self.variation;
        v.unit(s.maturity_spread, "variation.maturity_spread")?;
        v.unit(s.hue_spread, "variation.hue_spread")?;
        v.unit(s.blush_spread, "variation.blush_spread")?;
        v.unit(s.shape_jitter, "variation.shape_jitter")?;
        v.unit(s.stem_curve_jitter, "variation.stem_curve_jitter")
    }
}

/// Checks values and names the failing setting.
struct Checker;

impl Checker {
    /// Succeeds when `ok` is true; otherwise reports `field` and the
    /// requirement it failed.
    fn check(&self, ok: bool, field: &str, requirement: &'static str) -> Result<(), OptionsError> {
        if ok {
            Ok(())
        } else {
            Err(OptionsError::InvalidSetting {
                field: field.to_owned(),
                requirement,
            })
        }
    }

    /// Accepts values from 0 through 1; rejects NaN and infinities.
    fn unit(&self, value: f32, field: &str) -> Result<(), OptionsError> {
        self.check((0.0..=1.0).contains(&value), field, "between 0 and 1")
    }

    /// Accepts zero and positive finite values.
    fn non_negative(&self, value: f32, field: &str) -> Result<(), OptionsError> {
        self.check(
            (0.0..f32::INFINITY).contains(&value),
            field,
            "a finite value of 0 or more",
        )
    }
}
