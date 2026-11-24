//! Spreads stones over a stretch of ground: boulders, rocks and pebbles,
//! resting on the slope and partly buried.

use std::collections::HashMap;
use std::fmt;

use glam::{Affine3A, DVec2, Vec2, Vec3, Vec3Swizzles};
use procgena_core::hash::{pcg3d, unit_float};
use procgena_core::scatter::{Footprint, Placement, ScatterInput, place, steeper_than};
use serde::{Deserialize, Serialize};

use crate::field::StoneField;
use crate::options::{OptionsError, StoneOptions};
use crate::placement::StonePlacement;
use crate::presets::StonePreset;

/// Most classes one [`StoneScatterOptions`] can list.
pub const MAX_STONE_CLASSES: usize = 8;
/// Most shape variants one [`StoneClass`] can have.
pub const MAX_STONE_VARIANTS: u32 = 64;
/// Most stones one [`StoneScatter`] can ask for, before the slope, the
/// ground and spacing remove any.
pub const MAX_STONE_INSTANCES: usize = 262_144;
/// Most candidate spots one [`StoneScatter`] can test: the area divided by
/// each class's [`spacing`](StoneClass::spacing) squared, over all classes.
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

/// Salts that keep the random draws of each purpose apart.
const SALT_CLASS: u32 = 0x9e37_79b9;
const SALT_PATCH: u32 = 0x6a09_e667;
const SALT_MEMBER: u32 = 0xbb67_ae85;

/// Most stones one cluster of a [`StoneClass`] can hold.
pub const MAX_CLUSTER_MEMBERS: u32 = 16;

/// One kind of stone in a [`StoneScatterOptions`], such as boulders or
/// pebbles: its shapes, how many there are and how they lie.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoneClass {
    /// The shape settings every variant starts from. Variant `v` uses
    /// [`StoneOptions::variant`]`(world_seed, v, stretch_jitter)`, with the
    /// [`ScatterInput::seed`] as the World seed: the same shapes a
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
    /// A class of single stones shaped like `stone`, at `density` stones per
    /// square metre and at least `spacing` metres apart, with 8 variants, no
    /// stretch jitter, sizes from 0.8 to 1.25, each shape's own placement,
    /// burial from 0.5 to 1, a slope limit of 40 degrees, no clusters and no
    /// patches.
    pub fn new(stone: StoneOptions, density: f32, spacing: f32) -> Self {
        Self {
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
    /// claim their ground first: a stone is left out where it would overlap
    /// a stone of an earlier class. List the largest stones first.
    pub classes: Vec<StoneClass>,
}

impl StoneScatterOptions {
    /// A few large boulders, sparse rocks and many pebbles in small clusters,
    /// gathered in patches.
    pub fn boulder_field() -> Self {
        let mut boulders = StoneClass::new(StonePreset::boulder().stone.clone(), 0.004, 6.0);
        boulders.variants = 6;
        boulders.size = [0.6, 1.3];
        boulders.max_slope = 30.0;
        boulders.patchiness = 0.4;
        boulders.patch_size = 30.0;
        let mut rocks = StoneClass::new(StonePreset::river_stone().stone.clone(), 0.03, 1.5);
        rocks.patchiness = 0.3;
        rocks.patch_size = 12.0;
        let mut pebbles = StoneClass::new(StonePreset::pebble().stone.clone(), 0.25, 0.6);
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

    /// Checks every class's settings.
    ///
    /// # Errors
    ///
    /// Returns [`StoneScatterError::TooManyClasses`] or the first invalid
    /// class setting as [`StoneScatterError::Class`].
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
        }
        Ok(())
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
    /// The candidate cell it came from, within its class. The same input
    /// always gives the stone in this cell the same place and look.
    pub cell: [i32; 2],
    /// Its number within its cell's cluster, from 0. Single stones are 0.
    /// The class, cell and index together name the stone.
    pub index: u32,
    /// Its size factor, already part of [`transform`](Self::transform).
    pub scale: f32,
    /// Where its origin stands, relative to the area's centre, in metres.
    pub position: Vec3,
    /// How far its footprint reaches from its position, along the ground,
    /// in metres. Stones of different classes are at least the sum of their
    /// radii apart.
    pub radius: f32,
    /// Places a mesh of its variant (in metres, as [`crate::Stone`] builds
    /// it) on the ground: size, yaw, tilt to the slope and burial.
    pub transform: Affine3A,
}

/// A variant's shape measurements, in unscaled stone units.
struct VariantShape {
    /// Suggested burial depth.
    embed_depth: f32,
    /// Horizontal distance from the stone's axis to its side at the ground
    /// line, in each of [`FOOTPRINT_DIRECTIONS`] directions.
    footprint: Vec<Vec2>,
    /// The largest of those distances.
    radius: f32,
}

/// A stone that passed the density, slope and ground checks, waiting for the
/// spacing check.
struct Candidate {
    rank: f32,
    radius: f32,
    /// Whether it stands inside one of its class's patches; stones outside
    /// are removed after the spacing check.
    in_patch: bool,
    instance: StoneInstance,
}

/// Stones spread over a stretch of ground by a [`StoneScatterOptions`].
///
/// Build one with [`new`](Self::new), then draw each of
/// [`instances`](Self::instances) with a mesh of its class's variant, from
/// [`variant`](Self::variant) or a `StonePool` with the class's settings.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StoneScatter {
    variants: Vec<Vec<StoneOptions>>,
    instances: Vec<StoneInstance>,
}

