//! What is made from a canonical mesh: meshes to draw, the fit of a
//! regular neighbour's border to it, and collision geometry.
//!
//! None of these change the canonical mesh; they are derived from it.

use std::collections::HashMap;

use glam::{DVec2, DVec3, Vec3};

use crate::chunk::{ChunkKey, ChunkMesh, ChunkSide, ChunkSource, Transitions};
use crate::feature::FeatureLevel;

use super::cache::ContractCache;
use super::contract::{ChunkEdgeKey, border_normals, chunk_cm, step_cm};
use super::mesh::{TinChunk, TinVertex};
use super::numeric::{CM_PER_M, Cm};
use super::probe::TinError;
use super::tolerance::TinTolerance;

/// A chunk's canonical ground as triangles for a physics engine: the
/// canonical vertices and triangles, unchanged, in the chunk's own frame.
#[derive(Debug, Clone, PartialEq)]
pub struct TinCollider {
    /// Which chunk this is.
    pub key: ChunkKey,
    /// World x and z of the chunk's smallest corner. Place the collider at
    /// `(origin.x, 0, origin.y)`.
    pub origin: DVec2,
    /// Vertex positions relative to `origin` along x and z, and absolute
    /// along y (y up, metres).
    pub vertices: Vec<Vec3>,
    /// Triangles, three indices into `vertices` each, facing up
    /// (counter-clockwise seen from above in a right-handed frame with y
    /// up).
    pub indices: Vec<[u32; 3]>,
}

impl TinChunk {
    /// The chunk's smallest corner, in centimetres.
    fn origin_cm(&self) -> (i64, i64) {
        let (x, z) = ChunkEdgeKey::of_chunk(self.key)[0].start();
        (x.0, z.0)
    }

    /// The chunk's width, in centimetres.
    fn width_cm(&self) -> i64 {
        chunk_cm(self.key.level)
    }

    /// The collision geometry of this chunk: exactly its canonical
    /// triangles, with no skirts and no added vertices.
    pub fn collider(&self) -> TinCollider {
        let (x0, z0) = self.origin_cm();
        TinCollider {
            key: self.key,
            origin: self.key.min_corner(),
            vertices: self
                .vertices
                .iter()
                .map(|v| local_position(v.x.0 - x0, v.z.0 - z0, f64::from(v.h.0)))
                .collect(),
            indices: self.triangles.iter().map(|t| t.0).collect(),
        }
    }

    /// One upward unit normal for each of the chunk's vertices, for smooth
    /// shading, for a chunk built from `source` within
    /// [`tin_tolerance`](ChunkSource::tin_tolerance).
    ///
    /// The same as [`shading_normals_with`](Self::shading_normals_with)
    /// with a fresh contract cache, so it builds the chunk's four edge
    /// contracts again. Use `shading_normals_with` and the cache the chunk
    /// was built with to read no ground at all.
    ///
    /// # Errors
    ///
    /// As [`shading_normals_with`](Self::shading_normals_with).
    pub fn shading_normals(&self, source: &dyn ChunkSource) -> Result<Vec<Vec3>, TinError> {
        let tolerance = source.tin_tolerance();
        self.shading_normals_with(source, &tolerance, &ContractCache::new(&tolerance))
    }

