//! Where a clump covers the surface and how thick it is there.

use std::f32::consts::TAU;

use glam::Vec3;

use crate::bvh::Bvh;
use crate::options::{ClumpOptions, MAX_DIRECTIONS};
use procgena_core::hash::{pcg3d, unit_float};
use procgena_specimen::Surface;

/// Smallest lump radius, as a fraction of `lump_size`.
pub(crate) const LUMP_RADIUS_MIN: f32 = 0.3;

/// Largest lump radius, as a fraction of `lump_size`.
pub(crate) const LUMP_RADIUS_MAX: f32 = 0.55;

/// Smallest lump height, as a fraction of a full round lump of its radius.
pub(crate) const LUMP_HEIGHT_MIN: f32 = 0.6;

/// A point on the clump's working surface with its smooth normal and the
/// measurements the clump's height depends on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SurfacePoint {
    pub(crate) point: Vec3,
    pub(crate) normal: Vec3,
    /// Exposure per direction, from 0 (closed in) to 1 (fully exposed), in
    /// the order of the field's directions. Entries past the last direction
    /// are unused.
    pub(crate) exposure: [f32; MAX_DIRECTIONS],
    /// How thick the input is here, measured straight in along the normal:
    /// infinity where nothing limits it, and 0 where the point lies inside
    /// another part of the input. Blended between vertices.
    pub(crate) room: f32,
    /// How far the input rises above this point's simplified surface nearby,
    /// along the normal. 0 where the input wasn't simplified.
    pub(crate) rise: f32,
    /// How far the input dips below this point's simplified surface nearby,
    /// along the normal. 0 where the input wasn't simplified.
    pub(crate) dip: f32,
}

impl SurfacePoint {
    /// A point at `point` with `normal`, fully exposed, with unlimited room
    /// and no rise or dip. Measure it with [`ClumpField::measure`].
    pub(crate) fn new(point: Vec3, normal: Vec3) -> Self {
        Self {
            point,
            normal,
            exposure: [1.0; MAX_DIRECTIONS],
            room: f32::INFINITY,
            rise: 0.0,
            dip: 0.0,
        }
    }
}

/// One direction material arrives from, prepared for evaluation.
#[derive(Debug, Clone, Copy)]
struct Source {
    /// Unit vector pointing back toward where the material comes from.
    toward: Vec3,
    strength: f32,
}

/// A clump's coverage over the input surface, measured in metres after
/// `scale`. Rays are always cast against the full input.
pub(crate) struct ClumpField<'a> {
    surface: Surface<'a>,
    bvh: Bvh,
    sources: Vec<Source>,
    /// Exposure ray directions per source; empty when exposure isn't measured.
    ray_dirs: Vec<Vec<Vec3>>,
    /// Thinnest input a clump may grow on, after `scale`.
    min_support: f32,
    seed: u32,
    any_facing: bool,
    facing_min: f32,
    facing_full: f32,
    exposure_power: f32,
    thickness: f32,
    sink: f32,
    lumpiness: f32,
    lump_size: f32,
    /// Small distance used to step off the surface before casting rays.
    epsilon: f32,
    /// Whether the input is closed: every edge is shared by exactly two
    /// triangles, so it encloses a solid.
    closed_input: bool,
    /// Position id per input vertex; see [`welded`](Self::welded).
    weld: Vec<u32>,
    weld_count: usize,
}

