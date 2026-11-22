//! Turns a clump field into triangles by refining the covered part of the
//! surface and pushing it outward.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use glam::Vec3;

use crate::bvh::{Bvh, triangles_cross};
use crate::field::{ClumpField, SurfacePoint};
use crate::mesh::{BuildStats, ClumpMesh};
use crate::patch::{Patch, Tri};
use crate::surface::Surface;

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

/// Largest angle, in radians, between a vertex's push direction and its
/// surface normal (60°).
const MAX_PUSH_TILT: f32 = std::f32::consts::FRAC_PI_3;

/// Thickest a clump may be, as a fraction of how thick the input is at that
/// point, so thin parts such as twigs get a thin dusting.
const ROOM_FILL: f32 = 0.5;

/// Most passes when repairing folds or crossings. Each pass halves the
/// height left above the floor at every vertex involved.
const MAX_REPAIR_PASSES: u32 = 24;

/// Thinning passes in a row without fewer crossings after which strict mode
/// stops thinning and cuts out what still crosses.
const MAX_STALLED_PASSES: u32 = 2;

/// Most passes when blurring push directions, to bound the cost on meshes
/// whose edges are much shorter than the thickness.
const MAX_BLUR_PASSES: u32 = 256;

/// Fraction of the gap to the opposite surface that a clump may fill, so
/// clumps on facing walls of a crevice don't pass through each other.
const GAP_FILL: f32 = 0.5;