    /// One upward unit normal for each of the chunk's vertices, for smooth
    /// shading. Pass the `source`, base `tolerance` and `contracts` the
    /// chunk was built with (see
    /// [`build_chunk_with`](super::build_chunk_with)).
    ///
    /// A vertex on the chunk's border takes the normal its edge contract
    /// carries ([`EdgeContract::normals`](super::EdgeContract::normals)):
    /// the ground's slope one lattice step either side along x and z, which
    /// at level 0 is how a regular chunk shades its vertex. The chunks
    /// either side of a border share the contract, so their shading meets
    /// without a seam. A vertex inside the chunk takes the average of the
    /// normals of the triangles around it, weighted by their area. Nothing
    /// reads the ground unless `contracts` lacks one of the chunk's edges,
    /// which is then built, or the chunk has a border vertex no contract
    /// holds (it was triangulated from other contracts), whose normal is
    /// then read from the ground the same way. These normals are for
    /// drawing only.
    ///
    /// # Errors
    ///
    /// [`TinError::NotFinest`] if `source` can't give the finest level, and
    /// [`TinError::NoGround`] if the ground needed is missing.
    ///
    /// # Panics
    ///
    /// If `contracts` was made for another tolerance.
    pub fn shading_normals_with(
        &self,
        source: &dyn ChunkSource,
        tolerance: &TinTolerance,
        contracts: &ContractCache,
    ) -> Result<Vec<Vec3>, TinError> {
        let level = source.level(FeatureLevel::FINEST);
        if level != FeatureLevel::FINEST {
            return Err(TinError::NotFinest(level));
        }

        // Inside: the area-weighted normals of the triangles round each
        // vertex (each triangle's cross product is twice its area long).
        let (x0, z0) = self.origin_cm();
        let at = |i: u32| {
            let v = self.vertices[i as usize];
            DVec3::new(
                (v.x.0 - x0) as f64 / CM_PER_M,
                f64::from(v.h.0) / CM_PER_M,
                (v.z.0 - z0) as f64 / CM_PER_M,
            )
        };
        let mut sums = vec![DVec3::ZERO; self.vertices.len()];
        for t in &self.triangles {
            let [a, b, c] = t.0;
            let n = (at(b) - at(a)).cross(at(c) - at(a));
            let n = if n.y < 0.0 { -n } else { n };
            for i in t.0 {
                sums[i as usize] += n;
            }
        }
        let mut normals: Vec<Vec3> = sums
            .iter()
            .map(|n| n.normalize_or(DVec3::Y).as_vec3())
            .collect();

        // On the border: the normals the edge contracts carry.
        let mut carried: HashMap<(Cm, Cm), Vec3> = HashMap::new();
        for edge in ChunkEdgeKey::of_chunk(self.key) {
            let built = contracts.get_or_build(source, edge, tolerance)?;
            let contract = &built.0;
            for (v, &n) in contract.vertices.iter().zip(&contract.normals) {
                carried.insert((v.x, v.z), n);
            }
        }
        let width = self.width_cm();
        let on_border = |v: &TinVertex| {
            v.x.0 == x0 || v.x.0 == x0 + width || v.z.0 == z0 || v.z.0 == z0 + width
        };
        let mut missing: Vec<usize> = Vec::new();
        for (i, (v, normal)) in self.vertices.iter().zip(&mut normals).enumerate() {
            if on_border(v) {
                match carried.get(&(v.x, v.z)) {
                    Some(&n) => *normal = n,
                    None => missing.push(i),
                }
            }
        }

        // A border vertex no contract holds (a chunk triangulated from
        // other contracts): the same normal, from the ground.
        let points: Vec<(Cm, Cm)> = missing
            .iter()
            .map(|&i| (self.vertices[i].x, self.vertices[i].z))
            .collect();
        for (&i, n) in missing
            .iter()
            .zip(border_normals(source, &points, self.key.level)?)
        {
            normals[i] = n;
        }
        Ok(normals)
    }

    /// The height, in metres, of the mesh's border at the point `(x, z)`,
    /// or `None` if the point is not on the chunk's border. Along a side
    /// it is the straight line between the two border vertices either side
    /// of the point.
    ///
    /// [`render_mesh`](Self::render_mesh) and
    /// [`fit_neighbour`](Self::fit_neighbour) both use these heights, so
    /// they give the same `f32` at the same point.
    pub fn border_height(&self, x: Cm, z: Cm) -> Option<f64> {
        let (x0, z0) = self.origin_cm();
        let (u, v) = (x.0 - x0, z.0 - z0);
        let width = self.width_cm();
        let on = |side: ChunkSide| {
            let (across, along) = side_coordinates(side, u, v);
            (across == side_offset(side, width) && (0..=width).contains(&along)).then_some(along)
        };
        ChunkSide::ALL
            .into_iter()
            .find_map(|side| Some((side, on(side)?)))
            .map(|(side, along)| polyline_height(&self.side_line(side), along) / CM_PER_M)
    }

    /// The border vertices on `side`, as (centimetres along the side from
    /// its low end, height in centimetres), in order along the side.
    fn side_line(&self, side: ChunkSide) -> Vec<(i64, i32)> {
        let (x0, z0) = self.origin_cm();
        let width = self.width_cm();
        let mut line: Vec<(i64, i32)> = self
            .vertices
            .iter()
            .filter_map(|v| {
                let (across, along) = side_coordinates(side, v.x.0 - x0, v.z.0 - z0);
                (across == side_offset(side, width)).then_some((along, v.h.0))
            })
            .collect();
        line.sort_unstable();
        line
    }

