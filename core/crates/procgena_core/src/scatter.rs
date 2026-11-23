//! What every Scatter gets from the level above it, and the shared rules
//! for standing copies on the ground.
//!
//! A Scatter spreads many copies of one kind of asset (trees, sticks,
//! stones) over a stretch of ground. Every Scatter takes a [`ScatterInput`]:
//! the [`Ground`], an optional [`Mask`], the [`ScatterArea`] and a seed.
//! It checks candidates in the same order: the mask first, then
//! [`steeper_than`] from one ground sample, and only then the copy's
//! footprint, for example with [`settle`].

use glam::{DVec2, Mat3, Quat, Vec2, Vec3};

use crate::ground::{Ground, GroundSample};

/// A rectangle of World ground to scatter over, around a centre.
///
/// Scatters give copy positions relative to [`centre`](Self::centre), in
/// metres (`x` is X, `y` is Z), so they stay exact far from the World
/// origin. Use [`world`](Self::world) to turn one back into a World
/// position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScatterArea {
    /// The middle of the area, as a World position.
    pub centre: DVec2,
    /// Width along X and depth along Z, in metres.
    pub size: Vec2,
}

impl ScatterArea {
    /// An area `size` metres across (X, then Z), around World position
    /// `centre`.
    pub fn new(centre: DVec2, size: Vec2) -> Self {
        Self { centre, size }
    }

    /// Returns the World position of `local`, a position relative to the
    /// centre.
    pub fn world(&self, local: Vec2) -> DVec2 {
        self.centre + local.as_dvec2()
    }

    /// Whether `local`, a position relative to the centre, lies inside the
    /// area or on its edge.
    pub fn contains(&self, local: Vec2) -> bool {
        let half = self.size * 0.5;
        local.x.abs() <= half.x && local.y.abs() <= half.y
    }
}

/// How much of a Scatter's density is allowed at each World position, from
/// 0 (none) to 1 (all of it).
///
/// A mask only thins a Scatter out. It does not promise that nothing stands
/// in a spot: a copy near a masked-out patch can still reach into it.
///
/// Any closure `Fn(DVec2) -> f32` that is `Send + Sync` can be used as a
/// mask.
pub trait Mask: Send + Sync {
    /// Returns the share of the density allowed at World position `p`.
    /// Values outside 0 to 1 are clamped by [`ScatterInput::density`], and
    /// NaN counts as 0.
    fn density(&self, p: DVec2) -> f32;
}

impl<F: Fn(DVec2) -> f32 + Send + Sync> Mask for F {
    fn density(&self, p: DVec2) -> f32 {
        self(p)
    }
}

/// What every Scatter gets from the level above it: where the ground is,
/// where copies may stand, which stretch of ground to fill, and a seed.
///
/// The same input and options always give the same copies.
#[derive(Clone, Copy)]
pub struct ScatterInput<'a> {
    /// The ground copies stand on.
    pub ground: &'a dyn Ground,
    /// Thins the Scatter in places; `None` allows full density everywhere.
    pub mask: Option<&'a dyn Mask>,
    /// The stretch of ground to fill.
    pub area: ScatterArea,
    /// Picks where copies stand and how each one varies. Different seeds
    /// give different copies over the same ground.
    pub seed: u32,
}

impl<'a> ScatterInput<'a> {
    /// An input over `area` of `ground`, with no mask.
    pub fn new(ground: &'a dyn Ground, area: ScatterArea, seed: u32) -> Self {
        Self {
            ground,
            mask: None,
            area,
            seed,
        }
    }

    /// The same input, thinned by `mask`.
    #[must_use]
    pub fn with_mask(self, mask: &'a dyn Mask) -> Self {
        Self {
            mask: Some(mask),
            ..self
        }
    }

