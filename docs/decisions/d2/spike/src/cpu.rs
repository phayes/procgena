//! The CPU twin: places a chunk's stack with the same integer maths as the
//! WGSL kernels. It is the reference the GPU is compared against.

use std::sync::Mutex;
use std::time::Instant;

use crate::int::{
    Cand, Grid, Rect, SALT_RANK, SALT_SPECIES, bilinear, bilinear_with, cell_hash, conflict, hash,
    influence, lattice_pos, lower, tile_pos,
};
use crate::stack::{GRASS, Inputs, Kind, Spec, Windows, cells_reaching};

pub const STAGE_PRODUCT: usize = 0;
pub const STAGE_CAND: usize = 1;
pub const STAGE_KD: usize = 2;
pub const STAGE_STAMP: usize = 3;
pub const STAGE_GRASS: usize = 4;
pub const STAGE_NAMES: [&str; 5] = ["mask product", "candidates", "K_d", "stamp", "grass"];

/// Nanoseconds spent in each stage by [`build_with`] since the last
/// [`take_profile`].
static PROFILE: Mutex<[f64; 5]> = Mutex::new([0.0; 5]);

fn profile(stage: usize, since: Instant) {
    PROFILE.lock().unwrap()[stage] += since.elapsed().as_nanos() as f64;
}

/// Returns and clears the time spent per stage.
pub fn take_profile() -> [f64; 5] {
    std::mem::take(&mut *PROFILE.lock().unwrap())
}

/// What one lattice Scatter produced for a chunk.
#[derive(Clone)]
pub struct LatticeOut {
    /// Candidates over the evaluated window.
    pub cand: Grid<Cand>,
    /// `K_d` result over the evaluated window (only `own` is final).
    pub kept: Grid<u8>,
    /// The kept flags over the owned cells, which later masks read.
    pub own: Grid<u8>,
}

/// Everything a chunk build produced, for comparing with the GPU.
pub struct ChunkOut {
    pub avail: [Option<Grid<u8>>; 4],
    pub lattice: [Option<LatticeOut>; 3],
    pub masks: [Option<Grid<u8>>; 3],
    /// Grass: per owned tile cell, `n_keep` slots of positions, or
    /// `(i32::MIN, i32::MIN)` where the candidate was not kept.
    pub grass: Option<Vec<(i32, i32)>>,
}

/// Fills the available mask of Scatter `k` over `rect`: coverage, then
/// canopy shade if the Scatter reads it, then every footprint mask above.
pub fn product(
    spec: &Spec,
    k: usize,
    inp: &Inputs,
    rect: Rect,
    tfs: u32,
    masks: &[Option<Grid<u8>>; 3],
) -> Grid<u8> {
    let mut g = Grid::new(rect, 0u8);
    let size = 1 << tfs;
    for ty in rect.y0..rect.y1 {
        for tx in rect.x0..rect.x1 {
            let cx = tx * size + size / 2;
            let cy = ty * size + size / 2;
            let mut v = bilinear(&inp.cov, inp.gs, cx, cy);
            if spec.shade {
                let s = bilinear_with(inp.gs, cx, cy, |x, y| influence(inp.cov.get(x, y) as u32));
                v = v * s / 255;
            }
            for m in masks.iter().take(k.min(3)) {
                let m = m.as_ref().expect("mask above");
                v = v * m.get(tx, ty) as u32 / 255;
            }
            g.set(tx, ty, v as u8);
        }
    }
    g
}

/// The candidate of lattice cell `(i, j)`.
pub fn candidate(
    spec: &Spec,
    seed: u32,
    inp: &Inputs,
    avail: &Grid<u8>,
    tfs: u32,
    i: i32,
    j: i32,
) -> Cand {
    let Kind::Lattice {
        p24,
        share24,
        spacing,
        ..
    } = spec.kind
    else {
        unreachable!()
    };
    let h = cell_hash(seed, spec.id, i, j);
    let (x, y) = lattice_pos(h, spec.cs, i, j);
    let u24 = hash(h ^ SALT_RANK) >> 8;
    let species = if (hash(h ^ SALT_SPECIES) >> 8) < share24 {
        0
    } else {
        1
    };
    let local = bilinear(avail, tfs, x, y);
    let slope = bilinear(&inp.slope, inp.gs, x, y);
    let alive = local > 0 && u24 * 255 < p24 * local && slope <= spec.max_slope;
    Cand {
        x,
        y,
        key: u24 << 8 | local,
        meta: spacing[species] | (alive as u32) << 31,
    }
}

