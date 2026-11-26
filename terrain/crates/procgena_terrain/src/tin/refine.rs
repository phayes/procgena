//! Building a chunk's canonical mesh: its edge contracts, then vertices
//! added where the mesh misses the ground most, until it is close enough.

use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashSet};

use crate::chunk::{CHUNK_SIZE, ChunkKey, ChunkSource};

use super::cdt::ChunkCdt;
use super::contract::{ChunkEdgeKey, EdgeContract};
use super::mesh::{TinChunk, TinVertex, UnmetTolerance};
use super::numeric::{MM_PER_M, Mm, MmHeight};
use super::probe::{ProbeAddr, TinError, sample_heights};
use super::tolerance::TinTolerance;

/// How far apart the ground is sampled across a chunk, in millimetres:
/// the lattice every vertex sits on. The same spacing as along chunk edges.
const LATTICE_MM: i64 = 1000;

/// Lattice points along a chunk's side, both corners included.
const SIDE: usize = (CHUNK_SIZE as i64 * MM_PER_M as i64 / LATTICE_MM) as usize + 1;

/// The canonical mesh of finest-level chunk `key` over the ground of
/// `source` at the fine tier, within `tolerance`.
///
/// The ground is sampled on a 1 m lattice over the chunk. The border is
/// locked to the four [`EdgeContract`]s, so the neighbours' meshes meet it
/// exactly. Then, while some triangle misses a lattice sample by more than
/// `tolerance.height`, or has an edge longer than `tolerance.max_edge`,
/// the worst triangle gets a vertex at its worst sample. Where the vertex
/// budget runs out first, the triangles still missing are listed in
/// [`TinChunk::unmet`]. The same chunk always gets the same mesh.
///
/// # Panics
///
/// If `key` is not at level 0, or `tolerance` is not valid.
pub fn build_chunk(
    source: &dyn ChunkSource,
    key: ChunkKey,
    tolerance: &TinTolerance,
) -> Result<TinChunk, TinError> {
    assert_eq!(key.level, 0, "canonical meshes are for level 0 chunks");
    assert!(tolerance.is_valid(), "invalid tolerance {tolerance:?}");
    let edges = ChunkEdgeKey::of_chunk(key);
    let contracts = edges
        .iter()
        .map(|&edge| EdgeContract::build(source, edge, tolerance))
        .collect::<Result<Vec<_>, _>>()?;
    let lattice = Lattice::sample(source, key)?;
    let contracts = [&contracts[0], &contracts[1], &contracts[2], &contracts[3]];
    Ok(refine(ChunkCdt::new(key, contracts), &lattice, tolerance))
}

/// The ground sampled on the 1 m lattice over one chunk.
pub(crate) struct Lattice {
    /// The chunk's smallest corner, in millimetres.
    origin: (i64, i64),
    /// Heights, row by row along x, `SIDE` by `SIDE`.
    heights: Vec<MmHeight>,
}

impl Lattice {
    /// Samples the ground of `source` over chunk `key`.
    pub(crate) fn sample(source: &dyn ChunkSource, key: ChunkKey) -> Result<Self, TinError> {
        let (x0, z0) = ChunkEdgeKey::of_chunk(key)[0].start();
        let probes: Vec<ProbeAddr> = (0..SIDE * SIDE)
            .map(|i| {
                let (col, row) = ((i % SIDE) as i64, (i / SIDE) as i64);
                ProbeAddr::new(Mm(x0.0 + col * LATTICE_MM), Mm(z0.0 + row * LATTICE_MM))
            })
            .collect();
        Ok(Self {
            origin: (x0.0, z0.0),
            heights: sample_heights(source, &probes)?,
        })
    }

    /// The vertex at lattice column `col` and row `row`.
    fn vertex(&self, col: usize, row: usize) -> TinVertex {
        TinVertex {
            x: Mm(self.origin.0 + col as i64 * LATTICE_MM),
            z: Mm(self.origin.1 + row as i64 * LATTICE_MM),
            h: self.heights[row * SIDE + col],
        }
    }
}

