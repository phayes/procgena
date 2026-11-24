//! Settings for tree shape, leaves, bark, guide wires for branch growth, and
//! the fruit and flowers a tree grows.
//!
//! Use [`TreeOptions::default()`] or a [`crate::TreePreset`] as a starting
//! point, or read a `.tree.yml` file with [`crate::TreePreset::from_yaml`].

use std::fmt;
use std::ops::{Index, IndexMut};

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::attached::AttachedFruit;

/// Maximum value of [`BranchOptions::levels`]: three levels above the trunk.
pub const MAX_LEVELS: u32 = 3;

/// Settings for a tree's shape, leaves, bark, optional grid of guide wires,
/// and the fruit and flowers it grows.
///
/// Start with [`TreeOptions::default()`] and adjust individual settings. For
/// example, `options.branch.length[0] = 1.2` sets the trunk length to 1.2 metres.
/// All lengths are in metres. These settings do not generate or draw a tree yet.
///
/// The settings follow ez-tree's, written in snake case. Deserializing
/// directly with serde requires every field; a `.tree.yml` preset file read
/// with [`crate::TreePreset::from_yaml`] gives only what differs from a base.
///
/// Bark and leaf texture identifiers are included, but texture objects are not.
/// Applications must supply the textures themselves.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeOptions {
    /// Starting value for repeatable random choices when the tree is built
    /// on its own. Change it to request a different tree shape while keeping
    /// the other settings. A tree in a Scatter takes its seed from there.
    pub default_seed: i32,
    /// Branching style: broadleaf or conifer-like.
    #[serde(rename = "type")]
    pub tree_type: TreeType,
    /// Colour, shading, and texture settings for the trunk and branches.
    pub bark: BarkOptions,
    /// Settings for the trunk and successive levels of child branches.
    pub branch: BranchOptions,
    /// Settings for where leaves appear and how they are drawn.
    pub leaves: LeafOptions,
    /// Guide-wire grid that can encourage branches to grow along a plane.
    pub trellis: TrellisOptions,
    /// The fruit clusters the tree grows, and where. `None`, the default,
    /// grows no fruit.
    pub fruit: Option<AttachedFruit>,
    /// The flower clusters the tree grows, and where. `None`, the default,
    /// grows no flowers. Fruit and flowers grow from separate random
    /// streams, so changing one never moves the other.
    pub flowers: Option<AttachedFruit>,
}

/// Branching styles used by ez-tree to shape the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TreeType {
    /// Broadleaf style, with a continuation at the tip of non-final branches.
    #[default]
    Deciduous,
    /// Conifer-like style, with branches tapering to a point and no tip continuation.
    Evergreen,
}

/// Flat-surface arrangement used to draw each leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Billboard {
    /// One rectangular surface per leaf.
    Single,
    /// Two rectangular surfaces at right angles, making leaves visible from more directions.
    Double,
}

/// How bark images repeat along each branch. Texture coordinates `u` and `v`
/// place the image around and along the branch, respectively.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BarkUvMode {
    /// Reverses the bark image on every other branch section to match the
    /// JavaScript ez-tree generator. Lower detail stretches the image along
    /// the branch. Bevy and glTF export omit bark normal maps in this mode
    /// because it reverses their lighting detail on every other section.
    Mirrored,
    /// The default. Repeats the bark image in the same direction along the
    /// branch and keeps it aligned when detail changes. Use this mode for
    /// bark normal maps, which add lighting detail without extra geometry.
    /// This mode is available in procgena_tree only.
    #[default]
    Continuous,
}

/// A setting with a separate value for the trunk and each of three branch levels.
///
/// Index 0 is the trunk; branches at each later level grow from the previous
/// level. For example, `options.branch.length[0]` is the trunk length, and
/// `options.branch.length[1]` is the length setting for branches on the trunk.
///
/// Files use a map such as `{"0": 20, "1": 20, "2": 10, "3": 1}`, rather than
/// a list. Direct serde deserialization requires all four keys. In a
/// `.tree.yml` preset file, omitted levels keep their base's values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PerLevel<T>(pub [T; 4]);