    /// A mesh of this chunk's canonical triangles to draw, in the same frame
    /// as the regular chunks' meshes (see [`ChunkMesh`]), shaded with
    /// `normals` (one per vertex, as [`shading_normals`](Self::shading_normals)
    /// gives).
    ///
    /// Each side in `split` gets a vertex at every lattice step along it
    /// (every whole metre at level 0, every 2^L metres at level `L`, where
    /// a regular chunk of the same level has its vertices), at the border's
    /// own height there ([`border_height`](Self::border_height)),
    /// so a regular neighbour fitted with
    /// [`fit_neighbour`](Self::fit_neighbour) shares every vertex along
    /// that side. The surface is the canonical one either way. With `skirt`
    /// above zero, each side's edge is also copied `skirt` metres lower and
    /// joined to it, to hide gaps against neighbours read at another feature level.
    ///
    /// The mesh has no water ([`ChunkMesh::water`] is `None`).
    ///
    /// # Panics
    ///
    /// If `normals` doesn't have one normal for each vertex.
    pub fn render_mesh(&self, normals: &[Vec3], split: &[ChunkSide], skirt: f32) -> ChunkMesh {
        assert_eq!(
            normals.len(),
            self.vertices.len(),
            "one normal for each vertex"
        );
        let (x0, z0) = self.origin_cm();
        let (width, spacing) = (self.width_cm(), step_cm(self.key.level));
        let mut mesh = ChunkMesh {
            key: self.key,
            transitions: Transitions::NONE,
            level: FeatureLevel::FINEST,
            origin: self.key.min_corner(),
            positions: Vec::with_capacity(self.vertices.len()),
            normals: normals.to_vec(),
            indices: Vec::with_capacity(self.triangles.len() * 3),
            water: None,
            heights: (f32::INFINITY, f32::NEG_INFINITY),
        };
        let local: Vec<(i64, i64)> = self
            .vertices
            .iter()
            .map(|v| (v.x.0 - x0, v.z.0 - z0))
            .collect();
        for (v, &(u, w)) in self.vertices.iter().zip(&local) {
            mesh.positions.push(local_position(u, w, f64::from(v.h.0)));
        }

        // The vertices along each side, as (along, index), for the skirts.
        let mut sides: [Vec<(i64, u32)>; 4] = Default::default();
        for (i, &(u, w)) in local.iter().enumerate() {
            for (s, side) in ChunkSide::ALL.into_iter().enumerate() {
                let (across, along) = side_coordinates(side, u, w);
                if across == side_offset(side, width) {
                    sides[s].push((along, i as u32));
                }
            }
        }

        // Triangles, each with its split edges' extra vertices fanned in.
        let lines: Vec<(ChunkSide, Vec<(i64, i32)>)> = split
            .iter()
            .map(|&side| (side, self.side_line(side)))
            .collect();
        for triangle in &self.triangles {
            let corners = triangle.0;
            let mut ring: Vec<u32> = Vec::with_capacity(3);
            // How many edges are split, and the corner the last one starts
            // from.
            let mut split_edges = 0;
            let mut split_from = 0;
            for k in 0..3 {
                let (a, b) = (corners[k], corners[(k + 1) % 3]);
                ring.push(a);
                let (pa, pb) = (local[a as usize], local[b as usize]);
                let Some((side, line)) = lines.iter().find(|(side, _)| {
                    let offset = side_offset(*side, width);
                    side_coordinates(*side, pa.0, pa.1).0 == offset
                        && side_coordinates(*side, pb.0, pb.1).0 == offset
                }) else {
                    continue;
                };
                let (ta, tb) = (
                    side_coordinates(*side, pa.0, pa.1).1,
                    side_coordinates(*side, pb.0, pb.1).1,
                );
                let between = steps_between(ta, tb, spacing);
                if between.is_empty() {
                    continue;
                }
                split_edges += 1;
                split_from = k;
                let s = ChunkSide::ALL
                    .iter()
                    .position(|x| x == side)
                    .expect("a side");
                for along in between {
                    let t = (along - ta) as f32 / (tb - ta) as f32;
                    let normal = normals[a as usize].lerp(normals[b as usize], t).normalize();
                    let (u, w) = side_point(*side, along, width);
                    let index = mesh.positions.len() as u32;
                    mesh.positions
                        .push(local_position(u, w, polyline_height(line, along)));
                    mesh.normals.push(normal);
                    sides[s].push((along, index));
                    ring.push(index);
                }
            }
            match split_edges {
                0 => mesh.indices.extend(corners),
                // Fan from the corner opposite the split edge, which is
                // never in line with it.
                1 => {
                    let opposite = corners[(split_from + 2) % 3];
                    let start = ring
                        .iter()
                        .position(|&i| i == opposite)
                        .expect("every corner is in the ring");
                    let hub = opposite;
                    for k in 1..ring.len() - 1 {
                        let (p, q) = (
                            ring[(start + k) % ring.len()],
                            ring[(start + k + 1) % ring.len()],
                        );
                        mesh.indices.extend([hub, p, q]);
                    }
                }
                // Two split edges meet at a chunk corner: fan from the
                // triangle's centre.
                _ => {
                    let centre = corners
                        .iter()
                        .map(|&i| mesh.positions[i as usize])
                        .sum::<Vec3>()
                        / 3.0;
                    let normal = corners
                        .iter()
                        .map(|&i| normals[i as usize])
                        .sum::<Vec3>()
                        .normalize();
                    let hub = mesh.positions.len() as u32;
                    mesh.positions.push(centre);
                    mesh.normals.push(normal);
                    for k in 0..ring.len() {
                        mesh.indices
                            .extend([hub, ring[k], ring[(k + 1) % ring.len()]]);
                    }
                }
            }
        }
        for p in &mesh.positions {
            mesh.heights.0 = mesh.heights.0.min(p.y);
            mesh.heights.1 = mesh.heights.1.max(p.y);
        }

        // Skirts: each side's edge, copied `skirt` metres lower.
        if skirt > 0.0 {
            for (s, side) in ChunkSide::ALL.into_iter().enumerate() {
                sides[s].sort_unstable();
                add_skirt(&mut mesh, side, &sides[s], skirt);
            }
            mesh.heights.0 -= skirt;
        }
        mesh
    }