impl StoneScatter {
    /// Spreads the stones of `options` over `input`'s area.
    ///
    /// Each class has its own grid of candidate spots, one per
    /// [`spacing`](StoneClass::spacing)-wide cell, anchored to the area's
    /// centre. A spot is kept with a chance of its class's density times the
    /// input's mask; then it is left out where the ground is steeper than
    /// [`max_slope`](StoneClass::max_slope), where any part of the stone has
    /// no ground under it, or where the ground bends more than its
    /// [`placement`](StoneClass::placement) allows. Stones too close to a
    /// stone of an earlier class, or of their own class, are left out next,
    /// and last those between the class's patches. Each stone that is left
    /// stands as its placement says, sunk further by its burial.
    ///
    /// Lowering a class's density only removes stones of that class; the
    /// others keep their places, though stones of later classes may fill the
    /// gaps. Positions are relative to the area's centre.
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
        // Check the settings and the size of the job.
        options.validate()?;
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
        let candidates: f64 = options
            .classes
            .iter()
            .filter(|c| c.density > 0.0)
            .map(|c| area / f64::from(c.spacing * c.spacing))
            .sum();
        if candidates > MAX_STONE_CANDIDATES as f64 {
            return Err(StoneScatterError::TooLarge {
                candidates: candidates.ceil() as usize,
            });
        }