impl<T> Index<usize> for PerLevel<T> {
    type Output = T;

    /// # Panics
    ///
    /// Panics if `level` is outside `0..=3`.
    fn index(&self, level: usize) -> &T {
        &self.0[level]
    }
}

impl<T> IndexMut<usize> for PerLevel<T> {
    /// # Panics
    ///
    /// Panics if `level` is outside `0..=3`.
    fn index_mut(&mut self, level: usize) -> &mut T {
        &mut self.0[level]
    }
}

/// Colour, shading, and texture settings for the trunk and branches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BarkOptions {
    /// Texture-set identifier, such as `"Bark001"`, for the application to resolve.
    /// Setting this field does not load any textures.
    #[serde(rename = "type")]
    pub kind: String,
    /// Bark tint as `0xRRGGBB`; `0xffffff` leaves texture colours unchanged.
    pub tint: u32,
    /// Use faceted shading instead of a smooth-looking surface.
    pub flat_shading: bool,
    /// Whether bark should use textures supplied by the application.
    pub textured: bool,
    /// Texture scale around the branch (`x`) and along it (`y`). `x` is
    /// repeats around the branch per metre of branch radius, so thicker
    /// branches get more repeats; larger `y` stretches the image along it.
    #[serde(with = "serde_vec::vec2")]
    pub texture_scale: Vec2,
    /// How bark images repeat along branches. Keep the default,
    /// [`BarkUvMode::Continuous`], to use bark normal maps in Bevy and glTF export.
    /// Choose [`BarkUvMode::Mirrored`] to match the JavaScript ez-tree generator's
    /// texture layout; that generator ignores this setting.
    pub uv_mode: BarkUvMode,
}

/// Settings for the trunk and successive levels of child branches.
///
/// Level 0 is the trunk. Each successive level grows from the preceding one,
/// up to [`levels`](Self::levels). Entries for unused levels have no effect.
/// With more than one [`stems`](Self::stems), every stem is a level-0 trunk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BranchOptions {
    /// Number of branching steps above the trunk: 0 means trunk only.
    /// Must not exceed [`MAX_LEVELS`]; checked by [`TreeOptions::validate`].
    pub levels: u32,
    /// Number of trunks growing from the ground, as in a multi-stemmed shrub.
    /// Each stem uses the level-0 settings. The default, 1, grows a single
    /// upright trunk. Must be at least 1.
    pub stems: u32,
    /// Largest tilt of a stem away from vertical, in degrees, when there is more
    /// than one stem. Stems get random tilts up to this angle, spread evenly
    /// around the base; the more a stem tilts, the further from the centre it
    /// starts. Ignored for a single stem. Must be between 0 and 90.
    pub stem_angle: f32,
    /// Child-branch angle relative to its parent, in degrees, for levels 1–3.
    pub angle: PerLevel<f32>,
    /// Number of side branches per parent at levels 0–2, excluding any tip continuation.
    pub children: PerLevel<u32>,
    /// Direction and strength of a steady influence on branch growth.
    pub force: GrowthForce,
    /// Pull toward level, sideways growth at each level, in radians per
    /// lengthwise interval. Use it for branches that spread out in flat layers,
    /// as on a dogwood. A branch never bends past horizontal, and one pointing
    /// straight up or down is not pulled. 0, the default, adds no pull; negative
    /// values have no effect. Unlike [`force`](Self::force), the pull does not
    /// grow stronger on thinner branches. Random bending from
    /// [`gnarliness`](Self::gnarliness) and an upward `force` still apply and work
    /// against the pull, so the total turn a branch can make is at most this value
    /// times its [`sections`](Self::sections) count.
    pub flatten: PerLevel<f32>,
    /// Amount of random bending; larger values give more irregular branches.
    pub gnarliness: PerLevel<f32>,
    /// Length setting for each level, in metres.
    pub length: PerLevel<f32>,
    /// Trunk base radius at level 0, in metres. At levels 1–3, a multiplier of the parent
    /// radius where a side branch starts; 0.7 makes it 70% as thick.
    /// Tip continuations inherit the parent tip radius instead.
    pub radius: PerLevel<f32>,
    /// Number of lengthwise intervals per branch; 12 gives 13 cross-sections.
    /// More intervals allow finer bends. Deciduous tip continuations inherit the
    /// parent's count rather than this level's setting.
    /// [`TreeOptions::validate`] requires at least 1 for levels 0 through `levels`,
    /// even if no branches will grow at a configured level.
    pub sections: PerLevel<u32>,
    /// Number of sides around a branch cross-section; more sides look rounder.
    pub segments: PerLevel<u32>,
    /// Earliest child-branch position along the parent, from 0 (base) to 1 (tip).
    /// Indexed by the child level, 1–3.
    pub start: PerLevel<f32>,
    /// Radius reduction along deciduous branches: 0 keeps the radius constant,
    /// 1 tapers to a point. Evergreen branches always taper to a point.
    /// A branch that tapers to a point ends there, with no tip continuation.
    pub taper: PerLevel<f32>,
    /// Rotation around the branch axis for each lengthwise division, in radians.
    pub twist: PerLevel<f32>,
}

