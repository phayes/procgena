//! Spreads stones over a stretch of ground: boulders, rocks and pebbles,
//! resting on the slope and partly buried.

use std::fmt;

use glam::{Affine3A, DVec2, Vec2, Vec3};
use procgena_core::hash::{pcg2, seed_for, unit_float};
use procgena_core::scatter::lattice::{self, Fixed2};
use procgena_core::scatter::{
    DEFAULT_SPACING_ROUNDS, Disc, Footprint, Lattice, Placement, Rank, Reach, Scatter, ScatterCopy,
    ScatterError, ScatterInput, ScatterSlot, Spaced, keep_spaced, place, steeper_than,
};
use procgena_core::{CellId, ChildKey, CopyAddress};
use serde::{Deserialize, Serialize};

use crate::field::StoneField;
use crate::options::{OptionsError, StoneOptions};
use crate::placement::StonePlacement;
use crate::presets::StonePreset;

/// Most classes one [`StoneScatterOptions`] can list.
pub const MAX_STONE_CLASSES: usize = 8;
/// Most shape variants one [`StoneClass`] can have.
pub const MAX_STONE_VARIANTS: u32 = 64;
/// Most stones one [`StoneScatter`] can ask for over one area, before the
/// slope, the ground and spacing remove any.
pub const MAX_STONE_INSTANCES: usize = 262_144;
/// Most candidate spots one [`StoneScatter`] can test for one area: the
/// area, plus the margin its spacing needs, divided by the lattice cell's
/// area.
pub const MAX_STONE_CANDIDATES: usize = 4_194_304;

/// Directions around a stone in which its footprint is measured.
const FOOTPRINT_DIRECTIONS: u32 = 8;
/// Step, in unscaled stone units, when measuring a stone's footprint.
const FOOTPRINT_STEP: f32 = 1.0 / 128.0;
/// Footprint radius, in unscaled stone units, used when a stone is too thin
/// at its ground line to measure.
const MIN_FOOTPRINT: f32 = 0.05;
/// Lowest height, in unscaled stone units, at which the footprint is
/// measured, for stones with almost no suggested burial.
const MIN_GROUND_LINE: f32 = 0.02;
/// Half the width of the soft edge of a bare patch, in noise units.
const PATCH_EDGE: f32 = 0.05;
/// How far away, in metres, stones are drawn. Stones have no cull distance
/// of their own yet, so this is a fixed stand-in for the largest boulders.
const DRAW_RANGE: f32 = 1000.0;

// Seed keys for the Scatter's random streams, named
// `<crate>.<area>.<role>`. They are frozen: renaming one moves every stone.
const LATTICE_SEED: &str = "stone.scatter.lattice";
const PATCHES_SEED: &str = "stone.scatter.patches";

/// Salts that keep the random draws of each purpose apart. Class draws are
/// combined with the class's key.
const SALT_COUNT: u32 = 0x9e37_79b9;
const SALT_PATCH: u32 = 0x6a09_e667;
const SALT_MEMBER: u32 = 0xbb67_ae85;
const SALT_REACH: u32 = 0x3c6e_f372;
const SALT_ANGLE: u32 = 0xa54f_f53a;
const SALT_VARIANT: u32 = 0x510e_527f;
const SALT_SIZE: u32 = 0x9b05_688c;
const SALT_YAW: u32 = 0x1f83_d9ab;
const SALT_BURIAL: u32 = 0x5be0_cd19;

/// Most stones one cluster of a [`StoneClass`] can hold.
pub const MAX_CLUSTER_MEMBERS: u32 = 16;