/// Builds a layer clump's mesh from the working surface `patch`: splits its
/// triangles until no edge is longer than `edge_length`, and pushes every
/// vertex out by the clump's height there. The result is a single open sheet
/// whose edges sink below the surface.
///
/// With `closed`, the sheet also gets a buried underside and side walls, so
/// the result is watertight.
///
/// Top triangles that turn over are always repaired by lowering the clump
/// around them. With `strict`, the finished mesh is also checked for
/// triangles that pass through each other, and the clump is thinned around
/// them until none do or the pass limit is reached.
pub(crate) fn displaced_shell(
    field: &ClumpField,
    patch: Patch,
    edge_length: f32,
    max_slope: f32,
    closed: bool,
    strict: bool,
) -> (ClumpMesh, BuildStats) {
    let Patch {
        mut points,
        mut tris,
    } = patch;
    refine(field, &mut points, &mut tris, edge_length);

    // Push along normals blurred over about the clump's tallest height, so
    // pushes in tight hollows don't converge and cross.
    let neighbours = neighbour_lists(points.len(), &tris);
    let max_rise = points.iter().map(|p| p.rise).fold(0.0, f32::max);
    let tallest = field.thickness() + field.max_lump_height() + max_rise;
    // Averaging spreads about one edge per sqrt(pass), so reaching that
    // height takes (height / edge)² passes.
    let edge = mean_edge_length(&points, &tris).max(f32::MIN_POSITIVE);
    let passes = (tallest / edge).powi(2).ceil().min(MAX_BLUR_PASSES as f32) as u32;
    let blurred = smooth_directions(
        points.iter().map(|p| p.normal).collect(),
        &neighbours,
        passes,
    );

    // Keep each push direction close to its surface normal, so pushing
    // against it always goes into the input, even on thin lips.
    let dirs: Vec<Vec3> = points
        .iter()
        .zip(blurred)
        .map(|(p, d)| limit_tilt(d, p.normal))
        .collect();

    // Work out each vertex's height, staying under part of the gap to any
    // surface facing it and under part of the input's own thickness.
    let weights: Vec<f32> = points.iter().map(|p| field.weight(p)).collect();
    let mut thinned = 0;
    let mut tops: Vec<f32> = points
        .iter()
        .zip(&dirs)
        .zip(&weights)
        .map(|((p, &dir), &w)| {
            let top = layer_top(field, p, w);
            // Both limits are 0 or more, so they can't lower a top at or
            // below the surface, and a surface farther than the top's share
            // of the gap can't limit it.
            if top <= 0.0 {
                return top;
            }
            let gap = field.clearance(p.point, dir, top / GAP_FILL);
            let limit = (GAP_FILL * gap).min(ROOM_FILL * p.room);
            if top > limit {
                thinned += 1;
                return limit;
            }
            top
        })
        .collect();

    // Lower heights so no edge of the clump is steeper than `max_slope`.
    limit_slope(&points, &neighbours, &mut tops, max_slope);

    // Sink buried vertices into the input along a direction that really leads
    // inside it, and no more than part of the way through.
    let mut along = dirs.clone();
    for v in 0..points.len() {
        if tops[v] < 0.0 {
            let (dir, depth) = sink(field, &points[v], dirs[v], -tops[v]);
            along[v] = dir;
            tops[v] = -depth;
        }
    }

    // Drop triangles sunk well below the surface; they can't be seen. Keeping
    // those only slightly below stops flat triangles poking out of hollows.
    let hidden = -0.5 * field.sink();
    let kept: Vec<&Tri> = tris
        .iter()
        .filter(|t| {
            t.corners
                .iter()
                .any(|&v| weights[v as usize] > 0.0 && tops[v as usize] >= hidden)
        })
        .collect();

    // For a closed clump, place the underside below each kept vertex, never
    // more than part of the way through the input, and keep every top above
    // its underside. The minimum solid thickness shrinks on thin input, like
    // the clump itself.
    let min_solid: Vec<f32> = points
        .iter()
        .map(|p| (MIN_SOLID * field.thickness()).min(ROOM_FILL * p.room))
        .collect();
    let mut depths = vec![0.0; points.len()];
    let mut sink_dirs = dirs.clone();
    if closed {
        // Go deeper by the dip, so the underside clears input the
        // simplified surface passes over.
        let full_depth = field.sink() + BOTTOM_GAP * field.thickness();
        let mut done = vec![false; points.len()];
        for v in kept.iter().flat_map(|t| t.corners) {
            let v = v as usize;
            if std::mem::replace(&mut done[v], true) {
                continue;
            }
            let wanted = full_depth + points[v].dip;
            let (dir, depth) = sink(field, &points[v], dirs[v], wanted);
            sink_dirs[v] = dir;
            depths[v] = depth;
            if tops[v] < min_solid[v] - depth {
                tops[v] = min_solid[v] - depth;
                along[v] = if tops[v] < 0.0 { dir } else { dirs[v] };
            }
        }
    }

    let mut sheet = Sheet {
        points: &points,
        tris: kept.into_iter().copied().collect(),
        dirs: &dirs,
        along,
        sink_dirs,
        tops,
        depths,
        closed,
        min_solid,
    };

    // Lower the clump around any top triangle that turned over, until none do.
    let folded = sheet.remove_folds();

    // Build the mesh. In strict mode, thin the clump around any triangles
    // that pass through each other until thinning stops helping; then cut
    // out any that still do, and rebuild until none do. Every cut removes at
    // least one triangle, so this ends.
    let mut crossings = 0;
    let mut built = sheet.assemble(field);
    if strict {
        let mut fewest = usize::MAX;
        let mut stalled = 0;
        let mut cutting = false;
        for pass in 0.. {
            let crossing = crossing_triangles(&built.0);
            crossings = crossing.len();
            if crossings == 0 {
                break;
            }
            if crossings < fewest {
                fewest = crossings;
                stalled = 0;
            } else {
                stalled += 1;
            }
            // Once cutting starts, thinning never resumes.
            cutting |= pass >= MAX_REPAIR_PASSES || stalled >= MAX_STALLED_PASSES;
            if cutting {
                sheet.cut_out(&built.0, &built.1, &crossing);
            } else {
                sheet.relax_around(&built.0, &built.1, &crossing);
            }
            built = sheet.assemble(field);
        }
    }

    let (mut mesh, _) = built;
    remove_unused_vertices(&mut mesh);
    mesh.normals = face_normals(&mesh.positions, &mesh.indices);
    (
        mesh,
        BuildStats {
            folded,
            thinned,
            crossings,
            clumps: 0,
        },
    )
}

/// Where a vertex of an assembled clump mesh came from: a sheet vertex, and
/// whether it is that vertex's copy on the underside.
#[derive(Debug, Clone, Copy)]
struct Origin {
    point: u32,
    bottom: bool,
}

