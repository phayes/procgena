//! Ground queries on canonical meshes: the height and slope of the
//! realized ground under a point, from the chunks that are loaded.
//!
//! This sits beside the terrain's own ground (`Ground`, `ChunkSource`), not
//! in place of it. Placement, masks, slope tests and labels read the
//! terrain's own ground at every distance; these queries give the physical
//! ground near the player: what collision meets and where a nearby copy
//! comes to rest.

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::{DVec2, DVec3};

use crate::chunk::ChunkKey;

use super::contract::{ChunkEdgeKey, chunk_cm};
use super::mesh::TinChunk;
use super::numeric::{CM_PER_M, Cm};

/// Bins along each side of a chunk's triangle index.
const BINS: i64 = 8;

/// The ground under one point of a canonical mesh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroundSample {
    /// The mesh's height there, in metres.
    pub height: f64,
    /// The upward unit normal of the triangle the point lies in. It is
    /// constant across a triangle and jumps at its edges, so use it for
    /// physics, not for placement slope tests.
    pub normal: DVec3,
    /// The chunk the point belongs to.
    pub chunk: ChunkKey,
    /// The triangle the point lies in: an index into that chunk's
    /// [`TinChunk::triangles`].
    pub triangle: u32,
}

/// Something that answers where the realized ground is.
pub trait GroundSurface {
    /// The ground under world position `p` (x and z, metres), or `None`
    /// where there is no realized ground to ask.
    fn sample(&self, p: DVec2) -> Option<GroundSample>;

    /// [`sample`](Self::sample) for each of `points`, in order, with the
    /// same answers.
    fn sample_batch(&self, points: &[DVec2]) -> Vec<Option<GroundSample>> {
        points.iter().map(|&p| self.sample(p)).collect()
    }
}

/// The point `p` (metres) rounded to whole centimetres. `None` if it is not
/// finite or beyond the canonical range.
fn to_cm(p: DVec2) -> Option<(i64, i64)> {
    Some((Cm::from_m(p.x)?.0, Cm::from_m(p.y)?.0))
}

/// The chunk at `level` owning the centimetre point `(x, z)`: each chunk
/// owns its square including its low-x and low-z sides, but not its high
/// ones.
fn owner(x: i64, z: i64, level: u8) -> ChunkKey {
    let width = chunk_cm(level);
    ChunkKey::new(x.div_euclid(width), z.div_euclid(width), level)
}

/// Twice the signed area of `p`, `q`, `r` in the (x, z) plane: negative
/// when they run clockwise seen from above, which is how a canonical
/// triangle faces up.
fn orient(p: (i64, i64), q: (i64, i64), r: (i64, i64)) -> i64 {
    (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0)
}

/// One canonical chunk, ready for fast lookups of the triangle under a
/// point.
#[derive(Debug, Clone)]
pub struct TinIndex {
    chunk: Arc<TinChunk>,
    /// The chunk's smallest corner, in centimetres.
    origin: (i64, i64),
    /// The chunk's width, in centimetres.
    width: i64,
    /// Each vertex relative to `origin`, in centimetres.
    local: Vec<(i64, i64)>,
    /// Where each bin's triangles start in `items`, row by row along x,
    /// with one more entry at the end.
    starts: Vec<u32>,
    /// The triangles over each bin, in increasing order within a bin.
    items: Vec<u32>,
}