impl<'a> ClumpField<'a> {
    /// Prepares the field. `options` must already be validated.
    pub(crate) fn new(surface: Surface<'a>, options: &ClumpOptions) -> Self {
        let bvh = Bvh::new(&surface);
        let (lo, hi) = surface.bounds();
        // Small enough to stay inside fine features such as tapered branch
        // tips, large enough to step off a triangle in f32.
        let epsilon = (hi - lo).length().max(1e-6) * 1e-6;
        let sources: Vec<Source> = options
            .directions
            .iter()
            .map(|d| Source {
                toward: -Vec3::from(d.vector).normalize(),
                strength: d.strength,
            })
            .collect();

        // With no rays, or an exposure power of 0, exposure has no effect.
        let rays = options.mask.exposure_rays;
        let ray_dirs = if rays > 0 && options.mask.exposure_power != 0.0 {
            exposure_dirs(&sources, rays)
        } else {
            Vec::new()
        };

        let (weld, weld_count) = surface.welded();
        let closed_input = surface.is_closed_welded(&weld);
        let scale = options.scale;
        Self {
            surface,
            bvh,
            sources,
            ray_dirs,
            min_support: options.shape.min_support * scale,
            seed: options.seed,
            any_facing: options.mask.any_facing,
            facing_min: options.mask.facing_min,
            facing_full: options.mask.facing_full,
            exposure_power: options.mask.exposure_power,
            thickness: options.shape.thickness * scale,
            sink: options.shape.sink * scale,
            lumpiness: options.shape.lumpiness,
            lump_size: options.shape.lump_size * scale,
            epsilon,
            closed_input,
            weld,
            weld_count,
        }
    }

    /// Returns the position id of every input vertex, and how many ids
    /// there are. Vertices at the same position (as at texture seams) share
    /// an id and should be treated as one vertex.
    pub(crate) fn welded(&self) -> (&[u32], usize) {
        (&self.weld, self.weld_count)
    }