/// `K_d` over a candidate grid: `d` rounds of "kept if no lower-ranked
/// conflicting candidate was kept in the round before".
pub fn kd(spec: &Spec, cand: &Grid<Cand>, d: u32) -> Grid<u8> {
    let rect = cand.rect;
    let r = spec.r();
    // Edges to lower-ranked conflicting candidates, in CSR form.
    let n = rect.len();
    let mut start = Vec::with_capacity(n + 1);
    let mut edges: Vec<u32> = Vec::new();
    for y in rect.y0..rect.y1 {
        for x in rect.x0..rect.x1 {
            start.push(edges.len() as u32);
            let a = cand.get(x, y);
            if !a.alive() {
                continue;
            }
            for by in (y - r).max(rect.y0)..(y + r + 1).min(rect.y1) {
                for bx in (x - r).max(rect.x0)..(x + r + 1).min(rect.x1) {
                    if bx == x && by == y {
                        continue;
                    }
                    let bi = cand.index(bx, by);
                    let b = &cand.data[bi];
                    if b.alive() && lower(b, bx, by, &a, x, y) && conflict(&a, b) {
                        edges.push(bi as u32);
                    }
                }
            }
        }
    }
    start.push(edges.len() as u32);
    // Rounds: K_0 is every alive candidate.
    let mut prev: Vec<u8> = cand.data.iter().map(|c| c.alive() as u8).collect();
    let mut next = vec![0u8; n];
    for _ in 0..d {
        for a in 0..n {
            let blocked = edges[start[a] as usize..start[a + 1] as usize]
                .iter()
                .any(|&b| prev[b as usize] != 0);
            next[a] = (cand.data[a].alive() && !blocked) as u8;
        }
        std::mem::swap(&mut prev, &mut next);
    }
    Grid { rect, data: prev }
}

/// Stamps Scatter `spec`'s footprints into a mask over `rect`: 0 where a
/// kept copy's disc covers the texel centre, 255 elsewhere. Reads the kept
/// flags of the cells that can reach the texels.
pub fn stamp(spec: &Spec, seed: u32, own: &Grid<u8>, rect: Rect, tfs: u32) -> Grid<u8> {
    let mut g = Grid::new(rect, 255u8);
    let size = 1 << tfs;
    let f = spec.footprint();
    let f2 = (f * f) as u32;
    for ty in rect.y0..rect.y1 {
        for tx in rect.x0..rect.x1 {
            let cx = tx * size + size / 2;
            let cy = ty * size + size / 2;
            let cells = cells_reaching(spec, Rect::new(tx, ty, tx + 1, ty + 1), tfs);
            'cells: for j in cells.y0..cells.y1 {
                for i in cells.x0..cells.x1 {
                    if own.get(i, j) == 0 {
                        continue;
                    }
                    let (x, y) = lattice_pos(cell_hash(seed, spec.id, i, j), spec.cs, i, j);
                    let dx = x - cx;
                    let dy = y - cy;
                    if ((dx * dx + dy * dy) as u32) < f2 {
                        g.set(tx, ty, 0);
                        break 'cells;
                    }
                }
            }
        }
    }
    g
}

/// Grass over its owned tile cells: per cell `n_keep` slots in rank order.
pub fn grass(
    spec: &Spec,
    seed: u32,
    inp: &Inputs,
    avail: &Grid<u8>,
    tfs: u32,
    own: Rect,
) -> Vec<(i32, i32)> {
    let Kind::Tile { n_keep } = spec.kind else {
        unreachable!()
    };
    let mut out = Vec::with_capacity(own.len() * n_keep as usize);
    grass_each(spec, seed, inp, avail, tfs, own, |_, _, _, kept| {
        out.push(kept.unwrap_or((i32::MIN, i32::MIN)));
    });
    out
}

