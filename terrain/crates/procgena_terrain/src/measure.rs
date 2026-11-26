//! Measures of the ground at one point: how steep and how curved it is, and
//! how near water and roads are.

use glam::Vec2;

use crate::carve::WATER_MIX;
use crate::ports::BANK_REACH;

/// How far, in metres, [`Measures`] look for water and roads. A distance
/// of this much means "this far or further".
///
/// Every terrain sees the same water and roads within this distance of a
/// shared border, so the measures agree on both sides of it.
pub const MEASURE_REACH: f32 = BANK_REACH as f32;

/// How far, in metres, [`Measures::height_above_water`] looks for water:
/// [`MEASURE_REACH`] less the 10 m over which nearby waters' surfaces mix.
pub const WATER_HEIGHT_REACH: f32 = MEASURE_REACH - WATER_MIX as f32;

/// The step, in metres, at which [`Measures::curvature`] compares the
/// ground's height with its surroundings.
pub const CURVATURE_STEP: f64 = 4.0;

/// How much [`Terrain::query`](crate::Terrain::query) works out about a
/// point. Each level includes everything the levels before it give.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum SampleDetail {
    /// The height, water and road tags: the cheapest.
    #[default]
    Height,
    /// Also the gradient: how much the ground rises per metre along x and
    /// z.
    Gradient,
    /// Also the [`Measures`]: about four more heights than
    /// [`Gradient`](Self::Gradient).
    Measures,
    /// Also the [`Labels`](crate::Labels). The costliest: about eight more
    /// heights than [`Measures`](Self::Measures).
    Labels,
}

/// What the ground is like at one point, beyond its height (see
/// [`Terrain::query`](crate::Terrain::query)).
///
/// Every measure changes smoothly from point to point, without jumps,
/// and agrees on both sides of a border between two terrains.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Measures {
    /// How steep the ground is, as the sine of its angle from flat: 0 flat,
    /// 0.5 at 30°, 1 for a wall.
    pub slope: f32,
    /// How the ground bends, per metre: positive where it is convex (ridges,
    /// hilltops, the tops of banks), negative where it is concave (valley
    /// floors, hollows, the feet of banks), 0 on a plane. It is the ground's
    /// height minus the mean of the four heights [`CURVATURE_STEP`] away
    /// along x and z, divided by a quarter of the step squared, with rivers,
    /// ponds and roads cut in.
    pub curvature: f32,
    /// How far the ground lies above the nearby water's surface, in metres:
    /// negative under water. The surface is the nearest water's, mixed a
    /// little with those of other waters nearly as near (within 10 m of
    /// it), so it moves smoothly where two waters meet. Where waters at
    /// different levels meet, it can lie below the surface drawn there,
    /// which is the higher one. `None` where no water is
    /// within [`WATER_HEIGHT_REACH`].
    pub height_above_water: Option<f32>,
    /// How far it is, in metres, to the edge of the nearest river or pond:
    /// 0 in the water, and at most [`MEASURE_REACH`].
    pub water_distance: f32,
    /// How far it is, in metres, to the edge of the nearest road's surface:
    /// 0 on the road, and at most [`MEASURE_REACH`].
    pub road_distance: f32,
}

/// The [`Measures`] read from the features near a point alone, with the
/// same values: no extra heights are read for them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NearbyMeasures {
    /// As [`Measures::height_above_water`].
    pub height_above_water: Option<f32>,
    /// As [`Measures::water_distance`].
    pub water_distance: f32,
    /// As [`Measures::road_distance`].
    pub road_distance: f32,
}

/// The slope of ground whose gradient is `gradient`, as the sine of its
/// angle from flat (see [`Measures::slope`]). A gradient that isn't a
/// number gives 0, and an infinite one 1.
pub(crate) fn slope_of(gradient: Vec2) -> f32 {
    let g = gradient.length();
    if g.is_nan() {
        0.0
    } else if g.is_infinite() {
        1.0
    } else {
        g / g.hypot(1.0)
    }
}

/// The curvature (see [`Measures::curvature`]) at a point of height
/// `centre` whose four neighbours [`CURVATURE_STEP`] away along x and z
/// have heights `around`.
pub(crate) fn curvature_of(centre: f32, around: [f32; 4]) -> f32 {
    let mean = around.iter().sum::<f32>() / 4.0;
    let quarter_step_squared = (CURVATURE_STEP * CURVATURE_STEP / 4.0) as f32;
    (centre - mean) / quarter_step_squared
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The curvature of `height` at the origin.
    fn curvature_at(height: impl Fn(f64, f64) -> f64) -> f32 {
        let s = CURVATURE_STEP;
        let around = [(s, 0.0), (-s, 0.0), (0.0, s), (0.0, -s)].map(|(x, z)| height(x, z) as f32);
        curvature_of(height(0.0, 0.0) as f32, around)
    }

    #[test]
    fn curvature_is_the_bend_per_metre() {
        // Planes don't bend.
        assert_eq!(curvature_at(|x, z| 3.0 + 0.4 * x - 0.2 * z), 0.0);
        // A ridge falling by k x² / 2 across it bends by k; a dome bends
        // both ways, twice as much; a trough the other way.
        let k = 0.03;
        assert!((curvature_at(|x, _| -k * x * x / 2.0) - k as f32).abs() < 1e-6);
        assert!((curvature_at(|x, z| -k * (x * x + z * z) / 2.0) - 2.0 * k as f32).abs() < 1e-6);
        assert!((curvature_at(|_, z| k * z * z / 2.0) + k as f32).abs() < 1e-6);
    }

    #[test]
    fn slope_is_the_sine_of_the_angle() {
        assert_eq!(slope_of(Vec2::ZERO), 0.0);
        assert!((slope_of(Vec2::new(1.0, 0.0)) - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        assert!((slope_of(Vec2::new(0.6, 0.8)) - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        assert_eq!(slope_of(Vec2::new(f32::INFINITY, 0.0)), 1.0);
        assert_eq!(slope_of(Vec2::NAN), 0.0);
    }
}