/// A directional influence on growth, such as an upward pull toward light.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GrowthForce {
    /// Target direction in tree coordinates; `(0, 1, 0)` points upward.
    /// Its length need not be 1.
    #[serde(with = "serde_vec::vec3")]
    pub direction: Vec3,
    /// Bending strength. Positive values turn toward the direction, negative
    /// values turn away, and zero disables the influence. Thinner branches bend more.
    pub strength: f32,
}

/// Settings for where leaves appear and how they are drawn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeafOptions {
    /// Texture identifier, such as `"oak"`, for the application to resolve.
    /// Setting this field does not load a texture.
    #[serde(rename = "type")]
    pub kind: String,
    /// Flat-surface arrangement used to draw each leaf.
    pub billboard: Billboard,
    /// Leaf angle relative to its supporting branch, in degrees.
    pub angle: f32,
    /// Number of leaves placed along each outermost branch. Broadleaf trees also
    /// get one extra leaf at the tip of each outermost branch. Set to 0 for a
    /// bare tree with no leaves at all, tip leaves included.
    pub count: u32,
    /// Earliest leaf position along a branch, from 0 (base) to 1 (tip).
    pub start: f32,
    /// Base leaf size, in metres.
    pub size: f32,
    /// Random size variation as a fraction of [`size`](Self::size). For example,
    /// 0.2 varies sizes by up to 20% above or below the base size.
    pub size_variance: f32,
    /// Leaf tint as `0xRRGGBB`; `0xffffff` leaves texture colours unchanged.
    pub tint: u32,
    /// Opacity cutoff from 0 to 1. Texture pixels below this value are hidden,
    /// allowing a rectangular surface to show a leaf-shaped silhouette.
    pub alpha_test: f32,
    /// Shade leaves to suggest a rounded canopy rather than separate flat surfaces.
    pub rounded_normals: bool,
    /// How far to turn each leaf toward lying flat with its face up, from 0 to 1.
    /// 0, the default, keeps the angle from [`angle`](Self::angle) and the
    /// branch. 1 lays every leaf level, pointing in the horizontal direction it
    /// already leans, like the flat foliage sprays of a cedar. Leaves keep their
    /// positions. With [`Billboard::Double`], the second, crossing surface stands
    /// upright on a flattened leaf, so flat foliage usually suits
    /// [`Billboard::Single`]. Must be between 0 and 1.
    pub flatten: f32,
}

