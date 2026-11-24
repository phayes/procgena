//! The integer placement contract every Scatter shares: World fixed-point
//! positions, a lattice of candidate cells fixed to the World, ranks, the
//! density test, bounded spacing (`K_d`) and the chunk grid.
//!
//! Every decision about keeping a candidate is made in integers, so it
//! gives the same answer on any machine, in any build order, and (once a
//! GPU path exists) on the GPU too.

use std::cmp::Ordering;
use std::collections::HashMap;

use glam::{DVec2, Vec2};

use super::ScatterArea;
use crate::address::CellId;
use crate::hash::{pcg, pcg2};

/// Fixed-point units per metre: positions are whole 1/256ths of a metre.
pub const FIXED_PER_METRE: i64 = 256;

/// The number of spacing rounds a Scatter uses unless it says otherwise.
/// See [`keep_spaced`].
pub const DEFAULT_SPACING_ROUNDS: u32 = 5;

/// Salts that keep a candidate's draws apart. A Scatter that needs more
/// draws per candidate uses [`draw`] with salts of its own.
const SALT_JITTER_X: u32 = 0x68e3_1da4;
const SALT_JITTER_Z: u32 = 0xb529_7a4d;
const SALT_RANK: u32 = 0x1b56_c4e9;

/// The largest per-cell chance: 1 in 24-bit fixed point.
pub const CHANCE_ONE: u32 = 1 << 24;

/// Converts metres to whole fixed-point units, rounding to the nearest.
pub fn to_fixed(metres: f64) -> i64 {
    (metres * FIXED_PER_METRE as f64).round() as i64
}

/// Converts fixed-point units to metres.
pub fn to_metres(fixed: i64) -> f64 {
    fixed as f64 / FIXED_PER_METRE as f64
}

/// A World position in fixed point: whole 1/256ths of a metre along X and
/// Z.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Fixed2 {
    /// Units along X.
    pub x: i64,
    /// Units along Z.
    pub z: i64,
}

impl Fixed2 {
    /// The position `x`, `z` in fixed-point units.
    pub const fn new(x: i64, z: i64) -> Self {
        Self { x, z }
    }

    /// The fixed-point position nearest to World position `p` (X, then Z,
    /// in metres).
    pub fn from_world(p: DVec2) -> Self {
        Self::new(to_fixed(p.x), to_fixed(p.y))
    }

    /// The World position in metres (X, then Z).
    pub fn to_world(self) -> DVec2 {
        DVec2::new(to_metres(self.x), to_metres(self.z))
    }

    /// This position relative to World position `origin`, in metres.
    pub fn relative_to(self, origin: DVec2) -> Vec2 {
        (self.to_world() - origin).as_vec2()
    }

    /// The squared distance to `other`, in squared fixed-point units.
    pub fn distance_squared(self, other: Self) -> i128 {
        let dx = i128::from(self.x - other.x);
        let dz = i128::from(self.z - other.z);
        dx * dx + dz * dz
    }
}

/// A rectangle of the World in fixed point, holding the points from `min`
/// (included) up to `max` (left out).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FixedRect {
    /// The corner with the smallest X and Z, inside the rectangle.
    pub min: Fixed2,
    /// The corner with the largest X and Z, just outside the rectangle.
    pub max: Fixed2,
}

impl FixedRect {
    /// The rectangle from `min` (included) to `max` (left out).
    pub const fn new(min: Fixed2, max: Fixed2) -> Self {
        Self { min, max }
    }

    /// Whether `p` lies inside: at least `min` and below `max` on both axes.
    pub fn contains(&self, p: Fixed2) -> bool {
        p.x >= self.min.x && p.x < self.max.x && p.z >= self.min.z && p.z < self.max.z
    }

    /// Whether the rectangle holds no points.
    pub fn is_empty(&self) -> bool {
        self.max.x <= self.min.x || self.max.z <= self.min.z
    }

    /// The rectangle grown by `by` units on every side (shrunk when `by` is
    /// negative).
    pub fn grow(&self, by: i64) -> Self {
        Self::new(
            Fixed2::new(self.min.x - by, self.min.z - by),
            Fixed2::new(self.max.x + by, self.max.z + by),
        )
    }

