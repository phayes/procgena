//! Gullies: grooves running down valley walls towards a river.
//!
//! Written from the idea in Rune Skovbo Johansen's erosion filter (stripes
//! that follow the direction water flows, from jittered cells blended
//! together), not from its code. Here the direction is towards the river,
//! so every gully drains into it.

use std::f64::consts::TAU;

use glam::DVec2;

use crate::hash::{GULLY_CELLS, hash2, salt};

/// Seed for the gully cells' jitter: the same in every world (see
/// [`GULLY_CELLS`]).
const GULLY_SEED: u32 = salt(GULLY_CELLS);

/// How far the cells' pivots may move from the cell centres, as a share of
/// the cell size.
const JITTER: f64 = 0.3;

/// How far a pivot's stripe reaches, in cells. With [`JITTER`] it always
/// covers the whole of its own cell, and never reaches past the cells
/// next to it, so the 3 × 3 cells around a point hold every stripe that
/// counts there.
const STRIPE_REACH: f64 = 1.2;

/// The least amplitude the blended stripes are scaled back up from, as a
/// share of their weight. Where neighbouring cells' stripes cancel out,
/// this keeps the result from being blown up into noise.
const MIN_AMPLITUDE: f64 = 0.5;

/// Gully stripes at `p`: about 1 along a gully's floor, −1 along the spur
/// between two gullies, and in between elsewhere. Gullies run along `flow`
/// (a unit vector, the way water runs here) and are about `spacing`
/// metres apart. The pattern depends only on `p`, `flow`, `spacing` and
/// `seed`, so it is the same wherever it is evaluated.
pub(crate) fn stripes(p: DVec2, flow: DVec2, spacing: f64, seed: u32) -> f64 {
    // The stripe phase grows across the flow.
    let across = flow.perp();
    let cell = (p / spacing).floor();
    let cutoff = (-2.0 * STRIPE_REACH * STRIPE_REACH).exp();
    let reach_squared = (STRIPE_REACH * spacing).powi(2);
    let (mut c, mut s, mut total) = (0.0, 0.0, 0.0);
    for dz in -1..=1 {
        for dx in -1..=1 {
            // This cell's pivot, jittered from its centre.
            let k = cell + DVec2::new(f64::from(dx), f64::from(dz));
            // `seed` picks one octave of the fixed pattern, a second level
            // under `GULLY_SEED`, not a world seed.
            let h = hash2(k.x as i64, k.y as i64, seed ^ GULLY_SEED);
            let jitter = DVec2::new(
                f64::from(h & 0xffff) / 65535.0 - 0.5,
                f64::from(h >> 16) / 65535.0 - 0.5,
            ) * (2.0 * JITTER);
            let pivot = (k + DVec2::splat(0.5) + jitter) * spacing;

            // Its stripe through the pivot, weighted by distance, falling to
            // nothing at STRIPE_REACH cells.
            let offset = p - pivot;
            let distance_squared = offset.length_squared();
            if distance_squared >= reach_squared {
                continue;
            }
            let weight = (-2.0 * distance_squared / (spacing * spacing)).exp() - cutoff;
            let (sin, cos) = (TAU * offset.dot(across) / spacing).sin_cos();
            c += weight * cos;
            s += weight * sin;
            total += weight;
        }
    }

    // Blending stripes out of phase shrinks them; scale back to full size.
    c / c.hypot(s).max(MIN_AMPLITUDE * total)
}

