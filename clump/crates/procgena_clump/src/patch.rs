//! The working surface a layer clump is grown on: the part of the input the
//! clump may cover, simplified toward the target edge length, with its
//! exposure and room measured against the full input.

use std::collections::HashMap;

use glam::Vec3;

use crate::bvh::Bvh;
use crate::field::{ClumpField, SurfacePoint};
use procgena_mesh::Surface;

/// Smallest cosine allowed between a simplified triangle's normal and the
/// smooth normal at each of its corners, so triangles never turn far from
/// the surface they stand for.
const MIN_CORNER_COS: f32 = 0.2;

/// Smallest triangle quality a simplification step may create, where
/// quality is 1 for an equilateral triangle and 0 for a flat one. A step
/// that would go lower is still allowed if the triangles it replaces were
/// already worse.
const MIN_QUALITY: f32 = 0.2;

/// Length thresholds of the simplification sweeps, as fractions of the
/// target edge length. Short edges go first, so points thin out evenly.
const SWEEP_THRESHOLDS: [f32; 4] = [0.25, 0.5, 0.75, 1.0];

/// Most extra sweeps at the full target length, repeated while they still
/// collapse edges. Each sweep merges each point at most once, so coarsening
/// far below the input's own detail takes many sweeps.
const MAX_FINAL_SWEEPS: u32 = 32;

/// Farthest a merged point may move to reach the input surface, as a
/// fraction of the edge it replaces. A farther snap would land on another
/// part of the input.
const MAX_SNAP: f32 = 0.5;

/// A triangle of the working surface: its corners and a normal to tell when
/// it has turned over. The normal is the face normal of the input triangle
/// it came from, or of the simplified triangle that replaced it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Tri {
    pub(crate) corners: [u32; 3],
    pub(crate) source_normal: Vec3,
}

/// The working surface: shared points and the triangles between them.
pub(crate) struct Patch {
    pub(crate) points: Vec<SurfacePoint>,
    pub(crate) tris: Vec<Tri>,
}

impl Patch {
    /// Builds the working surface for `field`: copies every input triangle
    /// that faces into some direction, simplifies it so edges grow up to
    /// `edge_length`, records how far the input rises and dips around each
    /// remaining point, measures each point, and keeps only the triangles
    /// the clump may cover.
    ///
    /// Input copies of one position (as at texture seams) become one point,
    /// so the clump doesn't split along them. Every point lies on the input.
    pub(crate) fn new(field: &ClumpField, edge_length: f32) -> Self {
        let welded_normals = welded_normals(field);
        let (mut points, mut tris) = facing_triangles(field, &welded_normals);
        let fine: Vec<(Vec3, Vec3)> = points.iter().map(|p| (p.point, p.normal)).collect();
        let snap = Snap {
            field,
            normals: &welded_normals,
        };
        if simplify(&mut points, &mut tris, edge_length, &snap) {
            record_envelope(&mut points, &tris, &fine);
        }

        // Measure the points that are left, then drop what can't be covered.
        let points: Vec<SurfacePoint> = points.into_iter().map(|p| field.measure(p)).collect();
        tris.retain(|t| field.may_cover(t.corners.map(|v| &points[v as usize])));
        Self { points, tris }
    }

    /// Returns false if the clump can't cover any of the surface.
    pub(crate) fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }
}

/// Returns the smooth normal per input position id: the normalized sum of
/// the normals of the vertices at that position.
fn welded_normals(field: &ClumpField) -> Vec<Vec3> {
    let surface = field.surface();
    let (weld, count) = field.welded();
    let mut sums = vec![Vec3::ZERO; count];
    for (v, &id) in weld.iter().enumerate() {
        sums[id as usize] += surface.normal(v);
    }
    sums.into_iter().map(|n| n.normalize_or_zero()).collect()
}

