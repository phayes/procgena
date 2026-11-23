//! The Ecotope stack of the spike (trees, boulders, logs, grass), the World
//! inputs (coverage and slope bytes), and the windows each Scatter is placed
//! over for one chunk.

use crate::int::{FP, Grid, Rect, hash, unit};

/// Scatter ids, highest priority first.
pub const TREES: usize = 0;
pub const BOULDERS: usize = 1;
pub const LOGS: usize = 2;
pub const GRASS: usize = 3;
pub const NAMES: [&str; 4] = ["trees", "boulders", "logs", "grass"];

/// Points in grass's progressive tile (plant's `TILE_POINTS`).
pub const TILE_POINTS: u32 = 1024;

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    /// One candidate per cell, spacing by `K_d`.
    Lattice {
        /// Candidates per cell kept at full density, Q24 (at most 1 << 24).
        p24: u32,
        /// Share of species 0, Q24.
        share24: u32,
        /// Spacing of species 0 and 1, fixed point.
        spacing: [u32; 2],
        /// Rounds of `K_d`.
        d: u32,
        /// Footprint radius stamped into the Scatter's mask, fixed point.
        footprint: u32,
    },
    /// Plant's progressive tile: `TILE_POINTS` per cell, ranked by index.
    Tile {
        /// Points kept per cell at full density.
        n_keep: u32,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub id: u32,
    /// log2 of the cell (or tile) size in fixed point.
    pub cs: u32,
    pub kind: Kind,
    /// Largest quantised slope a copy stands on.
    pub max_slope: u32,
    /// Whether the Scatter reads canopy shade.
    pub shade: bool,
}

impl Spec {
    pub fn cell(&self) -> i32 {
        1 << self.cs
    }
    /// Stencil radius of `K_d` in cells: `ceil(R / c)`.
    pub fn r(&self) -> i32 {
        match self.kind {
            Kind::Lattice { spacing, .. } => {
                let s = spacing[0].max(spacing[1]) as i32;
                (s + self.cell() - 1) >> self.cs
            }
            Kind::Tile { .. } => 0,
        }
    }
    pub fn d(&self) -> u32 {
        match self.kind {
            Kind::Lattice { d, .. } => d,
            Kind::Tile { .. } => 0,
        }
    }
    pub fn footprint(&self) -> i32 {
        match self.kind {
            Kind::Lattice { footprint, .. } => footprint as i32,
            Kind::Tile { .. } => 0,
        }
    }
}

fn q24(x: f64) -> u32 {
    (x * (1u64 << 24) as f64).round() as u32
}

fn fp(m: f64) -> u32 {
    (m * FP as f64).round() as u32
}

/// Slope limit in degrees, as the quantised slope byte.
fn slope_byte(deg: f64) -> u32 {
    (deg / 90.0 * 255.0).round() as u32
}

/// The spike's stack, every lattice Scatter with `K_d` rounds `d`.
/// Densities: trees are the densest prototype case (ash large + aspen small
/// at scale 4); the boulder and log densities are guesses.
pub fn stack(d: u32) -> [Spec; 4] {
    [
        Spec {
            id: 11,
            cs: 7, // 0.5 m
            kind: Kind::Lattice {
                p24: q24(4.0 * 2760.0 / 1e4 * 0.25),
                share24: q24(160.0 / 2760.0),
                spacing: [fp(2.5), fp(0.6)],
                d,
                footprint: fp(1.0),
            },
            max_slope: slope_byte(35.0),
            shade: false,
        },
        Spec {
            id: 12,
            cs: 8, // 1 m
            kind: Kind::Lattice {
                p24: q24(0.04),
                share24: 0,
                spacing: [fp(1.5), fp(1.5)],
                d,
                footprint: fp(2.0),
            },
            max_slope: 255,
            shade: false,
        },
        Spec {
            id: 13,
            cs: 8, // 1 m
            kind: Kind::Lattice {
                p24: q24(0.06),
                share24: 0,
                spacing: [fp(1.0), fp(1.0)],
                d,
                footprint: fp(3.0),
            },
            max_slope: slope_byte(25.0),
            shade: false,
        },
        Spec {
            id: 14,
            cs: 9,                            // 2 m tiles
            kind: Kind::Tile { n_keep: 800 }, // 200 per m²
            max_slope: slope_byte(40.0),
            shade: true,
        },
    ]
}

