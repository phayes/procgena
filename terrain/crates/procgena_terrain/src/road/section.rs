//! Road cross-sections: the shape of the ground across a road, from its
//! crown out to where its cutting or embankment meets the natural ground.
//!
//! Across a road, at distance `d` from its centre line:
//!
//! - **Carriageway and shoulders** (`d` up to the half width plus the
//!   shoulder): the road surface, falling from the crown at the crossfall.
//!   The crown is rounded over [`CROWN`] metres.
//! - **Side slopes** (beyond the shoulder): the ground is held between a
//!   cutting rising at the cut slope and an embankment falling at the fill
//!   slope, both from the shoulder's edge. Where the natural ground lies
//!   between them, it is left as it is; the line where it starts to be is
//!   the daylight line.
//! - **Retaining walls**: over the last [`WALL`] metres of the reach the
//!   slopes steepen past [`MAX_ROAD_SIDE_SLOPE`] towards vertical, so every
//!   cutting and embankment meets the ground within [`BANK_REACH`] of the
//!   carriageway's edge.

use std::ops::{Add, Div, Mul, Neg, Sub};

use glam::DVec2;

use crate::options::{MAX_ROAD_SIDE_SLOPE, RoadOptions};
use crate::ports::BANK_REACH;

/// Over how many metres either side of the centre line the crown is
/// rounded, so the surface has no crease along the middle.
pub(crate) const CROWN: f64 = 0.5;

/// How wide the retaining wall band at the end of a road's reach is, in
/// metres.
pub(crate) const WALL: f64 = 3.0;

/// How softly cuttings and embankments meet the natural ground, in metres
/// of height.
pub(crate) const ROAD_BLEND: f64 = 0.2;

/// Over how many metres beyond the shoulder the softening grows from none
/// to [`ROAD_BLEND`], so the road surface itself is exact.
const BLEND_RAMP: f64 = 1.0;

/// The shape across a road beyond its carriageway: shoulders, crossfall
/// and side slopes. Read from [`RoadOptions`]; see there for each value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionShape {
    /// Shoulder width on each side, in metres.
    pub shoulder: f32,
    /// Fall from the crown to each edge, as fall over run.
    pub crossfall: f32,
    /// Cutting slope, as rise over run.
    pub cut: f32,
    /// Embankment slope, as fall over run.
    pub fill: f32,
}

impl SectionShape {
    /// The shape every border crossing piece has, so both regions carve
    /// the same ground at a border: [`RoadOptions::default`]'s.
    pub(crate) const PIECE: Self = Self {
        shoulder: 1.0,
        crossfall: 0.025,
        cut: 1.0,
        fill: 1.0 / 1.5,
    };

    /// The shape `options` ask for.
    pub(crate) fn of(options: &RoadOptions) -> Self {
        Self {
            shoulder: options.shoulder,
            crossfall: options.crossfall,
            cut: options.cut_slope,
            fill: options.fill_slope,
        }
    }

    /// The shape a share `t` (0 to 1) of the way from `self` to `other`.
    pub(crate) fn lerp(self, other: Self, t: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        Self {
            shoulder: mix(self.shoulder, other.shoulder),
            crossfall: mix(self.crossfall, other.crossfall),
            cut: mix(self.cut, other.cut),
            fill: mix(self.fill, other.fill),
        }
    }

    /// The four values, in field order.
    pub(crate) fn values(self) -> [f32; 4] {
        [self.shoulder, self.crossfall, self.cut, self.fill]
    }
}

/// A value with its gradient over x and z, so that a value's slope comes
/// out of the same arithmetic as the value itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Grad {
    pub v: f64,
    pub g: DVec2,
}

impl Grad {
    /// A value with gradient `g`.
    pub(crate) const fn new(v: f64, g: DVec2) -> Self {
        Self { v, g }
    }

    /// A value that doesn't change with position.
    pub(crate) const fn constant(v: f64) -> Self {
        Self { v, g: DVec2::ZERO }
    }

    /// `e` to the power of `self`.
    pub(crate) fn exp(self) -> Self {
        let e = self.v.exp();
        Self::new(e, self.g * e)
    }

    /// The square root. Its gradient is taken as zero at zero.
    pub(crate) fn sqrt(self) -> Self {
        let r = self.v.sqrt();
        let g = if r > 0.0 {
            self.g / (2.0 * r)
        } else {
            DVec2::ZERO
        };
        Self::new(r, g)
    }

    /// The absolute value.
    pub(crate) fn abs(self) -> Self {
        if self.v < 0.0 { -self } else { self }
    }

    /// The smaller of the two, with its gradient (`self`'s on a tie).
    pub(crate) fn min(self, other: Self) -> Self {
        if other.v < self.v { other } else { self }
    }

    /// The larger of the two, with its gradient (`self`'s on a tie).
    pub(crate) fn max(self, other: Self) -> Self {
        if other.v > self.v { other } else { self }
    }
}

