//! Turns a clump field into triangles by refining the covered part of the
//! surface and pushing it outward.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use glam::Vec3;

use crate::field::{ClumpField, SurfacePoint};
use crate::mesh::{BuildStats, ClumpMesh};

/// Most refinement passes. Each pass halves the longest edges, so this allows
/// edges 2^16 times longer than the target.
const MAX_PASSES: u32 = 16;

/// Extra depth of a closed clump's underside below the sink depth, as a
/// fraction of the thickness. Clump tops never go below the sink depth, so
/// this keeps the solid at least this thick where the input is thick enough.
const BOTTOM_GAP: f32 = 0.25;

/// Thinnest a closed clump may be anywhere, as a fraction of the thickness.
/// Applies where the input is too thin for the full underside depth.
const MIN_SOLID: f32 = 0.05;

/// Most passes when blurring push directions, to bound the cost on meshes
/// whose edges are much shorter than the thickness.
const MAX_BLUR_PASSES: u32 = 256;

/// Fraction of the gap to the opposite surface that a clump may fill, so
/// clumps on facing walls of a crevice don't pass through each other.
const GAP_FILL: f32 = 0.5;

/// A triangle being refined: its corners and the face normal of the input
/// triangle it came from.
#[derive(Debug, Clone, Copy)]
struct Tri {
    corners: [u32; 3],
    source_normal: Vec3,
}

/// Builds the clump mesh: copies the triangles the clump may cover, splits
/// them until no edge is longer than `edge_length`, and pushes every vertex
/// out by the clump's height there. The result is a single open sheet whose
/// edges sink below the surface.
///
/// With `closed`, the sheet also gets a buried underside and side walls, so
/// the result is watertight.
pub(crate) fn displaced_shell(
    field: &ClumpField,
    edge_length: f32,
    max_slope: f32,
    closed: bool,
) -> (ClumpMesh, BuildStats) {
    let (mut points, mut tris) = covered_triangles(field);
    refine(field, &mut points, &mut tris, edge_length);

    // Push along normals blurred over about the clump's thickness, so pushes
    // in tight hollows don't converge and cross.
    let neighbours = neighbour_lists(points.len(), &tris);
    // Averaging spreads about one edge per sqrt(pass), so reaching the
    // thickness takes (thickness / edge)² passes.
    let edge = mean_edge_length(&points, &tris).max(f32::MIN_POSITIVE);
    let passes = (field.thickness() / edge).powi(2).ceil().min(MAX_BLUR_PASSES as f32) as u32;
    let dirs = smooth_directions(points.iter().map(|p| p.normal).collect(), &neighbours, passes);

    // Work out each vertex's height, never more than part of the way to a
    // surface facing it.
    let weights: Vec<f32> = points.iter().map(|p| field.weight(p)).collect();
    let mut thinned = 0;
    let mut tops: Vec<f32> = points
        .iter()
        .zip(&dirs)
        .zip(&weights)
        .map(|((p, &dir), &w)| {
            let top = field.top(w);
            if top <= 0.0 {
                return top;
            }
            let limit = GAP_FILL * field.clearance(p.point, dir);
            if top > limit {
                thinned += 1;
                return limit;
            }
            top
        })
        .collect();

    // Lower heights so no edge of the clump is steeper than `max_slope`.
    limit_slope(&points, &neighbours, &mut tops, max_slope);

    // Drop triangles sunk well below the surface; they can't be seen. Keeping
    // those only slightly below stops flat triangles poking out of hollows.
    let hidden = -0.5 * field.sink();
    let kept: Vec<&Tri> = tris
        .iter()
        .filter(|t| t.corners.iter().any(|&v| weights[v as usize] > 0.0 && tops[v as usize] >= hidden))
        .collect();

    // For a closed clump, place the underside below each kept vertex, never
    // more than part of the way through the input, and keep every top above
    // its underside. The underside goes straight in along the surface normal:
    // the blurred push direction can lie almost along the surface on tight
    // curves, where pushing against it would leave the input.
    let mut bottoms = Vec::new();
    if closed {
        let full_depth = field.sink() + BOTTOM_GAP * field.thickness();
        let min_solid = MIN_SOLID * field.thickness();
        bottoms = vec![Vec3::ZERO; points.len()];
        let mut done = vec![false; points.len()];
        for v in kept.iter().flat_map(|t| t.corners) {
            let v = v as usize;
            if std::mem::replace(&mut done[v], true) {
                continue;
            }
            let (p, n) = (points[v].point, points[v].normal);
            let depth = full_depth.min(GAP_FILL * field.clearance(p, -n));
            bottoms[v] = p - n * depth;
            tops[v] = tops[v].max(min_solid - depth);
        }
    }

    // Push every vertex out.
    let mut mesh = ClumpMesh::default();
    for ((p, dir), &top) in points.iter().zip(&dirs).zip(&tops) {
        mesh.positions.push((p.point + *dir * top).to_array());
        mesh.fade.push(field.fade(top));
    }

    // Copy the kept triangles, counting any that turned over.
    let mut folded = 0;
    for tri in kept {
        let [d, e, f] = tri.corners.map(|v| Vec3::from(mesh.positions[v as usize]));
        if (e - d).cross(f - d).dot(tri.source_normal) < 0.0 {
            folded += 1;
        }
        mesh.indices.extend_from_slice(&tri.corners);
    }

    // Give a closed clump its underside and side walls.
    if closed {
        close_volume(&mut mesh, &bottoms);
    }

    remove_unused_vertices(&mut mesh);
    mesh.normals = face_normals(&mesh.positions, &mesh.indices);
    (mesh, BuildStats { folded, thinned })
}

