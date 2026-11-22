//! A bounding volume hierarchy over a surface's triangles, for ray casts and
//! closest-point queries.

use glam::Vec3;

use procgena_mesh::Surface;

/// Most triangles stored in one leaf before it is split.
const LEAF_SIZE: usize = 4;

/// The closest point on a surface to a query point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ClosestPoint {
    /// The point on the surface.
    pub(crate) point: Vec3,
    /// The triangle that holds the point.
    pub(crate) triangle: usize,
    /// Barycentric weights of the triangle's three corners at the point.
    pub(crate) barycentric: [f32; 3],
    /// Squared distance from the query point.
    pub(crate) distance_squared: f32,
}

/// An axis-aligned box.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Aabb {
    min: Vec3,
    max: Vec3,
}

impl Aabb {
    const EMPTY: Self = Self {
        min: Vec3::splat(f32::INFINITY),
        max: Vec3::splat(f32::NEG_INFINITY),
    };

    fn grow(self, p: Vec3) -> Self {
        Self {
            min: self.min.min(p),
            max: self.max.max(p),
        }
    }

    /// Squared distance from `p` to the box; zero inside it.
    fn distance_squared(&self, p: Vec3) -> f32 {
        let d = (self.min - p).max(p - self.max).max(Vec3::ZERO);
        d.length_squared()
    }

    /// Entry distance of the ray along `dir`, if it meets the box before `t_max`.
    /// `inv_dir` is the component-wise reciprocal of the direction.
    fn ray_entry(&self, origin: Vec3, inv_dir: Vec3, t_max: f32) -> Option<f32> {
        let t0 = (self.min - origin) * inv_dir;
        let t1 = (self.max - origin) * inv_dir;
        let near = t0.min(t1).max_element().max(0.0);
        let far = t0.max(t1).min_element().min(t_max);
        (near <= far).then_some(near)
    }
}

/// Most entries a traversal stack holds. Each split halves its triangles, so
/// a tree over any `u32` triangle count is at most 33 levels deep, and a
/// depth-first walk keeps at most one pending entry per level.
const STACK_SIZE: usize = 64;

/// A fixed-size stack of pending work for a tree walk.
struct Stack<T> {
    items: [T; STACK_SIZE],
    len: usize,
}

impl<T: Copy + Default> Stack<T> {
    /// Returns a stack holding only `first`.
    fn new(first: T) -> Self {
        let mut items = [T::default(); STACK_SIZE];
        items[0] = first;
        Self { items, len: 1 }
    }

    /// Adds `item` on top. Panics if the stack is full, which a walk over a
    /// [`Bvh`] never causes.
    fn push(&mut self, item: T) {
        self.items[self.len] = item;
        self.len += 1;
    }

    /// Removes and returns the top item, if any.
    fn pop(&mut self) -> Option<T> {
        self.len = self.len.checked_sub(1)?;
        Some(self.items[self.len])
    }
}

/// A tree node: either two children or a run of triangles.
#[derive(Debug, Clone, Copy)]
struct Node {
    bounds: Aabb,
    /// For a leaf, the first entry in `Bvh::triangles`; otherwise the right
    /// child. An inner node's left child is always the next node.
    start: u32,
    /// Number of triangles for a leaf, or 0 for an inner node.
    count: u32,
}

/// Triangle hierarchy built once per surface.
#[derive(Debug, Clone)]
pub(crate) struct Bvh {
    nodes: Vec<Node>,
    /// Triangle numbers, ordered so each leaf owns a contiguous run.
    triangles: Vec<u32>,
    /// Corner positions of every triangle, indexed by triangle number.
    corners: Vec<[Vec3; 3]>,
}

impl Bvh {
    /// Builds the hierarchy for every triangle of `surface`.
    pub(crate) fn new(surface: &Surface) -> Self {
        let corners: Vec<[Vec3; 3]> = (0..surface.triangle_count())
            .map(|t| surface.triangle_positions(t))
            .collect();
        let centroids: Vec<Vec3> = corners.iter().map(|c| (c[0] + c[1] + c[2]) / 3.0).collect();
        let mut bvh = Self {
            nodes: Vec::with_capacity(corners.len() * 2 / LEAF_SIZE + 1),
            triangles: (0..corners.len() as u32).collect(),
            corners,
        };
        bvh.build(0, bvh.triangles.len(), &centroids);
        bvh
    }

