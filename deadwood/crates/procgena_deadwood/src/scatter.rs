//! Sticks and twigs scattered over a rectangle of ground, like a
//! `procgena_plant` field. The ground is flat by default, or any shape you
//! describe with [`Ground`]. An optional density function thins the sticks
//! out in places.
//!
//! The result is a few distinct stick meshes ([`StickScatter::variants`])
//! and a list of placed copies ([`StickScatter::instances`]). A renderer can
//! build each mesh once and draw it many times, which is much cheaper than
//! a separate mesh per stick. This is often called instancing.

use std::collections::HashMap;
use std::fmt;

use glam::{Affine3A, Mat3, Quat, Vec2, Vec3};
use procgena_mesh::{Level, Object, Part, SurfaceMesh, TagFilter};

use crate::detail::DeadwoodLod;
use crate::options::{
    DeadwoodOptions, EndKind, OptionsError, PieceKind, int_range, positive, within,
};
use crate::piece::Piece;
use crate::presets::DeadwoodPreset;
use crate::rng::{Rng, sub_seed};
use crate::settle::surface_points;

/// Largest number of sticks one scatter may place.
pub const MAX_INSTANCES: usize = 200_000;
/// Largest number of distinct stick meshes.
pub const MAX_VARIANTS: u32 = 32;
/// Most contact points kept per variant for fitting it to the ground.
const MAX_CONTACTS: usize = 48;

/// The shape of the ground sticks lie on, as a height for each point in X
/// and Z, with Y up.
///
/// Any closure `Fn(f32, f32) -> f32` taking X and Z can be used as ground.
/// [`FlatGround`] and [`HeightGrid`] are ready-made grounds.
pub trait Ground {
    /// Returns the ground's height in metres at `x`, `z`, or NaN where there
    /// is no ground; sticks there are left out.
    fn height(&self, x: f32, z: f32) -> f32;
}

/// Flat ground at `y = 0`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FlatGround;

impl Ground for FlatGround {
    fn height(&self, _x: f32, _z: f32) -> f32 {
        0.0
    }
}

impl<F: Fn(f32, f32) -> f32> Ground for F {
    fn height(&self, x: f32, z: f32) -> f32 {
        self(x, z)
    }
}

/// Ground heights sampled on a regular grid in X and Z, blended smoothly
/// between samples.
///
/// Sampling once and reusing the grid is useful when the real ground is
/// slow to query, such as a mesh. Outside the grid there is no ground, and
/// neither is there where any of the four surrounding samples has none.
#[derive(Debug, Clone, PartialEq)]
pub struct HeightGrid {
    origin: Vec2,
    cell: f32,
    columns: u32,
    rows: u32,
    heights: Vec<f32>,
}

impl HeightGrid {
    /// Samples `height` every `cell` metres over the rectangle from `min`
    /// to `max` (each as `[x, z]`). `height` takes X and Z and may return
    /// NaN where there is no ground.
    ///
    /// # Panics
    ///
    /// Panics if `cell` is not positive or `max` is not above `min`.
    pub fn from_fn(
        min: [f32; 2],
        max: [f32; 2],
        cell: f32,
        height: impl Fn(f32, f32) -> f32,
    ) -> Self {
        assert!(cell > 0.0 && max[0] > min[0] && max[1] > min[1]);
        let columns = ((max[0] - min[0]) / cell).ceil() as u32 + 1;
        let rows = ((max[1] - min[1]) / cell).ceil() as u32 + 1;
        let heights = (0..rows)
            .flat_map(|r| (0..columns).map(move |c| (c, r)))
            .map(|(c, r)| height(min[0] + c as f32 * cell, min[1] + r as f32 * cell))
            .collect();
        Self {
            origin: Vec2::from(min),
            cell,
            columns,
            rows,
            heights,
        }
    }