impl Add for Grad {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self::new(self.v + o.v, self.g + o.g)
    }
}

impl Sub for Grad {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self::new(self.v - o.v, self.g - o.g)
    }
}

impl Mul for Grad {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        Self::new(self.v * o.v, self.g * o.v + o.g * self.v)
    }
}

impl Div for Grad {
    type Output = Self;
    fn div(self, o: Self) -> Self {
        Self::new(self.v / o.v, (self.g * o.v - o.g * self.v) / (o.v * o.v))
    }
}

impl Neg for Grad {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.v, -self.g)
    }
}

impl Add<f64> for Grad {
    type Output = Self;
    fn add(self, o: f64) -> Self {
        Self::new(self.v + o, self.g)
    }
}

impl Sub<f64> for Grad {
    type Output = Self;
    fn sub(self, o: f64) -> Self {
        Self::new(self.v - o, self.g)
    }
}

impl Mul<f64> for Grad {
    type Output = Self;
    fn mul(self, o: f64) -> Self {
        Self::new(self.v * o, self.g * o)
    }
}

impl Div<f64> for Grad {
    type Output = Self;
    fn div(self, o: f64) -> Self {
        Self::new(self.v / o, self.g / o)
    }
}

/// A road seen from one point: the distance to its centre line, and its
/// half width, level and shape there, each with its gradient.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RoadView {
    pub distance: Grad,
    pub half_width: Grad,
    pub level: Grad,
    pub shoulder: Grad,
    pub crossfall: Grad,
    pub cut: Grad,
    pub fill: Grad,
}

impl RoadView {
    /// How far beyond the shoulder's edge the point is: negative on the
    /// road.
    pub(crate) fn beyond(&self) -> Grad {
        self.distance - self.half_width - self.shoulder
    }

    /// How far the point is from the carriageway's edge: negative on the
    /// carriageway.
    pub(crate) fn gap(&self) -> Grad {
        self.distance - self.half_width
    }

    /// The road surface's height `d` metres from the centre line (the
    /// shoulders continue it): the level at the crown, falling at the
    /// crossfall.
    fn surface(&self, d: Grad) -> Grad {
        self.level - self.crossfall * (crown(d) - CROWN / 2.0)
    }

    /// How far beyond the shoulder's edge the road holds the ground, in
    /// metres.
    pub(crate) fn reach(&self) -> Grad {
        Grad::constant(BANK_REACH) - self.shoulder
    }

    /// The heights between which the road holds the ground here, or `None`
    /// beyond its reach. On the road the two are the road surface.
    pub(crate) fn band(&self) -> Option<Band> {
        let beyond = self.beyond();
        let reach = self.reach();
        if beyond.v >= reach.v {
            return None;
        }
        if beyond.v <= 0.0 {
            let surface = self.surface(self.distance);
            return Some(Band {
                low: surface,
                high: surface,
                beyond,
                rise: 0.0,
                fall: 0.0,
                wall: false,
            });
        }
        let edge = self.surface(self.half_width + self.shoulder);
        let (wall, wall_slope) = wall(beyond, reach);
        Some(Band {
            low: edge - self.fill * beyond - wall,
            high: edge + self.cut * beyond + wall,
            beyond,
            rise: self.cut.v + wall_slope,
            fall: self.fill.v + wall_slope,
            wall: wall_slope > 0.0,
        })
    }
}

/// The heights a road holds the ground between at one point (see
/// [`RoadView::band`]).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Band {
    /// The embankment's height: ground below it is raised to it.
    pub low: Grad,
    /// The cutting's height: ground above it is cut down to it.
    pub high: Grad,
    /// How far beyond the shoulder's edge the point is.
    pub beyond: Grad,
    /// The cutting's slope here across the road, as rise over run.
    pub rise: f64,
    /// The embankment's slope here across the road, as fall over run.
    pub fall: f64,
    /// Whether the point is in the retaining wall band.
    pub wall: bool,
}

impl Band {
    /// `ground` held between the band's heights: exactly the road surface
    /// on the road, and softly clamped (by up to [`ROAD_BLEND`]) beyond.
    pub(crate) fn hold(&self, ground: Grad) -> Grad {
        let soft = Grad::constant(ROAD_BLEND)
            * (self.beyond / BLEND_RAMP)
                .min(Grad::constant(1.0))
                .max(Grad::constant(0.0));
        let held = smooth_min(ground, self.high, soft);
        -smooth_min(-held, -self.low, soft)
    }
}

/// The rounded distance from the crown: `d` beyond [`CROWN`], and a
/// parabola meeting it with the same slope inside.
fn crown(d: Grad) -> Grad {
    if d.v >= CROWN {
        d
    } else {
        d * d / (2.0 * CROWN) + CROWN / 2.0
    }
}