    /// Appends the subtree for `triangles[start..end]` and returns its node index.
    fn build(&mut self, start: usize, end: usize, centroids: &[Vec3]) -> usize {
        // Bound the triangles and their centroids.
        let (bounds, centre_bounds) =
            self.triangles[start..end]
                .iter()
                .fold((Aabb::EMPTY, Aabb::EMPTY), |(b, c), &t| {
                    let [p0, p1, p2] = self.corners[t as usize];
                    (b.grow(p0).grow(p1).grow(p2), c.grow(centroids[t as usize]))
                });
        let index = self.nodes.len();
        self.nodes.push(Node {
            bounds,
            start: start as u32,
            count: (end - start) as u32,
        });
        let extent = centre_bounds.max - centre_bounds.min;
        if end - start <= LEAF_SIZE || extent.max_element() <= 0.0 {
            return index;
        }

        // Split at the median centroid along the longest axis.
        let axis = if extent.x >= extent.y && extent.x >= extent.z {
            0
        } else if extent.y >= extent.z {
            1
        } else {
            2
        };
        let mid = (start + end) / 2;
        self.triangles[start..end].select_nth_unstable_by(mid - start, |&a, &b| {
            centroids[a as usize][axis].total_cmp(&centroids[b as usize][axis])
        });
        // The left child always lands right after this node.
        self.build(start, mid, centroids);
        let right = self.build(mid, end, centroids);
        self.nodes[index].start = right as u32;
        self.nodes[index].count = 0;
        index
    }

    /// Returns the closest point on any triangle to `p`.
    pub(crate) fn closest_point(&self, p: Vec3) -> ClosestPoint {
        let mut best = ClosestPoint {
            point: Vec3::ZERO,
            triangle: 0,
            barycentric: [1.0, 0.0, 0.0],
            distance_squared: f32::INFINITY,
        };
        let mut stack = Stack::new(0usize);
        while let Some(n) = stack.pop() {
            let node = self.nodes[n];
            if node.bounds.distance_squared(p) >= best.distance_squared {
                continue;
            }
            if node.count > 0 {
                // Test each triangle in the leaf.
                let run = node.start as usize..(node.start + node.count) as usize;
                for &t in &self.triangles[run] {
                    let (point, barycentric) = closest_on_triangle(p, self.corners[t as usize]);
                    let d = (point - p).length_squared();
                    if d < best.distance_squared {
                        best = ClosestPoint {
                            point,
                            triangle: t as usize,
                            barycentric,
                            distance_squared: d,
                        };
                    }
                }
            } else {
                // Visit the nearer child first so the far one is more often pruned.
                let (left, right) = (n + 1, node.start as usize);
                let dl = self.nodes[left].bounds.distance_squared(p);
                let dr = self.nodes[right].bounds.distance_squared(p);
                let (first, second) = if dl < dr {
                    (left, right)
                } else {
                    (right, left)
                };
                stack.push(second);
                stack.push(first);
            }
        }
        best
    }