/// A clump's sheet before it becomes a mesh: the kept triangles and, per
/// vertex, its height and underside depth, which the repair passes adjust.
struct Sheet<'a> {
    points: &'a [SurfacePoint],
    tris: Vec<Tri>,
    /// Outward push directions.
    dirs: &'a [Vec3],
    /// Directions to move along at negative heights (sunk into the input).
    along: Vec<Vec3>,
    /// Directions the underside is sunk along (closed clumps only).
    sink_dirs: Vec<Vec3>,
    /// Heights above the surface; negative is sunk into it.
    tops: Vec<f32>,
    /// Underside depths below the surface (closed clumps only).
    depths: Vec<f32>,
    closed: bool,
    /// Thinnest a closed clump may be at each vertex.
    min_solid: Vec<f32>,
}

impl Sheet<'_> {
    /// Returns where top vertex `v` sits.
    fn top_position(&self, v: usize) -> Vec3 {
        let dir = if self.tops[v] < 0.0 {
            self.along[v]
        } else {
            self.dirs[v]
        };
        self.points[v].point + dir * self.tops[v]
    }

    /// Returns the height that lowering top vertex `v` heads toward: the
    /// surface, or for a closed clump, just thick enough above its underside.
    fn floor(&self, v: usize) -> f32 {
        if self.closed {
            (self.min_solid[v] - self.depths[v]).max(0.0)
        } else {
            0.0
        }
    }

    /// Moves top vertex `v` halfway toward its floor, or onto it when close.
    fn lower(&mut self, v: usize) {
        let floor = self.floor(v);
        let top = floor + (self.tops[v] - floor) * 0.5;
        self.tops[v] = if (top - floor).abs() < 1e-6 {
            floor
        } else {
            top
        };
    }

    /// Lowers the corners of every top triangle that has turned over relative
    /// to the input triangle it came from, repeating until none have or the
    /// pass limit is reached. Any still turned over after that, which are
    /// tiny slivers whose corners can't be lowered further, are removed.
    /// Returns how many were removed.
    fn remove_folds(&mut self) -> usize {
        for pass in 0..=MAX_REPAIR_PASSES {
            let folded: Vec<usize> = (0..self.tris.len())
                .filter(|&t| self.is_folded(t))
                .collect();
            if folded.is_empty() {
                return 0;
            }
            if pass == MAX_REPAIR_PASSES {
                // Leave out what lowering couldn't fix. Removing a triangle
                // moves no vertex, so no other triangle turns over.
                let mut keep = (0..self.tris.len()).map(|t| folded.binary_search(&t).is_err());
                self.tris.retain(|_| keep.next().unwrap_or(true));
                return folded.len();
            }
            let mut corners: Vec<u32> = folded.iter().flat_map(|&t| self.tris[t].corners).collect();
            corners.sort_unstable();
            corners.dedup();
            for v in corners {
                self.lower(v as usize);
            }
        }
        0
    }

    /// Returns whether top triangle `t` has turned over relative to the input
    /// triangle it came from.
    fn is_folded(&self, t: usize) -> bool {
        let tri = &self.tris[t];
        let [a, b, c] = tri.corners.map(|v| self.top_position(v as usize));
        (b - a).cross(c - a).dot(tri.source_normal) < 0.0
    }

    /// Thins the clump around the triangles in `crossing`, which index
    /// `mesh`'s triangles: lowers their top corners and raises their
    /// underside corners. `origin` maps `mesh`'s vertices back to the sheet.
    fn relax_around(&mut self, mesh: &ClumpMesh, origin: &[Origin], crossing: &[usize]) {
        let mut touched: Vec<Origin> = crossing
            .iter()
            .flat_map(|&t| {
                mesh.indices[t * 3..t * 3 + 3]
                    .iter()
                    .map(|&i| origin[i as usize])
            })
            .collect();
        touched.sort_unstable_by_key(|o| (o.point, o.bottom));
        touched.dedup_by_key(|o| (o.point, o.bottom));
        for o in touched {
            let v = o.point as usize;
            if o.bottom {
                // Bring the underside up, keeping the top above it.
                self.depths[v] *= 0.5;
                self.tops[v] = self.tops[v].max(self.min_solid[v] - self.depths[v]);
            } else {
                self.lower(v);
            }
        }
    }

    /// Removes the top triangles behind the triangles in `crossing`, which
    /// index `mesh`'s triangles, so the next build leaves a gap there. An
    /// underside triangle stands for the top triangle above it, and a wall
    /// triangle for the top triangles at its top corners. `origin` maps
    /// `mesh`'s vertices back to the sheet.
    ///
    /// `mesh` must come from [`assemble`](Self::assemble) on this sheet: its
    /// first triangles are the top triangles in order, followed, for a closed
    /// clump, by the matching underside triangles and then the walls.
    fn cut_out(&mut self, mesh: &ClumpMesh, origin: &[Origin], crossing: &[usize]) {
        let n = self.tris.len();
        let mut remove = vec![false; n];
        let mut wall_points = HashSet::new();
        for &t in crossing {
            if t < 2 * n {
                remove[t % n] = true;
            } else {
                // A wall: find the top triangles that use its top corners.
                let tops = mesh.indices[t * 3..t * 3 + 3]
                    .iter()
                    .map(|&i| origin[i as usize])
                    .filter(|o| !o.bottom)
                    .map(|o| o.point);
                wall_points.extend(tops);
            }
        }
        for (i, tri) in self.tris.iter().enumerate() {
            if tri.corners.iter().any(|c| wall_points.contains(c)) {
                remove[i] = true;
            }
        }
        let mut keep = remove.iter().map(|&r| !r);
        self.tris.retain(|_| keep.next().unwrap_or(true));
    }

    /// Builds the mesh: the top sheet and, for a closed clump, its underside
    /// and walls. Returns the mesh, which still has unused vertices, and the
    /// origin of each of its vertices.
    fn assemble(&self, field: &ClumpField) -> (ClumpMesh, Vec<Origin>) {
        let mut mesh = ClumpMesh::default();
        let mut origin = Vec::with_capacity(self.points.len());
        for v in 0..self.points.len() {
            mesh.positions.push(self.top_position(v).to_array());
            mesh.fade.push(field.fade(self.tops[v]));
            origin.push(Origin {
                point: v as u32,
                bottom: false,
            });
        }
        for tri in &self.tris {
            mesh.indices.extend_from_slice(&tri.corners);
        }
        if self.closed {
            let bottoms: Vec<Vec3> = (0..self.points.len())
                .map(|v| self.points[v].point - self.sink_dirs[v] * self.depths[v])
                .collect();
            origin.extend(close_volume(&mut mesh, &bottoms));
        }
        (mesh, origin)
    }
}