/// The World's coverage and slope bytes, computed once on the CPU and read by
/// both sides.
pub struct Inputs {
    /// log2 of the texel size in fixed point (9 = 2 m).
    pub gs: u32,
    pub cov: Grid<u8>,
    pub slope: Grid<u8>,
    /// Grass's progressive tile, `x | y << 16` in 1/65536ths of the tile.
    pub tile: Vec<u32>,
}

/// The extent of the World inputs, in metres: [-WORLD_PAD, 1024 + WORLD_PAD).
pub const WORLD_PAD: i32 = 512;

/// The uneven coverage of the CPU prototype, from 0 to 1.
pub fn coverage_fn(x: f64, z: f64) -> f64 {
    (0.55 + 0.45 * (x / 13.0).sin() * (z / 17.0 + 0.7 * (x / 29.0).sin()).cos()).clamp(0.0, 1.0)
}

/// Slope angle in degrees of a synthetic rolling heightfield.
pub fn slope_fn(x: f64, z: f64) -> f64 {
    // h = 6 sin(x/40) cos(z/55) + 2 sin((x+z)/11) + 0.6 sin(x/3.1) sin(z/4.3)
    let gx = 6.0 / 40.0 * (x / 40.0).cos() * (z / 55.0).cos()
        + 2.0 / 11.0 * ((x + z) / 11.0).cos()
        + 0.6 / 3.1 * (x / 3.1).cos() * (z / 4.3).sin();
    let gz = -6.0 / 55.0 * (x / 40.0).sin() * (z / 55.0).sin()
        + 2.0 / 11.0 * ((x + z) / 11.0).cos()
        + 0.6 / 4.3 * (x / 3.1).sin() * (z / 4.3).cos();
    (gx * gx + gz * gz).sqrt().atan().to_degrees()
}

impl Inputs {
    /// Inputs over the whole World at texels of `1 << gs` fixed-point units.
    pub fn new(gs: u32) -> Self {
        let size = 1i32 << gs;
        let lo = -WORLD_PAD * FP >> gs;
        let hi = (1024 + WORLD_PAD) * FP >> gs;
        let rect = Rect::new(lo, lo, hi, hi);
        let mut cov = Grid::new(rect, 0u8);
        let mut slope = Grid::new(rect, 0u8);
        for ty in lo..hi {
            for tx in lo..hi {
                let x = (tx * size + size / 2) as f64 / FP as f64;
                let z = (ty * size + size / 2) as f64 / FP as f64;
                cov.set(tx, ty, (coverage_fn(x, z) * 255.0).round() as u8);
                slope.set(
                    tx,
                    ty,
                    (slope_fn(x, z) / 90.0 * 255.0).round().min(255.0) as u8,
                );
            }
        }
        Self {
            gs,
            cov,
            slope,
            tile: quantised_tile(),
        }
    }
}

/// Plant's blue-noise tile (`best_candidate_tile(1024, 12, 0x6b8a_3a00)`),
/// quantised to 16 bits per axis.
pub fn quantised_tile() -> Vec<u32> {
    let count = TILE_POINTS as usize;
    let mut points: Vec<(f32, f32)> = Vec::with_capacity(count);
    let mut rng = 0x6b8a_3a00u32;
    let mut next = || {
        rng = hash(rng);
        unit(rng)
    };
    while points.len() < count {
        let mut best = (0.0f32, 0.0f32);
        let mut best_distance = -1.0f32;
        for _ in 0..12 {
            let c = (next(), next());
            let nearest = points
                .iter()
                .map(|p| {
                    let dx = (p.0 - c.0).abs();
                    let dy = (p.1 - c.1).abs();
                    let dx = dx.min(1.0 - dx);
                    let dy = dy.min(1.0 - dy);
                    dx * dx + dy * dy
                })
                .fold(f32::INFINITY, f32::min);
            if nearest > best_distance {
                best_distance = nearest;
                best = c;
            }
        }
        points.push(best);
    }
    points
        .iter()
        .map(|&(x, y)| {
            let qx = ((x * 65536.0) as u32).min(0xFFFF);
            let qy = ((y * 65536.0) as u32).min(0xFFFF);
            qx | qy << 16
        })
        .collect()
}

/// Where each Scatter of a stack is placed for one chunk.
#[derive(Clone, Debug)]
pub struct Windows {
    /// log2 of the footprint (and available-mask) texel size.
    pub tfs: u32,
    /// Per Scatter: cells whose copies are kept and committed.
    pub own: [Rect; 4],
    /// Per Scatter: cells whose candidates are evaluated (own + `d × r`).
    pub win: [Rect; 4],
    /// Per Scatter: available-mask texels it reads.
    pub avail: [Rect; 4],
    /// Per lattice Scatter: footprint-mask texels written (union of readers).
    pub mask: [Rect; 4],
    /// The lowest Scatter placed.
    pub lowest: usize,
}

