//! Ground shapes for scattering things over land: a height and a slope for
//! each World position in X and Z, with Y up.
//!
//! Generators that spread copies over ground, such as sticks or trees, take
//! a [`Ground`]. Use [`FlatGround`], any closure `Fn(DVec2) -> f32`, or a
//! [`HeightGrid`] sampled from a function or from another generator's mesh.

use glam::{DVec2, Vec2, Vec3};

/// Distance in metres on each side of a point that [`Ground::gradient`]'s
/// default looks at to work out the slope.
pub const GRADIENT_STEP: f64 = 0.5;

/// The ground at one point: its height and how fast it rises.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroundSample {
    /// Height in metres, or NaN where there is no ground.
    pub height: f32,
    /// How much the height rises per metre along X (`x`) and along Z (`y`).
    /// NaN where the ground can't say, such as next to a spot with no
    /// ground.
    pub gradient: Vec2,
}

impl GroundSample {
    /// Returns the ground's upward unit normal. NaN where the gradient is.
    pub fn normal(&self) -> Vec3 {
        Vec3::new(-self.gradient.x, 1.0, -self.gradient.y).normalize()
    }

    /// Returns the steepest slope at this point, in degrees from 0 (level)
    /// up to 90 (a wall). NaN where the gradient is.
    pub fn slope_degrees(&self) -> f32 {
        self.gradient.length().atan().to_degrees()
    }
}

/// The shape of the ground: a height for each World position in X and Z,
/// with Y up.
///
/// Positions are World positions in metres, as `DVec2` (`x` is X, `y` is
/// Z), so the ground stays exact far from the origin. Heights and slopes
/// are `f32`.
///
/// Any closure `Fn(DVec2) -> f32` that is `Send + Sync` can be used as
/// ground. [`FlatGround`] and [`HeightGrid`] are ready-made grounds.
pub trait Ground: Send + Sync {
    /// Returns the ground's height in metres at World position `p`, or NaN
    /// where there is no ground; nothing is placed there.
    fn height(&self, p: DVec2) -> f32;

    /// Returns how much the height rises per metre along X and Z at `p`.
    ///
    /// The default compares heights [`GRADIENT_STEP`] metres to each side
    /// of `p` (central differences), and is NaN if any of those four
    /// heights is NaN.
    fn gradient(&self, p: DVec2) -> Vec2 {
        central_differences(self, p, GRADIENT_STEP)
    }

    /// Returns the height and the gradient at `p` together. Grounds that can
    /// work out both at once override it; callers that need both should
    /// use it.
    fn sample(&self, p: DVec2) -> GroundSample {
        GroundSample {
            height: self.height(p),
            gradient: self.gradient(p),
        }
    }
}

/// Returns the gradient of `ground` at `p` from heights `step` metres to
/// each side along X and Z.
fn central_differences<G: Ground + ?Sized>(ground: &G, p: DVec2, step: f64) -> Vec2 {
    let dx = ground.height(p + DVec2::X * step) - ground.height(p - DVec2::X * step);
    let dz = ground.height(p + DVec2::Y * step) - ground.height(p - DVec2::Y * step);
    Vec2::new(dx, dz) / (2.0 * step) as f32
}

/// Flat ground at `y = 0`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FlatGround;

impl Ground for FlatGround {
    fn height(&self, _p: DVec2) -> f32 {
        0.0
    }

    fn gradient(&self, _p: DVec2) -> Vec2 {
        Vec2::ZERO
    }
}

impl<F: Fn(DVec2) -> f32 + Send + Sync> Ground for F {
    fn height(&self, p: DVec2) -> f32 {
        self(p)
    }
}

/// Ground heights sampled on a regular grid in X and Z, blended smoothly
/// between samples.
///
/// Sampling once and reusing the grid is useful when the real ground is
/// slow to query, such as a mesh. Outside the grid there is no ground, and
/// neither is there where any of the four surrounding samples has none.
///
/// Its gradient compares heights one cell to each side, so it is NaN within
/// one cell of the grid's edge or of a spot with no ground.
#[derive(Debug, Clone, PartialEq)]
pub struct HeightGrid {
    origin: DVec2,
    cell: f64,
    columns: u32,
    rows: u32,
    heights: Vec<f32>,
}