impl TinIndex {
    /// An index over `chunk`'s triangles, at any chunk level. Vertices may
    /// lie anywhere in the chunk, on its lattice or not.
    ///
    /// # Panics
    ///
    /// If a triangle refers to a missing vertex, or the chunk's level is
    /// above [`MAX_CHUNK_LEVEL`](crate::MAX_CHUNK_LEVEL).
    pub fn new(chunk: Arc<TinChunk>) -> Self {
        let (x0, z0) = ChunkEdgeKey::of_chunk(chunk.key)[0].start();
        let origin = (x0.0, z0.0);
        let width = chunk_cm(chunk.key.level);
        let bin_cm = width / BINS;
        let local: Vec<(i64, i64)> = chunk
            .vertices
            .iter()
            .map(|v| (v.x.0 - origin.0, v.z.0 - origin.1))
            .collect();

        // Each triangle goes in every bin its bounds touch, edges included.
        let bin_of = |cm: i64| (cm / bin_cm).clamp(0, BINS - 1);
        let mut bins: Vec<Vec<u32>> = vec![Vec::new(); (BINS * BINS) as usize];
        for (t, triangle) in chunk.triangles.iter().enumerate() {
            let corners = triangle.0.map(|i| local[i as usize]);
            let (lo_x, hi_x) = (
                corners.iter().map(|c| c.0).min().expect("three corners"),
                corners.iter().map(|c| c.0).max().expect("three corners"),
            );
            let (lo_z, hi_z) = (
                corners.iter().map(|c| c.1).min().expect("three corners"),
                corners.iter().map(|c| c.1).max().expect("three corners"),
            );
            // Every point of the triangle falls in a bin between these.
            for bz in bin_of(lo_z)..=bin_of(hi_z) {
                for bx in bin_of(lo_x)..=bin_of(hi_x) {
                    bins[(bz * BINS + bx) as usize].push(t as u32);
                }
            }
        }
        let mut starts = Vec::with_capacity(bins.len() + 1);
        let mut items = Vec::new();
        for bin in bins {
            starts.push(items.len() as u32);
            items.extend(bin);
        }
        starts.push(items.len() as u32);
        Self {
            chunk,
            origin,
            width,
            local,
            starts,
            items,
        }
    }

    /// The chunk this indexes.
    pub fn chunk(&self) -> &Arc<TinChunk> {
        &self.chunk
    }

    /// The triangle holding the point `p` (metres), after rounding it to
    /// whole centimetres, or `None` if the rounded point is not in this
    /// chunk's square (see [`ResidentTins`] for which chunk owns a point).
    ///
    /// A point on an edge or vertex shared by several triangles gets the
    /// one that comes first in the chunk's triangle order, so the answer
    /// never depends on how the chunk is indexed.
    pub fn locate(&self, p: DVec2) -> Option<u32> {
        let (x, z) = to_cm(p)?;
        self.locate_cm(x, z)
    }

    /// [`locate`](Self::locate) for a point already in centimetres.
    fn locate_cm(&self, x: i64, z: i64) -> Option<u32> {
        let q = (x - self.origin.0, z - self.origin.1);
        if !(0..=self.width).contains(&q.0) || !(0..=self.width).contains(&q.1) {
            return None;
        }
        let bin = |cm: i64| (cm / (self.width / BINS)).min(BINS - 1);
        let b = (bin(q.1) * BINS + bin(q.0)) as usize;
        let candidates = &self.items[self.starts[b] as usize..self.starts[b + 1] as usize];
        // Candidates are in increasing order, so the first hit is the
        // lowest index.
        candidates.iter().copied().find(|&t| {
            let [a, b, c] = self.corners(t);
            orient(a, b, q) <= 0 && orient(b, c, q) <= 0 && orient(c, a, q) <= 0
        })
    }

    /// Triangle `t`'s corners relative to the origin, in centimetres.
    fn corners(&self, t: u32) -> [(i64, i64); 3] {
        self.chunk.triangles[t as usize]
            .0
            .map(|i| self.local[i as usize])
    }

    /// The ground under `p` (metres) in this chunk, after rounding it to
    /// whole centimetres, or `None` if the rounded point is not in this
    /// chunk's square. Heights match the canonical vertices exactly at
    /// them, and agree along every edge shared by two triangles.
    pub fn sample(&self, p: DVec2) -> Option<GroundSample> {
        let (x, z) = to_cm(p)?;
        self.sample_cm(x, z)
    }