/// How much the retaining wall adds to a side slope `beyond` metres past
/// the shoulder, where the road holds the ground out to `reach`, and the
/// wall's own slope across the road. Zero before the last [`WALL`] metres;
/// then it rises at [`MAX_ROAD_SIDE_SLOPE`] and more, without bound at the
/// reach.
fn wall(beyond: Grad, reach: Grad) -> (Grad, f64) {
    let start = reach - WALL;
    let x = beyond - start;
    if x.v <= 0.0 {
        return (Grad::constant(0.0), 0.0);
    }
    let q8 = (x / WALL).v.powi(8);
    let q8_grad = x.g * (8.0 * q8 / x.v);
    let fall = Grad::new(1.0 - q8, -q8_grad);
    let steep = f64::from(MAX_ROAD_SIDE_SLOPE);
    let height = x * steep / fall;
    let slope = steep * (1.0 + 7.0 * q8) / (1.0 - q8).powi(2);
    (height, slope)
}

/// A smooth minimum of `a` and `b`, rounded where they are within `k` of
/// each other; the plain minimum where `k` is zero.
fn smooth_min(a: Grad, b: Grad, k: Grad) -> Grad {
    let least = a.min(b);
    if k.v <= 0.0 {
        return least;
    }
    let room = k - (a - b).abs();
    if room.v <= 0.0 {
        return least;
    }
    least - room * room / (k * 4.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight road along x through the origin, level 10, 6 m wide, with
    /// the default shape, seen from `p`.
    fn view(p: DVec2) -> RoadView {
        let shape = SectionShape::PIECE;
        let c = |v: f32| Grad::constant(f64::from(v));
        let d = p.y.abs();
        RoadView {
            distance: Grad::new(d, DVec2::new(0.0, p.y.signum())),
            half_width: Grad::constant(3.0),
            level: Grad::constant(10.0),
            shoulder: c(shape.shoulder),
            crossfall: c(shape.crossfall),
            cut: c(shape.cut),
            fill: c(shape.fill),
        }
    }

    #[test]
    fn the_piece_shape_is_the_default_options() {
        assert_eq!(
            SectionShape::PIECE,
            SectionShape::of(&RoadOptions::default())
        );
    }

    #[test]
    fn the_road_surface_is_exact_and_falls_from_the_crown() {
        for ground in [-5.0, 9.99, 10.0, 30.0] {
            let at = |z: f64| {
                let band = view(DVec2::new(0.0, z)).band().expect("on the road");
                band.hold(Grad::constant(ground)).v
            };
            assert_eq!(at(0.0), 10.0);
            // 2.5% down to the shoulder's edge, 4 m out.
            assert!((at(4.0) - (10.0 - 0.025 * (4.0 - CROWN / 2.0))).abs() < 1e-6);
            assert!(at(2.0) < at(1.0) && at(1.0) < at(0.0));
        }
    }

    #[test]
    fn cuttings_and_embankments_rise_and_fall_at_their_slopes() {
        let edge = 10.0 - 0.025 * (4.0 - CROWN / 2.0);
        // Far above: cut down to the 1:1 cutting, 10 m beyond the shoulder.
        let band = view(DVec2::new(0.0, 14.0)).band().expect("in reach");
        assert!((band.hold(Grad::constant(100.0)).v - (edge + 10.0)).abs() < 1e-6);
        // Far below: raised to the 1:1.5 embankment.
        assert!((band.hold(Grad::constant(-100.0)).v - (edge - 10.0 / 1.5)).abs() < 1e-6);
        // In between: left alone.
        assert_eq!(band.hold(Grad::constant(9.0)).v, 9.0);
        assert!(!band.wall);
    }

    #[test]
    fn a_cutting_that_never_meets_the_ground_ends_in_a_wall() {
        // Beyond the start of the wall band, 1.5 m short of the reach.
        let z = 3.0 + BANK_REACH - 1.5;
        let band = view(DVec2::new(0.0, z)).band().expect("in reach");
        assert!(band.wall);
        assert!(band.rise > f64::from(MAX_ROAD_SIDE_SLOPE));
        // Past the reach the road holds nothing.
        assert!(
            view(DVec2::new(0.0, 3.0 + BANK_REACH + 0.1))
                .band()
                .is_none()
        );
    }

    #[test]
    fn held_slopes_match_differences() {
        let step = 1e-5;
        for ground in [-40.0, 9.5, 9.85, 10.0, 60.0] {
            for z in [0.1, 0.4, 2.0, 3.99, 4.05, 4.5, 6.0, 20.0, 35.5, 37.0] {
                let at = |z: f64| {
                    view(DVec2::new(0.0, z))
                        .band()
                        .map_or(ground, |b| b.hold(Grad::constant(ground)).v)
                };
                let held = view(DVec2::new(0.0, z))
                    .band()
                    .map_or(Grad::constant(ground), |b| b.hold(Grad::constant(ground)));
                let difference = (at(z + step) - at(z - step)) / (2.0 * step);
                assert!(
                    (held.g.y - difference).abs() < 1e-4 * (1.0 + difference.abs()),
                    "ground {ground}, z {z}: slope {} against {difference}",
                    held.g.y
                );
            }
        }
    }
}