        // Measure every variant's shape.
        let variants: Vec<Vec<StoneOptions>> = options
            .classes
            .iter()
            .map(|class| {
                (0..class.variants)
                    .map(|v| class.variant(input.seed, v))
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

        // Keep each class's candidates apart in rank order, after the stones
        // of earlier classes. Rank order means the stones a lower density
        // keeps are visited first and decided the same way.
        let mut kept = Kept::new(options, &variants, &shapes);
        for (k, class) in options.classes.iter().enumerate() {
            let mut found = candidates_of(k, class, &variants[k], &shapes[k], input);
            found.sort_by(|a, b| {
                a.rank
                    .total_cmp(&b.rank)
                    .then(a.instance.cell.cmp(&b.instance.cell))
                    .then(a.instance.index.cmp(&b.instance.index))
            });
            let mut own = kept.empty_like();
            for candidate in found {
                if !kept.crowds(&candidate, class.spacing) && !own.crowds(&candidate, class.spacing)
                {
                    own.insert(candidate);
                }
            }

            // Clear the bare ground between patches last, so a stone there
            // still keeps its neighbours at a distance and raising the
            // patchiness only removes stones of this class.
            for stone in own.stones.into_iter().filter(|s| s.in_patch) {
                kept.insert(stone);
            }
        }
        Ok(Self {
            variants,
            instances: kept.stones.into_iter().map(|s| s.instance).collect(),
        })
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

    /// The placed stones, in no particular order, but the same order every
    /// time for the same inputs.
    pub fn instances(&self) -> &[StoneInstance] {
        &self.instances
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

/// Lists the stones of class `k` that pass the density, slope and ground
/// checks, each with its rank.
fn candidates_of(
    k: usize,
    class: &StoneClass,
    variants: &[StoneOptions],
    shapes: &[VariantShape],
    input: &ScatterInput<'_>,
) -> Vec<Candidate> {
    let mut found = Vec::new();
    if class.density <= 0.0 {
        return found;
    }

    // Visit every cell of the class's grid that touches the area.
    let cell = class.spacing;
    let half = input.area.size * 0.5;
    let span = |h: f32| ((-h / cell).floor() as i32, (h / cell).ceil() as i32);
    let ((x0, x1), (z0, z1)) = (span(half.x), span(half.y));
    let salt = input.seed ^ (k as u32 + 1).wrapping_mul(SALT_CLASS);
    let chance = class.density * cell * cell;
    for i in x0..x1 {
        for j in z0..z1 {
            // Draw the spot's place, rank and cluster from its cell.
            let a = pcg3d([i as u32, j as u32, salt]);
            let b = pcg3d(a);
            let centre = Vec2::new(
                (i as f32 + unit_float(a[0])) * cell,
                (j as f32 + unit_float(a[1])) * cell,
            );
            if !input.area.contains(centre) {
                continue;
            }

            // Keep it with a chance that grows with the local density, so a
            // lower density keeps a subset of a higher one's stones.
            let local_density = input.density(centre);
            if local_density <= 0.0 {
                continue;
            }
            let rank = unit_float(a[2]) / local_density;
            if rank >= chance {
                continue;
            }

            // Place each member of the spot's cluster (one stone without
            // clusters). The whole cluster shares one patch draw.
            let spread = class.members[1] - class.members[0] + 1;
            let count =
                class.members[0] + ((unit_float(b[0]) * spread as f32) as u32).min(spread - 1);
            let in_patch = unit_float(b[1]) < patches(class, input.area.world(centre), salt);
            for index in 0..count {
                let spot = Spot {
                    class: k,
                    cell: [i, j],
                    index,
                    centre,
                    draws: pcg3d([a[0], a[1] ^ index.wrapping_mul(SALT_MEMBER), b[2]]),
                };
                if let Some(instance) = place_member(class, variants, shapes, input, &spot) {
                    found.push(Candidate {
                        rank,
                        radius: instance.radius,
                        in_patch,
                        instance,
                    });
                }
            }
        }
    }
    found
}

/// One member of a kept spot's cluster, waiting to be placed.
struct Spot {
    /// The stone's class.
    class: usize,
    /// The candidate cell the spot came from.
    cell: [i32; 2],
    /// The member's number within the cell.
    index: u32,
    /// The cluster's middle, relative to the area's centre.
    centre: Vec2,
    /// The member's own random draws.
    draws: [u32; 3],
}

/// Places the member `spot` of a cluster of class `class`: offsets it from
/// the cluster's middle, then checks the mask, the slope and the ground and
/// rests it, sunk by its burial. Returns `None` where any check fails.
fn place_member(
    class: &StoneClass,
    variants: &[StoneOptions],
    shapes: &[VariantShape],
    input: &ScatterInput<'_>,
    spot: &Spot,
) -> Option<StoneInstance> {
    let b = spot.draws;
    let c = pcg3d(b);
    let d = pcg3d(c);

    // Offset the member evenly over a disc around the cluster's middle.
    let reach = class.cluster_radius * unit_float(d[0]).sqrt();
    let local = spot.centre + Vec2::from_angle(unit_float(d[1]) * std::f32::consts::TAU) * reach;
    if !input.area.contains(local) || input.density(local) <= 0.0 {
        return None;
    }

    // Check the slope at the spot, then rest the stone on the ground.
    if steeper_than(&input.sample(local), class.max_slope) {
        return None;
    }
    let variant = ((unit_float(b[0]) * class.variants as f32) as u32).min(class.variants - 1);
    let shape = &shapes[variant as usize];
    let metres = variants[variant as usize].scale;
    let scale = lerp_log(class.size, unit_float(b[1]));
    let yaw = unit_float(b[2]) * std::f32::consts::TAU;
    let burial = class.burial[0] + (class.burial[1] - class.burial[0]) * unit_float(c[0]);
    let size = metres * scale;
    let footprint = Footprint {
        contacts: shape
            .footprint
            .iter()
            .map(|p| Vec3::new(p.x, 0.0, p.y) * size)
            .collect(),
        extra: Vec::new(),
        size,
    };

    // Put it down as its placement says, sunk further by its share of the
    // suggested burial. The burial depth is in units of the stone's size.
    let mut placement = class
        .placement
        .unwrap_or_else(|| variants[variant as usize].ground_placement());
    placement.embed += burial * shape.embed_depth;
    let placed = place(
        input.ground,
        input.area.world(local),
        yaw,
        &footprint,
        &placement,
    )?;
    let position = Vec3::new(local.x, placed.height, local.y);
    Some(StoneInstance {
        class: spot.class,
        variant,
        cell: spot.cell,
        index: spot.index,
        scale,
        position,
        radius: shape.radius * size,
        transform: Affine3A::from_scale_rotation_translation(
            Vec3::splat(scale),
            placed.rotation,
            position,
        ),
    })
}

/// Returns the share of class `class`'s density its patches allow at World
/// position `world`: 1 inside a patch, 0 on bare ground between them, with
/// a soft edge.
fn patches(class: &StoneClass, world: DVec2, salt: u32) -> f32 {
    if class.patchiness <= 0.0 {
        return 1.0;
    }
    let n = value_noise(world / f64::from(class.patch_size), salt ^ SALT_PATCH);
    let t = class.patchiness;
    ((n - (t - PATCH_EDGE)) / (2.0 * PATCH_EDGE)).clamp(0.0, 1.0)
}

/// Smooth noise from 0 to 1, with features about one unit across.
fn value_noise(p: DVec2, salt: u32) -> f32 {
    let base = p.floor();
    let f = (p - base).as_vec2();
    let (cell_x, cell_z) = (base.x as i64 as u32, base.y as i64 as u32);
    let at = |dx: u32, dz: u32| {
        unit_float(pcg3d([cell_x.wrapping_add(dx), cell_z.wrapping_add(dz), salt])[0])
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

/// Stones kept so far, bucketed so a spacing check only looks nearby.
struct Kept {
    stones: Vec<Candidate>,
    /// Bucket width: at least the largest distance two stones are kept
    /// apart.
    reach: f32,
    buckets: HashMap<(i32, i32), Vec<usize>>,
}

impl Kept {
    /// An empty set whose buckets fit every stone `options` can place.
    fn new(
        options: &StoneScatterOptions,
        variants: &[Vec<StoneOptions>],
        shapes: &[Vec<VariantShape>],
    ) -> Self {
        // The widest footprint of any stone, at its largest size.
        let widest = options
            .classes
            .iter()
            .zip(variants.iter().zip(shapes))
            .flat_map(|(class, (list, shapes))| {
                list.iter()
                    .zip(shapes)
                    .map(move |(o, s)| s.radius * o.scale * class.size[1])
            })
            .fold(0.0, f32::max);
        let spacing = options
            .classes
            .iter()
            .map(|c| c.spacing)
            .fold(0.0, f32::max);
        Self {
            stones: Vec::new(),
            reach: spacing.max(2.0 * widest).max(1e-3),
            buckets: HashMap::new(),
        }
    }

    /// An empty set with the same buckets.
    fn empty_like(&self) -> Self {
        Self {
            stones: Vec::new(),
            reach: self.reach,
            buckets: HashMap::new(),
        }
    }

    /// Returns the bucket that holds `p`.
    fn bucket(&self, p: Vec2) -> (i32, i32) {
        let key = |v: f32| (v / self.reach).floor() as i32;
        (key(p.x), key(p.y))
    }

    /// Whether `candidate` is closer than `spacing` to a kept stone of its
    /// own class from another cell, or its footprint overlaps any kept
    /// stone's.
    fn crowds(&self, candidate: &Candidate, spacing: f32) -> bool {
        let p = candidate.instance.position.xz();
        let (bx, bz) = self.bucket(p);
        (bx - 1..=bx + 1)
            .flat_map(|cx| (bz - 1..=bz + 1).map(move |cz| (cx, cz)))
            .filter_map(|b| self.buckets.get(&b))
            .flatten()
            .any(|&n| {
                let other = &self.stones[n];
                let mut apart = candidate.radius + other.radius;
                let mine = &candidate.instance;
                if other.instance.class == mine.class && other.instance.cell != mine.cell {
                    apart = apart.max(spacing);
                }
                p.distance_squared(other.instance.position.xz()) < apart * apart
            })
    }

    /// Keeps `candidate`.
    fn insert(&mut self, candidate: Candidate) {
        let b = self.bucket(candidate.instance.position.xz());
        self.buckets.entry(b).or_default().push(self.stones.len());
        self.stones.push(candidate);
    }
}