/// Turns the top sheet in `mesh` into a closed solid. Adds an underside: a
/// copy of the sheet's triangles, reversed to face inward, with each vertex
/// moved to its position in `bottoms`. Then joins the two with a wall of quads
/// along every border edge of the sheet.
///
/// `mesh` must hold one vertex per entry of `bottoms` and the sheet's
/// triangles (before unused vertices are removed). Underside vertices get a
/// fade of 0.
fn close_volume(mesh: &mut ClumpMesh, bottoms: &[Vec3]) {
    let mut top: Vec<[u32; 3]> = mesh.indices.as_chunks::<3>().0.to_vec();

    // Give each fan of triangles at a pinch vertex its own copy of the
    // vertex, so every wall edge is shared by exactly two triangles.
    let source = split_pinches(mesh, &mut top, bottoms.len());
    mesh.indices = top.iter().flatten().copied().collect();

    // Make one underside vertex per sheet vertex, on demand.
    let mut below: HashMap<u32, u32> = HashMap::new();
    let mut bottom_of = |v: u32, mesh: &mut ClumpMesh| {
        *below.entry(v).or_insert_with(|| {
            let p = bottoms[source(v)];
            mesh.positions.push(p.to_array());
            mesh.fade.push(0.0);
            (mesh.positions.len() - 1) as u32
        })
    };

    // Underside triangles, wound the other way so they face into the input.
    for &[a, b, c] in &top {
        let (a2, b2, c2) = (bottom_of(a, mesh), bottom_of(b, mesh), bottom_of(c, mesh));
        mesh.indices.extend_from_slice(&[a2, c2, b2]);
    }

    // Find border edges: directed sheet edges whose reverse isn't in the sheet.
    let directed: HashSet<(u32, u32)> =
        top.iter().flat_map(|&[a, b, c]| [(a, b), (b, c), (c, a)]).collect();
    let mut border: Vec<(u32, u32)> =
        directed.iter().copied().filter(|&(a, b)| !directed.contains(&(b, a))).collect();
    border.sort_unstable();

    // Wall quad for each border edge a→b, using b→a on top and a'→b' below.
    for (a, b) in border {
        let (a2, b2) = (bottom_of(a, mesh), bottom_of(b, mesh));
        mesh.indices.extend_from_slice(&[b, a, a2, b, a2, b2]);
    }
}

