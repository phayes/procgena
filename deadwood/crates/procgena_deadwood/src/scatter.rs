//! Sticks, twigs and logs scattered over a rectangle of ground. Like every
//! procgena Scatter, it takes a [`ScatterInput`]: the ground, an optional
//! mask that thins the sticks out in places, the area and a seed.
//!
//! The result is a few distinct stick meshes ([`StickScatter::variants`])
//! and a list of placed copies ([`StickScatter::instances`]). A renderer can
//! build each mesh once and draw it many times, which is much cheaper than
//! a separate mesh per stick. This is often called instancing.

use std::fmt;

use glam::{Affine3A, DVec2, Mat3, Quat, Vec2, Vec3};
use procgena_core::hash::seed_for;
use procgena_core::scatter::lattice::{self, Fixed2};
use procgena_core::scatter::{
    DEFAULT_SPACING_ROUNDS, Disc, Footprint, Lattice, Placement, Rank, Reach, Rest, Scatter,
    ScatterCopy, ScatterInput, ScatterSlot, Spaced, keep_spaced, place, steeper_than,
};
use procgena_core::{CellId, CopyAddress, Quality, SpecimenId};
use procgena_specimen::{Level, Part, Specimen, SurfaceMesh};
use serde::{Deserialize, Serialize};

use crate::detail::{DeadwoodLod, default_lod};
use crate::options::{DeadwoodOptions, OptionsError, PieceKind, int_range, positive, within};
use crate::piece::Piece;
use crate::presets::DeadwoodPreset;
use crate::rng::stream;
use crate::seed_keys::{SCATTER_CELL, SCATTER_VARIANT};
use crate::settle::surface_points;

/// Largest number of sticks one scatter may place.
pub const MAX_INSTANCES: usize = 200_000;
/// Largest number of distinct stick meshes.
pub const MAX_VARIANTS: u32 = 32;
/// Most contact points kept per variant for fitting it to the ground.
const MAX_CONTACTS: usize = 48;
/// Smallest footprint disc radius, in metres, for the thinnest twigs.
const MIN_FOOTPRINT: f32 = 0.005;

/// Salts that give each draw of one candidate its own value.
const SALT_VARIANT: u32 = 0x9e37_79b9;
const SALT_YAW: u32 = 0x6a09_e667;

/// The ground shapes sticks lie on. They live in `procgena_core`, shared
/// with the other generators that scatter over ground.
pub use procgena_core::{FlatGround, Ground, HeightGrid};

/// Settings for a [`StickScatter`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScatterOptions {
    /// The seed to pass in [`ScatterInput::seed`] when this scatter is
    /// built on its own, such as in a viewer. [`StickScatter::new`] uses the
    /// input's seed, not this one.
    pub default_seed: u32,
    /// The steepest ground, in degrees from 0 through 90, that a stick is
    /// placed on. 90 places sticks on any measured slope.
    pub max_slope: f32,
    /// Sticks per square metre where the density function is 1, before
    /// [`min_spacing`](Self::min_spacing) removes any. Must be positive.
    pub density: f32,
    /// Smallest distance between sticks' centres, in metres. Must be zero
    /// or more; zero allows any spacing.
    pub min_spacing: f32,
    /// Number of distinct stick meshes, from 1 up to [`MAX_VARIANTS`].
    pub variants: u32,
    /// Smallest and largest decay of the variants, each from 0 through
    /// [`crate::MAX_DECAY`], smallest first.
    pub decay: [f32; 2],
    /// Smallest and largest stick diameter, in metres. Both must be
    /// positive, smallest first.
    pub diameter: [f32; 2],
    /// Smallest and largest stick length, in metres. Both must be positive,
    /// smallest first.
    pub length: [f32; 2],
    /// Settings every variant starts from, before its own seed, decay,
    /// diameter and length are chosen. Its kind decides how copies are put
    /// on the ground (see [`PieceKind::placement`]): logs and branches lie
    /// along it, stumps stand upright. Where a variant turns out too short
    /// for its ends, those ends become flat saw cuts.
    pub piece: DeadwoodOptions,
}