    /// Returns the share of the density allowed at `local` (relative to the
    /// area's centre), from 0 to 1. It is 1 everywhere without a mask, and 0
    /// where the mask gives NaN.
    pub fn density(&self, local: Vec2) -> f32 {
        let Some(mask) = self.mask else {
            return 1.0;
        };
        let d = mask.density(self.area.world(local));
        if d.is_nan() { 0.0 } else { d.clamp(0.0, 1.0) }
    }

    /// Returns the ground's height and slope at `local` (relative to the
    /// area's centre).
    pub fn sample(&self, local: Vec2) -> GroundSample {
        self.ground.sample(self.area.world(local))
    }
}

/// Whether the ground sample `s` is too steep for a slope limit of
/// `max_degrees`.
///
/// A limit of 90 degrees or more never rejects a measured slope. A NaN
/// gradient, where the ground can't tell its slope, always counts as too
/// steep.
pub fn steeper_than(s: &GroundSample, max_degrees: f32) -> bool {
    let rise = s.gradient.length_squared();
    if rise.is_nan() {
        return true;
    }
    if max_degrees >= 90.0 {
        return false;
    }
    let limit = max_degrees.to_radians().tan();
    rise > limit * limit
}

/// Where a rigid piece touches flat ground, so [`settle`] can rest it on
/// uneven ground.
///
/// Points are in the piece's own space, with Y up and the flat ground at
/// `y = 0`, measured from the point that should stand over the spot being
/// settled.
#[derive(Debug, Clone, PartialEq)]
pub struct Footing {
    /// Points on the piece's underside that touch, or nearly touch, flat
    /// ground. The piece is lifted until none of them is below the ground.
    pub contacts: Vec<Vec3>,
    /// More points (X and Z) where the ground is sampled to find its tilt,
    /// without resting on them, such as points either side of a thin
    /// stick so it still finds the slope across it.
    pub extra: Vec<Vec2>,
    /// How far, in metres, the ground under any contact, extra point or the
    /// origin may lie from the tilted plane the piece rests on. Over a dip or a ridge sharper than
    /// this the spot is rejected. Use `f32::INFINITY` to never reject.
    pub max_bend: f32,
}

/// A rigid piece rested on the ground by [`settle`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settled {
    /// Turns the piece from its own space to lie on the ground: the
    /// requested yaw, then a tilt to the ground's slope.
    pub rotation: Quat,
    /// The height in metres at which to put the piece's origin, after
    /// [`rotation`](Self::rotation), so no contact is below the ground.
    pub height: f32,
    /// The upward unit normal of the plane the piece rests on.
    pub normal: Vec3,
}