    /// The same rectangle as a [`ScatterArea`], whose
    /// [`bounds`](ScatterArea::bounds) give this rectangle back.
    pub fn area(&self) -> ScatterArea {
        let centre = DVec2::new(
            to_metres(self.min.x + self.max.x) * 0.5,
            to_metres(self.min.z + self.max.z) * 0.5,
        );
        let size = Vec2::new(
            to_metres(self.max.x - self.min.x) as f32,
            to_metres(self.max.z - self.min.z) as f32,
        );
        ScatterArea::new(centre, size)
    }
}

/// A Scatter's candidate cells: a square grid fixed to the World, one
/// candidate per cell, each standing somewhere inside its cell.
///
/// The cell size is a whole number of fixed-point units. Changing it moves
/// every cell, and so every copy and every copy address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Lattice {
    cell: i64,
}

impl Lattice {
    /// A lattice whose cells are `metres` wide, rounded to the nearest
    /// 1/256 m and at least that.
    pub fn new(metres: f32) -> Self {
        Self::from_fixed(to_fixed(f64::from(metres)))
    }

    /// A lattice whose cells are `units` fixed-point units wide, at least 1.
    pub fn from_fixed(units: i64) -> Self {
        Self { cell: units.max(1) }
    }

    /// The width of a cell in fixed-point units.
    pub fn cell_fixed(&self) -> i64 {
        self.cell
    }

    /// The width of a cell in metres.
    pub fn cell_metres(&self) -> f32 {
        to_metres(self.cell) as f32
    }

    /// The World position of `cell`'s corner with the smallest X and Z. The
    /// chunk holding this corner owns the cell's copies.
    pub fn corner(&self, cell: CellId) -> Fixed2 {
        Fixed2::new(
            i64::from(cell.x()) * self.cell,
            i64::from(cell.z()) * self.cell,
        )
    }

    /// The cell holding World position `p`.
    pub fn cell_of(&self, p: Fixed2) -> CellId {
        CellId::new(
            p.x.div_euclid(self.cell) as i32,
            p.z.div_euclid(self.cell) as i32,
        )
    }

    /// Every cell whose corner lies inside `rect`, row by row.
    pub fn cells_in(&self, rect: FixedRect) -> impl Iterator<Item = CellId> + use<> {
        let first = |v: i64| -(-v).div_euclid(self.cell);
        let (x0, x1) = (first(rect.min.x), first(rect.max.x));
        let (z0, z1) = (first(rect.min.z), first(rect.max.z));
        (z0..z1).flat_map(move |z| (x0..x1).map(move |x| CellId::new(x as i32, z as i32)))
    }

    /// How many cells have their corner inside `rect`.
    pub fn count_in(&self, rect: FixedRect) -> u64 {
        let first = |v: i64| -(-v).div_euclid(self.cell);
        let span = |a: i64, b: i64| (first(b) - first(a)).max(0) as u64;
        span(rect.min.x, rect.max.x).saturating_mul(span(rect.min.z, rect.max.z))
    }

    /// The hash of `cell` under `seed`, from which every draw of its
    /// candidate comes ([`rank`], [`draw`], [`Lattice::position`]).
    pub fn hash(&self, seed: u32, cell: CellId) -> u32 {
        pcg2(seed, pcg2(cell.x() as u32, cell.z() as u32))
    }

    /// Where the candidate of `cell`, hashed to `h`, stands: its corner plus
    /// a jitter of less than one cell along each axis.
    pub fn position(&self, h: u32, cell: CellId) -> Fixed2 {
        let jitter = |salt: u32| (i64::from(pcg(h ^ salt) >> 8) * self.cell) >> 24;
        let corner = self.corner(cell);
        Fixed2::new(
            corner.x + jitter(SALT_JITTER_X),
            corner.z + jitter(SALT_JITTER_Z),
        )
    }
}

/// The 24-bit rank of a candidate hashed to `h`. Lower ranks survive lower
/// densities.
pub fn rank(h: u32) -> u32 {
    pcg(h ^ SALT_RANK) >> 8
}