    /// Samples the top surface of a shared [`Object`], such as terrain from
    /// another procgena generator, every `cell` metres. Plant fields use
    /// ground meshes the same way.
    ///
    /// Only the parts of level `level` that `filter` matches are used.
    /// Where several surfaces overlap, the highest one is the ground. Points
    /// that no surface covers, seen from above, have no ground. Returns
    /// `None` when the filter matches nothing.
    ///
    /// # Panics
    ///
    /// Panics if `cell` is not positive or `level` is out of range.
    pub fn from_object(
        object: &Object,
        level: usize,
        filter: &TagFilter,
        cell: f32,
    ) -> Option<Self> {
        assert!(cell > 0.0);
        let part = object.merged(level, filter)?;
        let mesh = &part.mesh;
        if mesh.positions.is_empty() {
            return None;
        }
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for p in &mesh.positions {
            lo = lo.min(Vec2::new(p[0], p[2]));
            hi = hi.max(Vec2::new(p[0], p[2]));
        }
        let columns = ((hi.x - lo.x) / cell).ceil() as u32 + 1;
        let rows = ((hi.y - lo.y) / cell).ceil() as u32 + 1;
        let mut heights = vec![f32::NAN; (columns * rows) as usize];

        // Rasterise each triangle onto the grid, keeping the highest.
        for t in mesh.indices.chunks(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec3::from(mesh.positions[i as usize]));
            let (tmin, tmax) = (
                Vec2::new(a.x.min(b.x).min(c.x), a.z.min(b.z).min(c.z)),
                Vec2::new(a.x.max(b.x).max(c.x), a.z.max(b.z).max(c.z)),
            );
            let c0 = ((tmin.x - lo.x) / cell).ceil().max(0.0) as u32;
            let c1 = (((tmax.x - lo.x) / cell).floor() as u32).min(columns - 1);
            let r0 = ((tmin.y - lo.y) / cell).ceil().max(0.0) as u32;
            let r1 = (((tmax.y - lo.y) / cell).floor() as u32).min(rows - 1);
            for r in r0..=r1 {
                for col in c0..=c1 {
                    let p = Vec2::new(lo.x + col as f32 * cell, lo.y + r as f32 * cell);
                    if let Some(h) = height_in_triangle(p, a, b, c) {
                        let slot = &mut heights[(r * columns + col) as usize];
                        if slot.is_nan() || h > *slot {
                            *slot = h;
                        }
                    }
                }
            }
        }
        Some(Self {
            origin: lo,
            cell,
            columns,
            rows,
            heights,
        })
    }
}

impl Ground for HeightGrid {
    fn height(&self, x: f32, z: f32) -> f32 {
        let g = (Vec2::new(x, z) - self.origin) / self.cell;
        if g.x < 0.0 || g.y < 0.0 {
            return f32::NAN;
        }
        // Past the last sample there is no ground; exactly on it, blend from
        // the cell before so the far edge still counts.
        let (last_c, last_r) = ((self.columns - 1) as f32, (self.rows - 1) as f32);
        if g.x > last_c || g.y > last_r {
            return f32::NAN;
        }
        let c = (g.x.floor() as u32).min(self.columns.saturating_sub(2));
        let r = (g.y.floor() as u32).min(self.rows.saturating_sub(2));
        if c + 1 >= self.columns || r + 1 >= self.rows {
            return f32::NAN;
        }
        let (fx, fz) = (g.x - c as f32, g.y - r as f32);
        let at = |c: u32, r: u32| self.heights[(r * self.columns + c) as usize];
        let top = at(c, r) + (at(c + 1, r) - at(c, r)) * fx;
        let bottom = at(c, r + 1) + (at(c + 1, r + 1) - at(c, r + 1)) * fx;
        top + (bottom - top) * fz
    }
}

/// Returns the height of triangle `a`, `b`, `c` above point `p` (in X and
/// Z), or `None` if `p` is outside it.
fn height_in_triangle(p: Vec2, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let (a2, b2, c2) = (
        Vec2::new(a.x, a.z),
        Vec2::new(b.x, b.z),
        Vec2::new(c.x, c.z),
    );
    let area = (b2 - a2).perp_dot(c2 - a2);
    if area.abs() < 1e-12 {
        return None;
    }
    let w1 = (p - a2).perp_dot(c2 - a2) / area;
    let w2 = (b2 - a2).perp_dot(p - a2) / area;
    let w0 = 1.0 - w1 - w2;
    let inside = -1e-5;
    (w0 >= inside && w1 >= inside && w2 >= inside).then_some(w0 * a.y + w1 * b.y + w2 * c.y)
}

/// Settings for a [`StickScatter`].
#[derive(Debug, Clone, PartialEq)]
pub struct ScatterOptions {
    /// Number that picks the variants and positions. The same seed and
    /// settings always give the same scatter.
    pub seed: u32,
    /// Width (X) and depth (Z) of the rectangle, in metres, centred on the
    /// origin. Both must be positive.
    pub size: [f32; 2],
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
    /// diameter and length are chosen. Usually a [`PieceKind::Branch`]; a
    /// [`PieceKind::Stump`] is made into a branch. Where a variant turns out
    /// too short for its ends, those ends become flat saw cuts.
    pub piece: DeadwoodOptions,
}

impl ScatterOptions {
    /// Kindling: a mix of sticks 1 to 7 cm thick. Foresters call these the
    /// 10-hour and 100-hour fuel classes, after roughly how long the wood
    /// takes to dry out or soak up moisture.
    pub fn kindling() -> Self {
        Self {
            seed: 1,
            size: [6.0, 6.0],
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
            size: [4.0, 4.0],
            density: 8.0,
            min_spacing: 0.08,
            variants: 8,
            decay: [1.0, 4.0],
            diameter: [0.004, 0.02],
            length: [0.15, 0.5],
            piece: DeadwoodPreset::Twig.options(),
        }
    }