    /// Returns the input surface.
    pub(crate) fn surface(&self) -> &Surface<'a> {
        &self.surface
    }

    /// Returns the bounding volume hierarchy over the input surface.
    pub(crate) fn bvh(&self) -> &Bvh {
        &self.bvh
    }

    /// Returns the seed.
    pub(crate) fn seed(&self) -> u32 {
        self.seed
    }

    /// Returns the full thickness, after `scale`.
    pub(crate) fn thickness(&self) -> f32 {
        self.thickness
    }

    /// Returns the sink depth, after `scale`.
    pub(crate) fn sink(&self) -> f32 {
        self.sink
    }

    /// Returns `p` with its exposure and room measured against the input.
    /// Its rise and dip are kept.
    pub(crate) fn measure(&self, p: SurfacePoint) -> SurfacePoint {
        SurfacePoint {
            exposure: self.exposure(p.point, p.normal),
            room: self.room(p.point, p.normal),
            ..p
        }
    }

    /// Measures how thick the input is at surface point `p` with unit normal
    /// `n`: the distance straight in along the normal to the input's far
    /// side. Returns 0 where the input can't carry a clump: a point inside
    /// another part (a ray straight out first meets a surface from behind), a
    /// point where parts overlap (a ray straight in first meets another
    /// part's outside), or, on a closed input, a point where going straight
    /// in meets nothing. Returns infinity on an open input where nothing is
    /// met.
    pub(crate) fn room(&self, p: Vec3, n: Vec3) -> f32 {
        if n == Vec3::ZERO {
            return f32::INFINITY;
        }
        if self.exits_through(p, n) {
            return 0.0;
        }
        // Going straight in should reach this part's own far side. If it
        // first meets the outside of another part, this point is where parts
        // overlap, such as a branch joining its trunk, and clumps there fold
        // and cross each other. If it meets nothing on a closed input, the
        // shape is unclear. Neither carries a clump.
        match self.inside_span(p, -n) {
            Some((0.0, end)) => end,
            _ => 0.0,
        }
    }

    /// Returns whether a ray from surface point `point` along unit direction
    /// `dir` first meets a surface from behind, meaning `point` lies inside
    /// some part of the input.
    fn exits_through(&self, point: Vec3, dir: Vec3) -> bool {
        self.bvh
            .ray_hit(point + dir * self.epsilon, dir, f32::INFINITY)
            .is_some_and(|(_, tri)| self.leaves(tri, dir))
    }

    /// Returns whether a ray along `dir` crosses input triangle `tri` from
    /// behind, leaving the solid.
    fn leaves(&self, tri: usize, dir: Vec3) -> bool {
        let [a, b, c] = self.surface.triangle_positions(tri);
        (b - a).cross(c - a).dot(dir) > 0.0
    }

    /// Measures how exposed the point `p` with normal `n` is toward each
    /// direction's source, from 0 to 1: the share of that direction's rays
    /// that escape. Every entry is 1 when exposure isn't measured.
    fn exposure(&self, p: Vec3, n: Vec3) -> [f32; MAX_DIRECTIONS] {
        // Step off the surface along its normal, then count escaping rays.
        let origin = p + n * self.epsilon;
        let mut open = [1.0; MAX_DIRECTIONS];
        for (d, rays) in self.ray_dirs.iter().enumerate() {
            let escaped = rays
                .iter()
                .filter(|&&r| !self.bvh.ray_blocked(origin, r, f32::INFINITY))
                .count();
            open[d] = escaped as f32 / rays.len() as f32;
        }
        open
    }

    /// Returns the surface point halfway along the edge from `a` to `b`, with
    /// the normal and measurements blended. The point lies on the straight
    /// edge, not on a smoothed surface.
    pub(crate) fn midpoint(&self, a: &SurfacePoint, b: &SurfacePoint) -> SurfacePoint {
        SurfacePoint {
            point: (a.point + b.point) * 0.5,
            normal: (a.normal + b.normal).normalize_or(a.normal),
            exposure: std::array::from_fn(|d| (a.exposure[d] + b.exposure[d]) * 0.5),
            room: (a.room + b.room) * 0.5,
            rise: (a.rise + b.rise) * 0.5,
            dip: (a.dip + b.dip) * 0.5,
        }
    }

    /// Returns the facing part of the mask for unit normal `normal`: the sum
    /// over directions of strength × facing, as if fully exposed. The full
    /// mask is 0 wherever this is 0.
    pub(crate) fn facing(&self, normal: Vec3) -> f32 {
        self.sources
            .iter()
            .map(|s| s.strength * self.facing_term(normal, s))
            .sum()
    }

    /// Returns how directly `normal` faces into source `s`, from 0 to 1.
    fn facing_term(&self, normal: Vec3, s: &Source) -> f32 {
        if self.any_facing {
            1.0
        } else {
            smoothstep(self.facing_min, self.facing_full, normal.dot(s.toward))
        }
    }

    /// Returns where clumps may grow at `p`: the sum over directions of
    /// strength × facing × exposure. 0 means nothing grows; values above 1
    /// are allowed and are capped later, in [`weight`](Self::weight).
    pub(crate) fn mask(&self, p: &SurfacePoint) -> f32 {
        // Nothing grows where the input is thinner than the minimum, or on
        // parts that lie inside other parts of the input.
        if p.room <= 0.0 || p.room < self.min_support {
            return 0.0;
        }
        self.sources
            .iter()
            .enumerate()
            .map(|(d, s)| {
                let exposure = p.exposure[d].clamp(0.0, 1.0).powf(self.exposure_power);
                s.strength * self.facing_term(p.normal, s) * exposure
            })
            .sum()
    }

    /// Returns the coverage weight at `p`, from 0 (bare) to 1 (full thickness).
    pub(crate) fn weight(&self, p: &SurfacePoint) -> f32 {
        self.mask(p).clamp(0.0, 1.0)
    }

    /// Returns the clump's top height above the surface for weight `w`, not
    /// counting lumps. It is `-sink` at weight 0 and `thickness` at weight 1.
    pub(crate) fn top(&self, w: f32) -> f32 {
        (self.thickness + self.sink) * w - self.sink
    }

    /// Returns the height of the rounded lumps at `point`, 0 or more and at
    /// most [`max_lump_height`](Self::max_lump_height).
    ///
    /// Lumps are spheres on a jittered grid of `lump_size` cells, one per
    /// cell with its own radius and height; where they cut the surface they
    /// make round bumps.
    pub(crate) fn lump_height(&self, point: Vec3) -> f32 {
        if self.lumpiness <= 0.0 {
            return 0.0;
        }
        let q = point / self.lump_size;
        let cell = q.floor();
        let mut best: f32 = 0.0;
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let c = cell + Vec3::new(dx as f32, dy as f32, dz as f32);
                    // The casts keep the bit pattern of negative cell indices.
                    let h = pcg3d([
                        c.x as i32 as u32,
                        c.y as i32 as u32,
                        (c.z as i32 as u32) ^ self.seed.rotate_left(8),
                    ]);
                    let centre =
                        c + Vec3::new(unit_float(h[0]), unit_float(h[1]), unit_float(h[2]));
                    let k = pcg3d(h);
                    let r =
                        LUMP_RADIUS_MIN + (LUMP_RADIUS_MAX - LUMP_RADIUS_MIN) * unit_float(k[0]);
                    let amp = LUMP_HEIGHT_MIN + (1.0 - LUMP_HEIGHT_MIN) * unit_float(k[1]);
                    let d2 = (q - centre).length_squared();
                    if d2 < r * r {
                        best = best.max(amp * (r * r - d2).sqrt());
                    }
                }
            }
        }
        self.lumpiness * best * self.lump_size
    }

    /// Returns the tallest a lump can be.
    pub(crate) fn max_lump_height(&self) -> f32 {
        self.lumpiness * LUMP_RADIUS_MAX * self.lump_size
    }

    /// Returns the visible height fraction for a top height: 0 where the top
    /// is at or below the surface (the visible rim and beyond), 1 at full thickness.
    pub(crate) fn fade(&self, top: f32) -> f32 {
        (top / self.thickness).clamp(0.0, 1.0)
    }

    /// Returns the first stretch of a ray from `point` along unit direction
    /// `dir` that lies inside the input, as distances `(enter, leave)`
    /// along the ray. `enter` is 0 when `point` is already inside or on the
    /// surface heading in. Returns `None` if the ray never gets inside: it
    /// meets nothing on a closed input, or after entering it never leaves.
    /// On an open input (such as a ground patch), a ray that is inside and
    /// meets nothing leaves at infinity.
    pub(crate) fn inside_span(&self, point: Vec3, dir: Vec3) -> Option<(f32, f32)> {
        let open_end = (!self.closed_input).then_some(f32::INFINITY);
        let Some((t, tri)) = self
            .bvh
            .ray_hit(point + dir * self.epsilon, dir, f32::INFINITY)
        else {
            return open_end.map(|end| (0.0, end));
        };
        let t = t + self.epsilon;
        if self.leaves(tri, dir) {
            return Some((0.0, t));
        }

        // The point is outside: go in through the surface just met.
        let entry = point + dir * t;
        match self
            .bvh
            .ray_hit(entry + dir * self.epsilon, dir, f32::INFINITY)
        {
            Some((u, tri)) if self.leaves(tri, dir) => Some((t, t + u + self.epsilon)),
            Some(_) => None,
            None => open_end.map(|end| (t, end)),
        }
    }

    /// Returns the distance from `point` along unit direction `dir` to the
    /// first surface in the way, or infinity if nothing is within about
    /// `max` (pass infinity for no limit). The search starts just off the
    /// surface so `point`'s own triangle doesn't count.
    pub(crate) fn clearance(&self, point: Vec3, dir: Vec3, max: f32) -> f32 {
        self.bvh
            .ray_hit(point + dir * self.epsilon, dir, max)
            .map_or(f32::INFINITY, |(t, _)| t + self.epsilon)
    }

    /// Returns whether the triangle with these corners could be covered.
    /// Treats the mask's largest value on a triangle as being at a corner,
    /// which holds closely because its terms are blended from the corners.
    pub(crate) fn may_cover(&self, corners: [&SurfacePoint; 3]) -> bool {
        corners.iter().any(|p| self.mask(p) > 0.0)
    }

    /// Returns the field's settings packed as the `Field` struct in
    /// `shaders/field.wesl`, as 48 little-endian words.
    #[cfg(test)]
    pub(crate) fn gpu_words(&self) -> [u32; 48] {
        let mut words = [0u32; 48];

        // One vec4 per direction: the unit vector toward its source, then strength.
        for (d, s) in self.sources.iter().enumerate() {
            let v = s.toward.extend(s.strength).to_array();
            for (k, x) in v.iter().enumerate() {
                words[4 * d + k] = x.to_bits();
            }
        }

        // The scalars after the direction array.
        let scalars = [
            self.sources.len() as u32,
            self.any_facing as u32,
            self.facing_min.to_bits(),
            self.facing_full.to_bits(),
            self.exposure_power.to_bits(),
            self.min_support.to_bits(),
            self.thickness.to_bits(),
            self.sink.to_bits(),
            self.lumpiness.to_bits(),
            self.lump_size.to_bits(),
            self.seed,
            self.epsilon.to_bits(),
            self.closed_input as u32,
        ];
        words[32..32 + scalars.len()].copy_from_slice(&scalars);
        words
    }
}

