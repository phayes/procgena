//! Repeatable surface patterns built from smooth random values.
//!
//! Noise layers add detail at different sizes. Triplanar projection blends
//! patterns from three directions so roughness can cover a 3D rock.

use glam::{Vec2, Vec3};

use crate::hash::pcg3d_unit;

/// Row-wise transform that rotates and slightly scales coordinates between noise
/// layers, changing the alignment of successive details.
pub(crate) const FBM_ROTATION: [[f32; 2]; 2] = [[0.87, 0.48], [-0.48, 0.87]];
/// Coordinate offset applied after doubling the scale and before rotating each
/// noise layer, so layers sample different parts of the pattern.
pub(crate) const FBM_OFFSET: f32 = 100.0;
/// Minimum weight for each projection before the three weights are scaled to
/// sum to one. Even a surface facing one axis retains a small blend of the others.
pub(crate) const TRIPLANAR_MIN_WEIGHT: f32 = 0.001;

/// Blends `a` and `b`: `t = 0` returns `a`, and `t = 1` returns `b`.
/// Values outside that interval extrapolate beyond the endpoints.
fn lerp(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    a + (b - a) * t
}

/// Samples three smooth random channels at 2D position `p` using `seed`.
///
/// For ordinary finite coordinates, each channel lies in `[0, 1)`. Values at
/// integer grid corners are smoothly blended and then squared, favoring lower
/// values. Coordinate conversion saturates outside the `i32` range.
pub(crate) fn value_noise(p: Vec2, seed: u32) -> Vec3 {
    let ip = p.floor();
    let f = p - ip;
    let u = f * f * (Vec2::splat(3.0) - 2.0 * f);

    // Convert grid coordinates to signed integers, then preserve their bit
    // patterns as u32 values. The first cast saturates outside the i32 range.
    let x = ip.x as i32 as u32;
    let y = ip.y as i32 as u32;
    let corner =
        |dx: u32, dy: u32| Vec3::from(pcg3d_unit([x.wrapping_add(dx), y.wrapping_add(dy), seed]));

    let res = lerp(
        lerp(corner(0, 0), corner(1, 0), u.x),
        lerp(corner(0, 1), corner(1, 1), u.x),
        u.y,
    );
    res * res
}

/// Combines `octaves` noise layers into three channels of progressively finer detail.
///
/// Each layer roughly doubles the frequency and halves the strength. Zero
/// layers returns zero. For positive layer counts, the theoretical range of
/// each channel is `[0, 1 - 0.5^octaves)`; floating-point rounding may affect
/// that bound.
pub(crate) fn fbm(mut x: Vec2, seed: u32, octaves: u32) -> Vec3 {
    let [[r00, r01], [r10, r11]] = FBM_ROTATION;
    let mut v = Vec3::ZERO;
    let mut a = 0.5;
    for _ in 0..octaves {
        v += a * value_noise(x, seed);
        let y = x * 2.0 + Vec2::splat(FBM_OFFSET);
        x = Vec2::new(r00 * y.x + r01 * y.y, r10 * y.x + r11 * y.y);
        a *= 0.5;
    }
    v
}

/// Seed, pattern frequency, and layer count used to sample surface roughness.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Roughness {
    pub(crate) seed: u32,
    pub(crate) spread: f32,
    pub(crate) octaves: u32,
}

impl Roughness {
    /// Samples roughness at 3D position `p`, blending patterns on the yz, zx, and xy planes.
    ///
    /// This is triplanar projection: surfaces facing each axis favor the pattern
    /// on the corresponding plane. Pass a finite surface direction as `n`; it
    /// need not have unit length. A zero direction gives all planes equal weight.
    pub(crate) fn triplanar(&self, p: Vec3, n: Vec3) -> Vec3 {
        let w = (n * n).max(Vec3::splat(TRIPLANAR_MIN_WEIGHT));
        let w = w / (w.x + w.y + w.z);
        let height = |uv: Vec2| fbm(self.spread * uv, self.seed, self.octaves);
        height(Vec2::new(p.y, p.z)) * w.x
            + height(Vec2::new(p.z, p.x)) * w.y
            + height(Vec2::new(p.x, p.y)) * w.z
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_noise_hits_lattice_values_and_stays_in_range() {
        // At a lattice corner the result is that corner's random value, squared.
        let corner = Vec3::from(pcg3d_unit([(-3i32) as u32, 5, 9]));
        assert_eq!(value_noise(Vec2::new(-3.0, 5.0), 9), corner * corner);

        for i in 0..1000 {
            let p = Vec2::new(i as f32 * 0.137 - 60.0, i as f32 * -0.291 + 20.0);
            let n = value_noise(p, 1);
            assert!(
                n.min_element() >= 0.0 && n.max_element() < 1.0,
                "{p} -> {n}"
            );
        }
    }

    #[test]
    fn value_noise_is_continuous_across_cells() {
        let below = value_noise(Vec2::new(2.0 - 1e-4, 0.5), 3);
        let above = value_noise(Vec2::new(2.0, 0.5), 3);
        assert!((below - above).abs().max_element() < 1e-3);
    }

    #[test]
    fn fbm_depends_on_seed_and_octaves() {
        let p = Vec2::new(1.3, -2.7);
        assert_ne!(fbm(p, 1, 6), fbm(p, 2, 6));
        assert_ne!(fbm(p, 1, 6), fbm(p, 1, 5));
        let v = fbm(p, 1, 6);
        assert!(v.min_element() >= 0.0 && v.max_element() < 1.0 - 0.5f32.powi(6));
    }

    #[test]
    fn triplanar_uses_the_facing_plane() {
        let r = Roughness {
            seed: 4,
            spread: 10.0,
            octaves: 6,
        };
        let p = Vec3::new(0.11, -0.23, 0.37);
        // Facing +z, the xy-plane projection dominates (weights 0.001 : 0.001 : 1).
        let facing_z = r.triplanar(p, Vec3::Z);
        let xy = fbm(10.0 * Vec2::new(p.x, p.y), 4, 6);
        assert!((facing_z - xy).abs().max_element() < 0.01);
        // A zero direction blends all three planes equally.
        let blended = r.triplanar(p, Vec3::ZERO);
        let yz = fbm(10.0 * Vec2::new(p.y, p.z), 4, 6);
        let zx = fbm(10.0 * Vec2::new(p.z, p.x), 4, 6);
        assert!((blended - (xy + yz + zx) / 3.0).abs().max_element() < 1e-5);
    }
}