impl ScatterOptions {
    /// Kindling: a mix of sticks 1 to 7 cm thick. Foresters call these the
    /// 10-hour and 100-hour fuel classes, after roughly how long the wood
    /// takes to dry out or soak up moisture.
    pub fn kindling() -> Self {
        Self {
            default_seed: 1,
            max_slope: 35.0,
            density: 1.5,
            min_spacing: 0.3,
            variants: 6,
            decay: [1.0, 3.0],
            diameter: [0.01, 0.07],
            length: [0.4, 1.2],
            piece: DeadwoodPreset::stick().deadwood.clone(),
        }
    }

    /// Twig litter: many thin twigs under 2.5 cm thick, most drawn as flat
    /// strips at a distance.
    pub fn twig_litter() -> Self {
        Self {
            default_seed: 2,
            max_slope: 45.0,
            density: 8.0,
            min_spacing: 0.08,
            variants: 8,
            decay: [1.0, 4.0],
            diameter: [0.004, 0.02],
            length: [0.15, 0.5],
            piece: DeadwoodPreset::twig().deadwood.clone(),
        }
    }

    /// Fallen logs: a few wind-thrown trunks 20 to 50 cm thick and 2 to 6 m
    /// long, kept to gentle slopes.
    pub fn fallen_logs() -> Self {
        Self {
            default_seed: 3,
            max_slope: 25.0,
            density: 0.04,
            min_spacing: 3.0,
            variants: 4,
            decay: [1.0, 4.0],
            diameter: [0.2, 0.5],
            length: [2.0, 6.0],
            piece: DeadwoodPreset::windthrow_log().deadwood.clone(),
        }
    }

    /// Checks the settings, except [`piece`](Self::piece). The variants made
    /// from `piece` are checked when [`StickScatter::new`] builds them.
    ///
    /// # Errors
    ///
    /// Returns an [`OptionsError`] naming the first invalid setting.
    pub fn validate(&self) -> Result<(), OptionsError> {
        within("scatter.max_slope", self.max_slope, 0.0, 90.0)?;
        positive("scatter.density", self.density)?;
        within("scatter.min_spacing", self.min_spacing, 0.0, f32::MAX)?;
        int_range("scatter.variants", self.variants, 1, MAX_VARIANTS)?;
        for (field, [lo, hi]) in [
            ("scatter.decay", self.decay),
            ("scatter.diameter", self.diameter),
            ("scatter.length", self.length),
        ] {
            within(field, lo, 0.0, f32::MAX)?;
            within(field, hi, lo, f32::MAX)?;
        }
        within("scatter.decay", self.decay[1], 0.0, crate::MAX_DECAY)?;
        positive("scatter.diameter", self.diameter[0])?;
        positive("scatter.length", self.length[0])
    }
}

/// Why a scatter could not be made.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ScatterError {
    /// A setting, or a variant's settings, is out of range.
    Options(OptionsError),
    /// The density and size would place more than [`MAX_INSTANCES`]
    /// sticks.
    TooMany {
        /// The number of sticks the settings ask for.
        count: usize,
    },
}

impl fmt::Display for ScatterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScatterError::Options(e) => write!(f, "invalid scatter options: {e}"),
            ScatterError::TooMany { count } => write!(
                f,
                "the scatter asks for {count} sticks, but at most {MAX_INSTANCES} are allowed"
            ),
        }
    }
}

impl std::error::Error for ScatterError {}

impl From<OptionsError> for ScatterError {
    fn from(e: OptionsError) -> Self {
        ScatterError::Options(e)
    }
}

/// One placed copy of a variant.
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    /// Index into [`StickScatter::variants`].
    pub variant: u32,
    /// Names the copy anywhere in the World: the lattice cell its spot came
    /// from. The same input gives the copy at this address the same place
    /// and look from any area that holds its cell.
    pub address: CopyAddress,
    /// Where the middle of the stick is, in X and Z relative to the area's
    /// centre, with the ground height at that point in Y, in metres.
    pub position: Vec3,
    /// Moves the variant's meshes into place: turned, tilted to the
    /// ground's slope, and resting on it. Apply it to every vertex of the
    /// variant's mesh, at any level of detail.
    pub transform: Affine3A,
    /// The ground the piece takes up, in World fixed point: a chain of discs
    /// along a lying piece's main stem, or one disc for a stump. Other
    /// Scatters keep their copies out of it.
    pub footprint: Vec<Disc>,
}

