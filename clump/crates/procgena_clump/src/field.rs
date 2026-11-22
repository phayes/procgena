//! Where a clump covers the surface and how thick it is there.

use std::f32::consts::TAU;

use glam::Vec3;

use crate::bvh::Bvh;
use crate::hash::fbm;
use crate::options::ClumpOptions;
use crate::surface::Surface;

/// Noise layers used for lumps.
const LUMP_OCTAVES: u32 = 3;

use crate::options::MAX_DIRECTIONS;

/// A point on the surface with its smooth normal and, for each direction,
/// how exposed it is toward that direction's source.
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
}

/// One direction material arrives from, prepared for evaluation.
#[derive(Debug, Clone, Copy)]
struct Source {
    /// Unit vector pointing back toward where the material comes from.
    toward: Vec3,
    strength: f32,
}

/// A clump's coverage over a surface, measured in metres after `scale`.
pub(crate) struct ClumpField<'a> {
    surface: Surface<'a>,
    bvh: Bvh,
    sources: Vec<Source>,
    /// Exposure per position id and direction; see [`SurfacePoint::exposure`].
    /// Only positions that can affect the clump are measured; the rest stay
    /// at 1.
    exposure: Vec<[f32; MAX_DIRECTIONS]>,
    /// Input thickness per position id; see [`SurfacePoint::room`].
    room: Vec<f32>,
    /// Position id per surface vertex: vertices at the same position (as at
    /// texture seams) share one, and are treated as a single vertex.
    weld: Vec<u32>,
    /// Normal per position id: the normalized sum of its vertices' normals.
    welded_normals: Vec<Vec3>,
    /// Thinnest input a clump may grow on, after `scale`.
    min_support: f32,
    seed: u32,
    facing_min: f32,
    facing_full: f32,
    exposure_power: f32,
    centre: Option<Vec3>,
    radius: Option<f32>,
    edge_width: f32,
    thickness: f32,
    sink: f32,
    lumpiness: f32,
    lump_size: f32,
    /// Small distance used to step off the surface before casting rays.
    epsilon: f32,
    /// Whether the input is closed: every edge is shared by exactly two
    /// triangles, so it encloses a solid.
    closed_input: bool,
}

impl<'a> ClumpField<'a> {
    /// Prepares the field. `options` must already be validated.
    pub(crate) fn new(surface: Surface<'a>, options: &ClumpOptions) -> Self {
        let bvh = Bvh::new(&surface);
        let (lo, hi) = surface.bounds();
        // Small enough to stay inside fine features such as tapered branch
        // tips, large enough to step off a triangle in f32.
        let epsilon = (hi - lo).length().max(1e-6) * 1e-6;
        let sources = options
            .directions
            .iter()
            .map(|d| Source {
                toward: -Vec3::from(d.vector).normalize(),
                strength: d.strength,
            })
            .collect();
        let scale = options.scale;

        // Treat vertices at the same position as one, with a shared normal.
        let (weld, count) = surface.welded();
        let mut sums = vec![Vec3::ZERO; count];
        for (v, &id) in weld.iter().enumerate() {
            sums[id as usize] += surface.normal(v);
        }
        let welded_normals = sums.into_iter().map(|n| n.normalize_or_zero()).collect();
        let closed_input = surface.is_closed_welded(&weld);

        let mut field = Self {
            surface,
            bvh,
            sources,
            exposure: Vec::new(),
            room: Vec::new(),
            weld,
            welded_normals,
            min_support: options.shape.min_support * scale,
            seed: options.seed,
            facing_min: options.mask.facing_min,
            facing_full: options.mask.facing_full,
            exposure_power: options.mask.exposure_power,
            centre: None,
            radius: options.shape.radius.map(|r| r * scale),
            edge_width: options.shape.edge_width,
            thickness: options.shape.thickness * scale,
            sink: options.shape.sink * scale,
            lumpiness: options.shape.lumpiness,
            lump_size: options.shape.lump_size * scale,
            epsilon,
            closed_input,
        };

        // One vertex stands in for each position id when casting rays.
        let mut first = vec![u32::MAX; count];
        for (v, &id) in field.weld.iter().enumerate().rev() {
            first[id as usize] = v as u32;
        }
        field.room = first
            .iter()
            .map(|&v| field.vertex_room(v as usize))
            .collect();

        // Exposure only matters where the mask can be above 0. With no rays,
        // or an exposure power of 0, it has no effect at all.
        field.exposure = vec![[1.0; MAX_DIRECTIONS]; count];
        let rays = options.mask.exposure_rays;
        let measure = rays > 0 && field.exposure_power != 0.0;
        let ray_dirs = if measure {
            field.exposure_dirs(rays)
        } else {
            Vec::new()
        };
        let maskable: Vec<bool> = (0..count)
            .map(|id| measure && field.may_mask(first[id] as usize))
            .collect();
        for id in (0..count).filter(|&id| maskable[id]) {
            field.exposure[id] = field.vertex_exposure(first[id] as usize, &ray_dirs);
        }

        // The centre reads only maskable positions, which are measured now.
        field.centre = options
            .shape
            .at
            .map(Vec3::from)
            .or_else(|| field.best_vertex());

        // Midpoints blend exposure from every corner of a triangle the clump
        // may cover, so measure those corners too.
        if measure && field.centre.is_some() {
            let mut needed = vec![false; count];
            for t in 0..field.surface.triangle_count() {
                let corners = field.surface.triangle(t);
                let points = corners.map(|v| field.vertex_point(v));
                if field.may_cover([&points[0], &points[1], &points[2]]) {
                    for v in corners {
                        needed[field.weld[v] as usize] = true;
                    }
                }
            }
            for id in (0..count).filter(|&id| needed[id] && !maskable[id]) {
                field.exposure[id] = field.vertex_exposure(first[id] as usize, &ray_dirs);
            }
        }
        field
    }