/// Returns, for each source, `rays` ray directions spread over the
/// hemisphere around it and weighted toward the centre, for measuring
/// exposure. `rays` must be above 0.
fn exposure_dirs(sources: &[Source], rays: u32) -> Vec<Vec<Vec3>> {
    // Spread cosine-weighted rays with a golden-angle spiral.
    let golden = TAU * (1.0 - 1.0 / std::f32::consts::GOLDEN_RATIO);
    sources
        .iter()
        .map(|source| {
            let (tangent, bitangent) = source.toward.any_orthonormal_pair();
            (0..rays)
                .map(|i| {
                    let u = (i as f32 + 0.5) / rays as f32;
                    let r = u.sqrt();
                    let a = i as f32 * golden;
                    (tangent * (r * a.cos())
                        + bitangent * (r * a.sin())
                        + source.toward * (1.0 - u).sqrt())
                    .normalize()
                })
                .collect()
        })
        .collect()
}

/// Returns the exposure ray directions [`ClumpField::new`] would use for
/// `options`: `exposure_rays` per direction, direction after direction, or
/// none when exposure isn't measured. `options` must be validated.
#[cfg(feature = "gpu")]
pub(crate) fn exposure_ray_dirs(options: &ClumpOptions) -> Vec<Vec3> {
    let rays = options.mask.exposure_rays;
    if rays == 0 || options.mask.exposure_power == 0.0 {
        return Vec::new();
    }
    let sources: Vec<Source> = options
        .directions
        .iter()
        .map(|d| Source {
            toward: -Vec3::from(d.vector).normalize(),
            strength: d.strength,
        })
        .collect();
    exposure_dirs(&sources, rays)
        .into_iter()
        .flatten()
        .collect()
}