/// Returns the triangles of `mesh` that pass through another of its
/// triangles. Triangles sharing a corner position are never tested against
/// each other.
pub(crate) fn crossing_triangles(mesh: &ClumpMesh) -> Vec<usize> {
    if mesh.indices.is_empty() {
        return Vec::new();
    }

    // Give every distinct position one id, so neighbours that share a corner
    // through a copied vertex are recognized.
    let mut ids = HashMap::<[u32; 3], u32>::new();
    let welded: Vec<u32> = mesh
        .positions
        .iter()
        .map(|p| {
            let next = ids.len() as u32;
            *ids.entry(p.map(|c| (c + 0.0).to_bits())).or_insert(next)
        })
        .collect();

    // Test each triangle against the triangles whose boxes overlap it.
    let normals = vec![[0.0, 1.0, 0.0]; mesh.positions.len()];
    let surface = Surface::new(&mesh.positions, &normals, &mesh.indices)
        .expect("assembled clump meshes have valid buffers");
    let bvh = Bvh::new(&surface);
    let mut crossing = vec![false; surface.triangle_count()];
    let mut nearby = Vec::new();
    for t in 0..surface.triangle_count() {
        let corners = bvh.corners(t);
        let ids_t = surface.triangle(t).map(|i| welded[i]);
        nearby.clear();
        bvh.overlapping(
            corners[0].min(corners[1]).min(corners[2]),
            corners[0].max(corners[1]).max(corners[2]),
            &mut nearby,
        );
        for &u in &nearby {
            if u <= t
                || surface
                    .triangle(u)
                    .iter()
                    .any(|&i| ids_t.contains(&welded[i]))
            {
                continue;
            }
            if triangles_cross(corners, bvh.corners(u)) {
                crossing[t] = true;
                crossing[u] = true;
            }
        }
    }
    (0..crossing.len()).filter(|&t| crossing[t]).collect()
}