    /// Moves the vertices of `mesh`, a regular chunk's mesh, that lie on
    /// this chunk's border onto it, so the two meet without a gap: each
    /// such vertex gets [`border_height`](Self::border_height) there. Every
    /// vertex of `mesh` at the same position moves by the same amount, so a
    /// skirt hanging there moves with its edge. Returns how many positions
    /// moved.
    ///
    /// Call it for each canonical chunk beside `mesh`, and draw this chunk
    /// with the shared side split (see [`render_mesh`](Self::render_mesh))
    /// so both have the same vertices along it. Vertices are matched to
    /// the border by their position rounded to whole centimetres.
    pub fn fit_neighbour(&self, mesh: &mut ChunkMesh) -> usize {
        // The highest vertex at each border position is the edge itself;
        // any below it hang from it.
        let mut tops: HashMap<(i64, i64), f32> = HashMap::new();
        let mut at: Vec<Option<(i64, i64)>> = Vec::with_capacity(mesh.positions.len());
        for p in &mesh.positions {
            let x = Cm::from_m(mesh.origin.x + f64::from(p.x));
            let z = Cm::from_m(mesh.origin.y + f64::from(p.z));
            let key = x
                .zip(z)
                .filter(|&(x, z)| self.border_height(x, z).is_some());
            if let Some((x, z)) = key {
                let top = tops.entry((x.0, z.0)).or_insert(f32::NEG_INFINITY);
                *top = top.max(p.y);
            }
            at.push(key.map(|(x, z)| (x.0, z.0)));
        }
        // The edge vertex takes the border height exactly; those below it
        // move by as much.
        let targets: HashMap<(i64, i64), (f32, f32)> = tops
            .into_iter()
            .map(|((x, z), top)| {
                let target = self
                    .border_height(Cm(x), Cm(z))
                    .expect("found on the border") as f32;
                ((x, z), (top, target))
            })
            .collect();
        for (p, key) in mesh.positions.iter_mut().zip(&at) {
            if let Some(key) = key {
                let (top, target) = targets[key];
                p.y = if p.y == top {
                    target
                } else {
                    p.y + (target - top)
                };
            }
        }

        // The mesh's height range, again.
        mesh.heights = (f32::INFINITY, f32::NEG_INFINITY);
        let water = mesh.water.iter().flat_map(|w| &w.positions);
        for p in mesh.positions.iter().chain(water) {
            mesh.heights.0 = mesh.heights.0.min(p.y);
            mesh.heights.1 = mesh.heights.1.max(p.y);
        }
        targets.len()
    }
}