/// Returns the coverage settings of `options` packed as the `Field` struct in
/// `shaders/field.wesl`, as 48 little-endian words, with every length
/// multiplied by `scale` as [`ClumpField::new`] does. `options` must be
/// validated. The input-dependent `epsilon` and `closed_input` are left 0,
/// for the GPU to fill in.
#[cfg(any(test, feature = "gpu"))]
pub(crate) fn field_words(options: &ClumpOptions) -> [u32; 48] {
    let mut words = [0u32; 48];

    // One vec4 per direction: the unit vector toward its source, then strength.
    for (d, direction) in options.directions.iter().enumerate() {
        let toward = -Vec3::from(direction.vector).normalize();
        let v = toward.extend(direction.strength).to_array();
        for (k, x) in v.iter().enumerate() {
            words[4 * d + k] = x.to_bits();
        }
    }

    // The scalars after the direction array.
    let (mask, shape, scale) = (&options.mask, &options.shape, options.scale);
    let scalars = [
        options.directions.len() as u32,
        mask.any_facing as u32,
        mask.facing_min.to_bits(),
        mask.facing_full.to_bits(),
        mask.exposure_power.to_bits(),
        (shape.min_support * scale).to_bits(),
        (shape.thickness * scale).to_bits(),
        (shape.sink * scale).to_bits(),
        shape.lumpiness.to_bits(),
        (shape.lump_size * scale).to_bits(),
        options.seed,
    ];
    words[32..32 + scalars.len()].copy_from_slice(&scalars);
    words
}

/// Smooth Hermite step: 0 at or below `lo`, 1 at or above `hi`.
/// If `hi` is not above `lo`, it is a hard step at `lo`.
pub(crate) fn smoothstep(lo: f32, hi: f32, x: f32) -> f32 {
    if hi <= lo {
        return if x < lo { 0.0 } else { 1.0 };
    }
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