/// The gradient of [`stripes`] at `p`, where `flow` itself changes with
/// `p`: `flow_slope[0]` is how `flow` changes along x, and `flow_slope[1]`
/// along z.
pub(crate) fn stripes_slope(
    p: DVec2,
    flow: DVec2,
    flow_slope: [DVec2; 2],
    spacing: f64,
    seed: u32,
) -> DVec2 {
    let across = flow.perp();
    let across_slope = flow_slope.map(DVec2::perp);
    let cell = (p / spacing).floor();
    let cutoff = (-2.0 * STRIPE_REACH * STRIPE_REACH).exp();
    let reach_squared = (STRIPE_REACH * spacing).powi(2);
    let (mut c, mut s, mut total) = (0.0, 0.0, 0.0);
    let (mut dc, mut ds, mut dtotal) = (DVec2::ZERO, DVec2::ZERO, DVec2::ZERO);
    for dz in -1..=1 {
        for dx in -1..=1 {
            // The same pivots as `stripes`.
            let k = cell + DVec2::new(f64::from(dx), f64::from(dz));
            let h = hash2(k.x as i64, k.y as i64, seed ^ GULLY_SEED);
            let jitter = DVec2::new(
                f64::from(h & 0xffff) / 65535.0 - 0.5,
                f64::from(h >> 16) / 65535.0 - 0.5,
            ) * (2.0 * JITTER);
            let pivot = (k + DVec2::splat(0.5) + jitter) * spacing;
            let offset = p - pivot;
            let distance_squared = offset.length_squared();
            if distance_squared >= reach_squared {
                continue;
            }

            // The weight and the phase, with their slopes.
            let fall = (-2.0 * distance_squared / (spacing * spacing)).exp();
            let weight = fall - cutoff;
            let weight_slope = offset * (-4.0 * fall / (spacing * spacing));
            let phase = TAU * offset.dot(across) / spacing;
            let phase_slope = (across
                + DVec2::new(offset.dot(across_slope[0]), offset.dot(across_slope[1])))
                * (TAU / spacing);
            let (sin, cos) = phase.sin_cos();
            c += weight * cos;
            s += weight * sin;
            total += weight;
            dc += weight_slope * cos - phase_slope * (weight * sin);
            ds += weight_slope * sin + phase_slope * (weight * cos);
            dtotal += weight_slope;
        }
    }

    // The slope of `c / max(hypot(c, s), MIN_AMPLITUDE · total)`.
    let length = c.hypot(s);
    let floor = MIN_AMPLITUDE * total;
    let (scale, scale_slope) = if length >= floor {
        (length, (dc * c + ds * s) / length)
    } else {
        (floor, dtotal * MIN_AMPLITUDE)
    };
    (dc * scale - scale_slope * c) / (scale * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stripes_run_along_the_flow() {
        // Moving along the flow changes the stripes far less than moving
        // the same distance across it.
        let flow = DVec2::new(0.6, 0.8);
        let (mut along, mut across) = (0.0, 0.0);
        for k in 0..200 {
            let p = DVec2::new(f64::from(k) * 7.3, f64::from(k % 13) * 11.0);
            let here = stripes(p, flow, 30.0, 1);
            along += (stripes(p + flow * 3.0, flow, 30.0, 1) - here).abs();
            across += (stripes(p + flow.perp() * 3.0, flow, 30.0, 1) - here).abs();
        }
        assert!(along * 3.0 < across, "along {along}, across {across}");
    }

    #[test]
    fn stripes_are_continuous_across_cell_lines() {
        // Step finely across many cell lines; the stripes never jump.
        let flow = DVec2::new(0.8, -0.6);
        let spacing = 40.0;
        for row in 0..20 {
            let x = 13.7 * f64::from(row);
            let mut last = stripes(DVec2::new(x, -400.0), flow, spacing, 3);
            for k in 1..=80_000 {
                let p = DVec2::new(x, -400.0 + f64::from(k) * 0.01);
                let v = stripes(p, flow, spacing, 3);
                assert!((v - last).abs() < 0.01, "jump {} at {p}", (v - last).abs());
                last = v;
            }
        }
    }

    #[test]
    fn stripes_slope_matches_differences() {
        // A flow that turns with the point, like the way to a river.
        let towards = DVec2::new(30.0, -500.0);
        let flow_at = |p: DVec2| (towards - p).normalize();
        let flow_slope = |p: DVec2| {
            let n = towards - p;
            let f = n.normalize();
            [DVec2::X, DVec2::Y].map(|e| (-e - f * f.dot(-e)) / n.length())
        };
        let h = 1e-4;
        for k in 0..500 {
            let p = DVec2::new(f64::from(k) * 3.7 - 600.0, f64::from(k % 41) * 9.1);
            let f = |q: DVec2| stripes(q, flow_at(q), 40.0, 2);
            let numeric = DVec2::new(
                (f(p + DVec2::X * h) - f(p - DVec2::X * h)) / (2.0 * h),
                (f(p + DVec2::Y * h) - f(p - DVec2::Y * h)) / (2.0 * h),
            );
            let analytic = stripes_slope(p, flow_at(p), flow_slope(p), 40.0, 2);
            assert!(
                analytic.distance(numeric) < 1e-5 * (1.0 + numeric.length()),
                "{p}: {analytic} vs {numeric}"
            );
        }
    }

    #[test]
    fn stripes_stay_in_range() {
        for k in 0..1000 {
            let p = DVec2::new(f64::from(k) * 3.7, f64::from(k) * -2.9);
            let v = stripes(p, DVec2::X, 25.0, 7);
            assert!((-1.0..=1.0).contains(&v), "{v}");
        }
    }
}