/// A flat grid of guide wires that can bend nearby branches toward it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrellisOptions {
    /// Enable the trellis influence on branch growth.
    pub enabled: bool,
    /// Position of the bottom centre of the grid, which lies in the XY plane.
    #[serde(with = "serde_vec::vec3")]
    pub position: Vec3,
    /// Grid width along the X axis, centred on [`position`](Self::position).
    pub width: f32,
    /// Grid height upward along the Y axis from [`position`](Self::position).
    pub height: f32,
    /// Gap between neighbouring guide wires, in metres.
    pub spacing: f32,
    /// Strength, reach, and distance dependence of the trellis influence.
    pub force: TrellisForce,
    /// Radius to use when drawing the guide wires as cylinders.
    pub cylinder_radius: f32,
    /// Whether the application should draw the grid; independent of its growth influence.
    pub visible: bool,
    /// Guide-wire colour as `0xRRGGBB`.
    pub color: u32,
}

/// Settings for the pull toward the nearest trellis wire.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrellisForce {
    /// Base strength of the pull; zero disables it.
    pub strength: f32,
    /// Maximum distance from a guide wire at which the pull applies, in metres.
    pub max_distance: f32,
    /// Exponent controlling how the pull weakens with distance: 1 is linear;
    /// 2 weakens it more quickly away from the wire.
    pub falloff: f32,
}

impl Default for BarkOptions {
    fn default() -> Self {
        Self {
            kind: "Bark001".to_owned(),
            tint: 0xffffff,
            flat_shading: false,
            textured: true,
            texture_scale: Vec2::new(10.0, 1.0),
            uv_mode: BarkUvMode::Continuous,
        }
    }
}

impl Default for BranchOptions {
    fn default() -> Self {
        // Keep unused entries at zero: the trunk has no parent angle or start
        // position, and the final level has no child branches.
        Self {
            levels: 3,
            stems: 1,
            stem_angle: 0.0,
            angle: PerLevel([0.0, 70.0, 60.0, 60.0]),
            children: PerLevel([7, 7, 5, 0]),
            force: GrowthForce::default(),
            flatten: PerLevel([0.0; 4]),
            gnarliness: PerLevel([0.15, 0.2, 0.3, 0.02]),
            length: PerLevel([2.0, 2.0, 1.0, 0.1]),
            radius: PerLevel([0.15, 0.7, 0.7, 0.7]),
            sections: PerLevel([12, 10, 8, 6]),
            segments: PerLevel([8, 6, 4, 3]),
            start: PerLevel([0.0, 0.4, 0.3, 0.3]),
            taper: PerLevel([0.7; 4]),
            twist: PerLevel([0.0; 4]),
        }
    }
}

impl Default for GrowthForce {
    fn default() -> Self {
        Self {
            direction: Vec3::Y,
            strength: 0.001,
        }
    }
}

impl Default for LeafOptions {
    fn default() -> Self {
        Self {
            kind: "oak".to_owned(),
            billboard: Billboard::Double,
            angle: 10.0,
            count: 1,
            start: 0.0,
            size: 0.25,
            size_variance: 0.7,
            tint: 0xffffff,
            alpha_test: 0.5,
            rounded_normals: true,
            flatten: 0.0,
        }
    }
}

impl Default for TrellisOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            position: Vec3::new(0.0, 0.0, -0.2),
            width: 1.0,
            height: 2.0,
            spacing: 0.2,
            force: TrellisForce::default(),
            cylinder_radius: 0.005,
            visible: true,
            color: 0x8b4513,
        }
    }
}

impl Default for TrellisForce {
    fn default() -> Self {
        Self {
            strength: 0.002,
            max_distance: 0.3,
            falloff: 1.0,
        }
    }
}

/// A problem detected by [`TreeOptions::validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OptionsError {
    /// The requested branch level exceeds the supported maximum.
    TooManyLevels {
        /// Number of levels above the trunk requested by the caller.
        levels: u32,
    },
    /// A level from 0 through `branch.levels` requests zero lengthwise intervals.
    ZeroSections {
        /// Index of the zero entry in `branch.sections`; 0 identifies the trunk.
        level: u32,
    },
    /// A stem, fruit, or flower setting failed a range or finiteness check.
    InvalidSetting {
        /// Setting name using file field names, for example
        /// `"fruit.placement.tips"` or `"flowers.fruit.maturity"`.
        field: String,
        /// Description of the accepted value, for example `"between 0 and 1"`.
        requirement: &'static str,
    },
}