/// One kind of stone in a [`StoneScatterOptions`], such as boulders or
/// pebbles: its shapes, how many there are and how they lie.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoneClass {
    /// Names the class within its Scatter. The class's own random choices
    /// (which spots it takes, its clusters and patches) come from its key,
    /// not its place in the list. Keys must differ within a Scatter;
    /// changing one moves the class's stones.
    pub key: ChildKey,
    /// The shape settings every variant starts from. Variant `v` uses
    /// [`StoneOptions::variant`]`(world_seed, v, stretch_jitter)`, with the
    /// World seed the [`StoneScatter`] was built for: the same shapes a
    /// `StonePool` with these settings and that World seed draws. The
    /// stone's own seed is not used.
    pub stone: StoneOptions,
    /// Number of different shapes, from 1 to [`MAX_STONE_VARIANTS`].
    pub variants: u32,
    /// How much each variant's stretch is randomly reduced on each axis,
    /// from 0 up to (not including) 1. See [`StoneOptions::variant`].
    pub stretch_jitter: f32,
    /// Spots per square metre where the mask allows full density, before
    /// slope, ground and spacing remove any. Each spot holds one stone, or
    /// one cluster of [`members`](Self::members) stones. Must be 0 or more,
    /// and at most 1 / [`spacing`](Self::spacing)².
    pub density: f32,
    /// Smallest distance in metres between the centres of two stones of
    /// this class from different spots. Must be positive. Stones of the
    /// same cluster, and stones of different classes, are kept apart by
    /// their footprints instead, so they never overlap at the ground.
    pub spacing: f32,
    /// Fewest and most stones in each spot's cluster, from 1 up to
    /// [`MAX_CLUSTER_MEMBERS`], the first no larger than the second.
    /// `[1, 1]`, the default, places single stones.
    pub members: [u32; 2],
    /// How far in metres a cluster's stones may lie from its middle. 0 or
    /// more; 0 puts every member at the spot itself, where only the first
    /// fits.
    pub cluster_radius: f32,
    /// Smallest and largest size factor applied to the stone's
    /// [`scale`](StoneOptions::scale), picked per stone. Both positive, the
    /// first no larger than the second.
    pub size: [f32; 2],
    /// How the class's stones stand on the ground: how far they tilt to the
    /// slope, what they rest on, how deep they sink and how much bend under
    /// them is too much. `None`, the default, uses each shape's own
    /// [`StoneOptions::ground_placement`].
    pub placement: Option<Placement>,
    /// Smallest and largest share of each stone's suggested burial depth
    /// (see [`StonePlacement::embed_depth`]) it sinks by, picked per stone,
    /// from 0 to 1, on top of the placement's own
    /// [`embed`](Placement::embed).
    pub burial: [f32; 2],
    /// Steepest ground, in degrees, a stone of this class lies on, from 0 to
    /// 90. 90 means no limit.
    pub max_slope: f32,
    /// How much of the ground is left bare between patches of this class,
    /// from 0 (stones spread evenly, the default) up to (not including) 1.
    /// Raising it only removes stones of this class, though stones of later
    /// classes may then fill the ground they leave.
    pub patchiness: f32,
    /// Typical width in metres of a patch and of the bare ground between
    /// patches. Must be positive.
    pub patch_size: f32,
}

impl StoneClass {
    /// A class named `key` of single stones shaped like `stone`, at
    /// `density` stones per square metre and at least `spacing` metres
    /// apart, with 8 variants, no stretch jitter, sizes from 0.8 to 1.25,
    /// each shape's own placement, burial from 0.5 to 1, a slope limit of 40
    /// degrees, no clusters and no patches.
    pub fn new(key: ChildKey, stone: StoneOptions, density: f32, spacing: f32) -> Self {
        Self {
            key,
            stone,
            variants: 8,
            stretch_jitter: 0.0,
            density,
            spacing,
            members: [1, 1],
            cluster_radius: 0.0,
            size: [0.8, 1.25],
            placement: None,
            burial: [0.5, 1.0],
            max_slope: 40.0,
            patchiness: 0.0,
            patch_size: 8.0,
        }
    }

    /// Returns the settings of shape `variant` of this class in a World
    /// seeded with `world_seed` (see [`StoneOptions::variant`]). Use an index
    /// below [`variants`](Self::variants); this method does not check it.
    pub fn variant(&self, world_seed: u32, variant: u32) -> StoneOptions {
        self.stone.variant(world_seed, variant, self.stretch_jitter)
    }