/// Finds pinch vertices in the sheet `top`, where its border passes through
/// the same vertex more than once, and splits them: each fan of triangles
/// around a pinch vertex (triangles joined through edges at that vertex)
/// after the first gets its own copy of the vertex. Copies are appended to
/// `mesh` with the same position and fade.
///
/// `mesh` must hold exactly `original_count` vertices on entry. Returns a
/// function mapping any vertex index, original or copy, to the original index.
fn split_pinches(
    mesh: &mut ClumpMesh,
    top: &mut [[u32; 3]],
    original_count: usize,
) -> impl Fn(u32) -> usize + use<> {
    // Count outgoing border edges per vertex; more than one marks a pinch.
    let directed: HashSet<(u32, u32)> =
        top.iter().flat_map(|&[a, b, c]| [(a, b), (b, c), (c, a)]).collect();
    let mut outgoing: HashMap<u32, u32> = HashMap::new();
    for &(a, b) in &directed {
        if !directed.contains(&(b, a)) {
            *outgoing.entry(a).or_default() += 1;
        }
    }
    let mut pinches: Vec<u32> = outgoing.into_iter().filter(|&(_, n)| n > 1).map(|(v, _)| v).collect();
    pinches.sort_unstable();

    // Triangles touching each vertex.
    let mut around: HashMap<u32, Vec<usize>> = HashMap::new();
    for (t, tri) in top.iter().enumerate() {
        for &v in tri {
            if pinches.binary_search(&v).is_ok() {
                around.entry(v).or_default().push(t);
            }
        }
    }

    let mut copies: Vec<u32> = Vec::new();
    for v in pinches {
        // Group the triangles at v into fans: two triangles join when they
        // share an edge (v, x).
        let tris = &around[&v];
        let mut fan = vec![usize::MAX; tris.len()];
        let mut fans = 0;
        for start in 0..tris.len() {
            if fan[start] != usize::MAX {
                continue;
            }
            fan[start] = fans;
            let mut stack = vec![start];
            while let Some(i) = stack.pop() {
                let others_i: Vec<u32> = top[tris[i]].into_iter().filter(|&x| x != v).collect();
                for j in 0..tris.len() {
                    if fan[j] == usize::MAX && top[tris[j]].iter().any(|x| others_i.contains(x)) {
                        fan[j] = fans;
                        stack.push(j);
                    }
                }
            }
            fans += 1;
        }

        // Keep v for the first fan and give each later fan a copy.
        for f in 1..fans {
            let copy = mesh.positions.len() as u32;
            mesh.positions.push(mesh.positions[v as usize]);
            mesh.fade.push(mesh.fade[v as usize]);
            copies.push(v);
            for (i, &t) in tris.iter().enumerate() {
                if fan[i] == f {
                    top[t].iter_mut().filter(|x| **x == v).for_each(|x| *x = copy);
                }
            }
        }
    }

    // Every original vertex has a position, so copies start right after them.
    debug_assert_eq!(mesh.positions.len() - copies.len(), original_count);
    move |i: u32| {
        let i = i as usize;
        if i < original_count { i } else { copies[i - original_count] as usize }
    }
}

/// Copies every input triangle the clump may cover, sharing vertices.
fn covered_triangles(field: &ClumpField) -> (Vec<SurfacePoint>, Vec<Tri>) {
    let surface = field.surface();
    let mut points = Vec::new();
    let mut tris = Vec::new();
    let mut local: HashMap<usize, u32> = HashMap::new();
    for t in 0..surface.triangle_count() {
        let input = surface.triangle(t).map(|v| field.vertex_point(v));
        if !field.may_cover([&input[0], &input[1], &input[2]]) {
            continue;
        }
        let corners = surface.triangle(t).map(|v| {
            *local.entry(v).or_insert_with(|| {
                points.push(field.vertex_point(v));
                (points.len() - 1) as u32
            })
        });
        let [a, b, c] = surface.triangle_positions(t);
        tris.push(Tri { corners, source_normal: (b - a).cross(c - a) });
    }
    (points, tris)
}

