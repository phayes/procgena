//! The fruit and flower clusters a tree grows: which cluster, where on the
//! branches, and how many different clusters it draws from.

use procgena_fruit::FruitOptions;
use serde::{Deserialize, Serialize};

use crate::options::{Checker, OptionsError};

/// The number of distinct clusters a tree's fruit or flowers draw from
/// unless set otherwise; see [`AttachedFruit::default_slots`].
pub const DEFAULT_FRUIT_SLOTS: u32 = 6;

/// Fruit or flower clusters grown on a tree: what each cluster is, and
/// where the tree grows them.
///
/// The clusters themselves are `procgena_fruit` specimens. A tree grows a
/// few distinct ones, its *slots*, and each cluster on its branches shows
/// one of them, turned and scaled into place. Every tree grown from the same
/// settings and seed shows the same clusters in the same places.
///
/// In a `.tree.yml` file, [`fruit`](Self::fruit) is either the key of a
/// built-in `procgena_fruit` preset (`fruit: apple`) or a map of cluster
/// settings, which may name a preset as its `base` and give only what
/// differs. When deserialized, omitted fields use their defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AttachedFruit {
    /// What each cluster is: its arrangement, its items' shape, colour and
    /// maturity, and how much its items vary.
    #[serde(with = "fruit_file")]
    pub fruit: FruitOptions,
    /// Where on the branches clusters grow, and how each is turned and
    /// scaled there.
    pub placement: ClusterPlacement,
    /// How many distinct clusters the tree draws from when built on its own;
    /// at least 1 and at most 256. More gives more variety at the cost of
    /// more meshes. A World may set its own number. It is not part of the
    /// tree's options hash, but it is part of its
    /// [`TreeCacheKey`](crate::TreeCacheKey).
    pub default_slots: u32,
}

/// Where a tree grows fruit or flower clusters, and how it turns and scales
/// each one.
///
/// Set [`count`](Self::count), [`density`](Self::density) or
/// [`tips`](Self::tips) above zero for clusters to grow. When deserialized,
/// omitted fields use their defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClusterPlacement {
    /// First branch level allowed to carry clusters: 0 includes the trunk,
    /// 1 starts with branches growing from it, and so on. Deeper levels are
    /// also included. A value beyond the tree's branch levels grows no clusters.
    pub min_level: u32,
    /// Chance of an extra cluster at each eligible branch tip, from 0 (never)
    /// to 1 (always). The branch must meet [`min_level`](Self::min_level) and
    /// have no continuation at its tip: only the deepest level on deciduous
    /// trees, or any level on evergreen trees.
    pub tips: f32,
    /// Clusters along each eligible branch, in addition to any tip cluster.
    pub count: u32,
    /// Extra side clusters per metre of branch length, added to
    /// [`count`](Self::count). Only the part of the branch after
    /// [`start`](Self::start) is measured, so long branches get more clusters
    /// than short ones. Fractional results are rounded up or down at random.
    /// Must be finite and non-negative.
    pub density: f32,
    /// Start of the region used for side clusters, from 0 (branch base) to
    /// 1 (branch tip). Does not affect the chance of a tip cluster.
    pub start: f32,
    /// Maximum random tilt of each cluster away from the direction its
    /// cluster settings face ([`procgena_fruit::Facing`]), in degrees. Must
    /// be finite and non-negative; zero keeps that direction exactly.
    pub facing_jitter: f32,
    /// Size of each cluster relative to its cluster settings: 1 keeps it, 0.5
    /// halves it, stalks included. Must be finite and above zero.
    pub scale: f32,
    /// Random variation of [`scale`](Self::scale), from 0 (none) to 1. For
    /// example, 0.2 scales each cluster by 80% to 120% of `scale`.
    pub scale_variance: f32,
}

impl Default for AttachedFruit {
    /// The default cluster settings, two side clusters per branch from
    /// level 1, and [`DEFAULT_FRUIT_SLOTS`] slots.
    fn default() -> Self {
        Self {
            fruit: FruitOptions::default(),
            placement: ClusterPlacement::default(),
            default_slots: DEFAULT_FRUIT_SLOTS,
        }
    }
}