    /// Returns the distance along `dir` to the first triangle hit before
    /// `t_max`, from either side, and that triangle. When several triangles
    /// are hit at the same distance, the same one is always returned. `dir`
    /// must be normalized.
    pub(crate) fn ray_hit(&self, origin: Vec3, dir: Vec3, t_max: f32) -> Option<(f32, usize)> {
        let inv_dir = dir.recip();
        let mut nearest = t_max;
        // The hit's position in `triangles`; ties go to the lowest position.
        let mut hit: Option<(f32, usize)> = None;
        let entry = self.nodes[0].bounds.ray_entry(origin, inv_dir, nearest)?;
        let ray = Ray::new(origin, dir);

        // Walk nodes nearest first, with each node's entry distance.
        let mut stack = Stack::new((0usize, entry));
        while let Some((n, entry)) = stack.pop() {
            // A closer hit may have been found since this node was pushed.
            if entry > nearest {
                continue;
            }
            let node = self.nodes[n];
            if node.count > 0 {
                let run = node.start as usize..(node.start + node.count) as usize;
                for slot in run {
                    let Some(d) = ray.hit(self.corners[self.triangles[slot] as usize]) else {
                        continue;
                    };
                    let better = match hit {
                        None => d < nearest,
                        Some((best, best_slot)) => d < best || (d == best && slot < best_slot),
                    };
                    if better {
                        nearest = d;
                        hit = Some((d, slot));
                    }
                }
            } else {
                // Push the farther child first, so the nearer one is popped first.
                let children = [n + 1, node.start as usize]
                    .map(|c| (c, self.nodes[c].bounds.ray_entry(origin, inv_dir, nearest)));
                let [a, b] = children;
                let (near, far) = match (a.1, b.1) {
                    (Some(ea), Some(eb)) if eb < ea => (b, a),
                    _ => (a, b),
                };
                for (c, e) in [far, near] {
                    if let Some(e) = e {
                        stack.push((c, e));
                    }
                }
            }
        }
        hit.map(|(d, slot)| (d, self.triangles[slot] as usize))
    }

    /// Returns the corner positions of triangle `t`.
    pub(crate) fn corners(&self, t: usize) -> [Vec3; 3] {
        self.corners[t]
    }

    /// Appends to `out` every triangle whose bounding box overlaps the box
    /// from `lo` to `hi`. The triangles themselves may not touch the box.
    pub(crate) fn overlapping(&self, lo: Vec3, hi: Vec3, out: &mut Vec<usize>) {
        let mut stack = Stack::new(0usize);
        while let Some(n) = stack.pop() {
            let node = self.nodes[n];
            if node.bounds.max.cmplt(lo).any() || node.bounds.min.cmpgt(hi).any() {
                continue;
            }
            if node.count > 0 {
                let run = node.start as usize..(node.start + node.count) as usize;
                for &t in &self.triangles[run] {
                    let [a, b, c] = self.corners[t as usize];
                    if !(a.min(b).min(c).cmpgt(hi).any() || a.max(b).max(c).cmplt(lo).any()) {
                        out.push(t as usize);
                    }
                }
            } else {
                stack.push(node.start as usize);
                stack.push(n + 1);
            }
        }
    }

    /// Returns whether a ray from `origin` along `dir` hits any triangle before `t_max`.
    pub(crate) fn ray_blocked(&self, origin: Vec3, dir: Vec3, t_max: f32) -> bool {
        let inv_dir = dir.recip();
        let ray = Ray::new(origin, dir);
        let mut stack = Stack::new(0usize);
        while let Some(n) = stack.pop() {
            let node = self.nodes[n];
            if node.bounds.ray_entry(origin, inv_dir, t_max).is_none() {
                continue;
            }
            if node.count > 0 {
                let run = node.start as usize..(node.start + node.count) as usize;
                if self.triangles[run]
                    .iter()
                    .any(|&t| ray.hit(self.corners[t as usize]).is_some_and(|d| d < t_max))
                {
                    return true;
                }
            } else {
                stack.push(node.start as usize);
                stack.push(n + 1);
            }
        }
        false
    }
}

/// Returns the closest point to `p` on triangle `[a, b, c]` and its barycentric weights.
///
/// Follows Ericson, "Real-Time Collision Detection", section 5.1.5.
pub(crate) fn closest_on_triangle(p: Vec3, [a, b, c]: [Vec3; 3]) -> (Vec3, [f32; 3]) {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;

    // Vertex region A.
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return (a, [1.0, 0.0, 0.0]);
    }

    // Vertex region B.
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return (b, [0.0, 1.0, 0.0]);
    }

    // Edge region AB.
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return (a + ab * v, [1.0 - v, v, 0.0]);
    }

    // Vertex region C.
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return (c, [0.0, 0.0, 1.0]);
    }

    // Edge region AC.
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return (a + ac * w, [1.0 - w, 0.0, w]);
    }

    // Edge region BC.
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return (b + (c - b) * w, [0.0, 1.0 - w, w]);
    }

    // Face region. A degenerate triangle falls back to vertex A.
    let denom = va + vb + vc;
    if denom.abs() <= f32::MIN_POSITIVE {
        return (a, [1.0, 0.0, 0.0]);
    }
    let v = vb / denom;
    let w = vc / denom;
    (a + ab * v + ac * w, [1.0 - v - w, v, w])
}