/// Sticks, logs or stumps scattered over ground: the variants for one World
/// seed, and the copies of one area.
///
/// Create one with [`StickScatter::new`], then draw each of
/// [`instances`](Self::instances) using the mesh of its variant. It is also
/// a [`Scatter`], placing any other area with the same variants.
///
/// Copies stand on a lattice fixed to the World, one candidate per cell, so
/// an area holds the same copies, with the same addresses, as any larger
/// area around it, and as the chunks it splits into, built in any order.
#[derive(Debug, Clone, PartialEq)]
pub struct StickScatter {
    options: ScatterOptions,
    variants: Vec<Piece>,
    fits: Vec<Fit>,
    instances: Vec<Instance>,
}

/// How one variant is put on the ground.
#[derive(Debug, Clone, PartialEq)]
struct Fit {
    /// The middle of the variant in X and Z, at `y = 0`. Copies are placed
    /// around this point.
    centre: Vec3,
    /// Where the variant touches the ground, measured from `centre`.
    footprint: Footprint,
    /// How the variant follows the ground, from its kind.
    placement: Placement,
    /// The discs of ground it takes up when unturned, measured from
    /// `centre`: offsets in X and Z, and radii, in metres.
    outline: Vec<(Vec2, f32)>,
}

/// A copy that passed the density, exclusion, slope and ground checks,
/// waiting for the spacing check.
struct Candidate {
    position: Fixed2,
    rank: Rank,
    instance: Instance,
}

impl Spaced for Candidate {
    fn position(&self) -> Fixed2 {
        self.position
    }

    fn rank(&self) -> Rank {
        self.rank
    }
}

impl StickScatter {
    /// Builds the variants with `input`'s seed as the World seed, then
    /// scatters copies over `input`'s area of ground, thinned by its mask.
    ///
    /// Each lattice cell holds one candidate. It is kept while its rank is
    /// below the density times the mask; then it is left out where it would
    /// overlap a footprint the input keeps clear, where the ground is
    /// steeper than [`ScatterOptions::max_slope`], or where any part of it
    /// has no ground under it; then copies closer than
    /// [`ScatterOptions::min_spacing`] are thinned by rank. Lowering the
    /// density or the mask only removes copies. Positions in the result are
    /// relative to the area's centre, and the area holds the copies whose
    /// cell's corner lies inside it, so a copy may stand up to a cell outside
    /// it.
    ///
    /// # Errors
    ///
    /// Returns [`ScatterError::Options`] if a setting, the area's size or a
    /// variant is invalid, and [`ScatterError::TooMany`] if the settings ask
    /// for more than [`MAX_INSTANCES`] copies.
    pub fn new(options: &ScatterOptions, input: &ScatterInput<'_>) -> Result<Self, ScatterError> {
        let mut scatter = Self::prepare(options, input.seed)?;
        scatter.instances = scatter.sticks(input)?;
        Ok(scatter)
    }

    /// Builds the variants of `options` in a World seeded with
    /// `world_seed`, without placing any copies: place them with
    /// [`sticks`](Self::sticks) or as a [`Scatter`].
    ///
    /// # Errors
    ///
    /// Returns [`ScatterError::Options`] if a setting or a variant is
    /// invalid.
    pub fn prepare(options: &ScatterOptions, world_seed: u32) -> Result<Self, ScatterError> {
        options.validate()?;

        // Build the variants, each resting on flat ground in its own space.
        // Variant `v` is the specimen in slot `v` of the variant pool, so
        // its seed, and the size and decay drawn from it, depend only on the
        // World seed, the pool and the slot.
        let pool = options.variant_pool();
        let mut variants = Vec::with_capacity(options.variants as usize);
        for v in 0..options.variants {
            let mut o = options.piece.clone();
            o.default_seed = SpecimenId::new(world_seed, pool, v).seed();
            let mut rng = stream(o.default_seed, SCATTER_VARIANT);
            o.decay = rng.range(options.decay[0], options.decay[1]);
            let (lo, hi) = (options.diameter[0].ln(), options.diameter[1].ln());
            o.diameter = rng.range(lo, hi).exp();
            o.length = rng.range(options.length[0], options.length[1]);
            variants.push(Piece::new(o)?);
        }
        let fits: Vec<Fit> = variants.iter().map(fit).collect();
        Ok(Self {
            options: options.clone(),
            variants,
            fits,
            instances: Vec::new(),
        })
    }

