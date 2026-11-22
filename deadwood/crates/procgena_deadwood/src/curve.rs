//! Centre lines of stems: smooth curves that a stem's cross-section is swept
//! along to make its surface.
//!
//! Every sample on a curve carries a "frame": the direction along the curve
//! plus two directions across it, which say where angle 0 and angle 90° of
//! the cross-section point. The frames are carried along the curve by
//! "parallel transport": each one is turned only as much as the curve
//! itself turns. This keeps the cross-section from twisting around the
//! stem, so bark patterns and bumps don't spiral or smear.

use glam::{Mat3, Vec3};

/// A position on a curve with its local directions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Frame {
    /// Point on the centre line.
    pub position: Vec3,
    /// Unit direction of the curve.
    pub tangent: Vec3,
    /// Unit direction across the curve; angle 0 of a cross-section.
    pub normal: Vec3,
    /// `tangent × normal`; angle 90° of a cross-section.
    pub binormal: Vec3,
}

impl Frame {
    /// Returns the unit direction across the curve at angle `theta`, in
    /// radians, measured from [`normal`](Self::normal) toward
    /// [`binormal`](Self::binormal).
    pub(crate) fn radial(&self, theta: f32) -> Vec3 {
        self.normal * theta.cos() + self.binormal * theta.sin()
    }
}

/// A centre line sampled at evenly spaced parameter values from 0 (start)
/// to 1 (end), with a parallel-transported frame at each sample.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Curve {
    frames: Vec<Frame>,
    length: f32,
}

impl Curve {
    /// Builds a smooth curve that passes through every one of `points` (at
    /// least two), sampled `samples` times (at least two). The samples are
    /// evenly spaced by distance along the curve. `up` picks the first
    /// frame's normal: the direction across the curve closest to `up`.
    pub(crate) fn through(points: &[Vec3], samples: usize, up: Vec3) -> Self {
        debug_assert!(points.len() >= 2 && samples >= 2);
        let segments = points.len() - 1;
        let spline = |x: f32| {
            let u = x * segments as f32;
            let k = (u.floor() as usize).min(segments - 1);
            let t = u - k as f32;
            let p = |j: isize| {
                let j = (k as isize + j).clamp(0, segments as isize) as usize;
                points[j]
            };
            catmull_rom(p(-1), p(0), p(1), p(2), t)
        };

        // Sample the spline finely, then pick points evenly spaced by
        // distance, so the curve's parameter is proportional to length.
        let fine_count = samples * 8;
        let fine: Vec<Vec3> = (0..=fine_count)
            .map(|i| spline(i as f32 / fine_count as f32))
            .collect();
        let mut along = vec![0.0f32; fine.len()];
        for i in 1..fine.len() {
            along[i] = along[i - 1] + fine[i].distance(fine[i - 1]);
        }
        let total = along[fine.len() - 1];
        let mut k = 0;
        let positions: Vec<Vec3> = (0..samples)
            .map(|i| {
                let target = total * i as f32 / (samples - 1) as f32;
                while k + 2 < fine.len() && along[k + 1] < target {
                    k += 1;
                }
                let span = (along[k + 1] - along[k]).max(1e-12);
                let t = ((target - along[k]) / span).clamp(0.0, 1.0);
                fine[k].lerp(fine[k + 1], t)
            })
            .collect();
        Self::from_positions(positions, up)
    }

    /// Builds a curve from sampled positions, computing tangents, frames and
    /// length. `up` picks the first frame's normal as in
    /// [`through`](Self::through).
    pub(crate) fn from_positions(positions: Vec<Vec3>, up: Vec3) -> Self {
        let n = positions.len();
        let tangent = |i: usize| {
            let (a, b) = (
                positions[i.saturating_sub(1)],
                positions[(i + 1).min(n - 1)],
            );
            (b - a).normalize_or(Vec3::X)
        };

        // Carry the first normal along the curve, turning it only as much as
        // the tangent turns.
        let first = tangent(0);
        let mut normal = perpendicular(up, first);
        let mut frames = Vec::with_capacity(n);
        let mut previous = first;
        for (i, &position) in positions.iter().enumerate() {
            let t = tangent(i);
            normal = transport(normal, previous, t);
            frames.push(Frame {
                position,
                tangent: t,
                normal,
                binormal: t.cross(normal),
            });
            previous = t;
        }
        let length = positions.windows(2).map(|w| w[0].distance(w[1])).sum();
        Self { frames, length }
    }

    /// Length along the curve, in metres.
    pub(crate) fn length(&self) -> f32 {
        self.length
    }