/// A further draw from a candidate hashed to `h`, for the choice named by
/// `salt`.
pub fn draw(h: u32, salt: u32) -> u32 {
    pcg(h ^ salt)
}

/// [`draw`] as a number from 0 (inclusive) to 1 (exclusive).
pub fn unit(h: u32, salt: u32) -> f32 {
    crate::hash::unit_float(draw(h, salt))
}

/// A density share from 0 to 1 in 8 bits: 0 to 255, rounded. NaN counts as
/// 0.
pub fn quantise(share: f32) -> u8 {
    if share.is_nan() {
        0
    } else {
        (share.clamp(0.0, 1.0) * 255.0).round() as u8
    }
}

/// A chance per cell from 0 to 1 in 24-bit fixed point, rounded. Chances
/// above 1 count as 1.
pub fn chance(per_cell: f64) -> u32 {
    if per_cell.is_nan() || per_cell <= 0.0 {
        0
    } else {
        (per_cell.min(1.0) * f64::from(CHANCE_ONE)).round() as u32
    }
}

/// Whether a candidate of rank `rank` is dense enough to keep, where the
/// Scatter's chance per cell is `chance` (from [`chance`]) and the share of
/// it allowed here is `local` (from [`quantise`]).
///
/// It is `rank / 2^24 < chance / 2^24 × local / 255`, compared exactly by
/// cross-multiplying. A lower chance or share keeps a subset of the
/// candidates a higher one keeps.
pub fn dense_enough(rank: u32, chance: u32, local: u8) -> bool {
    u64::from(rank) * 255 < u64::from(chance) * u64::from(local)
}

/// The order in which candidates claim their space: the lower wins.
///
/// Candidates compare by `tier` first (a Scatter may let one kind of copy
/// win over another, whatever the ranks), then by `rank / local`
/// (cross-multiplied, so a candidate in a thinner part of the mask ranks
/// higher), then by cell and index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rank {
    /// Lower tiers always win.
    pub tier: u32,
    /// The candidate's 24-bit rank, from [`rank`].
    pub rank: u32,
    /// The density share allowed at the candidate, from [`quantise`].
    pub local: u8,
    /// The candidate's cell.
    pub cell: CellId,
    /// The candidate's number within its cell.
    pub index: u16,
}

impl Ord for Rank {
    fn cmp(&self, other: &Self) -> Ordering {
        let mine = u64::from(self.rank) * u64::from(other.local);
        let theirs = u64::from(other.rank) * u64::from(self.local);
        self.tier
            .cmp(&other.tier)
            .then(mine.cmp(&theirs))
            .then(self.cell.cmp(&other.cell))
            .then(self.index.cmp(&other.index))
    }
}

