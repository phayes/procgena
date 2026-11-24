//! Sticks, twigs and logs scattered over a rectangle of ground. Like every
//! procgena Scatter, it takes a [`ScatterInput`]: the ground, an optional
//! mask that thins the sticks out in places, the area and a seed.
//!
//! The result is a few distinct stick meshes ([`StickScatter::variants`])
//! and a list of placed copies ([`StickScatter::instances`]). A renderer can
//! build each mesh once and draw it many times, which is much cheaper than
//! a separate mesh per stick. This is often called instancing.

use std::collections::HashMap;
use std::fmt;

use glam::{Affine3A, Mat3, Vec2, Vec3};
use procgena_core::hash::{derive_seed, seed_for};
use procgena_core::scatter::{
    Footprint, Placement as GroundPlacement, Rest, ScatterInput, place, steeper_than,
};
use procgena_core::{Rng, SpecimenId};
use procgena_specimen::{Level, Part, Specimen, SurfaceMesh};

use crate::detail::DeadwoodLod;
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

/// The ground shapes sticks lie on. They live in `procgena_core`, shared
/// with the other generators that scatter over ground.
pub use procgena_core::{FlatGround, Ground, HeightGrid};

/// Settings for a [`StickScatter`].
#[derive(Debug, Clone, PartialEq)]
pub struct ScatterOptions {
    /// The seed to pass in [`ScatterInput::seed`] when this scatter is
    /// built on its own, such as in a viewer. [`StickScatter::new`] uses the
    /// input's seed, not this one.
    pub seed: u32,
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
            seed: 1,
            max_slope: 35.0,
            density: 1.5,
            min_spacing: 0.3,
            variants: 6,
            decay: [1.0, 3.0],
            diameter: [0.01, 0.07],
            length: [0.4, 1.2],
            piece: DeadwoodPreset::Stick.options(),
        }
    }

    /// Twig litter: many thin twigs under 2.5 cm thick, most drawn as flat
    /// strips at a distance.
    pub fn twig_litter() -> Self {
        Self {
            seed: 2,
            max_slope: 45.0,
            density: 8.0,
            min_spacing: 0.08,
            variants: 8,
            decay: [1.0, 4.0],
            diameter: [0.004, 0.02],
            length: [0.15, 0.5],
            piece: DeadwoodPreset::Twig.options(),
        }
    }

    /// Fallen logs: a few wind-thrown trunks 20 to 50 cm thick and 2 to 6 m
    /// long, kept to gentle slopes.
    pub fn fallen_logs() -> Self {
        Self {
            seed: 3,
            max_slope: 25.0,
            density: 0.04,
            min_spacing: 3.0,
            variants: 4,
            decay: [1.0, 4.0],
            diameter: [0.2, 0.5],
            length: [2.0, 6.0],
            piece: DeadwoodPreset::WindthrowLog.options(),
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Instance {
    /// Index into [`StickScatter::variants`].
    pub variant: u32,
    /// Where the middle of the stick is, in X and Z relative to the area's
    /// centre, with the ground height at that point in Y, in metres.
    pub position: Vec3,
    /// Moves the variant's meshes into place: turned, tilted to the
    /// ground's slope, and resting on it. Apply it to every vertex of the
    /// variant's mesh, at any level of detail.
    pub transform: Affine3A,
}

/// Sticks and twigs scattered over ground.
///
/// Create one with [`StickScatter::new`], then draw each of
/// [`instances`](Self::instances) using the mesh of its variant.
#[derive(Debug, Clone, PartialEq)]
pub struct StickScatter {
    variants: Vec<Piece>,
    instances: Vec<Instance>,
}

/// How one variant is put on the ground.
struct Fit {
    /// The middle of the variant in X and Z, at `y = 0`. Copies are placed
    /// around this point.
    centre: Vec3,
    /// Where the variant touches the ground, measured from `centre`.
    footprint: Footprint,
    /// How the variant follows the ground, from its kind.
    placement: GroundPlacement,
}

impl StickScatter {
    /// Scatters sticks over `input`'s area of ground, thinned by its mask
    /// and seeded by its seed.
    ///
    /// Sticks are left out where the ground is steeper than
    /// [`ScatterOptions::max_slope`], and where any part of them has no
    /// ground under it. Positions in the result are relative to the area's
    /// centre.
    ///
    /// # Errors
    ///
    /// Returns [`ScatterError::Options`] if a setting, the area's size or a
    /// variant is invalid, and [`ScatterError::TooMany`] if the settings ask
    /// for more than [`MAX_INSTANCES`] sticks.
    pub fn new(options: &ScatterOptions, input: &ScatterInput<'_>) -> Result<Self, ScatterError> {
        options.validate()?;
        let size = input.area.size;
        positive("area.size", size.x)?;
        positive("area.size", size.y)?;
        let count = (size.x * size.y * options.density).ceil() as usize;
        if count > MAX_INSTANCES {
            return Err(ScatterError::TooMany { count });
        }

        // Build the variants, each resting on flat ground in its own space.
        // Variant `v` is the specimen in slot `v` of the variant pool, so
        // its seed, and the size and decay drawn from it, depend only on the
        // input's seed, the pool and the slot.
        let pool = options.variant_pool();
        let mut variants = Vec::with_capacity(options.variants as usize);
        for v in 0..options.variants {
            let mut o = options.piece.clone();
            o.seed = SpecimenId::new(input.seed, pool, v).seed();
            let mut rng = stream(o.seed, SCATTER_VARIANT);
            o.decay = rng.range(options.decay[0], options.decay[1]);
            let (lo, hi) = (options.diameter[0].ln(), options.diameter[1].ln());
            o.diameter = rng.range(lo, hi).exp();
            o.length = rng.range(options.length[0], options.length[1]);
            variants.push(Piece::new(o)?);
        }
        let fits: Vec<Fit> = variants.iter().map(fit).collect();

        // One candidate per cell of a jittered grid starting at the area's
        // corner, so changing the area's size moves every candidate. Each
        // passes the mask, then the slope limit, then spacing, then must fit
        // the ground.
        let cell = 1.0 / options.density.sqrt();
        let half = size * 0.5;
        let (columns, rows) = ((size.x / cell).ceil() as u32, (size.y / cell).ceil() as u32);
        let mut spacing = SpacingGrid::new(options.min_spacing);
        let mut instances = Vec::new();
        let cell_seed = seed_for(input.seed, SCATTER_CELL);
        for r in 0..rows {
            for c in 0..columns {
                let mut rng = Rng::new(derive_seed(cell_seed, r * columns + c));
                let site = Vec2::new(
                    -half.x + (c as f32 + rng.next_f32()) * cell,
                    -half.y + (r as f32 + rng.next_f32()) * cell,
                );
                let (keep, variant, yaw) = (
                    rng.next_f32(),
                    (rng.next_f32() * options.variants as f32) as u32,
                    rng.range(0.0, std::f32::consts::TAU),
                );
                if site.x > half.x || site.y > half.y {
                    continue;
                }
                if keep >= input.density(site) {
                    continue;
                }
                let sample = input.sample(site);
                if steeper_than(&sample, options.max_slope) || !spacing.accepts(site) {
                    continue;
                }
                let variant = variant.min(options.variants - 1);
                let Some(transform) = put_down(&fits[variant as usize], site, yaw, input) else {
                    continue;
                };
                spacing.insert(site);
                let position = Vec3::new(site.x, sample.height, site.y);
                instances.push(Instance {
                    variant,
                    position,
                    transform,
                });
            }
        }
        Ok(Self {
            variants,
            instances,
        })
    }

    /// The distinct sticks. Each rests on flat ground at `y = 0`; mesh
    /// it with [`Piece::mesh`] or [`Piece::lod_meshes`] and place copies
    /// with each [`Instance::transform`].
    pub fn variants(&self) -> &[Piece] {
        &self.variants
    }

    /// The placed copies.
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
    pub fn placement(self) -> GroundPlacement {
        match self {
            PieceKind::Log | PieceKind::Branch => GroundPlacement {
                conform: 1.0,
                rest: Rest::Plane,
                ..GroundPlacement::default()
            },
            PieceKind::Stump => GroundPlacement {
                conform: 0.0,
                rest: Rest::Lowest,
                ..GroundPlacement::default()
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
    let extra = if stump {
        Vec::new()
    } else {
        let main = &piece.limbs()[0].curve;
        let (a, b) = (main.at(0.0).position, main.at(1.0).position);
        let axis = Vec3::new(b.x - a.x, 0.0, b.z - a.z).normalize_or(Vec3::X);
        let side = axis.cross(Vec3::Y) * width;
        vec![Vec2::new(side.x, side.z), -Vec2::new(side.x, side.z)]
    };
    Fit {
        centre,
        footprint: Footprint {
            contacts,
            extra,
            size: width,
        },
        placement: o.kind.placement(),
    }
}

/// Returns the transform that puts `fit`'s variant at `site` (relative to
/// the area's centre), turned by `yaw`, on `input`'s ground, or `None`
/// where there is no ground under it.
fn put_down(fit: &Fit, site: Vec2, yaw: f32, input: &ScatterInput<'_>) -> Option<Affine3A> {
    let at = input.area.world(site);
    let placed = place(input.ground, at, yaw, &fit.footprint, &fit.placement)?;
    Some(
        Affine3A::from_rotation_translation(
            placed.rotation,
            Vec3::new(site.x, placed.height, site.y),
        ) * Affine3A::from_translation(-fit.centre),
    )
}

/// Accepted sites, sorted into square buckets one spacing wide, so a
/// spacing check only looks at nearby buckets.
struct SpacingGrid {
    /// Smallest allowed distance between sites; zero or less disables the
    /// check.
    spacing: f32,
    /// Accepted sites, keyed by bucket.
    cells: HashMap<(i32, i32), Vec<Vec2>>,
}

impl SpacingGrid {
    /// Starts an empty grid for sites at least `spacing` apart.
    fn new(spacing: f32) -> Self {
        Self {
            spacing,
            cells: HashMap::new(),
        }
    }

    /// Returns the bucket that holds `p`.
    fn key(&self, p: Vec2) -> (i32, i32) {
        (
            (p.x / self.spacing).floor() as i32,
            (p.y / self.spacing).floor() as i32,
        )
    }

    /// Whether `p` is at least the spacing from every accepted site.
    fn accepts(&self, p: Vec2) -> bool {
        if self.spacing <= 0.0 {
            return true;
        }
        let (cx, cy) = self.key(p);
        for dy in -1..=1 {
            for dx in -1..=1 {
                if let Some(sites) = self.cells.get(&(cx + dx, cy + dy))
                    && sites.iter().any(|s| s.distance(p) < self.spacing)
                {
                    return false;
                }
            }
        }
        true
    }

    /// Records `p` as an accepted site.
    fn insert(&mut self, p: Vec2) {
        if self.spacing > 0.0 {
            let key = self.key(p);
            self.cells.entry(key).or_default().push(p);
        }
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