/// Turns the top sheet in `mesh` into a closed solid. Adds an underside: a
/// copy of the sheet's triangles, reversed to face inward, with each vertex
/// moved to its position in `bottoms`. Then joins the two with a wall of quads
/// along every border edge of the sheet.
///
/// `mesh` must hold one vertex per entry of `bottoms` and the sheet's
/// triangles (before unused vertices are removed). Underside vertices get a
/// fade of 0. Returns the origin of every vertex this adds, in order.
fn close_volume(mesh: &mut ClumpMesh, bottoms: &[Vec3]) -> Vec<Origin> {
    let mut top: Vec<[u32; 3]> = mesh.indices.as_chunks::<3>().0.to_vec();

    // Give each fan of triangles at a pinch vertex its own copy of the
    // vertex, so every wall edge is shared by exactly two triangles.
    let source = split_pinches(mesh, &mut top, bottoms.len());
    mesh.indices = top.iter().flatten().copied().collect();
    let mut added: Vec<Origin> = (bottoms.len()..mesh.positions.len())
        .map(|i| Origin {
            point: source(i as u32) as u32,
            bottom: false,
        })
        .collect();

    // Make one underside vertex per sheet vertex, on demand.
    let mut below: HashMap<u32, u32> = HashMap::new();
    let mut bottom_of = |v: u32, mesh: &mut ClumpMesh, added: &mut Vec<Origin>| {
        *below.entry(v).or_insert_with(|| {
            let s = source(v);
            mesh.positions.push(bottoms[s].to_array());
            mesh.fade.push(0.0);
            added.push(Origin {
                point: s as u32,
                bottom: true,
            });
            (mesh.positions.len() - 1) as u32
        })
    };

    // Underside triangles, wound the other way so they face into the input.
    for &[a, b, c] in &top {
        let [a2, b2, c2] = [a, b, c].map(|v| bottom_of(v, mesh, &mut added));
        mesh.indices.extend_from_slice(&[a2, c2, b2]);
    }

    // Find border edges: directed sheet edges whose reverse isn't in the sheet.
    let directed: HashSet<(u32, u32)> = top
        .iter()
        .flat_map(|&[a, b, c]| [(a, b), (b, c), (c, a)])
        .collect();
    let mut border: Vec<(u32, u32)> = directed
        .iter()
        .copied()
        .filter(|&(a, b)| !directed.contains(&(b, a)))
        .collect();
    border.sort_unstable();

    // Wall quad for each border edge a→b, using b→a on top and a'→b' below.
    for (a, b) in border {
        let (a2, b2) = (
            bottom_of(a, mesh, &mut added),
            bottom_of(b, mesh, &mut added),
        );
        mesh.indices.extend_from_slice(&[b, a, a2, b, a2, b2]);
    }
    added
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
    let directed: HashSet<(u32, u32)> = top
        .iter()
        .flat_map(|&[a, b, c]| [(a, b), (b, c), (c, a)])
        .collect();
    let mut outgoing: HashMap<u32, u32> = HashMap::new();
    for &(a, b) in &directed {
        if !directed.contains(&(b, a)) {
            *outgoing.entry(a).or_default() += 1;
        }
    }
    let mut pinches: Vec<u32> = outgoing
        .into_iter()
        .filter(|&(_, n)| n > 1)
        .map(|(v, _)| v)
        .collect();
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
                    top[t]
                        .iter_mut()
                        .filter(|x| **x == v)
                        .for_each(|x| *x = copy);
                }
            }
        }
    }

    // Every original vertex has a position, so copies start right after them.
    debug_assert_eq!(mesh.positions.len() - copies.len(), original_count);
    move |i: u32| {
        let i = i as usize;
        if i < original_count {
            i
        } else {
            copies[i - original_count] as usize
        }
    }
}