impl PartialOrd for Rank {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A candidate that [`keep_spaced`] can space out.
pub trait Spaced {
    /// Where the candidate stands.
    fn position(&self) -> Fixed2;
    /// Its claim on space: lower ranks win.
    fn rank(&self) -> Rank;
}

/// Spaces out `candidates`: which of them to keep, in the same order.
///
/// Runs `rounds` rounds of "keep a candidate if no lower-ranked candidate
/// that conflicts with it was kept in the round before", starting from
/// keeping every candidate. `conflict(a, b)` says whether two candidates
/// stand too close to both be kept; it must be symmetric and false for any
/// pair further apart than `reach` fixed-point units. A `reach` of 0 or less
/// keeps every candidate.
///
/// No two kept candidates conflict. A candidate's result depends only on
/// the candidates within `rounds × reach` of it, so a Scatter that spaces
/// every candidate within that margin of an area gets the same answer
/// inside the area as for the whole World.
///
/// # Panics
///
/// Panics if `rounds` is even: an even number of rounds can keep two
/// conflicting candidates.
pub fn keep_spaced<T: Spaced>(
    candidates: &[T],
    rounds: u32,
    reach: i64,
    conflict: impl Fn(&T, &T) -> bool,
) -> Vec<bool> {
    assert!(rounds % 2 == 1, "spacing needs an odd number of rounds");
    let n = candidates.len();
    if reach <= 0 {
        return vec![true; n];
    }

    // Bucket the candidates, then list for each one the lower-ranked
    // candidates it conflicts with.
    let bucket = |p: Fixed2| (p.x.div_euclid(reach), p.z.div_euclid(reach));
    let mut buckets: HashMap<(i64, i64), Vec<u32>> = HashMap::new();
    for (i, c) in candidates.iter().enumerate() {
        buckets
            .entry(bucket(c.position()))
            .or_default()
            .push(i as u32);
    }
    let mut start = Vec::with_capacity(n + 1);
    let mut lower: Vec<u32> = Vec::new();
    for a in candidates {
        start.push(lower.len());
        let (bx, bz) = bucket(a.position());
        let rank = a.rank();
        for x in bx - 1..=bx + 1 {
            for z in bz - 1..=bz + 1 {
                let Some(list) = buckets.get(&(x, z)) else {
                    continue;
                };
                for &j in list {
                    let b = &candidates[j as usize];
                    if b.rank() < rank && conflict(a, b) {
                        lower.push(j);
                    }
                }
            }
        }
    }
    start.push(lower.len());

    // Run the rounds.
    let mut kept = vec![true; n];
    let mut next = vec![false; n];
    for _ in 0..rounds {
        for (i, keep) in next.iter_mut().enumerate() {
            *keep = !lower[start[i]..start[i + 1]]
                .iter()
                .any(|&j| kept[j as usize]);
        }
        std::mem::swap(&mut kept, &mut next);
    }
    kept
}

/// The width of a chunk on the shared chunk grid at full detail, in
/// metres.
pub const CHUNK_METRES: f32 = 64.0;

/// One square of a [`ChunkGrid`]: column `x` along X and row `z` along Z.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkId {
    /// The chunk's column, along X.
    pub x: i32,
    /// The chunk's row, along Z.
    pub z: i32,
}

impl ChunkId {
    /// The chunk in column `x` and row `z`.
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }
}

/// Square chunks fixed to the World, which Scatters are placed in one at a
/// time. A copy belongs to the chunk holding its lattice cell's corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChunkGrid {
    size: i64,
}

impl Default for ChunkGrid {
    /// Chunks [`CHUNK_METRES`] wide.
    fn default() -> Self {
        Self::new(CHUNK_METRES)
    }
}

impl ChunkGrid {
    /// Chunks `metres` wide, rounded to the nearest 1/256 m and at least
    /// that.
    pub fn new(metres: f32) -> Self {
        Self {
            size: to_fixed(f64::from(metres)).max(1),
        }
    }

    /// The width of a chunk in fixed-point units.
    pub fn size_fixed(&self) -> i64 {
        self.size
    }

    /// The chunk holding World position `p`.
    pub fn chunk_of(&self, p: Fixed2) -> ChunkId {
        ChunkId::new(
            p.x.div_euclid(self.size) as i32,
            p.z.div_euclid(self.size) as i32,
        )
    }

    /// The ground `chunk` covers.
    pub fn rect(&self, chunk: ChunkId) -> FixedRect {
        let min = Fixed2::new(
            i64::from(chunk.x) * self.size,
            i64::from(chunk.z) * self.size,
        );
        FixedRect::new(min, Fixed2::new(min.x + self.size, min.z + self.size))
    }

    /// The ground `chunk` covers, as an area to place a Scatter over.
    pub fn area(&self, chunk: ChunkId) -> ScatterArea {
        self.rect(chunk).area()
    }

    /// Every chunk that holds part of `rect`, row by row.
    pub fn chunks_over(&self, rect: FixedRect) -> Vec<ChunkId> {
        if rect.is_empty() {
            return Vec::new();
        }
        let lo = self.chunk_of(rect.min);
        let hi = self.chunk_of(Fixed2::new(rect.max.x - 1, rect.max.z - 1));
        (lo.z..=hi.z)
            .flat_map(|z| (lo.x..=hi.x).map(move |x| ChunkId::new(x, z)))
            .collect()
    }
}

#[cfg(test)]
mod tests;