    /// Checks every setting, including the stone's own.
    ///
    /// # Errors
    ///
    /// Returns the first invalid setting found.
    pub fn validate(&self) -> Result<(), OptionsError> {
        self.stone.validate()?;
        if !(1..=MAX_STONE_VARIANTS).contains(&self.variants) {
            return Err(OptionsError::IntOutOfRange {
                field: "class.variants",
                value: self.variants,
                min: 1,
                max: MAX_STONE_VARIANTS,
            });
        }
        float(
            "class.stretch_jitter",
            self.stretch_jitter,
            (0.0..1.0).contains(&self.stretch_jitter),
            "at least 0 and below 1",
        )?;
        float(
            "class.spacing",
            self.spacing,
            self.spacing > 0.0,
            "positive",
        )?;
        float(
            "class.density",
            self.density,
            self.density >= 0.0 && self.density * self.spacing * self.spacing <= 1.0,
            "at least 0 and at most 1 / spacing squared",
        )?;
        let [fewest, most] = self.members;
        if !(1..=MAX_CLUSTER_MEMBERS).contains(&fewest) {
            return Err(OptionsError::IntOutOfRange {
                field: "class.members",
                value: fewest,
                min: 1,
                max: MAX_CLUSTER_MEMBERS,
            });
        }
        if !(fewest..=MAX_CLUSTER_MEMBERS).contains(&most) {
            return Err(OptionsError::IntOutOfRange {
                field: "class.members",
                value: most,
                min: fewest,
                max: MAX_CLUSTER_MEMBERS,
            });
        }
        float(
            "class.cluster_radius",
            self.cluster_radius,
            self.cluster_radius >= 0.0,
            "at least 0",
        )?;
        float("class.size", self.size[0], self.size[0] > 0.0, "positive")?;
        float(
            "class.size",
            self.size[1],
            self.size[1] >= self.size[0] && self.size[1].is_finite(),
            "at least the smallest size",
        )?;
        float(
            "class.burial",
            self.burial[0],
            (0.0..=1.0).contains(&self.burial[0]),
            "between 0 and 1",
        )?;
        float(
            "class.burial",
            self.burial[1],
            (self.burial[0]..=1.0).contains(&self.burial[1]),
            "between the smallest burial and 1",
        )?;
        float(
            "class.max_slope",
            self.max_slope,
            (0.0..=90.0).contains(&self.max_slope),
            "between 0 and 90",
        )?;
        if let Some(placement) = &self.placement {
            float(
                "class.placement.conform",
                placement.conform,
                (0.0..=1.0).contains(&placement.conform),
                "between 0 and 1",
            )?;
            float(
                "class.placement.embed",
                placement.embed,
                placement.embed >= 0.0,
                "at least 0",
            )?;
            if let Some(bend) = placement.max_bend {
                float("class.placement.max_bend", bend, bend >= 0.0, "at least 0")?;
            }
        }
        float(
            "class.patchiness",
            self.patchiness,
            (0.0..1.0).contains(&self.patchiness),
            "at least 0 and below 1",
        )?;
        float(
            "class.patch_size",
            self.patch_size,
            self.patch_size > 0.0,
            "positive",
        )
    }
}

/// Settings for a [`StoneScatter`]: the classes of stone it spreads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoneScatterOptions {
    /// The seed to pass in [`ScatterInput::seed`] when this scatter is
    /// built on its own, such as in a viewer. [`StoneScatter::new`] uses the
    /// input's seed, not this one.
    pub default_seed: u32,
    /// The kinds of stone, at most [`MAX_STONE_CLASSES`]. Earlier classes
    /// claim their ground first: a spot goes to the first class whose own
    /// density keeps it, and a stone is left out where it would overlap a
    /// stone of an earlier class. List the largest stones first.
    pub classes: Vec<StoneClass>,
}

impl StoneScatterOptions {
    /// A few large boulders, sparse rocks and many pebbles in small clusters,
    /// gathered in patches.
    pub fn boulder_field() -> Self {
        let key = ChildKey::from_static;
        let mut boulders = StoneClass::new(
            key("boulders"),
            StonePreset::boulder().stone.clone(),
            0.004,
            6.0,
        );
        boulders.variants = 6;
        boulders.size = [0.6, 1.3];
        boulders.max_slope = 30.0;
        boulders.patchiness = 0.4;
        boulders.patch_size = 30.0;
        let mut rocks = StoneClass::new(
            key("rocks"),
            StonePreset::river_stone().stone.clone(),
            0.03,
            1.5,
        );
        rocks.patchiness = 0.3;
        rocks.patch_size = 12.0;
        let mut pebbles = StoneClass::new(
            key("pebbles"),
            StonePreset::pebble().stone.clone(),
            0.25,
            0.6,
        );
        pebbles.members = [1, 5];
        pebbles.cluster_radius = 0.3;
        pebbles.size = [0.6, 1.6];
        pebbles.max_slope = 50.0;
        pebbles.patchiness = 0.5;
        pebbles.patch_size = 4.0;
        Self {
            default_seed: 0,
            classes: vec![boulders, rocks, pebbles],
        }
    }

    /// Checks every class's settings, and that the classes' keys differ.
    ///
    /// # Errors
    ///
    /// Returns [`StoneScatterError::TooManyClasses`], the first invalid
    /// class setting as [`StoneScatterError::Class`], or
    /// [`StoneScatterError::KeyClash`].
    pub fn validate(&self) -> Result<(), StoneScatterError> {
        if self.classes.len() > MAX_STONE_CLASSES {
            return Err(StoneScatterError::TooManyClasses {
                count: self.classes.len(),
            });
        }
        for (index, class) in self.classes.iter().enumerate() {
            class
                .validate()
                .map_err(|error| StoneScatterError::Class { index, error })?;
            if let Some(first) = self.classes[..index]
                .iter()
                .position(|other| other.key.salt() == class.key.salt())
            {
                return Err(StoneScatterError::KeyClash {
                    first,
                    second: index,
                });
            }
        }
        Ok(())
    }