    /// Places copies over `input`'s area with this Scatter's variants, as
    /// [`new`](Self::new) describes. The input's seed sets where copies lie,
    /// not their shapes.
    ///
    /// # Errors
    ///
    /// Returns [`ScatterError::Options`] if the area's size is not positive,
    /// and [`ScatterError::TooMany`] if the settings ask for more than
    /// [`MAX_INSTANCES`] copies.
    pub fn sticks(&self, input: &ScatterInput<'_>) -> Result<Vec<Instance>, ScatterError> {
        let options = &self.options;
        let size = input.area.size;
        positive("area.size", size.x)?;
        positive("area.size", size.y)?;
        let count = (size.x * size.y * options.density).ceil() as usize;
        if count > MAX_INSTANCES {
            return Err(ScatterError::TooMany { count });
        }

        // Look at every candidate near enough to decide the area's own.
        let lattice = self.lattice();
        let reach = self.reach();
        let own = input.area.bounds();
        let cell = f64::from(lattice.cell_metres());
        let chance = lattice::chance(f64::from(options.density) * cell * cell);
        let seed = seed_for(input.seed, SCATTER_CELL);
        let mut found = Vec::new();
        for cell in lattice.cells_in(reach.window(own)) {
            let h = lattice.hash(seed, cell);
            let at = lattice.position(h, cell);
            let local = input.local_density(at);
            let rank = lattice::rank(h);
            if !lattice::dense_enough(rank, chance, local) {
                continue;
            }
            let variant = ((lattice::unit(h, SALT_VARIANT) * options.variants as f32) as u32)
                .min(options.variants - 1);
            let yaw = lattice::unit(h, SALT_YAW) * std::f32::consts::TAU;
            let Some(instance) = self.put_down(variant, cell, at, yaw, input) else {
                continue;
            };
            found.push(Candidate {
                position: at,
                rank: Rank {
                    tier: 0,
                    rank,
                    local,
                    cell,
                    index: 0,
                },
                instance,
            });
        }

        // Thin copies closer than the spacing by rank, and keep the area's
        // own.
        let spacing = reach.spacing_fixed();
        let keep = keep_spaced(&found, reach.rounds, spacing, |a, b| {
            a.position.distance_squared(b.position) < i128::from(spacing) * i128::from(spacing)
        });
        Ok(found
            .into_iter()
            .zip(keep)
            .filter(|(c, keep)| *keep && own.contains(lattice.corner(c.rank.cell)))
            .map(|(c, _)| c.instance)
            .collect())
    }

    /// Puts variant `variant`, turned by `yaw`, at World position `at` on
    /// `input`'s ground, or `None` where it overlaps a footprint the input
    /// keeps clear, the ground is too steep, or there is no ground under it.
    fn put_down(
        &self,
        variant: u32,
        cell: CellId,
        at: Fixed2,
        yaw: f32,
        input: &ScatterInput<'_>,
    ) -> Option<Instance> {
        let fit = &self.fits[variant as usize];
        let world = at.to_world();

        // Keep the piece off the footprints above it, then check the slope.
        let turn = Quat::from_rotation_y(yaw);
        let footprint: Vec<Disc> = fit
            .outline
            .iter()
            .map(|&(offset, radius)| {
                let o = turn * Vec3::new(offset.x, 0.0, offset.y);
                Disc::new(world + DVec2::new(f64::from(o.x), f64::from(o.z)), radius)
            })
            .collect();
        if input.excluded(&footprint) {
            return None;
        }
        let sample = input.sample_at(at);
        if steeper_than(&sample, self.options.max_slope) {
            return None;
        }

        // Rest it on the ground as its kind says.
        let placed = place(input.ground, world, yaw, &fit.footprint, &fit.placement)?;
        let local = at.relative_to(input.area.centre);
        Some(Instance {
            variant,
            address: input.prefix.copy(cell, 0),
            position: Vec3::new(local.x, sample.height, local.y),
            transform: Affine3A::from_rotation_translation(
                placed.rotation,
                Vec3::new(local.x, placed.height, local.y),
            ) * Affine3A::from_translation(-fit.centre),
            footprint,
        })
    }

    /// This Scatter's variants with their copies placed over `input`
    /// instead, without building the variants again.
    ///
    /// The variants keep the World seed they were built with. When that is
    /// also `input`'s seed, the result matches [`new`](Self::new) with these
    /// options and `input`.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`sticks`](Self::sticks).
    pub fn placed_over(&self, input: &ScatterInput<'_>) -> Result<Self, ScatterError> {
        let instances = self.sticks(input)?;
        Ok(Self {
            instances,
            ..self.clone()
        })
    }