/// A triangle waiting to be split: worse first.
#[derive(PartialEq, Eq)]
struct Candidate {
    /// Whether an edge is longer than the tolerance allows.
    too_long: bool,
    /// How far the triangle misses its worst sample, in millimetres.
    error: i64,
    /// The triangle's corners by position (sorted by z, then x): ties go
    /// to the smallest.
    position: Reverse<[(Mm, Mm); 3]>,
    /// The triangle, anticlockwise in (x, z).
    face: [u32; 3],
    /// Where its worst sample is: lattice column and row.
    worst: (usize, usize),
}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.too_long, self.error, &self.position).cmp(&(
            other.too_long,
            other.error,
            &other.position,
        ))
    }
}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Adds vertices to `chunk` (its border already locked) where it misses
/// `lattice` most, within `tolerance`, and returns the canonical mesh.
pub(crate) fn refine(mut chunk: ChunkCdt, lattice: &Lattice, tolerance: &TinTolerance) -> TinChunk {
    let height = (tolerance.height * MM_PER_M) as i64;
    let max_edge = tolerance.max_edge * MM_PER_M;
    let mut taken: HashSet<(usize, usize)> = HashSet::new();
    for v in chunk.vertices() {
        taken.insert(lattice_of(lattice, *v));
    }

    // Every triangle, scored.
    let mut queue = BinaryHeap::new();
    let faces: Vec<[u32; 3]> = chunk.faces().collect();
    for face in faces {
        if let Some(c) = score(&chunk, lattice, &taken, face, height, max_edge) {
            queue.push(c);
        }
    }

    // Split the worst triangle while any misses; the triangles around
    // each new vertex replace those it split.
    let mut unmet = Vec::new();
    while let Some(c) = queue.pop() {
        if !chunk.has_face(c.face) {
            continue;
        }
        if chunk.vertices().len() >= tolerance.vertex_budget {
            // Out of vertices: list every triangle still missing.
            unmet.push(c);
            unmet.extend(queue.drain().filter(|c| chunk.has_face(c.face)));
            break;
        }
        taken.insert(c.worst);
        let index = chunk.insert(lattice.vertex(c.worst.0, c.worst.1));
        for face in chunk.faces_around(index) {
            if let Some(c) = score(&chunk, lattice, &taken, face, height, max_edge) {
                queue.push(c);
            }
        }
    }
    let unmet = unmet
        .into_iter()
        .filter(|c| c.error > height)
        .map(|c| {
            let v = lattice.vertex(c.worst.0, c.worst.1);
            UnmetTolerance {
                at: (v.x, v.z),
                error: MmHeight(i32::try_from(c.error).unwrap_or(i32::MAX)),
            }
        })
        .collect();
    chunk.to_chunk(unmet)
}

/// Lattice column and row of `v`, which must lie on the lattice.
fn lattice_of(lattice: &Lattice, v: TinVertex) -> (usize, usize) {
    (
        ((v.x.0 - lattice.origin.0) / LATTICE_MM) as usize,
        ((v.z.0 - lattice.origin.1) / LATTICE_MM) as usize,
    )
}

/// Triangle `face` scored for splitting, or `None` if it needs no split
/// (it is within `height` millimetres of every sample inside it, and no
/// edge is longer than `max_edge` millimetres) or has no free sample inside
/// to split at.
///
/// Samples on the chunk's border or at a vertex already in the mesh
/// (`taken`) are left out. The worst sample is the one the triangle misses
/// most; on a tie, the one nearest the triangle's centre, then the first
/// by row and column.
fn score(
    chunk: &ChunkCdt,
    lattice: &Lattice,
    taken: &HashSet<(usize, usize)>,
    face: [u32; 3],
    height: i64,
    max_edge: f64,
) -> Option<Candidate> {
    let corners = face.map(|i| chunk.vertices()[i as usize]);
    let [a, b, c] = corners
        .map(|v| lattice_of(lattice, v))
        .map(|(x, z)| (x as i64, z as i64));
    let too_long = [(a, b), (b, c), (c, a)].iter().any(|&(p, q)| {
        let (dx, dz) = (p.0 - q.0, p.1 - q.1);
        ((dx * dx + dz * dz) as f64).sqrt() * LATTICE_MM as f64 > max_edge
    });

    // The samples inside the triangle (or on its edges), in lattice units,
    // where the orientation tests are exact.
    let orient = |p: (i64, i64), q: (i64, i64), r: (i64, i64)| {
        (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0)
    };
    let area = orient(a, b, c);
    if area <= 0 {
        return None;
    }
    let heights = corners.map(|v| i128::from(v.h.0));
    let sum = (a.0 + b.0 + c.0, a.1 + b.1 + c.1);
    let last = (SIDE - 1) as i64;
    let (lo_x, hi_x) = (
        a.0.min(b.0).min(c.0).max(1),
        a.0.max(b.0).max(c.0).min(last - 1),
    );
    let (lo_z, hi_z) = (
        a.1.min(b.1).min(c.1).max(1),
        a.1.max(b.1).max(c.1).min(last - 1),
    );
    let mut best: Option<(i64, i64, (usize, usize))> = None;
    for z in lo_z..=hi_z {
        for x in lo_x..=hi_x {
            let p = (x, z);
            let (wa, wb, wc) = (orient(b, c, p), orient(c, a, p), orient(a, b, p));
            if wa < 0 || wb < 0 || wc < 0 {
                continue;
            }
            let at = (x as usize, z as usize);
            if taken.contains(&at) {
                continue;
            }
            // How far the flat triangle misses the sample, exactly, in
            // whole millimetres (rounded down).
            let flat = i128::from(wa) * heights[0]
                + i128::from(wb) * heights[1]
                + i128::from(wc) * heights[2];
            let sample = i128::from(lattice.heights[at.1 * SIDE + at.0].0) * i128::from(area);
            let error = ((sample - flat).abs() / i128::from(area)) as i64;
            // Nine times the squared distance to the centre.
            let (cx, cz) = (3 * x - sum.0, 3 * z - sum.1);
            let off_centre = cx * cx + cz * cz;
            let better = best.is_none_or(|(e, d, _)| error > e || (error == e && off_centre < d));
            if better {
                best = Some((error, off_centre, at));
            }
        }
    }
    let (error, _, worst) = best?;
    if error <= height && !too_long {
        return None;
    }
    let mut position = corners.map(|v| (v.z, v.x));
    position.sort();
    Some(Candidate {
        too_long,
        error,
        position: Reverse(position.map(|(z, x)| (x, z))),
        face,
        worst,
    })
}