/// A vertex position in a chunk's frame: `u` and `w` centimetres along x
/// and z from its corner, at height `h_cm` centimetres.
fn local_position(u: i64, w: i64, h_cm: f64) -> Vec3 {
    Vec3::new(
        (u as f64 / CM_PER_M) as f32,
        (h_cm / CM_PER_M) as f32,
        (w as f64 / CM_PER_M) as f32,
    )
}

/// The point `(u, w)` (centimetres from the chunk's corner) as
/// (centimetres across `side`'s line, centimetres along it).
fn side_coordinates(side: ChunkSide, u: i64, w: i64) -> (i64, i64) {
    match side {
        ChunkSide::MinX | ChunkSide::MaxX => (u, w),
        ChunkSide::MinZ | ChunkSide::MaxZ => (w, u),
    }
}

/// Where `side`'s line is across it, in centimetres from the corner of a
/// chunk `width` centimetres wide.
fn side_offset(side: ChunkSide, width: i64) -> i64 {
    match side {
        ChunkSide::MinX | ChunkSide::MinZ => 0,
        ChunkSide::MaxX | ChunkSide::MaxZ => width,
    }
}

/// The point `along` centimetres along `side`, as `(u, w)` centimetres from
/// the chunk's corner.
fn side_point(side: ChunkSide, along: i64, width: i64) -> (i64, i64) {
    match side {
        ChunkSide::MinX | ChunkSide::MaxX => (side_offset(side, width), along),
        ChunkSide::MinZ | ChunkSide::MaxZ => (along, side_offset(side, width)),
    }
}

/// The multiples of `spacing` strictly between `a` and `b` (all in
/// centimetres), in order from `a` to `b`.
fn steps_between(a: i64, b: i64, spacing: i64) -> Vec<i64> {
    let (lo, hi) = (a.min(b), a.max(b));
    let first = lo.div_euclid(spacing) * spacing + spacing;
    let mut points: Vec<i64> = (first..hi).step_by(spacing as usize).collect();
    if a > b {
        points.reverse();
    }
    points
}

/// The height in centimetres of the polyline `line` (sorted (along,
/// height) pairs) at `along`, which must lie within it.
fn polyline_height(line: &[(i64, i32)], along: i64) -> f64 {
    let after = line.partition_point(|&(t, _)| t < along);
    let (tb, hb) = line[after.min(line.len() - 1)];
    if tb == along || after == 0 {
        return f64::from(hb);
    }
    let (ta, ha) = line[after - 1];
    (f64::from(ha) * (tb - along) as f64 + f64::from(hb) * (along - ta) as f64) / (tb - ta) as f64
}

/// Adds a skirt to `mesh` along `side`: the vertices `edge` (sorted along
/// the side) copied `depth` metres lower, joined to them by quads drawn
/// with both windings, so they face out of the chunk and into it.
fn add_skirt(mesh: &mut ChunkMesh, side: ChunkSide, edge: &[(i64, u32)], depth: f32) {
    let outward = {
        let (dx, dz) = side.direction();
        Vec3::new(dx as f32, 0.0, dz as f32)
    };
    let top: Vec<u32> = edge.iter().map(|&(_, i)| i).collect();
    let bottom: Vec<u32> = top
        .iter()
        .map(|&t| {
            let p = mesh.positions[t as usize] - Vec3::Y * depth;
            mesh.positions.push(p);
            mesh.normals.push(mesh.normals[t as usize]);
            (mesh.positions.len() - 1) as u32
        })
        .collect();
    for w in 0..top.len().saturating_sub(1) {
        let (a, b) = (top[w], top[w + 1]);
        let (c, d) = (bottom[w + 1], bottom[w]);
        let along = mesh.positions[b as usize] - mesh.positions[a as usize];
        // Both windings: a skirt can be seen from inside the chunk too,
        // where steep ground beside a coarser neighbour leaves a gap.
        let (out, back) = ([a, d, c, a, c, b], [a, c, d, a, b, c]);
        if along.cross(-Vec3::Y).dot(outward) > 0.0 {
            mesh.indices.extend(out.into_iter().chain(back));
        } else {
            mesh.indices.extend(back.into_iter().chain(out));
        }
    }
}
