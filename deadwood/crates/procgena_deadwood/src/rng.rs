//! Repeatable random numbers and smooth noise.
//!
//! Everything here is "pseudo-random": it looks random but is worked out
//! from a seed, so the same seed always gives the same piece of wood.

use glam::Vec3;

/// Multiplier applied to each input before mixing in [`pcg3d`].
const PCG_MULTIPLIER: u32 = 1_664_525;
/// Increment added to each input before mixing in [`pcg3d`].
const PCG_INCREMENT: u32 = 1_013_904_223;

/// Turns three integers into three repeatable pseudo-random integers.
///
/// Uses the PCG3D hash from Jarzynski and Olano, "Hash Functions for GPU
/// Rendering" (2020), with wrapping 32-bit arithmetic.
pub(crate) fn pcg3d(v: [u32; 3]) -> [u32; 3] {
    let [mut x, mut y, mut z] =
        v.map(|c| c.wrapping_mul(PCG_MULTIPLIER).wrapping_add(PCG_INCREMENT));

    x = x.wrapping_add(y.wrapping_mul(z));
    y = y.wrapping_add(z.wrapping_mul(x));
    z = z.wrapping_add(x.wrapping_mul(y));

    x ^= x >> 16;
    y ^= y >> 16;
    z ^= z >> 16;

    x = x.wrapping_add(y.wrapping_mul(z));
    y = y.wrapping_add(z.wrapping_mul(x));
    z = z.wrapping_add(x.wrapping_mul(y));

    [x, y, z]
}

/// Converts a hash to a number from 0 (inclusive) to 1 (exclusive), using its
/// upper 24 bits so the result is exact in an `f32`.
pub(crate) fn unit_float(h: u32) -> f32 {
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

/// Mixes two numbers into one seed, so that sub-parts of a piece (a stub, a
/// root) get their own independent random streams.
pub(crate) fn sub_seed(seed: u32, id: u32) -> u32 {
    pcg3d([seed, id, 0x5eed_0001])[0]
}

/// A small seeded random number stream. The same seed and stream id always
/// give the same values.
#[derive(Debug, Clone)]
pub(crate) struct Rng {
    seed: u32,
    stream: u32,
    counter: u32,
}

impl Rng {
    /// Starts stream `stream` for `seed`. Different streams of one seed are
    /// independent, so adding draws to one never shifts another.
    pub(crate) fn new(seed: u32, stream: u32) -> Self {
        Self {
            seed,
            stream,
            counter: 0,
        }
    }

    /// Returns the next value in `[0, 1)`.
    pub(crate) fn next_f32(&mut self) -> f32 {
        let h = pcg3d([self.counter, self.seed, self.stream]);
        self.counter = self.counter.wrapping_add(1);
        unit_float(h[0])
    }

    /// Returns the next value in `[lo, hi)`.
    pub(crate) fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }

    /// Returns the next value in `[1 - amount, 1 + amount)`.
    pub(crate) fn jitter(&mut self, amount: f32) -> f32 {
        self.range(1.0 - amount, 1.0 + amount)
    }

    /// Returns true with probability `p`.
    pub(crate) fn chance(&mut self, p: f32) -> bool {
        self.next_f32() < p
    }
}

/// Returns smooth random noise at point `p`, from -1 to 1: a value that
/// changes gradually as `p` moves, with features about one unit apart.
///
/// `octaves` layers are added, each with features half the size and half
/// the strength of the one before, for finer detail. (The name stands for
/// "fractal Brownian motion", the usual term for noise layered this way.)
/// `seed` picks an independent pattern. Zero octaves gives 0.
pub(crate) fn fbm(p: Vec3, seed: u32, octaves: u32) -> f32 {
    let mut total = 0.0;
    let mut weight = 1.0;
    let mut norm = 0.0;
    let mut q = p;
    for octave in 0..octaves {
        total += value_noise(q, seed.wrapping_add(octave.wrapping_mul(0x9e37_79b9))) * weight;
        norm += weight;
        weight *= 0.5;
        q *= 2.0;
    }
    if norm > 0.0 { total / norm } else { 0.0 }
}