    /// The sampled frames, from start to end.
    pub(crate) fn frames(&self) -> &[Frame] {
        &self.frames
    }

    /// Returns the frame at parameter `u`, clamped to 0 through 1,
    /// blending between the nearest samples.
    pub(crate) fn at(&self, u: f32) -> Frame {
        let last = self.frames.len() - 1;
        let x = u.clamp(0.0, 1.0) * last as f32;
        let i = (x.floor() as usize).min(last - 1);
        let t = x - i as f32;
        let (a, b) = (&self.frames[i], &self.frames[i + 1]);
        let tangent = a.tangent.lerp(b.tangent, t).normalize_or(a.tangent);
        let normal = perpendicular(a.normal.lerp(b.normal, t), tangent);
        Frame {
            position: a.position.lerp(b.position, t),
            tangent,
            normal,
            binormal: tangent.cross(normal),
        }
    }

    /// Rotates and moves the whole curve: every point `p` becomes
    /// `rotation * p + translation`.
    pub(crate) fn transform(&mut self, rotation: Mat3, translation: Vec3) {
        for f in &mut self.frames {
            f.position = rotation * f.position + translation;
            f.tangent = rotation * f.tangent;
            f.normal = rotation * f.normal;
            f.binormal = rotation * f.binormal;
        }
    }

    /// Moves each sample by its entry in `offsets` (one per sample), then
    /// recomputes directions and length. The first frame keeps its normal as
    /// nearly as it can, so patterns around the stem stay in place.
    pub(crate) fn displace(&mut self, offsets: &[Vec3]) {
        debug_assert_eq!(offsets.len(), self.frames.len());
        let up = self.frames[0].normal;
        let positions = self
            .frames
            .iter()
            .zip(offsets)
            .map(|(f, &d)| f.position + d)
            .collect();
        *self = Self::from_positions(positions, up);
    }

    /// Number of samples.
    pub(crate) fn sample_count(&self) -> usize {
        self.frames.len()
    }
}

/// Returns the unit vector across `tangent` that is closest to `v`, or any
/// unit vector across it if `v` is parallel to it.
pub(crate) fn perpendicular(v: Vec3, tangent: Vec3) -> Vec3 {
    let p = v - tangent * v.dot(tangent);
    if p.length_squared() > 1e-10 {
        p.normalize()
    } else {
        tangent.any_orthonormal_vector()
    }
}

/// Turns `normal` by the rotation that takes direction `from` to `to`.
fn transport(normal: Vec3, from: Vec3, to: Vec3) -> Vec3 {
    let axis = from.cross(to);
    let sin = axis.length();
    let n = if sin < 1e-7 {
        normal
    } else {
        let axis = axis / sin;
        let cos = from.dot(to).clamp(-1.0, 1.0);
        normal * cos + axis.cross(normal) * sin + axis * axis.dot(normal) * (1.0 - cos)
    };
    perpendicular(n, to)
}

/// Returns the point at `t` (0 to 1) between `p1` and `p2` on a Catmull-Rom
/// spline: a smooth curve through `p0`, `p1`, `p2` and `p3` whose direction
/// at each point follows its neighbours.
fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let (t2, t3) = (t * t, t * t * t);
    0.5 * (2.0 * p1
        + (p2 - p0) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_curve_has_its_length_and_steady_frames() {
        let c = Curve::through(&[Vec3::ZERO, Vec3::new(2.0, 0.0, 0.0)], 9, Vec3::Y);
        assert!((c.length() - 2.0).abs() < 1e-5);
        for f in c.frames() {
            assert!((f.tangent - Vec3::X).length() < 1e-5);
            assert!((f.normal - Vec3::Y).length() < 1e-5);
        }
        let mid = c.at(0.5);
        assert!((mid.position - Vec3::new(1.0, 0.0, 0.0)).length() < 1e-5);
    }

    #[test]
    fn frames_stay_orthonormal_on_a_bend() {
        let pts = [
            Vec3::ZERO,
            Vec3::new(1.0, 0.3, 0.0),
            Vec3::new(2.0, 0.0, 0.5),
            Vec3::new(3.0, -0.2, 0.2),
        ];
        let c = Curve::through(&pts, 40, Vec3::Y);
        for f in c.frames() {
            assert!((f.tangent.length() - 1.0).abs() < 1e-4);
            assert!((f.normal.length() - 1.0).abs() < 1e-4);
            assert!(f.tangent.dot(f.normal).abs() < 1e-4);
        }
    }
}