/// A ray set up for testing against many triangles.
///
/// The test is watertight: a ray that meets an edge or corner shared by
/// several triangles hits at least one of them, so rays can't slip through a
/// closed mesh between its triangles. This follows Woop, Benthin and Wald,
/// "Watertight Ray/Triangle Intersection" (2013).
struct Ray {
    origin: Vec3,
    /// Rows that map a point relative to `origin` into the ray's frame, where
    /// the ray runs along the third axis: x and y say how far the point lies
    /// off the ray, and z how far along it.
    x: Vec3,
    y: Vec3,
    z: Vec3,
    /// Reciprocal of the direction's largest component, which turns the
    /// frame's z into distance along the ray.
    z_scale: f32,
}

impl Ray {
    /// Prepares a ray from `origin` along `dir`, which must be normalized.
    fn new(origin: Vec3, dir: Vec3) -> Self {
        // Run the ray along its largest axis, and swap the other two when it
        // points the negative way, so triangles keep their winding.
        let abs = dir.abs();
        let kz = if abs.x >= abs.y && abs.x >= abs.z {
            0
        } else if abs.y >= abs.z {
            1
        } else {
            2
        };
        let (mut kx, mut ky) = ((kz + 1) % 3, (kz + 2) % 3);
        if dir[kz] < 0.0 {
            std::mem::swap(&mut kx, &mut ky);
        }

        // Shear so the ray becomes the frame's third axis.
        let unit = |k: usize| {
            let mut v = Vec3::ZERO;
            v[k] = 1.0;
            v
        };
        Self {
            origin,
            x: unit(kx) - unit(kz) * (dir[kx] / dir[kz]),
            y: unit(ky) - unit(kz) * (dir[ky] / dir[kz]),
            z: unit(kz),
            z_scale: 1.0 / dir[kz],
        }
    }

    /// Returns the distance along the ray where it meets triangle
    /// `[a, b, c]`, from either side. A ray starting on the triangle hits it
    /// at distance 0. A triangle seen exactly edge-on is never hit.
    fn hit(&self, [a, b, c]: [Vec3; 3]) -> Option<f32> {
        // Move the corners into the ray's frame.
        let [a, b, c] = [a, b, c].map(|p| {
            let p = p - self.origin;
            Vec3::new(self.x.dot(p), self.y.dot(p), self.z.dot(p))
        });

        // Which side of each edge the ray passes. A corner shared by two
        // triangles maps to the same point for both, so a shared edge gives
        // them the same value with opposite signs. A nonzero value always
        // has the right sign, so two of opposite sign already mean a miss.
        let edge = |p: Vec3, q: Vec3| p.x * q.y - p.y * q.x;
        let (mut u, mut v) = (edge(c, b), edge(a, c));
        if (u < 0.0 && v > 0.0) || (u > 0.0 && v < 0.0) {
            return None;
        }
        let mut w = edge(b, a);
        if u == 0.0 || v == 0.0 || w == 0.0 {
            // Exactly on an edge in f32: decide in f64 instead.
            let edge =
                |p: Vec3, q: Vec3| (p.x as f64 * q.y as f64 - p.y as f64 * q.x as f64) as f32;
            (u, v, w) = (edge(c, b), edge(a, c), edge(b, a));
        }

        // The ray hits only if it is inside or on all three edges.
        if (u < 0.0 || v < 0.0 || w < 0.0) && (u > 0.0 || v > 0.0 || w > 0.0) {
            return None;
        }
        let det = u + v + w;
        if det == 0.0 {
            return None;
        }
        let t = (u * a.z + v * b.z + w * c.z) * self.z_scale / det;
        (t >= 0.0).then_some(t)
    }
}