impl HeightGrid {
    /// Samples `height` every `cell` metres over the rectangle from `min`
    /// to `max` (World positions, `x` and Z). `height` may return NaN where
    /// there is no ground.
    ///
    /// # Panics
    ///
    /// Panics if `cell` is not positive or `max` is not above `min` in both
    /// directions.
    pub fn from_fn(min: DVec2, max: DVec2, cell: f64, height: impl Fn(DVec2) -> f32) -> Self {
        assert!(cell > 0.0 && max.x > min.x && max.y > min.y);
        let columns = ((max.x - min.x) / cell).ceil() as u32 + 1;
        let rows = ((max.y - min.y) / cell).ceil() as u32 + 1;
        let heights = (0..rows)
            .flat_map(|r| (0..columns).map(move |c| (c, r)))
            .map(|(c, r)| height(min + DVec2::new(f64::from(c), f64::from(r)) * cell))
            .collect();
        Self::from_samples(min, cell, columns, rows, heights)
    }

    /// Builds a grid from heights you already sampled: `columns` along X by
    /// `rows` along Z, `cell` metres apart, with the first at World position
    /// `origin`. `heights` lists them row by row (Z outer, X inner), and may
    /// hold NaN where there is no ground.
    ///
    /// # Panics
    ///
    /// Panics if `cell` is not positive and finite, or `heights` does not
    /// hold `columns * rows` values.
    pub fn from_samples(
        origin: DVec2,
        cell: f64,
        columns: u32,
        rows: u32,
        heights: Vec<f32>,
    ) -> Self {
        assert!(cell > 0.0 && cell.is_finite());
        assert_eq!(heights.len() as u64, u64::from(columns) * u64::from(rows));
        Self {
            origin,
            cell,
            columns,
            rows,
            heights,
        }
    }
}

impl Ground for HeightGrid {
    fn height(&self, p: DVec2) -> f32 {
        let g = (p - self.origin) / self.cell;
        if g.x < 0.0 || g.y < 0.0 {
            return f32::NAN;
        }
        // Past the last sample there is no ground; exactly on it, blend from
        // the cell before so the far edge still counts.
        let (last_c, last_r) = (f64::from(self.columns - 1), f64::from(self.rows - 1));
        if g.x > last_c || g.y > last_r {
            return f32::NAN;
        }
        let c = (g.x.floor() as u32).min(self.columns.saturating_sub(2));
        let r = (g.y.floor() as u32).min(self.rows.saturating_sub(2));
        if c + 1 >= self.columns || r + 1 >= self.rows {
            return f32::NAN;
        }

        // Blend the four surrounding samples.
        let (fx, fz) = ((g.x - f64::from(c)) as f32, (g.y - f64::from(r)) as f32);
        let at = |c: u32, r: u32| self.heights[(r * self.columns + c) as usize];
        let top = at(c, r) + (at(c + 1, r) - at(c, r)) * fx;
        let bottom = at(c, r + 1) + (at(c + 1, r + 1) - at(c, r + 1)) * fx;
        top + (bottom - top) * fz
    }