/// Cells of a Scatter whose corner lies in a fixed-point rect.
pub fn cells_with_corner_in(spec: &Spec, fp: Rect) -> Rect {
    let c = spec.cell();
    let up = |v: i32| (v + c - 1) >> spec.cs;
    Rect::new(up(fp.x0), up(fp.y0), up(fp.x1), up(fp.y1))
}

/// Texels a bilinear read needs for every position of the cells in `cells`.
pub fn bilinear_texels(spec: &Spec, cells: Rect, tfs: u32) -> Rect {
    let half = 1 << (tfs - 1);
    let lo = |c: i32| ((c << spec.cs) - half) >> tfs;
    let hi = |c: i32| ((((c << spec.cs) - 1) - half) >> tfs) + 2;
    Rect::new(lo(cells.x0), lo(cells.y0), hi(cells.x1), hi(cells.y1))
}

/// Cells of Scatter `spec` whose copies can reach into the given mask texels.
pub fn cells_reaching(spec: &Spec, texels: Rect, tfs: u32) -> Rect {
    let size = 1 << tfs;
    let f = spec.footprint();
    let lo = |t: i32| (t * size + size / 2 - f) >> spec.cs;
    let hi = |t: i32| (((t - 1) * size + size / 2 + f) >> spec.cs) + 1;
    Rect::new(lo(texels.x0), lo(texels.y0), hi(texels.x1), hi(texels.y1))
}

/// Window A: every Scatter from `lowest` up is placed over the stacked
/// margins, so the chunk depends on no other chunk. `chunk` is in fixed point.
pub fn window_a(specs: &[Spec; 4], chunk: Rect, lowest: usize, tfs: u32) -> Windows {
    let mut w = Windows {
        tfs,
        own: [Rect::EMPTY; 4],
        win: [Rect::EMPTY; 4],
        avail: [Rect::EMPTY; 4],
        mask: [Rect::EMPTY; 4],
        lowest,
    };
    for k in (0..=lowest).rev() {
        let s = &specs[k];
        let mut own = cells_with_corner_in(s, chunk);
        if k < lowest {
            own = own.union(&cells_reaching(s, w.mask[k], tfs));
        }
        let win = own.grow(s.d() as i32 * s.r());
        let avail = bilinear_texels(s, win, tfs);
        for j in 0..k {
            w.mask[j] = w.mask[j].union(&avail);
        }
        w.own[k] = own;
        w.win[k] = win;
        w.avail[k] = avail;
    }
    w
}

/// Window B for Scatter `k`: only its own `d × r` margin; the masks above it
/// come from finished copies of the chunks around it.
pub fn window_b(specs: &[Spec; 4], chunk: Rect, k: usize, tfs: u32) -> Windows {
    let s = &specs[k];
    let own = cells_with_corner_in(s, chunk);
    let win = own.grow(s.d() as i32 * s.r());
    let avail = bilinear_texels(s, win, tfs);
    let mut w = Windows {
        tfs,
        own: [Rect::EMPTY; 4],
        win: [Rect::EMPTY; 4],
        avail: [Rect::EMPTY; 4],
        mask: [Rect::EMPTY; 4],
        lowest: k,
    };
    w.own[k] = own;
    w.win[k] = win;
    w.avail[k] = avail;
    for j in 0..k {
        w.mask[j] = avail;
    }
    w
}

/// Window with no margin at all: every Scatter evaluated over the chunk's
/// own cells only. Wrong at the borders; a timing baseline only.
pub fn window_none(specs: &[Spec; 4], chunk: Rect, tfs: u32) -> Windows {
    let mut w = Windows {
        tfs,
        own: [Rect::EMPTY; 4],
        win: [Rect::EMPTY; 4],
        avail: [Rect::EMPTY; 4],
        mask: [Rect::EMPTY; 4],
        lowest: GRASS,
    };
    for k in 0..4 {
        let s = &specs[k];
        let own = cells_with_corner_in(s, chunk);
        w.own[k] = own;
        w.win[k] = own;
        w.avail[k] = bilinear_texels(s, own, tfs);
    }
    for j in 0..3 {
        for k in j + 1..4 {
            w.mask[j] = w.mask[j].union(&w.avail[k]);
        }
    }
    w
}