    /// Checks the settings, except [`piece`](Self::piece). The variants made
    /// from `piece` are checked when [`StickScatter::new`] builds them.
    ///
    /// # Errors
    ///
    /// Returns an [`OptionsError`] naming the first invalid setting.
    pub fn validate(&self) -> Result<(), OptionsError> {
        positive("scatter.size", self.size[0])?;
        positive("scatter.size", self.size[1])?;
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
    /// Where the middle of the stick is, in X and Z, with the ground height
    /// at that point in Y, in metres.
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

/// Where a variant touches the ground, and which way it lies.
struct Footprint {
    /// The middle of the variant in X and Z, at `y = 0`.
    centre: Vec3,
    /// Surface points near the ground, measured from `centre`.
    contacts: Vec<Vec3>,
    /// The main stem's direction in X and Z, as a unit vector.
    axis: Vec3,
    /// How far either side of the middle to sample the ground's
    /// cross-slope, in metres.
    width: f32,
}

impl StickScatter {
    /// Scatters sticks over `ground`, thinned by `density`.
    ///
    /// `density` returns from 0 (no sticks) to 1 (the full
    /// [`ScatterOptions::density`]); `None` is full everywhere. Both
    /// `ground` and `density` take X and Z in metres. Sticks are left out
    /// where any part of them has no ground under it.
    ///
    /// # Errors
    ///
    /// Returns [`ScatterError::Options`] if a setting or a variant is
    /// invalid, and [`ScatterError::TooMany`] if the settings ask for more
    /// than [`MAX_INSTANCES`] sticks.
    pub fn new(
        options: &ScatterOptions,
        ground: &dyn Ground,
        density: Option<&dyn Fn(f32, f32) -> f32>,
    ) -> Result<Self, ScatterError> {
        options.validate()?;
        let area = options.size[0] * options.size[1];
        let count = (area * options.density).ceil() as usize;
        if count > MAX_INSTANCES {
            return Err(ScatterError::TooMany { count });
        }

        // Build the variants, each resting on flat ground in its own space.
        let mut rng = Rng::new(options.seed, 1);
        let mut variants = Vec::with_capacity(options.variants as usize);
        for v in 0..options.variants {
            let mut o = options.piece.clone();
            o.seed = sub_seed(options.seed, v);
            o.decay = rng.range(options.decay[0], options.decay[1]);
            let (lo, hi) = (options.diameter[0].ln(), options.diameter[1].ln());
            o.diameter = rng.range(lo, hi).exp();
            o.length = rng.range(options.length[0], options.length[1]);
            if o.kind == PieceKind::Stump {
                o.kind = PieceKind::Branch;
            }
            fit_ends(&mut o);
            variants.push(Piece::new(o)?);
        }
        let footprints: Vec<Footprint> = variants.iter().map(footprint).collect();

        // One candidate per cell of a jittered grid, thinned by density and
        // spacing.
        let cell = 1.0 / options.density.sqrt();
        let half = Vec2::from(options.size) * 0.5;
        let (columns, rows) = (
            (options.size[0] / cell).ceil() as u32,
            (options.size[1] / cell).ceil() as u32,
        );
        let mut spacing = SpacingGrid::new(options.min_spacing);
        let mut instances = Vec::new();
        for r in 0..rows {
            for c in 0..columns {
                let mut rng = Rng::new(options.seed, 1000 + r * columns + c);
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
                let d = density.map_or(1.0, |f| f(site.x, site.y));
                if keep >= d || !spacing.accepts(site) {
                    continue;
                }
                let variant = variant.min(options.variants - 1);
                let Some(transform) = place(&footprints[variant as usize], site, yaw, ground)
                else {
                    continue;
                };
                spacing.insert(site);
                let position = Vec3::new(site.x, ground.height(site.x, site.y), site.y);
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

    /// Bakes every placed copy into one [`Object`], with one level per
    /// level of `lod`. Each level holds one part per set of tags, with every
    /// copy's mesh merged in. Strips are left out, as in [`Piece::object`].
    ///
    /// # Errors
    ///
    /// Returns the first problem with `lod`; see [`DeadwoodLod::validate`].
    ///
    /// # Panics
    ///
    /// Panics if a variant's mesh needs more vertices than fit in a 32-bit
    /// index.
    pub fn object(&self, lod: &DeadwoodLod) -> Result<Object, OptionsError> {
        let per_variant: Vec<Object> = self
            .variants
            .iter()
            .map(|v| v.object(lod))
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
        Ok(Object { levels })
    }
}

/// Replaces ends that don't fit a short stick with flat cuts, so every
/// variant is valid whatever length and diameter it drew.
fn fit_ends(o: &mut DeadwoodOptions) {
    let room = crate::MAX_END_SHARE * o.length * 0.9;
    if o.ends.base.span(o.base_radius()) + o.ends.tip.span(o.tip_radius()) > room {
        o.ends.base = EndKind::Saw { tilt: 0.0 };
        if o.ends.tip.span(o.tip_radius()) > room {
            o.ends.tip = EndKind::Saw { tilt: 0.0 };
        }
    }
}

/// Measures where `piece` touches the ground and which way it lies.
fn footprint(piece: &Piece) -> Footprint {
    let points = surface_points(piece.limbs(), |_| true);
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in &points {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let centre = Vec3::new((lo.x + hi.x) * 0.5, 0.0, (lo.z + hi.z) * 0.5);
    let near = 0.25 * piece.options().diameter + 1e-3;
    let low: Vec<Vec3> = points
        .iter()
        .filter(|p| p.y <= near)
        .map(|&p| p - centre)
        .collect();
    let stride = low.len().div_ceil(MAX_CONTACTS).max(1);
    let contacts: Vec<Vec3> = low.into_iter().step_by(stride).collect();

    // The main stem's direction in X and Z.
    let main = &piece.limbs()[0].curve;
    let (a, b) = (main.at(0.0).position, main.at(1.0).position);
    let axis = Vec3::new(b.x - a.x, 0.0, b.z - a.z).normalize_or(Vec3::X);
    Footprint {
        centre,
        contacts,
        axis,
        width: piece.options().diameter.max(0.1 * piece.options().length),
    }
}

/// Returns the transform that puts footprint `f` at `site`, turned by
/// `yaw`, tilted to the ground and resting on it, or `None` where there is
/// no ground under it.
fn place(f: &Footprint, site: Vec2, yaw: f32, ground: &dyn Ground) -> Option<Affine3A> {
    let turn = Quat::from_rotation_y(yaw);
    let height = |p: Vec3| ground.height(site.x + p.x, site.y + p.z);

    // Fit a plane h = a·x + b·z + c to the ground under the contacts and two
    // points beside the middle, so a straight stick still finds the
    // cross-slope.
    let side = turn * f.axis.cross(Vec3::Y) * f.width;
    let mut samples: Vec<Vec3> = f.contacts.iter().map(|&p| turn * p).collect();
    samples.extend([side, -side]);
    let mut m = Mat3::ZERO;
    let mut rhs = Vec3::ZERO;
    for p in &samples {
        let h = height(*p);
        if !h.is_finite() {
            return None;
        }
        let row = Vec3::new(p.x, p.z, 1.0);
        m += Mat3::from_cols(row * row.x, row * row.y, row * row.z);
        rhs += row * h;
    }
    m += Mat3::from_diagonal(Vec3::new(1e-6, 1e-6, 0.0));
    let [a, b, _] = (m.inverse() * rhs).to_array();
    let normal = Vec3::new(-a, 1.0, -b).normalize();
    let rotation = Quat::from_rotation_arc(Vec3::Y, normal) * turn;

    // Rest on the ground: lift until no contact point is below it.
    let mut lift = f32::NEG_INFINITY;
    for &c in &f.contacts {
        let p = rotation * c;
        let h = height(p);
        if !h.is_finite() {
            return None;
        }
        lift = lift.max(h - p.y);
    }
    if !lift.is_finite() {
        lift = height(Vec3::ZERO);
    }
    Some(
        Affine3A::from_rotation_translation(rotation, Vec3::new(site.x, lift, site.y))
            * Affine3A::from_translation(-f.centre),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_grid_blends_and_has_edges() {
        let g = HeightGrid::from_fn([-1.0, -1.0], [1.0, 1.0], 0.5, |x, _| x);
        assert!((g.height(0.3, 0.2) - 0.3).abs() < 1e-5);
        assert!(g.height(5.0, 0.0).is_nan());
        // The far edge and far corner were sampled, so they have ground.
        assert!((g.height(1.0, 0.0) - 1.0).abs() < 1e-5);
        assert!((g.height(1.0, 1.0) - 1.0).abs() < 1e-5);
        assert!(g.height(1.01, 0.0).is_nan());
    }

    #[test]
    fn triangle_height_is_interpolated() {
        let (a, b, c) = (
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(0.0, 2.0, 1.0),
        );
        let h = height_in_triangle(Vec2::new(0.25, 0.25), a, b, c).unwrap();
        assert!((h - 0.75).abs() < 1e-5);
        assert!(height_in_triangle(Vec2::new(2.0, 2.0), a, b, c).is_none());
    }
}