/// Returns one layer of noise, from -1 to 1: random values at whole-number
/// grid points, blended smoothly in between.
pub(crate) fn value_noise(p: Vec3, seed: u32) -> f32 {
    let cell = p.floor();
    let f = p - cell;
    let w = f * f * (Vec3::splat(3.0) - 2.0 * f);
    let [cx, cy, cz] = [cell.x as i32, cell.y as i32, cell.z as i32];

    // Blend the random values at the eight corners of the grid cell around
    // `p`.
    let corner = |dx: i32, dy: i32, dz: i32| {
        let h = pcg3d([
            (cx + dx) as u32 ^ seed,
            (cy + dy) as u32,
            ((cz + dz) as u32).wrapping_add(seed.rotate_left(13)),
        ]);
        unit_float(h[0]) * 2.0 - 1.0
    };
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = lerp(corner(0, 0, 0), corner(1, 0, 0), w.x);
    let x10 = lerp(corner(0, 1, 0), corner(1, 1, 0), w.x);
    let x01 = lerp(corner(0, 0, 1), corner(1, 0, 1), w.x);
    let x11 = lerp(corner(0, 1, 1), corner(1, 1, 1), w.x);
    lerp(lerp(x00, x10, w.y), lerp(x01, x11, w.y), w.z)
}

/// Returns how far `p` is from the nearest border between random cells.
///
/// Space is split into cells around randomly placed centres, one centre per
/// unit cube. The result is the gap between the distances to the nearest
/// and second-nearest centre. It is 0 on a border and grows toward a cell's
/// middle, so `1 - smoothstep(0, width, ..)` draws thin cracks between
/// plates.
pub(crate) fn cell_border(p: Vec3, seed: u32) -> f32 {
    let cell = p.floor();
    let [cx, cy, cz] = [cell.x as i32, cell.y as i32, cell.z as i32];
    let (mut f1, mut f2) = (f32::MAX, f32::MAX);

    // Check the jittered centres of the 27 cells around `p`.
    for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let h = pcg3d([
                    (cx + dx) as u32 ^ seed,
                    (cy + dy) as u32,
                    ((cz + dz) as u32).wrapping_add(seed.rotate_left(7)),
                ]);
                let centre = Vec3::new(
                    (cx + dx) as f32 + unit_float(h[0]),
                    (cy + dy) as f32 + unit_float(h[1]),
                    (cz + dz) as f32 + unit_float(h[2]),
                );
                let d = centre.distance(p);
                if d < f1 {
                    f2 = f1;
                    f1 = d;
                } else if d < f2 {
                    f2 = d;
                }
            }
        }
    }
    f2 - f1
}

/// Returns 0 below `edge0`, 1 above `edge1`, and a smooth S-curve between.
pub(crate) fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_repeat_and_differ() {
        let draw = |seed, stream| {
            let mut r = Rng::new(seed, stream);
            (0..8).map(|_| r.next_f32()).collect::<Vec<_>>()
        };
        assert_eq!(draw(3, 1), draw(3, 1));
        assert_ne!(draw(3, 1), draw(3, 2));
        assert_ne!(draw(3, 1), draw(4, 1));
    }

    #[test]
    fn noise_is_bounded_and_continuous() {
        for i in 0..200 {
            let p = Vec3::new(i as f32 * 0.137, i as f32 * -0.071, i as f32 * 0.29);
            let a = fbm(p, 5, 3);
            let b = fbm(p + Vec3::splat(1e-4), 5, 3);
            assert!((-1.0..=1.0).contains(&a));
            assert!((a - b).abs() < 1e-2);
        }
    }

    #[test]
    fn cell_border_is_non_negative() {
        for i in 0..200 {
            let p = Vec3::new(i as f32 * 0.31, i as f32 * 0.17, i as f32 * -0.23);
            assert!(cell_border(p, 9) >= 0.0);
        }
    }
}