/// Copies every input triangle with a corner whose facing term is above 0,
/// sharing one point per input position, with `normals` per position id.
/// Triangles that collapse once copies are merged are skipped.
fn facing_triangles(field: &ClumpField, normals: &[Vec3]) -> (Vec<SurfacePoint>, Vec<Tri>) {
    let surface = field.surface();
    let (weld, _) = field.welded();
    let mut points = Vec::new();
    let mut tris = Vec::new();
    let mut local: HashMap<u32, u32> = HashMap::new();
    for t in 0..surface.triangle_count() {
        let corners = surface.triangle(t);
        let ids = corners.map(|v| weld[v]);
        if ids[0] == ids[1] || ids[1] == ids[2] || ids[2] == ids[0] {
            continue;
        }
        if ids
            .iter()
            .all(|&id| field.facing(normals[id as usize]) <= 0.0)
        {
            continue;
        }
        let corners = [0, 1, 2].map(|k| {
            *local.entry(ids[k]).or_insert_with(|| {
                let p = SurfacePoint::new(surface.position(corners[k]), normals[ids[k] as usize]);
                points.push(p);
                (points.len() - 1) as u32
            })
        });
        let [a, b, c] = surface.triangle_positions(t);
        tris.push(Tri {
            corners,
            source_normal: (b - a).cross(c - a),
        });
    }
    (points, tris)
}

/// Moves points onto the input surface.
struct Snap<'a, 'f> {
    field: &'a ClumpField<'f>,
    /// Smooth normal per input position id.
    normals: &'a [Vec3],
}

impl Snap<'_, '_> {
    /// Returns the closest point on the input to `p` and the smooth normal
    /// there.
    fn onto_input(&self, p: Vec3) -> (Vec3, Vec3) {
        let closest = self.field.bvh().closest_point(p);
        let surface = self.field.surface();
        let (weld, _) = self.field.welded();
        let corners = surface.triangle(closest.triangle);
        let normal: Vec3 = (0..3)
            .map(|k| self.normals[weld[corners[k]] as usize] * closest.barycentric[k])
            .sum();
        (closest.point, normal.normalize_or_zero())
    }
}

/// Simplifies the triangles by merging the ends of edges shorter than
/// `edge_length`. Each merged point goes to whichever of the two ends, or
/// the edge's midpoint moved onto the input with `snap`, gives the
/// best-shaped triangles. A merge is skipped if it would create an edge
/// longer than `edge_length`, turn a triangle over or far from its corners'
/// normals, make a poor sliver, or tear the surface.
///
/// Points on the border of the triangles only slide along it, keeping its
/// shape, and points on edges shared by more than two triangles never move.
///
/// Edges are swept in index order with a growing length limit, which keeps
/// the result repeatable. Removes unused points and returns whether anything
/// was merged.
fn simplify(
    points: &mut Vec<SurfacePoint>,
    tris: &mut Vec<Tri>,
    edge_length: f32,
    snap: &Snap,
) -> bool {
    let mut mesh = CollapseMesh::new(points, tris);

    // Sweep with growing limits, then repeat at the full length while it helps.
    let mut collapsed = false;
    let mut pass = 0;
    let limits = SWEEP_THRESHOLDS
        .iter()
        .map(|f| f * edge_length)
        .chain(std::iter::repeat_n(edge_length, MAX_FINAL_SWEEPS as usize));
    for (i, limit) in limits.enumerate() {
        pass += 1;
        let merged = mesh.sweep(pass, limit, edge_length, snap);
        collapsed |= merged > 0;
        if merged == 0 && i >= SWEEP_THRESHOLDS.len() - 1 {
            break;
        }
    }
    if collapsed {
        let (kept_points, kept_tris) = mesh.compact(points);
        *points = kept_points;
        *tris = kept_tris;
    }
    collapsed
}

/// Where a merged point goes, and its normal.
#[derive(Debug, Clone, Copy)]
struct Placement {
    point: Vec3,
    normal: Vec3,
}

/// Triangles being simplified, with the lookups edge collapse needs.
struct CollapseMesh {
    point: Vec<Vec3>,
    normal: Vec<Vec3>,
    tris: Vec<Tri>,
    alive: Vec<bool>,
    /// The live triangles at each point, kept exact.
    around: Vec<Vec<u32>>,
    removed: Vec<bool>,
    /// Points that may not move.
    locked: Vec<bool>,
    /// Points on the border of the triangles.
    border: Vec<bool>,
    /// Sweep in which each point last changed; changed points wait for the
    /// next sweep, so merges spread evenly.
    changed: Vec<u32>,
    /// Number of merges done so far.
    merges: u32,
    /// Value of `merges` when each point or anything around it last changed.
    stamp: Vec<u32>,
    /// Per triangle edge (three per triangle, edge `k` running from corner
    /// `k`), the value of `merges` when it last failed to merge, or
    /// `u32::MAX` if it hasn't. An edge is only tried again once something
    /// around it has changed.
    failed: Vec<u32>,
    /// Live triangles, refreshed before each sweep.
    live: Vec<u32>,
    /// Scratch lists, reused between attempts.
    ring_a: Vec<u32>,
    ring_b: Vec<u32>,
}