    /// The settings this Scatter was built from.
    pub fn options(&self) -> &ScatterOptions {
        &self.options
    }

    /// The distinct sticks. Each rests on flat ground at `y = 0`; mesh
    /// it with [`Piece::mesh`] or [`Piece::lod_meshes`] and place copies
    /// with each [`Instance::transform`].
    pub fn variants(&self) -> &[Piece] {
        &self.variants
    }

    /// The copies placed by [`new`](Self::new).
    pub fn instances(&self) -> &[Instance] {
        &self.instances
    }

    /// Bakes every placed copy into one [`Specimen`], with one level per
    /// level of `lod`. Each level holds one part per set of tags, with every
    /// copy's mesh merged in. Strips are left out, as in [`Piece::specimen`].
    ///
    /// # Errors
    ///
    /// Returns the first problem with `lod`; see [`crate::validate_lod`].
    ///
    /// # Panics
    ///
    /// Panics if a variant's mesh needs more vertices than fit in a 32-bit
    /// index.
    pub fn specimen(&self, lod: &DeadwoodLod) -> Result<Specimen, OptionsError> {
        let per_variant: Vec<Specimen> = self
            .variants
            .iter()
            .map(|v| v.specimen(lod))
            .collect::<Result<_, _>>()?;
        let levels = lod
            .levels
            .iter()
            .enumerate()
            .map(|(l, level)| {
                let mut parts: Vec<Part> = Vec::new();
                for inst in &self.instances {
                    for part in &per_variant[inst.variant as usize].levels[l].parts {
                        merge_part(&mut parts, part, &inst.transform);
                    }
                }
                Level {
                    start: level.distance,
                    parts,
                }
            })
            .collect();
        Ok(Specimen { levels })
    }
}

impl PieceKind {
    /// How copies of a piece of this kind are put on uneven ground by a
    /// [`StickScatter`]: logs and branches lie along the ground's slope,
    /// resting on the plane fitted under them; stumps stay upright, sunk
    /// until the lowest ground around their base meets it.
    pub fn placement(self) -> Placement {
        match self {
            PieceKind::Log | PieceKind::Branch => Placement {
                conform: 1.0,
                rest: Rest::Plane,
                ..Placement::default()
            },
            PieceKind::Stump => Placement {
                conform: 0.0,
                rest: Rest::Lowest,
                ..Placement::default()
            },
        }
    }
}