impl Default for ClusterPlacement {
    /// Two side clusters per branch from level 1, over the outer 70% of the
    /// branch, tilted up to 15°, at their own size.
    fn default() -> Self {
        Self {
            min_level: 1,
            tips: 0.0,
            count: 2,
            density: 0.0,
            start: 0.3,
            facing_jitter: 15.0,
            scale: 1.0,
            scale_variance: 0.0,
        }
    }
}

impl AttachedFruit {
    /// Clusters from preset settings `fruit`, placed with `placement`, with
    /// [`DEFAULT_FRUIT_SLOTS`] slots.
    pub fn new(fruit: FruitOptions, placement: ClusterPlacement) -> Self {
        Self {
            fruit,
            placement,
            default_slots: DEFAULT_FRUIT_SLOTS,
        }
    }

    /// Whether any cluster can grow: some count, density or tip chance.
    pub fn grows(&self) -> bool {
        let p = &self.placement;
        p.count > 0 || p.density > 0.0 || p.tips > 0.0
    }

    /// Checks the placement, the slot count and the cluster settings,
    /// naming failing settings under `v`'s group.
    pub(crate) fn validate(&self, v: &Checker) -> Result<(), OptionsError> {
        let p = &self.placement;
        v.unit(p.tips, "placement.tips")?;
        v.non_negative(p.density, "placement.density")?;
        v.unit(p.start, "placement.start")?;
        v.non_negative(p.facing_jitter, "placement.facing_jitter")?;
        v.check(
            p.scale > 0.0 && p.scale.is_finite(),
            "placement.scale",
            "a finite value above 0",
        )?;
        v.unit(p.scale_variance, "placement.scale_variance")?;
        v.check(
            (1..=procgena_fruit::MAX_SLOTS).contains(&self.default_slots),
            "default_slots",
            "between 1 and 256",
        )?;
        self.fruit.validate().map_err(|error| match error {
            procgena_fruit::OptionsError::InvalidSetting { field, requirement } => {
                OptionsError::InvalidSetting {
                    field: format!("{}.fruit.{field}", v.0),
                    requirement,
                }
            }
            _ => OptionsError::InvalidSetting {
                field: format!("{}.fruit", v.0),
                requirement: "valid fruit settings",
            },
        })
    }
}

/// Reads and writes [`AttachedFruit::fruit`] as a preset key or a map of
/// settings with an optional `base` preset key.
mod fruit_file {
    use procgena_core::preset::{from_value, merge, to_value};
    use procgena_fruit::{FruitOptions, FruitPreset};
    use serde::de::Error;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use serde_json::Value;

    /// Writes the key of the built-in preset with these exact settings, or
    /// every setting when none matches.
    pub(crate) fn serialize<S: Serializer>(
        fruit: &FruitOptions,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match FruitPreset::key_of(fruit) {
            Some(key) => serializer.serialize_str(key),
            None => fruit.serialize(serializer),
        }
    }

    /// Reads a preset key, or settings laid over a `base` preset's or over
    /// the defaults.
    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<FruitOptions, D::Error> {
        let preset = |key: &str| {
            FruitPreset::named(key).ok_or_else(|| {
                let keys: Vec<_> = FruitPreset::all().map(|p| p.key.as_ref()).collect();
                D::Error::custom(format!(
                    "unknown fruit preset {key:?}; valid keys are {}",
                    keys.join(", ")
                ))
            })
        };
        match Value::deserialize(deserializer)? {
            Value::String(key) => Ok(preset(&key)?.fruit.clone()),
            Value::Object(mut settings) => {
                let mut value = match settings.remove("base") {
                    Some(Value::String(key)) => to_value(&preset(&key)?.fruit),
                    Some(_) => return Err(D::Error::custom("fruit.base must be a preset key")),
                    None => to_value(&FruitOptions::default()),
                };
                merge(&mut value, Value::Object(settings));
                from_value(value, "fruit").map_err(D::Error::custom)
            }
            _ => Err(D::Error::custom(
                "fruit must be a preset key or a map of settings",
            )),
        }
    }
}