impl fmt::Display for OptionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyLevels { levels } => write!(
                f,
                "branch.levels is {levels}, but at most {MAX_LEVELS} levels are supported"
            ),
            Self::ZeroSections { level } => write!(
                f,
                "branch.sections[{level}] is 0, but every level in use needs at least 1 section"
            ),
            Self::InvalidSetting { field, requirement } => {
                write!(f, "{field} must be {requirement}")
            }
        }
    }
}

impl std::error::Error for OptionsError {}

impl TreeOptions {
    /// Checks supported ranges for branches, stems, leaf flattening, fruit and flowers.
    ///
    /// Every level from the trunk through `branch.levels` must have at least
    /// one section and a finite radius above zero, even if no branches grow
    /// at that level. Higher levels are ignored. Fruit and flower settings
    /// are checked in full when present, including those their cluster
    /// arrangement doesn't use.
    ///
    /// Not all settings are checked, including branch lengths and leaf
    /// dimensions, so success does not guarantee valid geometry.
    ///
    /// # Errors
    ///
    /// Returns [`OptionsError::TooManyLevels`] if `branch.levels` exceeds
    /// [`MAX_LEVELS`], or [`OptionsError::ZeroSections`] if a configured level
    /// has no sections. Other checked settings outside their allowed ranges
    /// return [`OptionsError::InvalidSetting`]; checked floating-point values
    /// must be finite. Radius errors identify `"branch.radius.<level>"`.
    ///
    /// Returns the first error, checking depth, section counts, stem settings,
    /// radii, leaf flattening, fruit and flowers in that order.
    pub fn validate(&self) -> Result<(), OptionsError> {
        if self.branch.levels > MAX_LEVELS {
            return Err(OptionsError::TooManyLevels {
                levels: self.branch.levels,
            });
        }
        for level in 0..=self.branch.levels {
            if self.branch.sections[level as usize] == 0 {
                return Err(OptionsError::ZeroSections { level });
            }
        }
        let v = Checker("branch");
        v.check(self.branch.stems >= 1, "stems", "at least 1")?;
        v.check(
            (0.0..=90.0).contains(&self.branch.stem_angle),
            "stem_angle",
            "between 0 and 90",
        )?;

        // Bending divides by the radius, so zero would produce invalid geometry.
        for level in 0..=self.branch.levels {
            let radius = self.branch.radius[level as usize];
            v.check(
                radius > 0.0 && radius.is_finite(),
                &format!("radius.{level}"),
                "a finite value above 0",
            )?;
        }
        Checker("leaves").unit(self.leaves.flatten, "flatten")?;
        for (part, attached) in [("fruit", &self.fruit), ("flowers", &self.flowers)] {
            if let Some(attached) = attached {
                attached.validate(&Checker(part))?;
            }
        }
        Ok(())
    }
}