/// Measures where `piece` touches flat ground and how it follows uneven
/// ground.
fn fit(piece: &Piece) -> Fit {
    let o = piece.options();
    let points = surface_points(piece.limbs(), |_| true);
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in &points {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let centre = Vec3::new((lo.x + hi.x) * 0.5, 0.0, (lo.z + hi.z) * 0.5);

    // Contacts: for a lying piece, the surface near the ground; for a stump,
    // whose base and roots reach below the ground, the ring where its
    // surface crosses the ground, flattened onto `y = 0`.
    let near = 0.25 * o.diameter + 1e-3;
    let stump = o.kind == PieceKind::Stump;
    let low: Vec<Vec3> = if stump {
        points
            .iter()
            .filter(|p| p.y.abs() <= near)
            .map(|p| Vec3::new(p.x, 0.0, p.z) - centre)
            .collect()
    } else {
        points
            .iter()
            .filter(|p| p.y <= near)
            .map(|&p| p - centre)
            .collect()
    };
    let stride = low.len().div_ceil(MAX_CONTACTS).max(1);
    let contacts: Vec<Vec3> = low.into_iter().step_by(stride).collect();

    // A lying piece also samples the ground either side of its middle,
    // across its main stem, so a straight stick still finds the cross-slope.
    let width = o.diameter.max(0.1 * o.length);
    let main = &piece.limbs()[0].curve;
    let (a, b) = (main.at(0.0).position, main.at(1.0).position);
    let extra = if stump {
        Vec::new()
    } else {
        let axis = Vec3::new(b.x - a.x, 0.0, b.z - a.z).normalize_or(Vec3::X);
        let side = axis.cross(Vec3::Y) * width;
        vec![Vec2::new(side.x, side.z), -Vec2::new(side.x, side.z)]
    };

    // The ground it takes up: one disc for a stump, and a chain of discs
    // along a lying piece's main stem, close enough to leave no gaps.
    let radius = (0.5 * o.diameter).max(MIN_FOOTPRINT);
    let outline = if stump {
        vec![(Vec2::ZERO, radius)]
    } else {
        let (a, b) = (
            Vec2::new(a.x - centre.x, a.z - centre.z),
            Vec2::new(b.x - centre.x, b.z - centre.z),
        );
        let steps = (a.distance(b) / radius).ceil().max(1.0) as u32;
        (0..=steps)
            .map(|i| (a.lerp(b, i as f32 / steps as f32), radius))
            .collect()
    };
    Fit {
        centre,
        footprint: Footprint {
            contacts,
            extra,
            size: width,
        },
        placement: o.kind.placement(),
        outline,
    }
}

/// A deadwood Scatter places its copies on a lattice fixed to the World,
/// one candidate per cell. Its slots are its variants, from the World seed
/// given to [`slots`](Scatter::slots); the variants it draws are the ones
/// it was built with (see [`StickScatter::prepare`]).
impl Scatter for StickScatter {
    fn lattice(&self) -> Lattice {
        // Cells as wide as the spacing, narrowed until one holds at most one
        // copy at full density.
        let density = self.options.density;
        let fill = 1.0 / density.sqrt();
        let spacing = self.options.min_spacing;
        let cell = if spacing > 0.0 {
            spacing.min(fill)
        } else {
            fill
        };
        Lattice::new(cell)
    }

    fn reach(&self) -> Reach {
        let footprint = self
            .fits
            .iter()
            .flat_map(|fit| fit.outline.iter().map(|(o, r)| o.length() + r))
            .fold(0.0, f32::max);
        Reach {
            rounds: DEFAULT_SPACING_ROUNDS,
            spacing: self.options.min_spacing,
            displacement: self.lattice().cell_metres(),
            footprint,
        }
    }

    fn draw_range(&self) -> f32 {
        // The default detail levels' cull distance, for the longest piece.
        let lod = default_lod();
        let longest = self.options.length[1].max(self.options.diameter[1]);
        lod.cull_distance.unwrap_or(f32::INFINITY) * lod.factor(longest, Quality::High)
    }

    fn slots(&self, world_seed: u32) -> Vec<ScatterSlot> {
        let pool = self.options.variant_pool();
        (0..self.options.variants)
            .map(|v| ScatterSlot {
                entry: 0,
                specimen: SpecimenId::new(world_seed, pool, v),
            })
            .collect()
    }

    fn place(
        &self,
        input: &ScatterInput<'_>,
    ) -> Result<Vec<ScatterCopy>, procgena_core::scatter::ScatterError> {
        Ok(self
            .sticks(input)?
            .into_iter()
            .map(|stick| ScatterCopy {
                address: stick.address,
                slot: stick.variant,
                transform: stick.transform,
                footprint: stick.footprint,
            })
            .collect())
    }
}

/// Adds `part`, moved by `transform`, to the part in `parts` with the same
/// tags, or as a new part.
fn merge_part(parts: &mut Vec<Part>, part: &Part, transform: &Affine3A) {
    let target = match parts.iter().position(|p| p.tags == part.tags) {
        Some(i) => &mut parts[i],
        None => {
            let mut empty = Part::new(part.tags.clone(), SurfaceMesh::default());
            empty.closed = true;
            parts.push(empty);
            parts.last_mut().expect("just pushed")
        }
    };
    let mesh = std::sync::Arc::make_mut(&mut target.mesh);
    let base = mesh.positions.len() as u32;
    let first_triangle = (mesh.indices.len() / 3) as u32;
    let rotation = Mat3::from(transform.matrix3);
    mesh.positions.extend(
        part.mesh
            .positions
            .iter()
            .map(|p| transform.transform_point3(Vec3::from(*p)).to_array()),
    );
    mesh.normals.extend(
        part.mesh
            .normals
            .iter()
            .map(|n| (rotation * Vec3::from(*n)).normalize().to_array()),
    );
    mesh.indices
        .extend(part.mesh.indices.iter().map(|i| i + base));
    target.shells.extend(
        part.shells
            .iter()
            .map(|s| s.start + first_triangle..s.end + first_triangle),
    );
    target.closed &= part.closed;
}
