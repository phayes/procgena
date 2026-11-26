//! Building a chunk's canonical mesh: its edge contracts and feature
//! lines, then vertices added where the mesh misses the ground most, until
//! it is close enough.

use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashSet};

use glam::DVec2;

use crate::chunk::{CHUNK_SIZE, ChunkKey, ChunkSource};
use crate::feature::FeatureTier;

use super::cdt::ChunkCdt;
use super::contract::{ChunkEdgeKey, EdgeContract};
use super::mesh::{TinChunk, TinVertex, UnmetTolerance};
use super::numeric::{MM_PER_M, Mm, MmHeight};
use super::probe::{ProbeAddr, TinError, sample_heights};
use super::resolve::{Piece, Pt, Square, point_along, resolve_chunk, untangle};
use super::tolerance::{DomainTolerance, TinTolerance};
use super::water::Water;

/// How far apart the ground is sampled across a chunk, in millimetres:
/// the lattice refinement vertices sit on. The same spacing as along chunk
/// edges.
const LATTICE_MM: i64 = 1000;

/// Lattice points along a chunk's side, both corners included.
const SIDE: usize = (CHUNK_SIZE as i64 * MM_PER_M as i64 / LATTICE_MM) as usize + 1;

/// Slope is not checked at samples this close to a water's edge, in
/// metres: the ground creases there, and the lattice's slope estimate
/// reaches a metre either side.
const CREASE_CLEARANCE: f64 = 1.5;

/// The canonical mesh of finest-level chunk `key` over the ground of
/// `source` at the fine tier, within `tolerance`.
///
/// The border is locked to the four [`EdgeContract`]s, so the neighbours'
/// meshes meet it exactly. The rivers' and ponds' lines in the chunk (see
/// [`ChunkSource::lines_within`]) are kept as edges: each river's lowest
/// line and the outline of its water, and each pond's shore, cut where
/// they cross. Then the ground is sampled on a 1 m lattice, and while some
/// triangle misses a sample by more than that sample's height tolerance,
/// differs from its slope by more than its slope tolerance, or has an edge
/// longer than allowed, the worst triangle gets a vertex at its worst
/// sample. Samples within `tolerance.water_band` of water follow
/// `tolerance.water` (and a triangle touching one takes the water's
/// longest edge); the rest follow `tolerance.general`.
///
/// Where the vertex budget runs out first, the triangles still missing
/// their height tolerance are listed in [`TinChunk::unmet`]; feature line
/// pieces that couldn't be kept are listed in [`TinChunk::dropped`]. The
/// same chunk always gets the same mesh, whatever order chunks are built
/// in.
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
    let contracts = [&contracts[0], &contracts[1], &contracts[2], &contracts[3]];
    let mut chunk = ChunkCdt::new(key, contracts);

    // The feature lines, cut to the chunk and where they cross.
    let (lo, hi) = (key.min_corner(), key.max_corner());
    let water = Water::new(
        &source.lines_within(lo, hi, FeatureTier::Fine),
        tolerance,
        lo,
        hi,
    );
    let (x0, z0) = edges[0].start();
    let square = Square {
        lo: (x0.0, z0.0),
        hi: (
            x0.0 + (SIDE as i64 - 1) * LATTICE_MM,
            z0.0 + (SIDE as i64 - 1) * LATTICE_MM,
        ),
    };
    let resolved = resolve_chunk(
        water.pieces(),
        square,
        &chunk.border_positions(),
        |piece| water.hidden(piece),
        |piece, at| water.hidden_at(piece, at),
    );
    let followed = follow_ground(source, &resolved.pieces, square, &water, tolerance)?;
    let (pieces, mut dropped) = untangle(followed, square);
    dropped.extend(resolved.dropped);
    dropped.sort();
    let mut ends: Vec<Pt> = pieces
        .iter()
        .flat_map(|p| [p.a, p.b])
        .filter(|&p| square.strictly_inside(p))
        .collect();
    ends.sort();
    ends.dedup();
    let probes: Vec<ProbeAddr> = ends
        .iter()
        .map(|&(x, z)| ProbeAddr::new(Mm(x), Mm(z)))
        .collect();
    let heights = sample_heights(source, &probes)?;
    let line_vertices: Vec<TinVertex> = probes
        .iter()
        .zip(heights)
        .map(|(p, h)| TinVertex { x: p.x, z: p.z, h })
        .collect();
    chunk.record_dropped(&dropped);
    chunk.add_lines(&pieces, &line_vertices);

    let lattice = Lattice::sample(source, key, &water, tolerance)?;
    Ok(refine(chunk, &lattice, tolerance, &water))
}