/// Returns whether two triangles pass through each other: an edge of one
/// crosses the interior of the other. Triangles that only touch, at an edge
/// or corner or within a tiny margin, don't count, and neither do triangles
/// lying in the same plane.
pub(crate) fn triangles_cross(a: [Vec3; 3], b: [Vec3; 3]) -> bool {
    let edges = |t: [Vec3; 3]| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])];
    edges(a).into_iter().any(|(p, q)| segment_crosses(p, q, b))
        || edges(b).into_iter().any(|(p, q)| segment_crosses(p, q, a))
}

/// Returns whether the segment from `p` to `q` passes through the interior of
/// triangle `[a, b, c]`, away from its edges and the segment's ends.
fn segment_crosses(p: Vec3, q: Vec3, [a, b, c]: [Vec3; 3]) -> bool {
    // Margin, as a fraction of the segment and of the triangle's barycentric
    // range, inside which a meeting counts as touching rather than crossing.
    const MARGIN: f32 = 1e-4;
    let dir = q - p;
    let e1 = b - a;
    let e2 = c - a;
    let pvec = dir.cross(e2);
    let det = e1.dot(pvec);
    if det.abs() < 1e-12 {
        return false;
    }
    let inv_det = 1.0 / det;
    let tvec = p - a;
    let u = tvec.dot(pvec) * inv_det;
    let qvec = tvec.cross(e1);
    let v = dir.dot(qvec) * inv_det;
    let t = e2.dot(qvec) * inv_det;
    u > MARGIN && v > MARGIN && u + v < 1.0 - MARGIN && t > MARGIN && t < 1.0 - MARGIN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossing_triangles_are_told_apart_from_touching_ones() {
        let flat = [
            Vec3::new(-1.0, 0.0, -1.0),
            Vec3::new(1.0, 0.0, -1.0),
            Vec3::new(0.0, 0.0, 1.0),
        ];
        let through = [
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(0.2, 1.0, 0.0),
            Vec3::new(-0.2, 1.0, 0.1),
        ];
        let above = through.map(|p| p + Vec3::Y * 2.0);
        let sharing_edge = [flat[0], flat[1], Vec3::new(0.0, 1.0, -1.0)];
        assert!(triangles_cross(flat, through));
        assert!(!triangles_cross(flat, above));
        assert!(!triangles_cross(flat, sharing_edge));
    }
    use crate::hash::Rng;
    use procgena_mesh::SurfaceMesh;

    /// A bumpy ground patch, so triangles are not all coplanar.
    fn bumpy() -> SurfaceMesh {
        let mut mesh = SurfaceMesh::ground(4.0, 12);
        for p in &mut mesh.positions {
            p[1] = (p[0] * 1.7).sin() * 0.3 + (p[2] * 2.3).cos() * 0.2;
        }
        mesh
    }

    #[test]
    fn closest_point_matches_brute_force() {
        let mesh = bumpy();
        let surface = mesh.surface().unwrap();
        let bvh = Bvh::new(&surface);
        let mut rng = Rng::new(7);
        for _ in 0..200 {
            let p = Vec3::new(
                rng.range(-3.0, 3.0),
                rng.range(-1.0, 1.5),
                rng.range(-3.0, 3.0),
            );
            let found = bvh.closest_point(p);
            let brute = (0..surface.triangle_count())
                .map(|t| {
                    (closest_on_triangle(p, surface.triangle_positions(t)).0 - p).length_squared()
                })
                .fold(f32::INFINITY, f32::min);
            assert!(
                (found.distance_squared - brute).abs() < 1e-5,
                "{p}: {} vs {brute}",
                found.distance_squared
            );

            // The barycentric weights reproduce the point.
            let [a, b, c] = surface.triangle_positions(found.triangle);
            let [u, v, w] = found.barycentric;
            assert!((a * u + b * v + c * w - found.point).length() < 1e-4);
        }
    }

    #[test]
    fn ray_hit_matches_brute_force() {
        let mesh = bumpy();
        let surface = mesh.surface().unwrap();
        let bvh = Bvh::new(&surface);
        let mut rng = Rng::new(11);
        for _ in 0..200 {
            let origin = Vec3::new(rng.range(-2.0, 2.0), 2.0, rng.range(-2.0, 2.0));
            let dir = Vec3::new(rng.range(-0.5, 0.5), -1.0, rng.range(-0.5, 0.5)).normalize();
            let ray = Ray::new(origin, dir);
            let brute = (0..surface.triangle_count())
                .filter_map(|t| ray.hit(surface.triangle_positions(t)))
                .fold(f32::INFINITY, f32::min);
            let found = bvh
                .ray_hit(origin, dir, f32::INFINITY)
                .map_or(f32::INFINITY, |h| h.0);
            assert!(
                found == brute || (found - brute).abs() < 1e-5,
                "{found} vs {brute}"
            );
            assert_eq!(
                bvh.ray_blocked(origin, dir, f32::INFINITY),
                brute.is_finite()
            );
        }
    }

    /// A closed tube with `sides` sides around the y axis, from y = 0 to
    /// y = 1, turned by `turn` radians about each axis, with rings at y = 0,
    /// 0.5 and 1 and flat caps. Returns the mesh and the middle ring's
    /// vertices.
    fn tube(
        sides: u32,
        radius: f32,
        turn: Vec3,
    ) -> (Vec<[f32; 3]>, Vec<u32>, std::ops::Range<u32>) {
        let rotate = glam::Quat::from_euler(glam::EulerRot::XYZ, turn.x, turn.y, turn.z);
        let mut positions = Vec::new();
        for ring in 0..3 {
            for i in 0..sides {
                let a = i as f32 * std::f32::consts::TAU / sides as f32;
                let p = Vec3::new(a.cos() * radius, ring as f32 * 0.5, a.sin() * radius);
                positions.push((rotate * p).to_array());
            }
        }
        let mut indices = Vec::new();
        for ring in 0..2 {
            for i in 0..sides {
                let (a, b) = (ring * sides + i, ring * sides + (i + 1) % sides);
                indices.extend_from_slice(&[a, a + sides, b, b, a + sides, b + sides]);
            }
        }
        for i in 1..sides - 1 {
            indices.extend_from_slice(&[0, i, i + 1]);
            indices.extend_from_slice(&[2 * sides, 2 * sides + i + 1, 2 * sides + i]);
        }
        (positions, indices, sides..2 * sides)
    }

    #[test]
    fn rays_through_a_closed_tube_never_slip_between_triangles() {
        // From each middle-ring vertex, straight in toward the axis: with an
        // even side count, the ray meets the far wall exactly on an edge.
        let mut rng = Rng::new(5);
        for sides in [6, 8, 12, 16, 24] {
            for _ in 0..40 {
                let turn = Vec3::new(
                    rng.range(-3.0, 3.0),
                    rng.range(-3.0, 3.0),
                    rng.range(-3.0, 3.0),
                );
                let radius = rng.range(0.001, 2.0);
                let (positions, indices, middle) = tube(sides, radius, turn);
                let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
                let surface = Surface::new(&positions, &normals, &indices).unwrap();
                let bvh = Bvh::new(&surface);
                let axis =
                    (Vec3::from(positions[0]) + Vec3::from(positions[(sides / 2) as usize])) * 0.5;
                for v in middle {
                    let p = Vec3::from(positions[v as usize]);
                    let centre = axis
                        + (Vec3::from(positions[(2 * sides) as usize]) - Vec3::from(positions[0]))
                            * 0.5;
                    let inward = (centre - p).normalize();
                    let start = p + inward * radius * 1e-3;
                    assert!(
                        bvh.ray_hit(start, inward, f32::INFINITY).is_some(),
                        "{sides} sides, radius {radius}, turn {turn}: ray from vertex {v} escaped"
                    );
                }
            }
        }
    }
}