    /// The lattice every class's spots sit on: cells as wide as the
    /// smallest spacing of a class with stones, narrowed until every cell
    /// can hold a spot of each class.
    fn lattice(&self) -> Lattice {
        let live = self.classes.iter().filter(|c| c.density > 0.0);
        let narrowest = live
            .clone()
            .map(|c| c.spacing)
            .fold(f32::INFINITY, f32::min);
        let total: f32 = live.map(|c| c.density).sum();
        if !narrowest.is_finite() {
            return Lattice::new(1.0);
        }
        let cell = if total * narrowest * narrowest > 1.0 {
            1.0 / total.sqrt()
        } else {
            narrowest
        };
        Lattice::new(cell)
    }
}

impl Default for StoneScatterOptions {
    /// The [`boulder_field`](Self::boulder_field) scatter.
    fn default() -> Self {
        Self::boulder_field()
    }
}

/// Why a [`StoneScatter`] could not be built.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum StoneScatterError {
    /// A setting of class `index` is invalid.
    Class {
        /// The class's position in [`StoneScatterOptions::classes`].
        index: usize,
        /// What is wrong.
        error: OptionsError,
    },
    /// More than [`MAX_STONE_CLASSES`] classes are listed.
    TooManyClasses {
        /// The number of classes listed.
        count: usize,
    },
    /// Two classes have the same key, or keys that share a salt.
    KeyClash {
        /// The first class's position in [`StoneScatterOptions::classes`].
        first: usize,
        /// The second class's position.
        second: usize,
    },
    /// The densities over the area ask for more than
    /// [`MAX_STONE_INSTANCES`] stones.
    TooMany {
        /// The number of stones asked for.
        count: usize,
    },
    /// The area holds more than [`MAX_STONE_CANDIDATES`] candidate spots.
    /// Use a smaller area or wider spacings.
    TooLarge {
        /// The number of candidate spots.
        candidates: usize,
    },
}

impl fmt::Display for StoneScatterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Class { index, error } => write!(f, "stone class {index}: {error}"),
            Self::TooManyClasses { count } => write!(
                f,
                "{count} stone classes listed, but at most {MAX_STONE_CLASSES} are allowed"
            ),
            Self::KeyClash { first, second } => write!(
                f,
                "stone classes {first} and {second} have the same key, or keys with the same salt"
            ),
            Self::TooMany { count } => write!(
                f,
                "the scatter asks for {count} stones, but at most {MAX_STONE_INSTANCES} are allowed"
            ),
            Self::TooLarge { candidates } => write!(
                f,
                "the area holds {candidates} candidate spots, but at most \
                 {MAX_STONE_CANDIDATES} are allowed"
            ),
        }
    }
}

impl std::error::Error for StoneScatterError {}

/// One placed stone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StoneInstance {
    /// The stone's class: its position in [`StoneScatterOptions::classes`].
    pub class: usize,
    /// Which of its class's shapes it is.
    pub variant: u32,
    /// Names the stone anywhere in the World: its spot's lattice cell, and
    /// its number within the spot's cluster (0 for single stones). The same
    /// input always gives the stone at this address the same place and
    /// look, from any area that holds its cell.
    pub address: CopyAddress,
    /// Its size factor, already part of [`transform`](Self::transform).
    pub scale: f32,
    /// Where its origin stands, relative to the area's centre, in metres.
    pub position: Vec3,
    /// How far its footprint reaches from its position, along the ground,
    /// in metres. Stones of different classes are at least the sum of their
    /// radii apart.
    pub radius: f32,
    /// The ground it takes up, in World fixed point: a disc of
    /// [`radius`](Self::radius) around where it stands. Other Scatters keep
    /// their copies out of it.
    pub footprint: Disc,
    /// Places a mesh of its variant (in metres, as [`crate::Stone`] builds
    /// it) on the ground: size, yaw, tilt to the slope and burial.
    pub transform: Affine3A,
}

/// A variant's shape measurements, in unscaled stone units.
#[derive(Debug, Clone, PartialEq)]
struct VariantShape {
    /// Suggested burial depth.
    embed_depth: f32,
    /// Horizontal distance from the stone's axis to its side at the ground
    /// line, in each of [`FOOTPRINT_DIRECTIONS`] directions.
    footprint: Vec<Vec2>,
    /// The largest of those distances.
    radius: f32,
}

/// A stone that passed the density, exclusion, slope and ground checks,
/// waiting for the spacing check.
struct Candidate {
    position: Fixed2,
    rank: Rank,
    /// Its footprint radius in fixed point.
    radius: i64,
    /// Its class's spacing in fixed point.
    spacing: i64,
    /// Whether it stands inside one of its class's patches; stones outside
    /// are removed after the spacing check.
    in_patch: bool,
    instance: StoneInstance,
}

impl Spaced for Candidate {
    fn position(&self) -> Fixed2 {
        self.position
    }

    fn rank(&self) -> Rank {
        self.rank
    }
}