/// `pieces` (which cross nothing) cut further so each follows the ground
/// along its length: each is sampled about every metre (at points rounded
/// to the millimetre, strictly inside `square`), and, like an edge
/// contract, a stretch gets a vertex at the sample it misses most among
/// those it misses by more than their height tolerance, or at its middle
/// sample if it is longer than its longest edge allows. A sample within
/// `tolerance.water_band` of `water` follows `tolerance.water`.
fn follow_ground(
    source: &dyn ChunkSource,
    pieces: &[Piece],
    square: Square,
    water: &Water,
    tolerance: &TinTolerance,
) -> Result<Vec<Piece>, TinError> {
    // Each piece's stations, ends included, and all of them sampled at once.
    let stations: Vec<Vec<Pt>> = pieces
        .iter()
        .map(|piece| {
            let (dx, dz) = (
                (piece.b.0 - piece.a.0) as f64,
                (piece.b.1 - piece.a.1) as f64,
            );
            let n = ((dx * dx + dz * dz).sqrt() / LATTICE_MM as f64)
                .ceil()
                .max(1.0) as i128;
            let mut at = vec![piece.a];
            at.extend(
                (1..n)
                    .map(|j| point_along(piece.a, piece.b, j, n))
                    .filter(|&p| square.strictly_inside(p) && p != piece.a && p != piece.b),
            );
            at.push(piece.b);
            at.dedup();
            at
        })
        .collect();
    let probes: Vec<ProbeAddr> = stations
        .iter()
        .flatten()
        .map(|&(x, z)| ProbeAddr::new(Mm(x), Mm(z)))
        .collect();
    let mut heights = sample_heights(source, &probes)?.into_iter();

    // Split each piece where it strays, as an edge contract does.
    let mut out = Vec::with_capacity(pieces.len());
    for (piece, at) in pieces.iter().zip(&stations) {
        let h: Vec<f64> = heights
            .by_ref()
            .take(at.len())
            .map(|h| f64::from(h.0))
            .collect();
        let domain = |p: Pt| {
            let m = DVec2::new(Mm(p.0).to_m(), Mm(p.1).to_m());
            if water.gap(m) < tolerance.water_band {
                &tolerance.water
            } else {
                &tolerance.general
            }
        };
        let along = |i: usize| {
            let (dx, dz) = ((at[i].0 - at[0].0) as f64, (at[i].1 - at[0].1) as f64);
            (dx * dx + dz * dz).sqrt()
        };
        let mut kept = vec![false; at.len()];
        kept[0] = true;
        kept[at.len() - 1] = true;
        let mut stack = vec![(0, at.len() - 1)];
        while let Some((a, b)) = stack.pop() {
            if b - a < 2 {
                continue;
            }
            let (sa, sb) = (along(a), along(b));
            let worst = (a + 1..b)
                .filter_map(|i| {
                    let t = (along(i) - sa) / (sb - sa);
                    let error = (h[i] - (h[a] + (h[b] - h[a]) * t)).abs();
                    (error > domain(at[i]).height * MM_PER_M).then_some((i, error))
                })
                .fold(None, |best: Option<(usize, f64)>, c| match best {
                    Some(best) if c.1 <= best.1 => Some(best),
                    _ => Some(c),
                });
            let max_edge = (a..=b)
                .map(|i| domain(at[i]).max_edge)
                .fold(f64::INFINITY, f64::min);
            let split = if let Some((worst, _)) = worst {
                worst
            } else if sb - sa > max_edge * MM_PER_M {
                (a + b) / 2
            } else {
                continue;
            };
            kept[split] = true;
            stack.push((a, split));
            stack.push((split, b));
        }
        let ends: Vec<Pt> = (0..at.len()).filter(|&i| kept[i]).map(|i| at[i]).collect();
        out.extend(
            ends.windows(2)
                .filter_map(|w| Piece::new(piece.kind, piece.feature, w[0], w[1])),
        );
    }
    Ok(out)
}