/// Splits triangles the clump may cover until no edge is longer than
/// `edge_length`.
///
/// Each pass halves every edge that is too long and splits the triangles
/// around it to match, so covered triangles always share their edges and the
/// clump has no cracks. Triangles the clump can't cover are left whole; they
/// are dropped later, so the unmatched split points along their edges never
/// show.
fn refine(field: &ClumpField, points: &mut Vec<SurfacePoint>, tris: &mut Vec<Tri>, edge_length: f32) {
    let limit = edge_length * edge_length;
    for _ in 0..MAX_PASSES {
        let too_long = |a: u32, b: u32, points: &[SurfacePoint]| {
            points[a as usize].point.distance_squared(points[b as usize].point) > limit
        };
        let active: Vec<bool> = tris
            .iter()
            .map(|t| field.may_cover(t.corners.map(|v| &points[v as usize])))
            .collect();
        if !tris.iter().zip(&active).any(|(t, &on)| {
            let [a, b, c] = t.corners;
            on && (too_long(a, b, points) || too_long(b, c, points) || too_long(c, a, points))
        }) {
            return;
        }

        // Split each covered triangle by how many of its edges are too long.
        let mut midpoints: HashMap<(u32, u32), u32> = HashMap::new();
        let mut next = Vec::with_capacity(tris.len() * 3);
        for (tri, &on) in tris.iter().zip(&active) {
            let v = tri.corners;
            let marked = [0, 1, 2].map(|k| on && too_long(v[k], v[(k + 1) % 3], points));
            let mut mid = |a: u32, b: u32| {
                *midpoints.entry((a.min(b), a.max(b))).or_insert_with(|| {
                    let m = field.midpoint(&points[a as usize], &points[b as usize]);
                    points.push(m);
                    (points.len() - 1) as u32
                })
            };
            let push = |next: &mut Vec<Tri>, corners: [u32; 3]| {
                next.push(Tri { corners, source_normal: tri.source_normal });
            };
            match marked.iter().filter(|&&m| m).count() {
                0 => next.push(*tri),
                1 => {
                    // Rotate so the long edge is a→b, then halve it.
                    let k = marked.iter().position(|&m| m).unwrap_or(0);
                    let [a, b, c] = [v[k], v[(k + 1) % 3], v[(k + 2) % 3]];
                    let ab = mid(a, b);
                    push(&mut next, [a, ab, c]);
                    push(&mut next, [ab, b, c]);
                }
                2 => {
                    // Rotate so the short edge is c→a, halve a→b and b→c, and
                    // cut the remaining quad along its shorter diagonal.
                    let k = marked.iter().position(|&m| !m).unwrap_or(0);
                    let [a, b, c] = [v[(k + 1) % 3], v[(k + 2) % 3], v[k]];
                    let (ab, bc) = (mid(a, b), mid(b, c));
                    push(&mut next, [ab, b, bc]);
                    let at = |i: u32| points[i as usize].point;
                    if at(a).distance_squared(at(bc)) <= at(ab).distance_squared(at(c)) {
                        push(&mut next, [a, ab, bc]);
                        push(&mut next, [a, bc, c]);
                    } else {
                        push(&mut next, [a, ab, c]);
                        push(&mut next, [ab, bc, c]);
                    }
                }
                _ => {
                    let [a, b, c] = v;
                    let (ab, bc, ca) = (mid(a, b), mid(b, c), mid(c, a));
                    push(&mut next, [a, ab, ca]);
                    push(&mut next, [ab, b, bc]);
                    push(&mut next, [ca, bc, c]);
                    push(&mut next, [ab, bc, ca]);
                }
            }
        }
        *tris = next;
    }
}

/// Lowers `tops` until, along every edge, the height changes by at most
/// `max_slope` times the edge's length. Heights are only ever lowered.
///
/// This is a shortest-path pass over the mesh edges: each vertex ends at the
/// lowest of its own height and every other vertex's height plus `max_slope`
/// times the path length between them.
fn limit_slope(points: &[SurfacePoint], neighbours: &[Vec<u32>], tops: &mut [f32], max_slope: f32) {
    // Spread the low heights outward, lowest first.
    let mut queue: BinaryHeap<Reverse<(OrderedHeight, u32)>> =
        (0..tops.len() as u32).map(|v| Reverse((OrderedHeight(tops[v as usize]), v))).collect();
    while let Some(Reverse((OrderedHeight(h), v))) = queue.pop() {
        if h > tops[v as usize] {
            continue;
        }
        let p = points[v as usize].point;
        for &n in &neighbours[v as usize] {
            let allowed = h + max_slope * p.distance(points[n as usize].point);
            if allowed < tops[n as usize] {
                tops[n as usize] = allowed;
                queue.push(Reverse((OrderedHeight(allowed), n)));
            }
        }
    }
}