/// Stones spread over the ground by a [`StoneScatterOptions`]: the shapes
/// of each class for one World seed, and the stones of one area.
///
/// Build one with [`new`](Self::new), then draw each of
/// [`instances`](Self::instances) with a mesh of its class's variant, from
/// [`variant`](Self::variant) or a `StonePool` with the class's settings.
/// It is also a [`Scatter`], placing any other area with the same shapes.
///
/// Every class's spots sit on one lattice fixed to the World, one spot per
/// cell at most, so an area holds the same stones, with the same addresses,
/// as any larger area around it, and as the chunks it splits into.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StoneScatter {
    options: StoneScatterOptions,
    world_seed: u32,
    variants: Vec<Vec<StoneOptions>>,
    shapes: Vec<Vec<VariantShape>>,
    instances: Vec<StoneInstance>,
}

impl StoneScatter {
    /// Builds the shapes of `options` with `input`'s seed as the World seed,
    /// then spreads its stones over `input`'s area.
    ///
    /// A spot goes to the first class whose own density, times the input's
    /// mask, keeps it. Then each of its stones is left out where it overlaps
    /// a footprint the input keeps clear, where the ground is steeper than
    /// [`max_slope`](StoneClass::max_slope), where any part of the stone has
    /// no ground under it, or where the ground bends more than its
    /// [`placement`](StoneClass::placement) allows. Stones too close to a
    /// stone of an earlier class, or of their own class, are left out next,
    /// and last those between the class's patches. Each stone that is left
    /// stands as its placement says, sunk further by its burial.
    ///
    /// Lowering a class's density only removes stones of that class; the
    /// earlier classes keep their places, though stones of later classes may
    /// fill the gaps. Positions are relative to the area's centre, and the
    /// area holds the stones whose spot's cell corner lies inside it, so a
    /// stone may stand outside it by up to a cell plus the cluster radius.
    ///
    /// Building measures every variant's shape on the CPU, which takes tens
    /// of milliseconds per variant.
    ///
    /// # Errors
    ///
    /// Returns any [`StoneScatterError`] from
    /// [`StoneScatterOptions::validate`], [`StoneScatterError::TooMany`] if
    /// the densities ask for more than [`MAX_STONE_INSTANCES`] stones, and
    /// [`StoneScatterError::TooLarge`] if the area holds more than
    /// [`MAX_STONE_CANDIDATES`] candidate spots.
    pub fn new(
        options: &StoneScatterOptions,
        input: &ScatterInput<'_>,
    ) -> Result<Self, StoneScatterError> {
        let mut scatter = Self::prepare(options, input.seed)?;
        scatter.instances = scatter.stones(input)?;
        Ok(scatter)
    }

    /// Builds the shapes of `options` in a World seeded with `world_seed`,
    /// without placing any stones: place them with
    /// [`stones`](Self::stones) or as a [`Scatter`].
    ///
    /// # Errors
    ///
    /// Returns any [`StoneScatterError`] from
    /// [`StoneScatterOptions::validate`].
    pub fn prepare(
        options: &StoneScatterOptions,
        world_seed: u32,
    ) -> Result<Self, StoneScatterError> {
        options.validate()?;
        let variants: Vec<Vec<StoneOptions>> = options
            .classes
            .iter()
            .map(|class| {
                (0..class.variants)
                    .map(|v| class.variant(world_seed, v))
                    .collect()
            })
            .collect();
        let shapes: Vec<Vec<VariantShape>> = variants
            .iter()
            .enumerate()
            .map(|(index, list)| {
                list.iter()
                    .map(measure)
                    .collect::<Result<_, _>>()
                    .map_err(|error| StoneScatterError::Class { index, error })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            options: options.clone(),
            world_seed,
            variants,
            shapes,
            instances: Vec::new(),
        })
    }

    /// This Scatter's shapes with their stones placed over `input` instead,
    /// as [`new`](Self::new) would place them with these options and a World
    /// seed of [`world_seed`](Self::world_seed), without building the shapes
    /// again.
    ///
    /// The input's seed sets where stones lie, not their shapes, so give the
    /// input the World seed the shapes were built with when the result
    /// should match `new`.
    ///
    /// # Errors
    ///
    /// Returns [`StoneScatterError::TooMany`] or
    /// [`StoneScatterError::TooLarge`] as [`new`](Self::new) does.
    pub fn placed_over(&self, input: &ScatterInput<'_>) -> Result<Self, StoneScatterError> {
        let instances = self.stones(input)?;
        Ok(Self {
            instances,
            ..self.clone()
        })
    }

    /// The World seed the shapes were built with.
    pub fn world_seed(&self) -> u32 {
        self.world_seed
    }

    /// The settings this Scatter was built from.
    pub fn options(&self) -> &StoneScatterOptions {
        &self.options
    }

