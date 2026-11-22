//! A bounding volume hierarchy over a surface's triangles, for ray casts and
//! closest-point queries.

use glam::Vec3;

use crate::surface::Surface;

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
        Self { min: self.min.min(p), max: self.max.max(p) }
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
        let (bounds, centre_bounds) = self.triangles[start..end].iter().fold(
            (Aabb::EMPTY, Aabb::EMPTY),
            |(b, c), &t| {
                let [p0, p1, p2] = self.corners[t as usize];
                (b.grow(p0).grow(p1).grow(p2), c.grow(centroids[t as usize]))
            },
        );
        let index = self.nodes.len();
        self.nodes.push(Node { bounds, start: start as u32, count: (end - start) as u32 });
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
        let mut stack = vec![0usize];
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
                        best = ClosestPoint { point, triangle: t as usize, barycentric, distance_squared: d };
                    }
                }
            } else {
                // Visit the nearer child first so the far one is more often pruned.
                let (left, right) = (n + 1, node.start as usize);
                let dl = self.nodes[left].bounds.distance_squared(p);
                let dr = self.nodes[right].bounds.distance_squared(p);
                if dl < dr {
                    stack.extend([right, left]);
                } else {
                    stack.extend([left, right]);
                }
            }
        }
        best
    }

    /// Returns the distance along `dir` to the first triangle hit before
    /// `t_max`, from either side. `dir` must be normalized.
    pub(crate) fn ray_hit(&self, origin: Vec3, dir: Vec3, t_max: f32) -> Option<(f32, usize)> {
        let inv_dir = dir.recip();
        let mut nearest = t_max;
        let mut hit = None;
        let mut stack = vec![0usize];
        while let Some(n) = stack.pop() {
            let node = self.nodes[n];
            if node.bounds.ray_entry(origin, inv_dir, nearest).is_none() {
                continue;
            }
            if node.count > 0 {
                let run = node.start as usize..(node.start + node.count) as usize;
                for &t in &self.triangles[run] {
                    if let Some(d) = ray_triangle(origin, dir, self.corners[t as usize])
                        && d < nearest
                    {
                        nearest = d;
                        hit = Some((d, t as usize));
                    }
                }
            } else {
                stack.extend([node.start as usize, n + 1]);
            }
        }
        hit
    }

    /// Returns whether a ray from `origin` along `dir` hits any triangle before `t_max`.
    pub(crate) fn ray_blocked(&self, origin: Vec3, dir: Vec3, t_max: f32) -> bool {
        let inv_dir = dir.recip();
        let mut stack = vec![0usize];
        while let Some(n) = stack.pop() {
            let node = self.nodes[n];
            if node.bounds.ray_entry(origin, inv_dir, t_max).is_none() {
                continue;
            }
            if node.count > 0 {
                let run = node.start as usize..(node.start + node.count) as usize;
                if self.triangles[run]
                    .iter()
                    .any(|&t| ray_triangle(origin, dir, self.corners[t as usize]).is_some_and(|d| d < t_max))
                {
                    return true;
                }
            } else {
                stack.extend([node.start as usize, n + 1]);
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

/// Returns the distance along `dir` where the ray meets triangle `[a, b, c]`,
/// from either side, using the Möller–Trumbore test. A ray starting on the
/// triangle hits it at distance 0.
fn ray_triangle(origin: Vec3, dir: Vec3, [a, b, c]: [Vec3; 3]) -> Option<f32> {
    let e1 = b - a;
    let e2 = c - a;
    let pvec = dir.cross(e2);
    let det = e1.dot(pvec);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    let tvec = origin - a;
    let u = tvec.dot(pvec) * inv_det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let qvec = tvec.cross(e1);
    let v = dir.dot(qvec) * inv_det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(qvec) * inv_det;
    (t >= 0.0).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::Rng;
    use crate::surface::SurfaceMesh;

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
            let p = Vec3::new(rng.range(-3.0, 3.0), rng.range(-1.0, 1.5), rng.range(-3.0, 3.0));
            let found = bvh.closest_point(p);
            let brute = (0..surface.triangle_count())
                .map(|t| (closest_on_triangle(p, surface.triangle_positions(t)).0 - p).length_squared())
                .fold(f32::INFINITY, f32::min);
            assert!((found.distance_squared - brute).abs() < 1e-5, "{p}: {} vs {brute}", found.distance_squared);

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
            let brute = (0..surface.triangle_count())
                .filter_map(|t| ray_triangle(origin, dir, surface.triangle_positions(t)))
                .fold(f32::INFINITY, f32::min);
            let found = bvh.ray_hit(origin, dir, f32::INFINITY).map_or(f32::INFINITY, |h| h.0);
            assert!(found == brute || (found - brute).abs() < 1e-5, "{found} vs {brute}");
            assert_eq!(bvh.ray_blocked(origin, dir, f32::INFINITY), brute.is_finite());
        }
    }
}