/// Splits triangles the clump may cover until no edge is longer than
/// `edge_length`.
///
/// Each pass halves every edge that is too long and splits the triangles
/// around it to match, so covered triangles always share their edges and the
/// clump has no cracks. Triangles the clump can't cover are left whole; they
/// are dropped later, so the unmatched split points along their edges never
/// show.
fn refine(
    field: &ClumpField,
    points: &mut Vec<SurfacePoint>,
    tris: &mut Vec<Tri>,
    edge_length: f32,
) {
    let limit = edge_length * edge_length;
    for _ in 0..MAX_PASSES {
        let too_long = |a: u32, b: u32, points: &[SurfacePoint]| {
            points[a as usize]
                .point
                .distance_squared(points[b as usize].point)
                > limit
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
                next.push(Tri {
                    corners,
                    source_normal: tri.source_normal,
                });
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
    let mut queue: BinaryHeap<Reverse<(OrderedHeight, u32)>> = (0..tops.len() as u32)
        .map(|v| Reverse((OrderedHeight(tops[v as usize]), v)))
        .collect();
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

/// Picks how to sink surface point `p` up to `wanted` below the surface.
/// Tries against the push direction `dir` first, then against the surface
/// normal, taking the first that really leads into the input; the depth is
/// capped at [`GAP_FILL`] of the way through the input, counted from where
/// the ray enters it (the point itself, unless it lies over a hollow in a
/// simplified surface). Returns the outward direction to move against and
/// the depth, which is 0 if neither direction leads into the input (a sharp
/// ridge, for example).
fn sink(field: &ClumpField, p: &SurfacePoint, dir: Vec3, wanted: f32) -> (Vec3, f32) {
    for d in [dir, p.normal] {
        if let Some((enter, leave)) = field.inside_span(p.point, -d) {
            return (d, wanted.min(enter + GAP_FILL * (leave - enter)));
        }
    }
    (dir, 0.0)
}

/// Returns the wanted top height of a layer clump at `p`, which has coverage
/// weight `w`, before any limits: the base height plus lumps, which shrink
/// with the weight. Visible tops are raised by `p`'s rise, in step with how
/// visible they are, so bumps of the input that the simplified surface cuts
/// through stay covered. Buried tops are lowered by up to its dip, so the
/// clump's edges stay under the input.
fn layer_top(field: &ClumpField, p: &SurfacePoint, w: f32) -> f32 {
    let top = field.top(w) + w * field.lump_height(p.point);
    if top > 0.0 {
        top + p.rise * field.fade(top)
    } else {
        let buried = if field.sink() > 0.0 {
            (-top / field.sink()).min(1.0)
        } else {
            1.0
        };
        top - p.dip * buried
    }
}

/// Returns unit direction `dir` turned toward unit normal `normal` just enough
/// that the angle between them is at most [`MAX_PUSH_TILT`] radians.
fn limit_tilt(dir: Vec3, normal: Vec3) -> Vec3 {
    let cos = dir.dot(normal);
    if cos >= MAX_PUSH_TILT.cos() {
        return dir;
    }
    // Rotate within the plane of the two vectors to sit at the limit.
    let side = (dir - normal * cos).normalize_or_zero();
    if side == Vec3::ZERO {
        return normal;
    }
    normal * MAX_PUSH_TILT.cos() + side * MAX_PUSH_TILT.sin()
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
pub(crate) fn remove_unused_vertices(mesh: &mut ClumpMesh) {
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
pub(crate) fn face_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for tri in indices.as_chunks::<3>().0 {
        let [a, b, c] = tri.map(|i| Vec3::from(positions[i as usize]));
        let n = (b - a).cross(c - a);
        for &i in tri {
            normals[i as usize] += n;
        }
    }
    normals
        .into_iter()
        .map(|n| n.normalize_or_zero().to_array())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pinched_sheet_closes_into_a_manifold_solid() {
        // Two flat triangles that touch only at vertex 0: a bowtie.
        let corners = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, -1.0],
            [-1.0, 0.0, 0.0],
            [-1.0, 0.0, 1.0],
        ];
        let bottoms: Vec<Vec3> = corners
            .iter()
            .map(|&[x, _, z]| Vec3::new(x, -0.05, z))
            .collect();
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