/// Returns the average length of the triangles' edges, counting shared edges
/// once per triangle. Returns 0 for no triangles.
fn mean_edge_length(points: &[SurfacePoint], tris: &[Tri]) -> f32 {
    if tris.is_empty() {
        return 0.0;
    }
    let total: f32 = tris
        .iter()
        .flat_map(|t| [0, 1, 2].map(|k| (t.corners[k], t.corners[(k + 1) % 3])))
        .map(|(a, b)| points[a as usize].point.distance(points[b as usize].point))
        .sum();
    total / (tris.len() * 3) as f32
}

/// Returns each vertex's neighbours along triangle edges, sorted.
fn neighbour_lists(vertex_count: usize, tris: &[Tri]) -> Vec<Vec<u32>> {
    let mut neighbours: Vec<Vec<u32>> = vec![Vec::new(); vertex_count];
    for tri in tris {
        for k in 0..3 {
            let (a, b) = (tri.corners[k], tri.corners[(k + 1) % 3]);
            neighbours[a as usize].push(b);
            neighbours[b as usize].push(a);
        }
    }
    for list in &mut neighbours {
        list.sort_unstable();
        list.dedup();
    }
    neighbours
}

/// Blurs unit directions by averaging each with its neighbours, `passes`
/// times, and returns them normalized. A direction that averages to zero
/// keeps its previous value.
fn smooth_directions(mut dirs: Vec<Vec3>, neighbours: &[Vec<u32>], passes: u32) -> Vec<Vec3> {
    for _ in 0..passes {
        dirs = dirs
            .iter()
            .zip(neighbours)
            .map(|(&d, list)| {
                let sum = list.iter().fold(d, |s, &n| s + dirs[n as usize]);
                sum.normalize_or(d)
            })
            .collect();
    }
    dirs
}

/// A height with a total order, for the priority queue.
#[derive(Debug, Clone, Copy, PartialEq)]
struct OrderedHeight(f32);

impl Eq for OrderedHeight {}

impl PartialOrd for OrderedHeight {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedHeight {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

/// Removes vertices that no triangle uses, keeping the rest in order.
/// Expects positions and fade filled and normals not yet computed.
fn remove_unused_vertices(mesh: &mut ClumpMesh) {
    let mut used = vec![false; mesh.positions.len()];
    for &i in &mesh.indices {
        used[i as usize] = true;
    }
    let mut remap = vec![u32::MAX; used.len()];
    let mut next = 0;
    for (old, keep) in used.iter().enumerate() {
        if *keep {
            remap[old] = next;
            mesh.positions[next as usize] = mesh.positions[old];
            mesh.fade[next as usize] = mesh.fade[old];
            next += 1;
        }
    }
    mesh.positions.truncate(next as usize);
    mesh.fade.truncate(next as usize);
    for i in &mut mesh.indices {
        *i = remap[*i as usize];
    }
}

/// Returns area-weighted vertex normals from the triangles.
fn face_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for tri in indices.as_chunks::<3>().0 {
        let [a, b, c] = tri.map(|i| Vec3::from(positions[i as usize]));
        let n = (b - a).cross(c - a);
        for &i in tri {
            normals[i as usize] += n;
        }
    }
    normals.into_iter().map(|n| n.normalize_or_zero().to_array()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pinched_sheet_closes_into_a_manifold_solid() {
        // Two flat triangles that touch only at vertex 0: a bowtie.
        let corners = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [-1.0, 0.0, 1.0]];
        let bottoms: Vec<Vec3> = corners.iter().map(|&[x, _, z]| Vec3::new(x, -0.05, z)).collect();
        let mut mesh = ClumpMesh {
            positions: corners.iter().map(|&[x, _, z]| [x, 0.1, z]).collect(),
            fade: vec![1.0; corners.len()],
            indices: vec![0, 2, 1, 0, 4, 3],
            ..ClumpMesh::default()
        };
        close_volume(&mut mesh, &bottoms);

        // Every directed edge appears once and is matched by its reverse.
        let mut seen = HashSet::new();
        for &[a, b, c] in mesh.indices.as_chunks::<3>().0 {
            for e in [(a, b), (b, c), (c, a)] {
                assert!(seen.insert(e), "edge {e:?} repeated");
            }
        }
        assert!(seen.iter().all(|&(a, b)| seen.contains(&(b, a))));

        // Two separate wedges: 2 tops, 2 bottoms and 3 wall quads each.
        assert_eq!(mesh.triangle_count(), 2 * (1 + 1 + 3 * 2));
    }
}