    /// The settings of shape `variant` of class `class`, ready for
    /// [`crate::Stone::new`].
    ///
    /// # Panics
    ///
    /// Panics if `class` or `variant` is out of range.
    pub fn variant(&self, class: usize, variant: u32) -> &StoneOptions {
        &self.variants[class][variant as usize]
    }

    /// The stones placed by [`new`](Self::new), in no particular order, but
    /// the same order every time for the same inputs.
    pub fn instances(&self) -> &[StoneInstance] {
        &self.instances
    }

    /// The position in [`Scatter::slots`] of class `class`'s shape
    /// `variant`.
    ///
    /// # Panics
    ///
    /// Panics if `class` is out of range.
    pub fn slot(&self, class: usize, variant: u32) -> u32 {
        self.options.classes[..class]
            .iter()
            .map(|c| c.variants)
            .sum::<u32>()
            + variant
    }

    /// Places the stones over `input`'s area with this Scatter's shapes, as
    /// [`new`](Self::new) describes. The input's seed sets where stones lie,
    /// not their shapes.
    ///
    /// # Errors
    ///
    /// Returns [`StoneScatterError::TooMany`] or
    /// [`StoneScatterError::TooLarge`] as [`new`](Self::new) does.
    pub fn stones(
        &self,
        input: &ScatterInput<'_>,
    ) -> Result<Vec<StoneInstance>, StoneScatterError> {
        let options = &self.options;

        // Check the size of the job.
        let size = input.area.size;
        let area = f64::from(size.x.max(0.0)) * f64::from(size.y.max(0.0));
        let count: f64 = options
            .classes
            .iter()
            .map(|c| f64::from(c.density) * f64::from(c.members[1]) * area)
            .sum();
        if count > MAX_STONE_INSTANCES as f64 {
            return Err(StoneScatterError::TooMany {
                count: count.ceil() as usize,
            });
        }
        let lattice = options.lattice();
        let reach = self.reach();
        let own = input.area.bounds();
        let window = reach.window(own);
        let candidates = lattice.count_in(window);
        if candidates > MAX_STONE_CANDIDATES as u64 {
            return Err(StoneScatterError::TooLarge {
                candidates: candidates as usize,
            });
        }

        // Find every candidate stone near enough to decide the area's own.
        let seeds = Seeds::new(input.seed);
        let chances: Vec<u32> = options
            .classes
            .iter()
            .map(|c| {
                let cell = f64::from(lattice.cell_metres());
                lattice::chance(f64::from(c.density) * cell * cell)
            })
            .collect();
        let mut found = Vec::new();
        for cell in lattice.cells_in(window) {
            self.candidates_in(cell, &lattice, &chances, &seeds, input, &mut found);
        }

        // Space them out: earlier classes first, then by rank. Then keep the
        // area's own, and clear the bare ground between patches last, so a
        // stone there still keeps its neighbours at a distance and raising
        // the patchiness only removes stones of its class.
        let keep = keep_spaced(&found, reach.rounds, reach.spacing_fixed(), |a, b| {
            let mut apart = a.radius + b.radius;
            if a.instance.class == b.instance.class && a.rank.cell != b.rank.cell {
                apart = apart.max(a.spacing);
            }
            let apart = i128::from(apart);
            a.position.distance_squared(b.position) < apart * apart
        });
        Ok(found
            .into_iter()
            .zip(keep)
            .filter(|(c, keep)| *keep && c.in_patch && own.contains(lattice.corner(c.rank.cell)))
            .map(|(c, _)| c.instance)
            .collect())
    }

    /// Adds to `found` the stones of the spot in `cell`, if a class takes it.
    fn candidates_in(
        &self,
        cell: CellId,
        lattice: &Lattice,
        chances: &[u32],
        seeds: &Seeds,
        input: &ScatterInput<'_>,
        found: &mut Vec<Candidate>,
    ) {
        let h = lattice.hash(seeds.lattice, cell);
        let centre = lattice.position(h, cell);
        let local = input.local_density(centre);

        // The spot goes to the first class whose own rank its density keeps.
        let Some((k, class, rank)) = self
            .options
            .classes
            .iter()
            .enumerate()
            .map(|(k, class)| (k, class, lattice::rank(pcg2(h, class.key.salt()))))
            .find(|&(k, _, rank)| lattice::dense_enough(rank, chances[k], local))
        else {
            return;
        };

        // Place each member of the spot's cluster (one stone without
        // clusters). The whole cluster shares one patch draw.
        let salt = class.key.salt();
        let spread = class.members[1] - class.members[0] + 1;
        let count = class.members[0]
            + ((lattice::unit(h, SALT_COUNT ^ salt) * spread as f32) as u32).min(spread - 1);
        let world = centre.to_world();
        let in_patch =
            lattice::unit(h, SALT_PATCH ^ salt) < patches(class, world, pcg2(seeds.patches, salt));
        for index in 0..count {
            let draws = pcg2(h ^ salt, index ^ SALT_MEMBER);
            let Some((position, instance)) =
                self.place_member(k, cell, index as u16, world, draws, input)
            else {
                continue;
            };
            found.push(Candidate {
                position,
                rank: Rank {
                    tier: k as u32,
                    rank,
                    local,
                    cell,
                    index: index as u16,
                },
                radius: instance.footprint.radius,
                spacing: lattice::to_fixed(f64::from(class.spacing)),
                in_patch,
                instance,
            });
        }
    }