/// The ground sampled on the 1 m lattice over one chunk, with each
/// sample's tolerance.
pub(crate) struct Lattice {
    /// The chunk's smallest corner, in millimetres.
    origin: (i64, i64),
    /// Heights, row by row along x, `SIDE` by `SIDE`.
    heights: Vec<MmHeight>,
    /// Whether each sample is within the water band (and follows the
    /// water's tolerance).
    near_water: Vec<bool>,
    /// The ground's slope at each sample (rise over run along x and z), or
    /// `None` where it isn't checked.
    slopes: Vec<Option<DVec2>>,
}

impl Lattice {
    /// Samples the ground of `source` over chunk `key`, with each sample's
    /// place relative to `water`.
    pub(crate) fn sample(
        source: &dyn ChunkSource,
        key: ChunkKey,
        water: &Water,
        tolerance: &TinTolerance,
    ) -> Result<Self, TinError> {
        let (x0, z0) = ChunkEdgeKey::of_chunk(key)[0].start();
        let probes: Vec<ProbeAddr> = (0..SIDE * SIDE)
            .map(|i| {
                let (col, row) = ((i % SIDE) as i64, (i / SIDE) as i64);
                ProbeAddr::new(Mm(x0.0 + col * LATTICE_MM), Mm(z0.0 + row * LATTICE_MM))
            })
            .collect();
        let heights = sample_heights(source, &probes)?;
        let gaps: Vec<f64> = probes.iter().map(|p| water.gap(p.to_m())).collect();
        let near_water = gaps.iter().map(|&g| g < tolerance.water_band).collect();

        // Slopes by differences across each sample (one-sided on the
        // border), unless slope is unchecked there or a crease is near.
        let checked = |i: usize| {
            let domain = if gaps[i] < tolerance.water_band {
                &tolerance.water
            } else {
                &tolerance.general
            };
            domain.gradient.is_some() && gaps[i].abs() >= CREASE_CLEARANCE
        };
        let h = |col: usize, row: usize| f64::from(heights[row * SIDE + col].0);
        let slopes = (0..SIDE * SIDE)
            .map(|i| {
                if !checked(i) {
                    return None;
                }
                let (col, row) = (i % SIDE, i / SIDE);
                let diff = |lo: usize, hi: usize, at: &dyn Fn(usize) -> f64| {
                    (at(hi) - at(lo)) / ((hi - lo) as f64 * LATTICE_MM as f64)
                };
                let (cl, ch) = (col.saturating_sub(1), (col + 1).min(SIDE - 1));
                let (rl, rh) = (row.saturating_sub(1), (row + 1).min(SIDE - 1));
                Some(DVec2::new(
                    diff(cl, ch, &|c| h(c, row)),
                    diff(rl, rh, &|r| h(col, r)),
                ))
            })
            .collect();
        Ok(Self {
            origin: (x0.0, z0.0),
            heights,
            near_water,
            slopes,
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

    /// The tolerance of the sample at `(col, row)`.
    fn domain<'a>(&self, at: (usize, usize), tolerance: &'a TinTolerance) -> &'a DomainTolerance {
        if self.near_water[at.1 * SIDE + at.0] {
            &tolerance.water
        } else {
            &tolerance.general
        }
    }

    /// The lattice column and row of `v`, if it sits on the lattice.
    fn lattice_of(&self, v: TinVertex) -> Option<(usize, usize)> {
        let (dx, dz) = (v.x.0 - self.origin.0, v.z.0 - self.origin.1);
        (dx % LATTICE_MM == 0 && dz % LATTICE_MM == 0)
            .then_some(((dx / LATTICE_MM) as usize, (dz / LATTICE_MM) as usize))
    }
}

/// A triangle waiting to be split: worse first.
#[derive(PartialEq, Eq)]
struct Candidate {
    /// Whether an edge is longer than the tolerance allows.
    too_long: bool,
    /// How far the triangle misses its worst sample, in millimetres.
    error: i64,
    /// Whether that miss is more than the sample's height tolerance.
    misses: bool,
    /// The triangle's corners by position (sorted by z, then x): ties go
    /// to the smallest.
    position: Reverse<[(Mm, Mm); 3]>,
    /// The triangle, anticlockwise in (x, z).
    face: [u32; 3],
    /// Where to split it: lattice column and row.
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

/// Adds vertices to `chunk` (its border and feature lines already locked)
/// where it misses `lattice` most, within `tolerance`, and returns the
/// canonical mesh.
pub(crate) fn refine(
    mut chunk: ChunkCdt,
    lattice: &Lattice,
    tolerance: &TinTolerance,
    water: &Water,
) -> TinChunk {
    let mut taken: HashSet<(usize, usize)> = HashSet::new();
    for v in chunk.vertices() {
        taken.extend(lattice.lattice_of(*v));
    }
    let scorer = Scorer {
        lattice,
        tolerance,
        water,
    };

    // Every triangle, scored.
    let mut queue = BinaryHeap::new();
    let faces: Vec<[u32; 3]> = chunk.faces().collect();
    for face in faces {
        if let Some(c) = scorer.score(&chunk, &taken, face) {
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
            if let Some(c) = scorer.score(&chunk, &taken, face) {
                queue.push(c);
            }
        }
    }
    let unmet = unmet
        .into_iter()
        .filter(|c| c.misses)
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

/// Scores triangles against the lattice.
struct Scorer<'a> {
    lattice: &'a Lattice,
    tolerance: &'a TinTolerance,
    water: &'a Water,
}

impl Scorer<'_> {
    /// Triangle `face` scored for splitting, or `None` if it needs no
    /// split or has no free sample inside to split at.
    ///
    /// It needs a split if it misses a sample inside it (or on its edges)
    /// by more than that sample's height tolerance, its slope differs from
    /// a sample's by more than that sample's slope tolerance, or an edge is
    /// longer than allowed (the water's longest edge if a corner or sample
    /// is within the water band). Samples on the chunk's border or at a
    /// vertex already in the mesh (`taken`) are left out. It is split at
    /// the sample it misses most in height (on a tie the one nearest its
    /// centre, then the first by row and column), unless only slope is
    /// missed: then at the sample whose slope it misses most.
    fn score(
        &self,
        chunk: &ChunkCdt,
        taken: &HashSet<(usize, usize)>,
        face: [u32; 3],
    ) -> Option<Candidate> {
        let lattice = self.lattice;
        let corners = face.map(|i| chunk.vertices()[i as usize]);
        // Corners in millimetres from the chunk's corner, where the
        // orientation tests are exact.
        let [a, b, c] = corners.map(|v| (v.x.0 - lattice.origin.0, v.z.0 - lattice.origin.1));
        let orient = |p: (i64, i64), q: (i64, i64), r: (i64, i64)| {
            (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0)
        };
        let area = orient(a, b, c);
        if area <= 0 {
            return None;
        }

        // The samples inside the triangle (or on its edges).
        let heights = corners.map(|v| i128::from(v.h.0));
        let sum = (a.0 + b.0 + c.0, a.1 + b.1 + c.1);
        let last = (SIDE - 1) as i64;
        let cell = |lo: i64, hi: i64| {
            (
                lo.div_euclid(LATTICE_MM) + i64::from(lo.rem_euclid(LATTICE_MM) != 0),
                hi.div_euclid(LATTICE_MM),
            )
        };
        let (lo_x, hi_x) = cell(a.0.min(b.0).min(c.0), a.0.max(b.0).max(c.0));
        let (lo_z, hi_z) = cell(a.1.min(b.1).min(c.1), a.1.max(b.1).max(c.1));
        let (lo_x, hi_x) = (lo_x.max(1), hi_x.min(last - 1));
        let (lo_z, hi_z) = (lo_z.max(1), hi_z.min(last - 1));
        let slope = self.slope(corners);
        let mut near_water = false;
        let mut best: Option<(i64, i64, (usize, usize))> = None;
        let mut misses = false;
        let mut steepest: Option<(f64, (usize, usize))> = None;
        for z in lo_z..=hi_z {
            for x in lo_x..=hi_x {
                let p = (x * LATTICE_MM, z * LATTICE_MM);
                let (wa, wb, wc) = (orient(b, c, p), orient(c, a, p), orient(a, b, p));
                if wa < 0 || wb < 0 || wc < 0 {
                    continue;
                }
                let at = (x as usize, z as usize);
                near_water |= lattice.near_water[at.1 * SIDE + at.0];
                if taken.contains(&at) {
                    continue;
                }
                let domain = lattice.domain(at, self.tolerance);
                // How far the flat triangle misses the sample, exactly, in
                // whole millimetres (rounded down).
                let flat = i128::from(wa) * heights[0]
                    + i128::from(wb) * heights[1]
                    + i128::from(wc) * heights[2];
                let sample = i128::from(lattice.heights[at.1 * SIDE + at.0].0) * i128::from(area);
                let error = ((sample - flat).abs() / i128::from(area)) as i64;
                misses |= error > (domain.height * MM_PER_M) as i64;
                // Nine times the squared distance to the centre.
                let (cx, cz) = (3 * p.0 - sum.0, 3 * p.1 - sum.1);
                let off_centre = cx * cx + cz * cz;
                let better =
                    best.is_none_or(|(e, d, _)| error > e || (error == e && off_centre < d));
                if better {
                    best = Some((error, off_centre, at));
                }
                // How far its slope is off, beyond the sample's tolerance.
                if let (Some(limit), Some(ground)) =
                    (domain.gradient, lattice.slopes[at.1 * SIDE + at.0])
                {
                    let off = (slope - ground).length() - limit;
                    if off > 0.0 && steepest.is_none_or(|(o, _)| off > o) {
                        steepest = Some((off, at));
                    }
                }
            }
        }
        let (error, _, worst) = best?;
        near_water |= corners.iter().any(|v| {
            self.water.gap(DVec2::new(v.x.to_m(), v.z.to_m())) < self.tolerance.water_band
        });
        let max_edge = if near_water {
            self.tolerance.water.max_edge
        } else {
            self.tolerance.general.max_edge
        } * MM_PER_M;
        let too_long = [(a, b), (b, c), (c, a)].iter().any(|&(p, q)| {
            let (dx, dz) = (p.0 - q.0, p.1 - q.1);
            ((dx * dx + dz * dz) as f64).sqrt() > max_edge
        });
        let worst = match steepest {
            Some((_, at)) if !misses && !too_long => at,
            _ if misses || too_long => worst,
            _ => return None,
        };
        let mut position = corners.map(|v| (v.z, v.x));
        position.sort();
        Some(Candidate {
            too_long,
            error,
            misses,
            position: Reverse(position.map(|(z, x)| (x, z))),
            face,
            worst,
        })
    }

    /// The slope of the flat triangle through `corners`, rise over run
    /// along x and z.
    fn slope(&self, corners: [TinVertex; 3]) -> DVec2 {
        let [a, b, c] = corners.map(|v| {
            (
                (v.x.0 - self.lattice.origin.0) as f64,
                (v.z.0 - self.lattice.origin.1) as f64,
                f64::from(v.h.0),
            )
        });
        let (ux, uz, uh) = (b.0 - a.0, b.1 - a.1, b.2 - a.2);
        let (vx, vz, vh) = (c.0 - a.0, c.1 - a.1, c.2 - a.2);
        let det = ux * vz - uz * vx;
        DVec2::new((uh * vz - vh * uz) / det, (vh * ux - uh * vx) / det)
    }
}