impl CollapseMesh {
    fn new(points: &[SurfacePoint], tris: &[Tri]) -> Self {
        let mut around = vec![Vec::new(); points.len()];
        let mut uses: HashMap<(u32, u32), u32> = HashMap::new();
        for (t, tri) in tris.iter().enumerate() {
            for k in 0..3 {
                let (a, b) = (tri.corners[k], tri.corners[(k + 1) % 3]);
                around[a as usize].push(t as u32);
                *uses.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }

        // Points on edges shared by more than two triangles never move, nor
        // do border points where the border doesn't simply pass through.
        let mut locked = vec![false; points.len()];
        let mut border_edges = vec![0u32; points.len()];
        for (&(a, b), &n) in &uses {
            if n > 2 {
                locked[a as usize] = true;
                locked[b as usize] = true;
            } else if n == 1 {
                border_edges[a as usize] += 1;
                border_edges[b as usize] += 1;
            }
        }
        for (lock, &n) in locked.iter_mut().zip(&border_edges) {
            *lock |= n != 0 && n != 2;
        }
        Self {
            point: points.iter().map(|p| p.point).collect(),
            normal: points.iter().map(|p| p.normal).collect(),
            tris: tris.to_vec(),
            alive: vec![true; tris.len()],
            around,
            removed: vec![false; points.len()],
            locked,
            border: border_edges.iter().map(|&n| n > 0).collect(),
            changed: vec![0; points.len()],
            merges: 0,
            stamp: vec![0; points.len()],
            failed: vec![u32::MAX; tris.len() * 3],
            live: Vec::new(),
            ring_a: Vec::new(),
            ring_b: Vec::new(),
        }
    }

    /// Tries every live edge shorter than `limit`, in triangle order, whose
    /// ends haven't changed in sweep `pass`. Returns how many merged.
    fn sweep(&mut self, pass: u32, limit: f32, edge_length: f32, snap: &Snap) -> usize {
        let mut merged = 0;
        let mut live = std::mem::take(&mut self.live);
        live.clear();
        live.extend((0..self.tris.len() as u32).filter(|&t| self.alive[t as usize]));
        for &t in &live {
            let t = t as usize;
            for k in 0..3 {
                if !self.alive[t] {
                    break;
                }
                let corners = self.tris[t].corners;
                let (a, b) = (corners[k], corners[(k + 1) % 3]);
                // An inner edge appears in two triangles, once each way; try
                // it from one side only. A border edge appears once.
                let may_be_border = self.border[a as usize] && self.border[b as usize];
                if a > b && !may_be_border {
                    continue;
                }
                if self.changed[a as usize] == pass || self.changed[b as usize] == pass {
                    continue;
                }
                let when = self.failed[t * 3 + k];
                if when != u32::MAX
                    && self.stamp[a as usize] <= when
                    && self.stamp[b as usize] <= when
                {
                    continue;
                }
                if self.point[a as usize].distance(self.point[b as usize]) >= limit {
                    continue;
                }
                if !self.try_merge(a, b, edge_length, snap) {
                    self.failed[t * 3 + k] = self.merges;
                    continue;
                }

                // Mark a and everything around it as changed.
                self.merges += 1;
                for i in 0..self.around[a as usize].len() {
                    let tri = self.around[a as usize][i];
                    for w in self.tris[tri as usize].corners {
                        self.changed[w as usize] = pass;
                        self.stamp[w as usize] = self.merges;
                    }
                }
                merged += 1;
            }
        }
        self.live = live;
        merged
    }

    /// Writes the points joined to `v` by an edge into `out`, sorted.
    fn ring(&self, v: u32, out: &mut Vec<u32>) {
        out.clear();
        for &t in &self.around[v as usize] {
            out.extend(self.tris[t as usize].corners.iter().filter(|&&w| w != v));
        }
        out.sort_unstable();
        out.dedup();
    }

    /// Merges point `b` into point `a`, placing the result where the
    /// triangles come out best, if that is allowed. Returns whether it
    /// happened.
    fn try_merge(&mut self, a: u32, b: u32, edge_length: f32, snap: &Snap) -> bool {
        let (ai, bi) = (a as usize, b as usize);
        if self.locked[ai] && self.locked[bi] {
            return false;
        }

        // An inner edge has two triangles, a border edge one. An inner edge
        // between two border points would pinch the surface.
        let shared = self.around[ai]
            .iter()
            .filter(|&&t| self.tris[t as usize].corners.contains(&b))
            .count();
        let along_border = match shared {
            1 => true,
            2 if !(self.border[ai] && self.border[bi]) => false,
            _ => return false,
        };

        // The two ends may share no neighbours beyond the shared triangles'
        // third corners, or the surface tears.
        let mut ring_a = std::mem::take(&mut self.ring_a);
        let mut ring_b = std::mem::take(&mut self.ring_b);
        self.ring(a, &mut ring_a);
        self.ring(b, &mut ring_b);
        let common = ring_a
            .iter()
            .filter(|w| ring_b.binary_search(w).is_ok())
            .count();
        self.ring_a = ring_a;
        self.ring_b = ring_b;
        if common != shared {
            return false;
        }

        // Places the merged point may go: a fixed end stays put, a border
        // point stays on the border, and otherwise the midpoint is tried too.
        let end = |v: usize| Placement {
            point: self.point[v],
            normal: self.normal[v],
        };
        let fixed_a = self.locked[ai] || (!along_border && self.border[ai]);
        let fixed_b = self.locked[bi] || (!along_border && self.border[bi]);
        let mut options = [None; 3];
        match (fixed_a, fixed_b) {
            (true, true) => return false,
            (true, false) => options[0] = Some(end(ai)),
            (false, true) => options[0] = Some(end(bi)),
            (false, false) => {
                options[0] = Some(end(ai));
                options[1] = Some(end(bi));
                if !along_border {
                    options[2] = self.snapped_midpoint(a, b, snap);
                }
            }
        }

        // Keep the allowed placement with the best worst triangle.
        let best = options
            .into_iter()
            .flatten()
            .filter_map(|p| {
                self.worst_quality(a, b, p, edge_length, along_border)
                    .map(|q| (q, p))
            })
            .max_by(|x, y| x.0.total_cmp(&y.0));
        let Some((_, placement)) = best else {
            return false;
        };
        self.merge(a, b, placement);
        true
    }

    /// Returns the midpoint of edge `a`–`b` moved onto the input, or `None`
    /// if the closest input is too far away or faces the other way.
    fn snapped_midpoint(&self, a: u32, b: u32, snap: &Snap) -> Option<Placement> {
        let (pa, pb) = (self.point[a as usize], self.point[b as usize]);
        let mid = (pa + pb) * 0.5;
        let (point, normal) = snap.onto_input(mid);
        let facing = normal.dot(self.normal[a as usize] + self.normal[b as usize]) > 0.0;
        (facing && point.distance(mid) <= MAX_SNAP * pa.distance(pb))
            .then_some(Placement { point, normal })
    }

    /// Checks merging `a` and `b` at `placement`. Returns the quality of the
    /// worst triangle that would change, or `None` if the merge would make an
    /// edge longer than `edge_length`, turn a triangle, make a sliver, or (for
    /// a merge `along_border`) bend the border.
    fn worst_quality(
        &self,
        a: u32,
        b: u32,
        placement: Placement,
        edge_length: f32,
        along_border: bool,
    ) -> Option<f32> {
        if along_border && !self.border_stays_straight(a, b, placement.point, edge_length) {
            return None;
        }
        let mut worst = f32::INFINITY;
        for v in [a, b] {
            for &t in &self.around[v as usize] {
                let old = self.tris[t as usize].corners;
                if old.contains(&a) && old.contains(&b) {
                    continue;
                }
                let at = |w: u32| {
                    if w == a || w == b {
                        placement.point
                    } else {
                        self.point[w as usize]
                    }
                };
                let normal_at = |w: u32| {
                    if w == a || w == b {
                        placement.normal
                    } else {
                        self.normal[w as usize]
                    }
                };
                let new = old.map(at);
                if old.iter().any(|&w| {
                    w != a
                        && w != b
                        && placement.point.distance(self.point[w as usize]) > edge_length
                }) {
                    return None;
                }
                let before = old.map(|w| self.point[w as usize]);
                let n_new = (new[1] - new[0]).cross(new[2] - new[0]);
                let n_old = (before[1] - before[0]).cross(before[2] - before[0]);
                let unit = n_new.normalize_or_zero();
                if n_new.dot(n_old) <= 0.0
                    || old.iter().any(|&w| unit.dot(normal_at(w)) < MIN_CORNER_COS)
                {
                    return None;
                }
                let q = quality(new);
                if q < MIN_QUALITY.min(quality(before)) {
                    return None;
                }
                worst = worst.min(q);
            }
        }
        Some(worst)
    }

    /// Returns whether the border stays within a quarter of `edge_length` of
    /// its old shape when border points `a` and `b` merge at `point`: both
    /// old points must lie near the new border edges through `point`.
    fn border_stays_straight(&self, a: u32, b: u32, point: Vec3, edge_length: f32) -> bool {
        // The border neighbours of a and b, other than each other.
        let outer = |v: u32, other: u32| {
            let mut found = None;
            for &t in &self.around[v as usize] {
                for w in self.tris[t as usize].corners {
                    if w == v || w == other {
                        continue;
                    }
                    let uses = self.around[v as usize]
                        .iter()
                        .filter(|&&s| self.tris[s as usize].corners.contains(&w))
                        .count();
                    if uses == 1 {
                        found = Some(w);
                    }
                }
            }
            found
        };
        let (Some(na), Some(nb)) = (outer(a, b), outer(b, a)) else {
            return false;
        };
        let tolerance = 0.25 * edge_length;
        let near = |p: Vec3, q: Vec3, x: Vec3| {
            let pq = q - p;
            let t = ((x - p).dot(pq) / pq.length_squared().max(f32::MIN_POSITIVE)).clamp(0.0, 1.0);
            x.distance(p + pq * t) <= tolerance
        };
        let (pa, pb) = (self.point[a as usize], self.point[b as usize]);
        let (qa, qb) = (self.point[na as usize], self.point[nb as usize]);
        (near(point, qa, pa) || near(point, qb, pa)) && (near(point, qa, pb) || near(point, qb, pb))
    }

    /// Merges `b` into `a` at `placement`: removes the triangles on edge
    /// `a`–`b` and points `b`'s other triangles at `a`.
    fn merge(&mut self, a: u32, b: u32, placement: Placement) {
        let (ai, bi) = (a as usize, b as usize);
        self.point[ai] = placement.point;
        self.normal[ai] = placement.normal;
        self.locked[ai] |= self.locked[bi];
        self.border[ai] |= self.border[bi];

        // Drop the shared triangles from every list that holds them.
        let shared: Vec<u32> = self.around[ai]
            .iter()
            .copied()
            .filter(|&t| self.tris[t as usize].corners.contains(&b))
            .collect();
        for &t in &shared {
            self.alive[t as usize] = false;
            for w in self.tris[t as usize].corners {
                self.around[w as usize].retain(|&s| s != t);
            }
        }

        // Hand b's triangles to a.
        let moved = std::mem::take(&mut self.around[bi]);
        for &t in &moved {
            for w in &mut self.tris[t as usize].corners {
                if *w == b {
                    *w = a;
                }
            }
        }
        self.around[ai].extend(moved);
        self.removed[bi] = true;

        // Every triangle at a has a new shape: record its normal.
        for i in 0..self.around[ai].len() {
            let t = self.around[ai][i] as usize;
            let [p, q, r] = self.tris[t].corners.map(|w| self.point[w as usize]);
            self.tris[t].source_normal = (q - p).cross(r - p);
        }
    }

    /// Returns the remaining points and live triangles, renumbered. `points`
    /// supplies everything but position and normal, which come from here.
    fn compact(&self, points: &[SurfacePoint]) -> (Vec<SurfacePoint>, Vec<Tri>) {
        let mut remap = vec![u32::MAX; points.len()];
        let mut kept = Vec::new();
        for (v, p) in points.iter().enumerate() {
            if !self.removed[v] {
                remap[v] = kept.len() as u32;
                kept.push(SurfacePoint {
                    point: self.point[v],
                    normal: self.normal[v],
                    ..*p
                });
            }
        }
        let tris = self
            .tris
            .iter()
            .zip(&self.alive)
            .filter(|&(_, &alive)| alive)
            .map(|(t, _)| Tri {
                corners: t.corners.map(|v| remap[v as usize]),
                source_normal: t.source_normal,
            })
            .collect();
        (kept, tris)
    }
}

/// Returns a triangle's shape quality: 1 for equilateral, 0 for flat.
fn quality([a, b, c]: [Vec3; 3]) -> f32 {
    let area2 = (b - a).cross(c - a).length();
    let edges = (b - a).length_squared() + (c - b).length_squared() + (a - c).length_squared();
    if edges <= 0.0 {
        return 0.0;
    }
    // 4·√3·area / Σ edge², with area2 being twice the area.
    2.0 * 3f32.sqrt() * area2 / edges
}

/// Records on each simplified point how far the original surface `fine`
/// (positions and smooth normals) rises above and dips below the simplified
/// triangles around it.
///
/// Each original point is matched to the closest simplified triangle facing
/// the same way, and its height above that triangle raises the rise or dip
/// of all three corners, so blending over the triangle covers it.
fn record_envelope(points: &mut [SurfacePoint], tris: &[Tri], fine: &[(Vec3, Vec3)]) {
    if tris.is_empty() {
        return;
    }
    let positions: Vec<[f32; 3]> = points.iter().map(|p| p.point.to_array()).collect();
    let normals: Vec<[f32; 3]> = points.iter().map(|p| p.normal.to_array()).collect();
    let indices: Vec<u32> = tris.iter().flat_map(|t| t.corners).collect();
    let coarse = Surface::new(&positions, &normals, &indices)
        .expect("simplified triangles index their own points");
    let bvh = Bvh::new(&coarse);

    let mut rise = vec![0.0f32; points.len()];
    let mut dip = vec![0.0f32; points.len()];
    for &(q, n) in fine {
        let closest = bvh.closest_point(q);
        let corners = coarse.triangle(closest.triangle);
        let [a, b, c] = coarse.triangle_positions(closest.triangle);
        let face = (b - a).cross(c - a).normalize_or_zero();
        if face.dot(n) <= 0.0 {
            continue;
        }
        let h = (q - closest.point).dot(face);
        for v in corners {
            if h > 0.0 {
                rise[v] = rise[v].max(h);
            } else {
                dip[v] = dip[v].max(-h);
            }
        }
    }
    for (p, (r, d)) in points.iter_mut().zip(rise.into_iter().zip(dip)) {
        p.rise = r;
        p.dip = d;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::ClumpOptions;
    use procgena_mesh::SurfaceMesh;

    /// A `size` metre ground patch with `divisions` cells per side and
    /// gentle bumps, with matching normals.
    fn bumpy_ground(size: f32, divisions: u32) -> SurfaceMesh {
        let mut ground = SurfaceMesh::ground(size, divisions);
        for (p, n) in ground.positions.iter_mut().zip(&mut ground.normals) {
            let (x, z) = (p[0], p[2]);
            p[1] = 0.05 * (5.0 * x).sin() * (4.0 * z).cos();
            let dx = 0.25 * (5.0 * x).cos() * (4.0 * z).cos();
            let dz = -0.2 * (5.0 * x).sin() * (4.0 * z).sin();
            *n = Vec3::new(-dx, 1.0, -dz).normalize().to_array();
        }
        ground
    }

    /// Returns each edge's number of triangles.
    fn edge_uses(tris: &[Tri]) -> HashMap<(u32, u32), u32> {
        let mut uses = HashMap::new();
        for t in tris {
            for k in 0..3 {
                let (a, b) = (t.corners[k], t.corners[(k + 1) % 3]);
                *uses.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        uses
    }

    #[test]
    fn simplifying_keeps_the_surface_whole() {
        let ground = bumpy_ground(2.0, 100);
        let mut options = ClumpOptions::default();
        options.mask.exposure_rays = 0;
        let field = ClumpField::new(ground.surface().unwrap(), &options);
        for edge_length in [0.04, 0.1, 0.3] {
            let patch = Patch::new(&field, edge_length);

            // Every edge has one or two triangles, and every one-triangle
            // edge lies on the ground patch's own border.
            let on_border = |v: u32| {
                let p = patch.points[v as usize].point;
                p.x.abs().max(p.z.abs()) > 1.0 - 1e-4
            };
            for (&(a, b), &n) in &edge_uses(&patch.tris) {
                assert!(n <= 2, "edge {a}-{b} has {n} triangles at {edge_length}");
                assert!(
                    n == 2 || (on_border(a) && on_border(b)),
                    "hole at {:?} at {edge_length}",
                    patch.points[a as usize].point
                );
            }

            // Every triangle still faces up.
            for t in &patch.tris {
                assert!(t.source_normal.y > 0.0);
            }
        }
    }
}