    /// Places member `index` of a cluster of class `k` around World
    /// position `centre`, with its own draws `draws`: offsets it from the
    /// cluster's middle, then checks the mask, the exclusions, the slope and
    /// the ground and rests it, sunk by its burial. Returns `None` where any
    /// check fails.
    fn place_member(
        &self,
        k: usize,
        cell: CellId,
        index: u16,
        centre: DVec2,
        draws: u32,
        input: &ScatterInput<'_>,
    ) -> Option<(Fixed2, StoneInstance)> {
        let class = &self.options.classes[k];

        // Offset the member evenly over a disc around the cluster's middle.
        let reach = class.cluster_radius * lattice::unit(draws, SALT_REACH).sqrt();
        let turn = lattice::unit(draws, SALT_ANGLE) * std::f32::consts::TAU;
        let offset = Vec2::from_angle(turn) * reach;
        let at = Fixed2::from_world(centre + offset.as_dvec2());
        if input.local_density(at) == 0 {
            return None;
        }

        // Keep it off the footprints above it, then check the slope there.
        let variant = ((lattice::unit(draws, SALT_VARIANT) * class.variants as f32) as u32)
            .min(class.variants - 1);
        let shape = &self.shapes[k][variant as usize];
        let options = &self.variants[k][variant as usize];
        let scale = lerp_log(class.size, lattice::unit(draws, SALT_SIZE));
        let size = options.scale * scale;
        let world = at.to_world();
        let footprint = Disc::new(world, shape.radius * size);
        if input.excluded(&[footprint]) || steeper_than(&input.sample_at(at), class.max_slope) {
            return None;
        }

        // Put it down as its placement says, sunk further by its share of the
        // suggested burial. The burial depth is in units of the stone's size.
        let yaw = lattice::unit(draws, SALT_YAW) * std::f32::consts::TAU;
        let burial = class.burial[0]
            + (class.burial[1] - class.burial[0]) * lattice::unit(draws, SALT_BURIAL);
        let contacts = Footprint {
            contacts: shape
                .footprint
                .iter()
                .map(|p| Vec3::new(p.x, 0.0, p.y) * size)
                .collect(),
            extra: Vec::new(),
            size,
        };
        let mut placement = class
            .placement
            .unwrap_or_else(|| options.ground_placement());
        placement.embed += burial * shape.embed_depth;
        let placed = place(input.ground, world, yaw, &contacts, &placement)?;
        let local = at.relative_to(input.area.centre);
        let position = Vec3::new(local.x, placed.height, local.y);
        Some((
            at,
            StoneInstance {
                class: k,
                variant,
                address: input.prefix.copy(cell, index),
                scale,
                position,
                radius: shape.radius * size,
                footprint,
                transform: Affine3A::from_scale_rotation_translation(
                    Vec3::splat(scale),
                    placed.rotation,
                    position,
                ),
            },
        ))
    }

    /// The widest footprint radius of any stone, in metres.
    fn widest(&self) -> f32 {
        self.options
            .classes
            .iter()
            .zip(self.variants.iter().zip(&self.shapes))
            .flat_map(|(class, (list, shapes))| {
                list.iter()
                    .zip(shapes)
                    .map(move |(o, s)| s.radius * o.scale * class.size[1])
            })
            .fold(0.0, f32::max)
    }
}

/// A stone Scatter places every class on one lattice fixed to the World.
/// Its slots are each class's shapes, in class order; the shapes it draws
/// are the ones it was built with (see [`StoneScatter::prepare`]).
impl Scatter for StoneScatter {
    fn lattice(&self) -> Lattice {
        self.options.lattice()
    }

    fn reach(&self) -> Reach {
        let widest = self.widest();
        let spacing = self
            .options
            .classes
            .iter()
            .map(|c| c.spacing)
            .fold(0.0, f32::max)
            .max(2.0 * widest);
        let cluster = self
            .options
            .classes
            .iter()
            .map(|c| c.cluster_radius)
            .fold(0.0, f32::max);
        Reach {
            rounds: DEFAULT_SPACING_ROUNDS,
            spacing,
            displacement: self.options.lattice().cell_metres() + cluster,
            footprint: widest,
        }
    }