    /// Returns the input surface.
    pub(crate) fn surface(&self) -> &Surface<'a> {
        &self.surface
    }

    /// Returns the full thickness, after `scale`.
    pub(crate) fn thickness(&self) -> f32 {
        self.thickness
    }

    /// Returns the sink depth, after `scale`.
    pub(crate) fn sink(&self) -> f32 {
        self.sink
    }

    /// Returns false if the clump can't cover any part of the surface.
    pub(crate) fn is_empty(&self) -> bool {
        self.centre.is_none()
    }

    /// Measures how thick the input is at each vertex: the distance straight
    /// in along the normal to the input's far side. Returns 0 where the input
    /// can't carry a clump: a vertex inside another part (a ray straight out
    /// first meets a surface from behind), a vertex where parts overlap (a
    /// ray straight in first meets another part's outside), or, on a closed
    /// input, a vertex where going straight in meets nothing. Returns
    /// infinity on an open input where nothing is met.
    fn vertex_room(&self, v: usize) -> f32 {
        let (p, n) = (self.surface.position(v), self.normal(v));
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
        self.inside_run(p, -n).unwrap_or(0.0)
    }

    /// Returns whether a ray from surface point `point` along unit direction
    /// `dir` first meets a surface from behind, meaning `point` lies inside
    /// some part of the input.
    fn exits_through(&self, point: Vec3, dir: Vec3) -> bool {
        self.bvh
            .ray_hit(point + dir * self.epsilon, dir, f32::INFINITY)
            .is_some_and(|(_, tri)| {
                let [a, b, c] = self.surface.triangle_positions(tri);
                (b - a).cross(c - a).dot(dir) > 0.0
            })
    }

    /// Returns, for each direction, `rays` ray directions spread over the
    /// hemisphere around its source and weighted toward the centre, for
    /// [`vertex_exposure`](Self::vertex_exposure). `rays` must be above 0.
    fn exposure_dirs(&self, rays: u32) -> Vec<Vec<Vec3>> {
        // Spread cosine-weighted rays with a golden-angle spiral.
        let golden = TAU * (1.0 - 1.0 / std::f32::consts::GOLDEN_RATIO);
        self.sources
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

    /// Measures how exposed vertex `v` is toward each direction's source, from
    /// 0 to 1: the share of that direction's rays in `dirs` (from
    /// [`exposure_dirs`](Self::exposure_dirs)) that escape.
    fn vertex_exposure(&self, v: usize, dirs: &[Vec<Vec3>]) -> [f32; MAX_DIRECTIONS] {
        // Step off the surface along its normal, then count escaping rays.
        let origin = self.surface.position(v) + self.normal(v) * self.epsilon;
        let mut open = [1.0; MAX_DIRECTIONS];
        for (d, rays) in dirs.iter().enumerate() {
            let escaped = rays
                .iter()
                .filter(|&&r| !self.bvh.ray_blocked(origin, r, f32::INFINITY))
                .count();
            open[d] = escaped as f32 / rays.len() as f32;
        }
        open
    }

    /// Returns whether the mask at vertex `v` could be above 0 for some
    /// exposure. Its room must already be measured.
    fn may_mask(&self, v: usize) -> bool {
        // Full exposure gives the largest mask, and the mask is 0 for every
        // exposure exactly when it is 0 at full exposure.
        let point = SurfacePoint {
            exposure: [1.0; MAX_DIRECTIONS],
            ..self.vertex_point(v)
        };
        self.mask(&point) > 0.0
    }

    /// Returns the vertex with the highest mask, or `None` if every mask is 0.
    fn best_vertex(&self) -> Option<Vec3> {
        let mut best = None;
        let mut best_mask = 0.0;
        for v in 0..self.surface.positions().len() {
            let point = self.vertex_point(v);
            let m = self.mask(&point);
            if m > best_mask {
                best_mask = m;
                best = Some(point.point);
            }
        }
        best
    }

    /// Returns the surface point halfway along the edge from `a` to `b`, with
    /// the normal, exposure and input thickness blended. The point lies on the
    /// flat triangle, not on a smoothed surface.
    pub(crate) fn midpoint(&self, a: &SurfacePoint, b: &SurfacePoint) -> SurfacePoint {
        SurfacePoint {
            point: (a.point + b.point) * 0.5,
            normal: (a.normal + b.normal).normalize_or(a.normal),
            exposure: std::array::from_fn(|d| (a.exposure[d] + b.exposure[d]) * 0.5),
            room: (a.room + b.room) * 0.5,
        }
    }

    /// Returns the normal of vertex `v`, shared by every vertex at its position.
    fn normal(&self, v: usize) -> Vec3 {
        self.welded_normals[self.weld[v] as usize]
    }

    /// Returns the id of vertex `v`'s position. Vertices at the same position
    /// share an id and should be treated as one vertex.
    pub(crate) fn position_id(&self, v: usize) -> u32 {
        self.weld[v]
    }

    /// Returns the surface point of vertex `v`.
    pub(crate) fn vertex_point(&self, v: usize) -> SurfacePoint {
        let id = self.weld[v] as usize;
        SurfacePoint {
            point: self.surface.position(v),
            normal: self.welded_normals[id],
            exposure: self.exposure[id],
            room: self.room[id],
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
                let facing = smoothstep(self.facing_min, self.facing_full, p.normal.dot(s.toward));
                s.strength * facing * p.exposure[d].clamp(0.0, 1.0).powf(self.exposure_power)
            })
            .sum()
    }

    /// Returns the coverage weight at `p`, from 0 (bare) to 1 (full thickness).
    pub(crate) fn weight(&self, p: &SurfacePoint) -> f32 {
        let Some(centre) = self.centre else {
            return 0.0;
        };

        // Thin out toward the edge of the radius.
        let falloff = match self.radius {
            None => 1.0,
            Some(r) => {
                let inner = r * (1.0 - self.edge_width);
                1.0 - smoothstep(inner, r, p.point.distance(centre))
            }
        };

        // Vary the thickness with noise.
        let lumps = fbm(p.point / self.lump_size, self.seed, LUMP_OCTAVES);
        let lump = 1.0 + self.lumpiness * (2.0 * lumps - 1.0);
        (self.mask(p) * falloff * lump).clamp(0.0, 1.0)
    }

    /// Returns the clump's top height above the surface for weight `w`.
    /// It is `-sink` at weight 0 and `thickness` at weight 1.
    pub(crate) fn top(&self, w: f32) -> f32 {
        (self.thickness + self.sink) * w - self.sink
    }

    /// Returns the visible height fraction for a top height: 0 where the top
    /// is at or below the surface (the visible rim and beyond), 1 at full thickness.
    pub(crate) fn fade(&self, top: f32) -> f32 {
        (top / self.thickness).clamp(0.0, 1.0)
    }

    /// Returns how far `point`, on the surface, can travel along unit
    /// direction `dir` behind the surface before coming out again. Returns
    /// `None` if `dir` doesn't lead behind the surface: the first surface it
    /// meets is crossed from the front, or, on a closed input, it meets
    /// nothing. On an open input (such as a ground patch), meeting nothing
    /// returns infinity.
    pub(crate) fn inside_run(&self, point: Vec3, dir: Vec3) -> Option<f32> {
        let Some((t, tri)) = self
            .bvh
            .ray_hit(point + dir * self.epsilon, dir, f32::INFINITY)
        else {
            return (!self.closed_input).then_some(f32::INFINITY);
        };
        let [a, b, c] = self.surface.triangle_positions(tri);
        let leaving = (b - a).cross(c - a).dot(dir) > 0.0;
        leaving.then_some(t + self.epsilon)
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
    /// Errs on the side of yes.
    ///
    /// Treats the mask's largest value on a triangle as being at a corner,
    /// which holds closely because its terms are blended from the corners.
    /// Lumps never reach zero unless `lumpiness` is 1.
    pub(crate) fn may_cover(&self, corners: [&SurfacePoint; 3]) -> bool {
        let Some(centre) = self.centre else {
            return false;
        };
        if corners.iter().all(|p| self.mask(p) <= 0.0) {
            return false;
        }
        match self.radius {
            None => true,
            Some(r) => {
                let [a, b, c] = corners.map(|p| p.point);
                let (lo, hi) = (a.min(b).min(c), a.max(b).max(c));
                (centre.clamp(lo, hi) - centre).length() <= r
            }
        }
    }
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