    fn gradient(&self, p: DVec2) -> Vec2 {
        central_differences(self, p, self.cell)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// About a thousand kilometres from the origin, where `f32` positions
    /// would be a few centimetres apart.
    const FAR: DVec2 = DVec2::new(1.0e6, -1.0e6);

    /// A plane rising 0.3 m per metre along X and falling 0.1 along Z,
    /// at height 0 at `anchor`.
    fn tilted(anchor: DVec2) -> impl Fn(DVec2) -> f32 {
        move |p: DVec2| (0.3 * (p.x - anchor.x) - 0.1 * (p.y - anchor.y)) as f32
    }

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).abs().max_element() < 1e-5
    }

    #[test]
    fn height_grid_blends_and_has_edges() {
        let g = HeightGrid::from_fn(DVec2::splat(-1.0), DVec2::splat(1.0), 0.5, |p| p.x as f32);
        assert!((g.height(DVec2::new(0.3, 0.2)) - 0.3).abs() < 1e-5);
        assert!(g.height(DVec2::new(5.0, 0.0)).is_nan());
        // The far edge and far corner were sampled, so they have ground.
        assert!((g.height(DVec2::new(1.0, 0.0)) - 1.0).abs() < 1e-5);
        assert!((g.height(DVec2::new(1.0, 1.0)) - 1.0).abs() < 1e-5);
        assert!(g.height(DVec2::new(1.01, 0.0)).is_nan());
    }

    #[test]
    fn closures_and_flat_ground_are_grounds() {
        let slope = |p: DVec2| 0.5 * p.x as f32;
        assert_eq!(slope.height(DVec2::new(2.0, 7.0)), 1.0);
        assert_eq!(FlatGround.height(DVec2::new(3.0, -4.0)), 0.0);
    }

    #[test]
    fn flat_ground_is_level() {
        let s = FlatGround.sample(FAR);
        assert_eq!(s.height, 0.0);
        assert_eq!(s.gradient, Vec2::ZERO);
        assert_eq!(s.normal(), Vec3::Y);
        assert_eq!(s.slope_degrees(), 0.0);
    }

    #[test]
    fn closures_get_their_slope_from_central_differences() {
        let ground = tilted(DVec2::ZERO);
        let s = ground.sample(DVec2::new(2.0, 3.0));
        assert!((s.height - 0.3).abs() < 1e-6);
        assert!(close(s.gradient, Vec2::new(0.3, -0.1)));

        // A curved ground: the default's error is the step squared times
        // the third derivative over 6, which is zero for a parabola.
        let bowl = |p: DVec2| (p.x * p.x + 2.0 * p.y * p.y) as f32;
        assert!(close(bowl.gradient(DVec2::new(1.0, -0.5)), Vec2::new(2.0, -2.0)));
    }

    #[test]
    fn a_missing_neighbour_makes_the_slope_nan() {
        let cliff = |p: DVec2| if p.x > 0.2 { f32::NAN } else { 0.0 };
        let s = cliff.sample(DVec2::ZERO);
        assert_eq!(s.height, 0.0);
        assert!(s.gradient.x.is_nan());
        assert!(s.normal().is_nan());
        assert!(s.slope_degrees().is_nan());
    }

    #[test]
    fn a_tilted_height_grid_has_the_planes_slope() {
        let grid = HeightGrid::from_fn(
            DVec2::splat(-4.0),
            DVec2::splat(4.0),
            0.5,
            tilted(DVec2::ZERO),
        );
        // Bilinear blending is exact on a plane, so the slope is too, on
        // and between samples.
        for p in [DVec2::new(0.0, 0.0), DVec2::new(1.3, -2.1), DVec2::new(-3.4, 3.4)] {
            assert!(close(grid.gradient(p), Vec2::new(0.3, -0.1)), "{p}");
        }
        // Within one cell of the edge there is no neighbour to compare with.
        assert!(grid.gradient(DVec2::new(3.8, 0.0)).x.is_nan());
    }

    #[test]
    fn the_normal_and_slope_angle_follow_the_gradient() {
        let s = GroundSample {
            height: 0.0,
            gradient: Vec2::new(1.0, 0.0),
        };
        assert!((s.slope_degrees() - 45.0).abs() < 1e-5);
        let n = s.normal();
        assert!((n - Vec3::new(-1.0, 1.0, 0.0).normalize()).length() < 1e-6);
    }

    #[test]
    fn slope_stays_exact_far_from_the_origin() {
        let ground = tilted(FAR);
        assert!(close(ground.gradient(FAR), Vec2::new(0.3, -0.1)));
        assert!(close(
            ground.gradient(FAR + DVec2::new(0.25, 0.75)),
            Vec2::new(0.3, -0.1)
        ));

        let grid = HeightGrid::from_fn(FAR - 5.0, FAR + 5.0, 1.0, tilted(FAR));
        assert!(grid.height(FAR).abs() < 1e-6);
        assert!(close(
            grid.gradient(FAR + DVec2::new(0.4, -1.3)),
            Vec2::new(0.3, -0.1)
        ));
        assert_eq!(FlatGround.gradient(FAR), Vec2::ZERO);
    }
}