    fn draw_range(&self) -> f32 {
        DRAW_RANGE
    }

    fn slots(&self, world_seed: u32) -> Vec<ScatterSlot> {
        self.options
            .classes
            .iter()
            .enumerate()
            .flat_map(|(entry, class)| {
                (0..class.variants).map(move |v| ScatterSlot {
                    entry: entry as u16,
                    specimen: class.stone.variant_id(world_seed, v, class.stretch_jitter),
                })
            })
            .collect()
    }

    fn place(&self, input: &ScatterInput<'_>) -> Result<Vec<ScatterCopy>, ScatterError> {
        Ok(self
            .stones(input)?
            .iter()
            .map(|stone| ScatterCopy {
                address: stone.address,
                slot: self.slot(stone.class, stone.variant),
                transform: stone.transform,
                footprint: vec![stone.footprint],
            })
            .collect())
    }
}

/// The seeds of a stone Scatter's random choices, from its placement seed.
struct Seeds {
    /// Candidate spots: where each stands, which class takes it and its
    /// members' own draws.
    lattice: u32,
    /// Each class's patches, salted by the class's key.
    patches: u32,
}

impl Seeds {
    fn new(seed: u32) -> Self {
        Self {
            lattice: seed_for(seed, LATTICE_SEED),
            patches: seed_for(seed, PATCHES_SEED),
        }
    }
}

/// Returns `Ok` if `ok`, otherwise an error naming `field` and `value`.
fn float(
    field: &'static str,
    value: f32,
    ok: bool,
    requirement: &'static str,
) -> Result<(), OptionsError> {
    if ok && value.is_finite() {
        Ok(())
    } else {
        Err(OptionsError::FloatOutOfRange {
            field,
            value,
            requirement,
        })
    }
}

/// Measures the burial depth and footprint of a stone with `options`.
fn measure(options: &StoneOptions) -> Result<VariantShape, OptionsError> {
    let field = StoneField::new(options)?;
    let placement = StonePlacement::new(&field, &options.placement);
    let embed_depth = placement.embed_depth();

    // Walk out from the stone's axis at its ground line in each direction,
    // and note the last point still inside the stone.
    let line = embed_depth.max(MIN_GROUND_LINE);
    let mut footprint = Vec::with_capacity(FOOTPRINT_DIRECTIONS as usize);
    for d in 0..FOOTPRINT_DIRECTIONS {
        let angle = d as f32 / FOOTPRINT_DIRECTIONS as f32 * std::f32::consts::TAU;
        let direction = Vec2::from_angle(angle);
        let mut reach = 0.0;
        let mut r = 0.0;
        while r < 0.9 {
            let placed = Vec3::new(direction.x * r, line, direction.y * r);
            if field.distance(placement.field_position(placed)) < 0.0 {
                reach = r;
            }
            r += FOOTPRINT_STEP;
        }
        footprint.push(direction * reach.max(MIN_FOOTPRINT));
    }
    let radius = footprint.iter().map(|p| p.length()).fold(0.0, f32::max);
    Ok(VariantShape {
        embed_depth,
        footprint,
        radius,
    })
}

/// Returns the share of class `class`'s density its patches allow at World
/// position `world`: 1 inside a patch, 0 on bare ground between them, with
/// a soft edge.
fn patches(class: &StoneClass, world: DVec2, salt: u32) -> f32 {
    if class.patchiness <= 0.0 {
        return 1.0;
    }
    let n = value_noise(world / f64::from(class.patch_size), salt);
    let t = class.patchiness;
    ((n - (t - PATCH_EDGE)) / (2.0 * PATCH_EDGE)).clamp(0.0, 1.0)
}

/// Smooth noise from 0 to 1, with features about one unit across.
fn value_noise(p: DVec2, salt: u32) -> f32 {
    let base = p.floor();
    let f = (p - base).as_vec2();
    let (cell_x, cell_z) = (base.x as i64 as u32, base.y as i64 as u32);
    let at = |dx: u32, dz: u32| {
        unit_float(pcg2(
            salt,
            pcg2(cell_x.wrapping_add(dx), cell_z.wrapping_add(dz)),
        ))
    };
    let s = f * f * (Vec2::splat(3.0) - 2.0 * f);
    let top = at(0, 0) + (at(1, 0) - at(0, 0)) * s.x;
    let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * s.x;
    top + (bottom - top) * s.y
}

/// Returns the value a share `u` of the way from `range[0]` to `range[1]`,
/// evenly in proportion rather than in difference.
fn lerp_log(range: [f32; 2], u: f32) -> f32 {
    (range[0].ln() + (range[1].ln() - range[0].ln()) * u).exp()
}
