//! The integer placement maths shared by the CPU twin and the WGSL kernels.
//!
//! Every function here has a twin of the same name in `shaders/place.wgsl`.
//! Only `u32`/`i32` wrapping operations are used, so both sides agree bit
//! for bit. Positions are World fixed point, 1/256 m.

/// Fixed-point units per metre.
pub const FP: i32 = 256;

/// PCG hash (plant's `mirror::hash`).
pub fn hash(x: u32) -> u32 {
    let state = x.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277_803_737);
    (word >> 22) ^ word
}

/// Hash of two values (plant's `mirror::hash2`).
pub fn hash2(a: u32, b: u32) -> u32 {
    hash(a ^ hash(b))
}

pub const SALT_JX: u32 = 0x68E3_1DA4;
pub const SALT_JY: u32 = 0xB529_7A4D;
pub const SALT_RANK: u32 = 0x1B56_C4E9;
pub const SALT_SPECIES: u32 = 0x9E37_79B9;

/// Shade a tree canopy casts at full coverage, out of 255.
pub const SHADE: u32 = 150;

/// An inclusive-exclusive rectangle of cells or texels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Rect {
    pub fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Self { x0, y0, x1, y1 }
    }
    pub fn w(&self) -> i32 {
        self.x1 - self.x0
    }
    pub fn h(&self) -> i32 {
        self.y1 - self.y0
    }
    pub fn len(&self) -> usize {
        (self.w().max(0) * self.h().max(0)) as usize
    }
    pub fn grow(&self, n: i32) -> Self {
        Self::new(self.x0 - n, self.y0 - n, self.x1 + n, self.y1 + n)
    }
    pub fn union(&self, o: &Rect) -> Self {
        if self.len() == 0 {
            return *o;
        }
        if o.len() == 0 {
            return *self;
        }
        Self::new(
            self.x0.min(o.x0),
            self.y0.min(o.y0),
            self.x1.max(o.x1),
            self.y1.max(o.y1),
        )
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }
    pub const EMPTY: Rect = Rect {
        x0: 0,
        y0: 0,
        x1: 0,
        y1: 0,
    };
}

/// A dense grid of values over a rectangle. Reading outside it panics, which
/// is how the CPU twin catches a margin that is too small.
#[derive(Clone, Debug)]
pub struct Grid<T> {
    pub rect: Rect,
    pub data: Vec<T>,
}

impl<T: Copy> Grid<T> {
    pub fn new(rect: Rect, fill: T) -> Self {
        Self {
            rect,
            data: vec![fill; rect.len()],
        }
    }
    pub fn index(&self, x: i32, y: i32) -> usize {
        assert!(
            self.rect.contains(x, y),
            "read ({x}, {y}) outside {:?}",
            self.rect
        );
        ((y - self.rect.y0) * self.rect.w() + (x - self.rect.x0)) as usize
    }
    pub fn get(&self, x: i32, y: i32) -> T {
        self.data[self.index(x, y)]
    }
    pub fn set(&mut self, x: i32, y: i32, v: T) {
        let i = self.index(x, y);
        self.data[i] = v;
    }
}

/// Fixed-point bilinear read of an 8-bit grid whose texels are `1 << shift`
/// fixed-point units wide, at World position `p`. Texel `t` has its centre at
/// `t * size + size / 2`. Returns 0..=255.
pub fn bilinear(g: &Grid<u8>, shift: u32, px: i32, py: i32) -> u32 {
    bilinear_with(shift, px, py, |x, y| g.get(x, y) as u32)
}

/// [`bilinear`] over texel values given by `at`.
pub fn bilinear_with(shift: u32, px: i32, py: i32, at: impl Fn(i32, i32) -> u32) -> u32 {
    let half = 1i32 << (shift - 1);
    let ux = px.wrapping_sub(half);
    let uy = py.wrapping_sub(half);
    let tx = ux >> shift;
    let ty = uy >> shift;
    let wx = (((ux - (tx << shift)) as u32) << 8) >> shift;
    let wy = (((uy - (ty << shift)) as u32) << 8) >> shift;
    let top = at(tx, ty) * (256 - wx) + at(tx + 1, ty) * wx;
    let bot = at(tx, ty + 1) * (256 - wx) + at(tx + 1, ty + 1) * wx;
    (top * (256 - wy) + bot * wy) >> 16
}

/// Canopy shade (an influence mask texel) for a coverage texel.
pub fn influence(cov: u32) -> u32 {
    255 - cov * SHADE / 255
}

/// The hash of a cell of one Scatter.
pub fn cell_hash(seed: u32, sid: u32, i: i32, j: i32) -> u32 {
    hash2(hash2(seed, sid), hash2(i as u32, j as u32))
}

/// Position of a lattice cell's candidate: the cell corner plus a hashed
/// jitter of up to one cell. `cs` is log2 of the cell size in fixed point.
pub fn lattice_pos(h: u32, cs: u32, i: i32, j: i32) -> (i32, i32) {
    let jx = (hash(h ^ SALT_JX) >> (32 - cs)) as i32;
    let jy = (hash(h ^ SALT_JY) >> (32 - cs)) as i32;
    ((i << cs).wrapping_add(jx), (j << cs).wrapping_add(jy))
}

/// Position of point `point` of a tile cell. `tile` holds 16-bit coordinates
/// packed as `x | y << 16`; `cs` is log2 of the tile size in fixed point.
pub fn tile_pos(ch: u32, q: u32, cs: u32, i: i32, j: i32) -> (i32, i32) {
    let ox = ch >> 16;
    let oy = hash(ch) >> 16;
    let px = (((q & 0xFFFF) + ox) & 0xFFFF) >> (16 - cs);
    let py = (((q >> 16) + oy) & 0xFFFF) >> (16 - cs);
    ((i << cs) + px as i32, (j << cs) + py as i32)
}

/// One lattice candidate after the density threshold and the footing check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Cand {
    pub x: i32,
    pub y: i32,
    /// `u24 << 8 | local`.
    pub key: u32,
    /// Spacing in fixed point, with bit 31 set when alive.
    pub meta: u32,
}

impl Cand {
    pub fn alive(&self) -> bool {
        self.meta >> 31 != 0
    }
    pub fn spacing(&self) -> u32 {
        self.meta & 0x7FFF_FFFF
    }
}

/// Whether `b` ranks below `a`: `u_b / local_b < u_a / local_a`, compared by
/// cross-multiplying, ties broken by the cell (x then y).
pub fn lower(b: &Cand, bi: i32, bj: i32, a: &Cand, ai: i32, aj: i32) -> bool {
    let lb = b.key & 0xFF;
    let la = a.key & 0xFF;
    let l = (b.key >> 8) * la;
    let r = (a.key >> 8) * lb;
    l < r || (l == r && (bi < ai || (bi == ai && bj < aj)))
}

/// Whether two candidates are closer than the larger of their spacings.
pub fn conflict(a: &Cand, b: &Cand) -> bool {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    let s = a.spacing().max(b.spacing());
    ((dx * dx + dy * dy) as u32) < s * s
}

/// A hash mapped to `[0, 1)` (plant's `mirror::unit`). Only used to build
/// the grass tile on the CPU, which is then uploaded as integers.
pub fn unit(h: u32) -> f32 {
    (h >> 8) as f32 / 16_777_216.0
}