/// Rests a rigid piece with `footing`, turned by `yaw` radians about Y, on
/// `ground` at World position `at`.
///
/// The piece is tilted to the plane that best fits the ground under its
/// contacts and extra points, then lifted until no contact is below the
/// ground. Put the piece's origin at `at` and [`Settled::height`], and turn
/// it by [`Settled::rotation`].
///
/// The ground is sampled under every contact, every extra point and the
/// piece's origin. Returns `None` where any of them has no ground (NaN),
/// or lies further than [`Footing::max_bend`] from the fitted plane. Check the slope with [`steeper_than`] first: it is cheaper
/// than settling.
pub fn settle(ground: &dyn Ground, at: DVec2, yaw: f32, footing: &Footing) -> Option<Settled> {
    let turn = Quat::from_rotation_y(yaw);
    let height = |p: Vec3| ground.height(at + DVec2::new(f64::from(p.x), f64::from(p.z)));

    // Sample the ground under the turned contacts and extra points.
    let mut samples: Vec<(Vec3, f32)> = Vec::new();
    let turned = footing.contacts.iter().map(|&c| turn * c).chain(
        footing
            .extra
            .iter()
            .map(|e| turn * Vec3::new(e.x, 0.0, e.y)),
    );
    for p in turned.chain(std::iter::once(Vec3::ZERO)) {
        let h = height(p);
        if !h.is_finite() {
            return None;
        }
        samples.push((p, h));
    }

    // Fit a plane h = a·x + b·z + c by least squares. The tiny diagonal
    // term keeps a single point or a straight line of points solvable, as
    // a level plane across it.
    let mut m = Mat3::ZERO;
    let mut rhs = Vec3::ZERO;
    for &(p, h) in &samples {
        let row = Vec3::new(p.x, p.z, 1.0);
        m += Mat3::from_cols(row * row.x, row * row.y, row * row.z);
        rhs += row * h;
    }
    m += Mat3::from_diagonal(Vec3::new(1e-6, 1e-6, 0.0));
    let [a, b, c] = (m.inverse() * rhs).to_array();
    let normal = Vec3::new(-a, 1.0, -b).normalize();

    // Reject ground that bends too far from the plane under the piece.
    let bent = samples
        .iter()
        .any(|&(p, h)| (h - (a * p.x + b * p.z + c)).abs() > footing.max_bend);
    if bent {
        return None;
    }

    // Tilt, then lift until no contact is below the ground.
    let rotation = Quat::from_rotation_arc(Vec3::Y, normal) * turn;
    let mut lift = f32::NEG_INFINITY;
    for &contact in &footing.contacts {
        let p = rotation * contact;
        let h = height(p);
        if !h.is_finite() {
            return None;
        }
        lift = lift.max(h - p.y);
    }
    if !lift.is_finite() {
        lift = samples[samples.len() - 1].1;
    }
    Some(Settled {
        rotation,
        height: lift,
        normal,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FlatGround;

    fn sample(gradient: Vec2) -> GroundSample {
        GroundSample {
            height: 0.0,
            gradient,
        }
    }

    #[test]
    fn the_area_measures_from_its_centre() {
        let area = ScatterArea::new(DVec2::new(1.0e6, -2.0e6), Vec2::new(10.0, 4.0));
        assert_eq!(
            area.world(Vec2::new(1.5, -2.0)),
            DVec2::new(1.0e6 + 1.5, -2.0e6 - 2.0)
        );
        assert!(area.contains(Vec2::new(5.0, -2.0)));
        assert!(!area.contains(Vec2::new(5.01, 0.0)));
        assert!(!area.contains(Vec2::new(0.0, 2.01)));
    }

    #[test]
    fn the_mask_is_read_at_world_positions_and_clamped() {
        let area = ScatterArea::new(DVec2::new(100.0, 0.0), Vec2::splat(10.0));
        let input = ScatterInput::new(&FlatGround, area, 3);
        assert_eq!(input.density(Vec2::ZERO), 1.0);

        // Full density east of x = 100 in World, none west of it.
        let east = |p: DVec2| if p.x >= 100.0 { 2.0 } else { -1.0 };
        let masked = input.with_mask(&east);
        assert_eq!(masked.density(Vec2::new(1.0, 0.0)), 1.0);
        assert_eq!(masked.density(Vec2::new(-1.0, 0.0)), 0.0);

        let broken = |_: DVec2| f32::NAN;
        assert_eq!(input.with_mask(&broken).density(Vec2::ZERO), 0.0);
    }

    #[test]
    fn the_ground_is_sampled_at_world_positions() {
        let area = ScatterArea::new(DVec2::new(50.0, 20.0), Vec2::splat(10.0));
        let ramp = |p: DVec2| (p.x * 0.5) as f32;
        let input = ScatterInput::new(&ramp, area, 0);
        let s = input.sample(Vec2::new(2.0, 0.0));
        assert!((s.height - 26.0).abs() < 1e-5);
        assert!((s.gradient.x - 0.5).abs() < 1e-4);
    }

    #[test]
    fn slope_limits_compare_against_the_steepest_slope() {
        // A 1 in 1 rise is 45 degrees.
        let s = sample(Vec2::new(0.6, 0.8));
        assert!(steeper_than(&s, 44.0));
        assert!(!steeper_than(&s, 46.0));
        assert!(!steeper_than(&sample(Vec2::ZERO), 0.0));
    }

    #[test]
    fn ninety_degrees_is_no_limit_but_nan_still_rejects() {
        let cliff = sample(Vec2::new(1.0e6, 0.0));
        assert!(!steeper_than(&cliff, 90.0));
        assert!(!steeper_than(&cliff, 120.0));
        let unknown = sample(Vec2::new(f32::NAN, 0.0));
        assert!(steeper_than(&unknown, 90.0));
        assert!(steeper_than(&unknown, 30.0));
    }

    /// A box 2 m long along X and 1 m wide, resting on its four corners.
    fn plank(max_bend: f32) -> Footing {
        Footing {
            contacts: vec![
                Vec3::new(-1.0, 0.0, -0.5),
                Vec3::new(1.0, 0.0, -0.5),
                Vec3::new(-1.0, 0.0, 0.5),
                Vec3::new(1.0, 0.0, 0.5),
            ],
            extra: Vec::new(),
            max_bend,
        }
    }

    #[test]
    fn a_piece_on_flat_ground_stays_level() {
        let s = settle(&FlatGround, DVec2::new(1.0e6, 3.0), 0.7, &plank(0.01)).unwrap();
        assert!((s.normal - Vec3::Y).length() < 1e-5);
        assert!(s.height.abs() < 1e-5);
        assert!((s.rotation * Vec3::X - Quat::from_rotation_y(0.7) * Vec3::X).length() < 1e-5);
    }

    #[test]
    fn a_piece_tilts_to_a_slope_and_rests_on_it() {
        let ramp = |p: DVec2| (0.3 * p.x - 0.1 * p.y) as f32;
        let at = DVec2::new(4.0, -2.0);
        let s = settle(&ramp, at, 1.1, &plank(0.01)).unwrap();
        let expected = Vec3::new(-0.3, 1.0, 0.1).normalize();
        assert!((s.normal - expected).length() < 1e-4);

        // Every contact sits on the ramp, none below it.
        for c in plank(0.0).contacts {
            let p = s.rotation * c + Vec3::new(0.0, s.height, 0.0);
            let ground = ramp(at + DVec2::new(f64::from(p.x), f64::from(p.z)));
            assert!((p.y - ground).abs() < 1e-3, "{} vs {ground}", p.y);
        }
    }

    #[test]
    fn a_piece_over_a_gully_is_rejected_unless_bending_is_allowed() {
        // A V-shaped gully along Z, 0.5 m deep at its middle.
        let gully = |p: DVec2| (p.x.abs() * 0.5) as f32;
        assert!(settle(&gully, DVec2::ZERO, 0.0, &plank(0.05)).is_none());
        let s = settle(&gully, DVec2::ZERO, 0.0, &plank(f32::INFINITY)).unwrap();
        // It bridges the gully, resting on the high sides.
        assert!((s.height - 0.5).abs() < 1e-3);
    }

    #[test]
    fn missing_ground_under_a_piece_rejects_it() {
        let edge = |p: DVec2| if p.x > 0.5 { f32::NAN } else { 0.0 };
        assert!(settle(&edge, DVec2::ZERO, 0.0, &plank(1.0)).is_none());
    }

    #[test]
    fn extra_points_find_the_slope_across_a_thin_piece() {
        // A stick along X on ground that rises along Z.
        let stick = Footing {
            contacts: vec![Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)],
            extra: vec![Vec2::new(0.0, -0.2), Vec2::new(0.0, 0.2)],
            max_bend: 0.01,
        };
        let ramp = |p: DVec2| (0.5 * p.y) as f32;
        let s = settle(&ramp, DVec2::ZERO, 0.0, &stick).unwrap();
        assert!((s.normal - Vec3::new(0.0, 1.0, -0.5).normalize()).length() < 1e-3);
    }
}