/// Grass over its owned tile cells, calling `sink(i, j, point, kept
/// position)` for every candidate in cell-major, rank order.
pub fn grass_each(
    spec: &Spec,
    seed: u32,
    inp: &Inputs,
    avail: &Grid<u8>,
    tfs: u32,
    own: Rect,
    mut sink: impl FnMut(i32, i32, u32, Option<(i32, i32)>),
) {
    let Kind::Tile { n_keep } = spec.kind else {
        unreachable!()
    };
    for j in own.y0..own.y1 {
        for i in own.x0..own.x1 {
            let ch = cell_hash(seed, spec.id, i, j);
            for point in 0..n_keep {
                let (x, y) = tile_pos(ch, inp.tile[point as usize], spec.cs, i, j);
                let local = bilinear(avail, tfs, x, y);
                let slope = bilinear(&inp.slope, inp.gs, x, y);
                let keep = point * 255 < n_keep * local && slope <= spec.max_slope;
                sink(i, j, point, keep.then_some((x, y)));
            }
        }
    }
}

/// Places the stack from the top down to `w.lowest` over window A.
pub fn build(specs: &[Spec; 4], seed: u32, inp: &Inputs, w: &Windows) -> ChunkOut {
    build_with(specs, seed, inp, w, |s, avail, own| {
        Some(grass(s, seed, inp, avail, w.tfs, own))
    })
}

/// [`build`], with grass placed by `on_grass(spec, available mask, owned
/// cells)`, whose result is stored as the grass output.
pub fn build_with(
    specs: &[Spec; 4],
    seed: u32,
    inp: &Inputs,
    w: &Windows,
    mut on_grass: impl FnMut(&Spec, &Grid<u8>, Rect) -> Option<Vec<(i32, i32)>>,
) -> ChunkOut {
    let mut out = ChunkOut {
        avail: Default::default(),
        lattice: Default::default(),
        masks: Default::default(),
        grass: None,
    };
    let tfs = w.tfs;
    for k in 0..=w.lowest {
        let s = &specs[k];
        let t = Instant::now();
        let avail = product(s, k, inp, w.avail[k], tfs, &out.masks);
        profile(STAGE_PRODUCT, t);
        if k == GRASS {
            let t = Instant::now();
            out.grass = on_grass(s, &avail, w.own[k]);
            profile(STAGE_GRASS, t);
        } else {
            let t = Instant::now();
            let mut cand = Grid::new(w.win[k], Cand::default());
            for j in w.win[k].y0..w.win[k].y1 {
                for i in w.win[k].x0..w.win[k].x1 {
                    cand.set(i, j, candidate(s, seed, inp, &avail, tfs, i, j));
                }
            }
            profile(STAGE_CAND, t);
            let t = Instant::now();
            let kept = kd(s, &cand, s.d());
            profile(STAGE_KD, t);
            let mut own = Grid::new(w.own[k], 0u8);
            for j in w.own[k].y0..w.own[k].y1 {
                for i in w.own[k].x0..w.own[k].x1 {
                    own.set(i, j, kept.get(i, j));
                }
            }
            if w.mask[k].len() > 0 {
                let t = Instant::now();
                out.masks[k] = Some(stamp(s, seed, &own, w.mask[k], tfs));
                profile(STAGE_STAMP, t);
            }
            out.lattice[k] = Some(LatticeOut { cand, kept, own });
        }
        out.avail[k] = Some(avail);
    }
    out
}

/// Greedy spacing in rank order (today's grove rule), for the oracle checks.
pub fn greedy(spec: &Spec, cand: &Grid<Cand>) -> Grid<u8> {
    let rect = cand.rect;
    let r = spec.r();
    let mut order: Vec<(i32, i32)> = Vec::new();
    for y in rect.y0..rect.y1 {
        for x in rect.x0..rect.x1 {
            if cand.get(x, y).alive() {
                order.push((x, y));
            }
        }
    }
    // Rank order: lower() is a strict total order on alive candidates.
    order.sort_by(|&(ax, ay), &(bx, by)| {
        let a = cand.get(ax, ay);
        let b = cand.get(bx, by);
        if lower(&a, ax, ay, &b, bx, by) {
            std::cmp::Ordering::Less
        } else if lower(&b, bx, by, &a, ax, ay) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    let mut kept = Grid::new(rect, 0u8);
    for (x, y) in order {
        let a = cand.get(x, y);
        let mut ok = true;
        'n: for by in (y - r).max(rect.y0)..(y + r + 1).min(rect.y1) {
            for bx in (x - r).max(rect.x0)..(x + r + 1).min(rect.x1) {
                if kept.get(bx, by) != 0 && conflict(&a, &cand.get(bx, by)) {
                    ok = false;
                    break 'n;
                }
            }
        }
        if ok {
            kept.set(x, y, 1);
        }
    }
    kept
}