/// Checks values and prefixes error field names with their settings group,
/// such as `fruit` or `flowers`.
pub(crate) struct Checker(pub(crate) &'static str);

impl Checker {
    /// Succeeds when `ok` is true; otherwise reports the full setting path and
    /// the requirement that was not met.
    pub(crate) fn check(
        &self,
        ok: bool,
        field: &str,
        requirement: &'static str,
    ) -> Result<(), OptionsError> {
        if ok {
            Ok(())
        } else {
            Err(OptionsError::InvalidSetting {
                field: format!("{}.{field}", self.0),
                requirement,
            })
        }
    }

    /// Accepts values from 0 through 1, including both endpoints.
    /// Rejects NaN and infinities.
    pub(crate) fn unit(&self, value: f32, field: &str) -> Result<(), OptionsError> {
        self.check((0.0..=1.0).contains(&value), field, "between 0 and 1")
    }

    /// Accepts zero and positive finite values; rejects negatives, NaN, and infinities.
    pub(crate) fn non_negative(&self, value: f32, field: &str) -> Result<(), OptionsError> {
        self.check(
            (0.0..f32::INFINITY).contains(&value),
            field,
            "a finite value of 0 or more",
        )
    }
}

mod per_level_serde {
    use std::fmt;
    use std::marker::PhantomData;

    use serde::de::{Error, MapAccess, Visitor};
    use serde::ser::SerializeMap;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::PerLevel;

    const KEYS: [&str; 4] = ["0", "1", "2", "3"];

    impl<T: Serialize> Serialize for PerLevel<T> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut map = serializer.serialize_map(Some(KEYS.len()))?;
            for (key, value) in KEYS.iter().zip(&self.0) {
                map.serialize_entry(key, value)?;
            }
            map.end()
        }
    }

    /// Reads per-level values from an object with keys `"0"`, `"1"`, `"2"`, and `"3"`.
    ///
    /// All four keys are required, and any other key is an error. A `.tree.yml`
    /// preset file may give only some levels: the others come from its base.
    impl<'de, T: Deserialize<'de>> Deserialize<'de> for PerLevel<T> {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            deserializer.deserialize_map(PerLevelVisitor(PhantomData))
        }
    }

    struct PerLevelVisitor<T>(PhantomData<T>);

    impl<'de, T: Deserialize<'de>> Visitor<'de> for PerLevelVisitor<T> {
        type Value = PerLevel<T>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("an object with keys \"0\", \"1\", \"2\" and \"3\"")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            // Read the level keys; any other key is an error.
            let mut slots: [Option<T>; 4] = [None, None, None, None];
            while let Some(key) = map.next_key::<String>()? {
                match KEYS.iter().position(|k| *k == key) {
                    Some(level) => slots[level] = Some(map.next_value()?),
                    None => return Err(A::Error::unknown_field(&key, &KEYS)),
                }
            }

            // Direct PerLevel deserialization has no defaults to fill missing slots.
            match slots {
                [Some(a), Some(b), Some(c), Some(d)] => Ok(PerLevel([a, b, c, d])),
                _ => Err(A::Error::custom(
                    "per-level value is missing one of the keys \"0\" to \"3\"",
                )),
            }
        }
    }
}

/// Reads and writes vectors as ez-tree objects: `{"x":1,"y":2}` for 2D
/// and `{"x":1,"y":2,"z":3}` for 3D, rather than arrays.
mod serde_vec {
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    struct Xy {
        x: f32,
        y: f32,
    }

    #[derive(Serialize, Deserialize)]
    struct Xyz {
        x: f32,
        y: f32,
        z: f32,
    }

    pub(super) mod vec2 {
        use glam::Vec2;
        use serde::{Deserialize, Deserializer, Serialize, Serializer};

        pub(crate) fn serialize<S: Serializer>(v: &Vec2, serializer: S) -> Result<S::Ok, S::Error> {
            super::Xy { x: v.x, y: v.y }.serialize(serializer)
        }

        pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Vec2, D::Error> {
            let super::Xy { x, y } = super::Xy::deserialize(deserializer)?;
            Ok(Vec2::new(x, y))
        }
    }

    pub(super) mod vec3 {
        use glam::Vec3;
        use serde::{Deserialize, Deserializer, Serialize, Serializer};

        pub(crate) fn serialize<S: Serializer>(v: &Vec3, serializer: S) -> Result<S::Ok, S::Error> {
            super::Xyz {
                x: v.x,
                y: v.y,
                z: v.z,
            }
            .serialize(serializer)
        }

        pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Vec3, D::Error> {
            let super::Xyz { x, y, z } = super::Xyz::deserialize(deserializer)?;
            Ok(Vec3::new(x, y, z))
        }
    }
}