    /// [`sample`](Self::sample) for a point already in centimetres.
    fn sample_cm(&self, x: i64, z: i64) -> Option<GroundSample> {
        let t = self.locate_cm(x, z)?;
        let q = (x - self.origin.0, z - self.origin.1);
        let [a, b, c] = self.corners(t);
        let heights = self.chunk.triangles[t as usize]
            .0
            .map(|i| i128::from(self.chunk.vertices[i as usize].h.0));

        // Barycentric interpolation, exact until the one division.
        let area = i128::from(orient(a, b, c));
        let weights = [orient(b, c, q), orient(c, a, q), orient(a, b, q)].map(i128::from);
        let sum: i128 = (0..3).map(|k| weights[k] * heights[k]).sum();
        let height = sum as f64 / area as f64 / CM_PER_M;

        // The face's upward normal, from its edges in metres.
        let point = |k: usize, (u, v): (i64, i64)| {
            DVec3::new(u as f64, heights[k] as f64, v as f64) / CM_PER_M
        };
        let (pa, pb, pc) = (point(0, a), point(1, b), point(2, c));
        let normal = (pb - pa).cross(pc - pa).normalize();
        Some(GroundSample {
            height,
            normal,
            chunk: self.chunk.key,
            triangle: t,
        })
    }
}

/// The canonical chunks loaded now, at any chunk levels, answering ground
/// queries from them alone.
///
/// At each level, a point belongs to one chunk: the chunk whose square
/// holds it once rounded to whole centimetres, with its low-x and low-z
/// sides but not its high ones. A point is answered by the loaded chunk
/// it belongs to at the finest level that has one, so where loaded chunks
/// of different levels overlap, the finer one answers. If no loaded chunk
/// owns the point the answer is `None`, even on a side shared with a
/// loaded chunk. Nothing is built or sampled from the terrain to answer a
/// query.
#[derive(Debug, Clone, Default)]
pub struct ResidentTins {
    chunks: BTreeMap<ChunkKey, Arc<TinIndex>>,
    /// How many chunks are loaded at each level.
    per_level: BTreeMap<u8, usize>,
}

impl ResidentTins {
    /// No chunks.
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads `index`'s chunk, replacing any already loaded at its key.
    pub fn insert(&mut self, index: Arc<TinIndex>) {
        let key = index.chunk.key;
        if self.chunks.insert(key, index).is_none() {
            *self.per_level.entry(key.level).or_default() += 1;
        }
    }

    /// Unloads chunk `key`, returning it if it was loaded.
    pub fn remove(&mut self, key: ChunkKey) -> Option<Arc<TinIndex>> {
        let removed = self.chunks.remove(&key)?;
        let count = self.per_level.get_mut(&key.level).expect("counted");
        *count -= 1;
        if *count == 0 {
            self.per_level.remove(&key.level);
        }
        Some(removed)
    }

    /// The loaded chunk owning the centimetre point `(x, z)` at the finest
    /// level that has one.
    fn owning(&self, x: i64, z: i64) -> Option<&Arc<TinIndex>> {
        self.per_level
            .keys()
            .find_map(|&level| self.chunks.get(&owner(x, z, level)))
    }

    /// The loaded chunk `key`, if any.
    pub fn get(&self, key: ChunkKey) -> Option<&Arc<TinIndex>> {
        self.chunks.get(&key)
    }

    /// How many chunks are loaded.
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    /// Whether no chunk is loaded.
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// The loaded chunks, by key.
    pub fn iter(&self) -> impl Iterator<Item = &Arc<TinIndex>> {
        self.chunks.values()
    }
}

impl GroundSurface for ResidentTins {
    fn sample(&self, p: DVec2) -> Option<GroundSample> {
        let (x, z) = to_cm(p)?;
        self.owning(x, z)?.sample_cm(x, z)
    }

    /// With chunks of one level loaded, looks each chunk up once for each
    /// run of points in the same chunk.
    fn sample_batch(&self, points: &[DVec2]) -> Vec<Option<GroundSample>> {
        let mut levels = self.per_level.keys();
        let (Some(&level), None) = (levels.next(), levels.next()) else {
            return points.iter().map(|&p| self.sample(p)).collect();
        };
        let mut last: Option<(ChunkKey, Option<&Arc<TinIndex>>)> = None;
        points
            .iter()
            .map(|&p| {
                let (x, z) = to_cm(p)?;
                let key = owner(x, z, level);
                let index = match last {
                    Some((k, index)) if k == key => index,
                    _ => {
                        let index = self.chunks.get(&key);
                        last = Some((key, index));
                        index
                    }
                };
                index?.sample_cm(x, z)
            })
            .collect()
    }
}
