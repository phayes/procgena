//! Cutting rivers, ponds and roads into the ground.

use std::collections::BTreeMap;

use glam::DVec2;

use crate::feature::{FeatureId, FeatureIndex, FeatureShape, Part, TerrainFeature, feature_keys};
use crate::gully;
use crate::ports::BANK_REACH;
use crate::seam::{smootherstep, smootherstep_slope};

/// How steeply a river or pond bank, and a river's valley wall, rises, as
/// rise over run (about 19°).
const RIVER_BANK: f64 = 0.35;

/// How much wider than its cut needs a river's valley may reach, so its
/// wall still meets ground that rises away from the river.
const VALLEY_SPARE: f64 = 1.5;

/// The furthest a river's valley reaches from the water's edge, in metres.
pub(crate) const MAX_VALLEY_REACH: f64 = 300.0;

/// How far a road may sit above or below the ground beneath it before its
/// banks start, in metres.
const ROAD_TOLERANCE: f64 = 0.25;

/// How steeply road cuttings and embankments start on gentle ground, as
/// rise over run (about 31°).
pub(crate) const ROAD_BANK: f64 = 0.6;

/// How many times steeper than the natural ground a road's banks start.
const BANK_FACTOR: f64 = 2.0;

/// The steepest a road's banks start, as rise over run (about 87°).
const MAX_BANK: f64 = 20.0;

/// The slope, as rise over run, from which a road cutting's face is a
/// bluff: 45°.
const BLUFF_SLOPE: f64 = 1.0;

/// The slope, as rise over run, from which a road cutting's face is a
/// cliff: 65° (tan 65°).
const CLIFF_SLOPE: f64 = 2.144_506_920_509_559_5;

/// How softly river and pond cuts meet the ground, in metres of height. The
/// cut edge is rounded instead of creased, so the ground's slope stays
/// continuous.
const RIVER_BLEND: f64 = 0.5;

/// How softly road cuts and fills meet the ground, in metres of height.
/// Smaller than the road's tolerance band.
const ROAD_BLEND: f64 = 0.2;

/// How much further off than the nearest water, in metres, another water's
/// surface still counts towards [`Nearby::water_level`].
pub(crate) const WATER_MIX: f64 = 10.0;

/// A smooth minimum: `min(a, b)` with the corner rounded where the two are
/// within `k` of each other. Its slope is continuous. Inigo Quilez's
/// polynomial smooth minimum (MIT).
fn smooth_min(a: f64, b: f64, k: f64) -> f64 {
    let h = (k - (a - b).abs()).max(0.0) / k;
    a.min(b) - h * h * k * 0.25
}

/// A smooth maximum, the mirror of [`smooth_min`].
fn smooth_max(a: f64, b: f64, k: f64) -> f64 {
    -smooth_min(-a, -b, k)
}

/// What a terrain feature is (see [`FeatureId`](crate::FeatureId)). A
/// [`FeatureLine`] is a river or a road.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FeatureKind {
    /// A river. Its levels are the water surface, falling (or level) from
    /// the first point to the last.
    River,
    /// A road. Its levels are the road surface.
    Road,
    /// A pond where a river ends (see [`Pond`]).
    Pond,
}

/// A river or road as a line of points, each with a level.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureLine {
    /// What the line is.
    pub kind: FeatureKind,
    /// The line's points, in metres (x and z).
    pub points: Vec<DVec2>,
    /// The water or road surface height at each point, in metres.
    pub levels: Vec<f32>,
    /// The width at each point, in metres. A river keeps one width; a road
    /// narrows through deep cuttings, down to its `min_width`.
    pub widths: Vec<f32>,
    /// For a river, depth of the bed below the water, in metres. 0 for roads.
    pub depth: f32,
}

/// A pond where a river ends: a round hollow holding water.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pond {
    /// Centre, in metres (x and z).
    pub centre: DVec2,
    /// Water surface height, in metres.
    pub level: f32,
    /// Radius of the water, in metres.
    pub radius: f64,
    /// Depth at the centre below the water, in metres.
    pub depth: f32,
}

/// What a road made of the ground at a point, as a set of tags; a point
/// can carry several, such as [`RoadTags::CUT`] and [`RoadTags::CLIFF`].
/// Empty where no road touches the ground.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RoadTags(u8);

impl RoadTags {
    /// No tags.
    pub const NONE: Self = Self(0);
    /// The road's own surface.
    pub const ROAD: Self = Self(1);
    /// A bank cut down into the ground beside the road.
    pub const CUT: Self = Self(1 << 1);
    /// A bank built up beside the road, below 65°.
    pub const EMBANKMENT: Self = Self(1 << 2);
    /// A built-up bank of 65° or steeper: a wall holding up the road.
    pub const RETAINING_WALL: Self = Self(1 << 3);
    /// A bank between 45° and 65°.
    pub const BLUFF: Self = Self(1 << 4);
    /// A bank of 65° or steeper, up to vertical.
    pub const CLIFF: Self = Self(1 << 5);

    /// Whether every tag of `other` is in `self`.
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether there are no tags.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The tags' names, in the order the constants are listed:
    /// `"road"`, `"cut"`, `"embankment"`, `"retaining_wall"`, `"bluff"`,
    /// `"cliff"`.
    pub fn names(self) -> Vec<&'static str> {
        [
            (Self::ROAD, "road"),
            (Self::CUT, "cut"),
            (Self::EMBANKMENT, "embankment"),
            (Self::RETAINING_WALL, "retaining_wall"),
            (Self::BLUFF, "bluff"),
            (Self::CLIFF, "cliff"),
        ]
        .into_iter()
        .filter(|&(tag, _)| self.contains(tag))
        .map(|(_, name)| name)
        .collect()
    }
}

impl std::ops::BitOr for RoadTags {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOrAssign for RoadTags {
    fn bitor_assign(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

/// What a [`PointOfInterest`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PoiKind {
    /// Where the region's roads lead: the end of the road network.
    RoadEnd,
    /// Where a road stops at a river bank. Its
    /// [`partner`](PointOfInterest::partner) is where the road goes on, on
    /// the far bank; the caller may span the gap between, with a bridge or
    /// anything else.
    RiverCrossing,
    /// A bend tighter than the road's
    /// [`min_radius`](crate::RoadOptions::min_radius), where the ground
    /// left no room for a wider one.
    SharpBend,
}

/// A place the caller may fill with content of their own, such as a
/// village at the end of a road or a bridge over a river.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointOfInterest {
    /// What the place is.
    pub kind: PoiKind,
    /// Position, in metres (x and z).
    pub position: DVec2,
    /// The height of the place, in metres: the road surface there.
    pub level: f32,
    /// For a place that comes in a pair, such as the two banks of a
    /// [`RiverCrossing`](PoiKind::RiverCrossing), the other one, as an
    /// index into the same list of points of interest.
    pub partner: Option<usize>,
}

/// All features of one terrain, ready to carve the ground at any point.
#[derive(Debug, Clone, Default)]
pub(crate) struct Features {
    pub rivers: Vec<FeatureLine>,
    pub ponds: Vec<Pond>,
    pub roads: Vec<FeatureLine>,
    /// For each river, its valley's shape at each point.
    river_valleys: Vec<Vec<ValleyShape>>,
    /// For each road, how steeply its banks start at each point, as rise
    /// over run.
    road_banks: Vec<Vec<f32>>,
    /// For each road, whether its start and its end are open: the road
    /// stops there (at a river bank) and holds no ground beyond.
    road_open: Vec<[bool; 2]>,
    /// Places left for the caller to fill; they don't change the ground.
    pub points_of_interest: Vec<PointOfInterest>,
    /// Each feature's box outside which it changes nothing, and its place
    /// in its kind's list.
    bounds: BTreeMap<FeatureId, ([DVec2; 2], u32)>,
    index: FeatureIndex,
}

/// The ground after carving, the water surface if there is water, and what
/// the roads made of the ground there.
pub(crate) struct Carved {
    pub height: f32,
    pub water: Option<f32>,
    pub tags: RoadTags,
}

/// The water and roads near a point (see [`Features::nearby`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Nearby {
    /// Distance to the nearest water's edge, 0 in the water, at most the
    /// reach asked for.
    pub water_distance: f64,
    /// Distance to the nearest road surface's edge, 0 on the road, at most
    /// the reach asked for.
    pub road_distance: f64,
    /// The nearest water's surface, mixed with those of waters within
    /// [`WATER_MIX`] of as near by the inverse square of their distance;
    /// `None` where no water is within the reach less [`WATER_MIX`].
    pub water_level: Option<f32>,
    /// What the nearest water is, [`FeatureKind::River`] or
    /// [`FeatureKind::Pond`]; `None` where no water is within the reach.
    /// Where a river and a pond are equally near (a river inside the pond
    /// it ends in), the pond.
    pub water_kind: Option<FeatureKind>,
}

/// The closest point on a feature line's segment: distance to it and the
/// level there.
fn segment_query(line: &FeatureLine, segment: usize, p: DVec2) -> (f64, f32) {
    let (d, t) = segment_position(line, segment, p);
    let (la, lb) = (line.levels[segment], line.levels[segment + 1]);
    (d, la + (lb - la) * t as f32)
}

/// Half the wider of a feature line's widths at the two ends of segment
/// `segment`, in metres.
fn half_width(line: &FeatureLine, segment: usize) -> f64 {
    0.5 * f64::from(line.widths[segment].max(line.widths[segment + 1]))
}

/// The distance from `p` to a feature line's segment, and how far along
/// the segment (0 to 1) its closest point is.
fn segment_position(line: &FeatureLine, segment: usize, p: DVec2) -> (f64, f64) {
    let (a, b) = (line.points[segment], line.points[segment + 1]);
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 {
        ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p.distance(a + ab * t), t)
}

/// How the distance from `p` to a feature line's segment, and the share
/// along it of its closest point, change along x and z (see
/// [`segment_position`]). Both are taken as not changing where the closest
/// point is an end, or `p` is on the segment.
fn segment_slopes(line: &FeatureLine, segment: usize, p: DVec2) -> (DVec2, DVec2) {
    let (a, b) = (line.points[segment], line.points[segment + 1]);
    let ab = b - a;
    let (d, t) = segment_position(line, segment, p);
    let distance = if d > 0.0 {
        (p - (a + ab * t)) / d
    } else {
        DVec2::ZERO
    };
    let along = if t > 0.0 && t < 1.0 {
        ab / ab.length_squared()
    } else {
        DVec2::ZERO
    };
    (distance, along)
}

/// How many times faster the distance from a road grows past an open end
/// (see [`Features::new`]) than across the road: the road's hold on the
/// ground ends within a tenth of [`BANK_REACH`] past the end.
const OPEN_END: f64 = 10.0;

/// For `p` beyond an open end of segment `segment` of road `line` (`open`
/// says which ends are open), the distance to the road with the part along
/// the road counted [`OPEN_END`] times over, and its gradient. `None` if
/// `p` is not beyond an open end of this segment.
fn past_open_end(
    line: &FeatureLine,
    segment: usize,
    open: [bool; 2],
    p: DVec2,
) -> Option<(f64, DVec2)> {
    let last = line.points.len() - 2;
    let (a, b) = (line.points[segment], line.points[segment + 1]);
    let direction = (b - a).normalize_or_zero();
    if direction == DVec2::ZERO {
        return None;
    }
    // How far past the end, and which way "past" points.
    let (end, outward) = if open[0] && segment == 0 && (p - a).dot(direction) < 0.0 {
        (a, -direction)
    } else if open[1] && segment == last && (p - b).dot(direction) > 0.0 {
        (b, direction)
    } else {
        return None;
    };
    let past = (p - end).dot(outward);
    let across = (p - end).perp_dot(direction);
    let d = across.hypot(OPEN_END * past);
    let slope = if d > 0.0 {
        (-direction.perp() * across + outward * (OPEN_END * OPEN_END * past)) / d
    } else {
        DVec2::ZERO
    };
    Some((d, slope))
}

/// The gradients of [`smooth_min`]'s result by `a` and by `b`.
fn smooth_min_slopes(a: f64, b: f64, k: f64) -> (f64, f64) {
    let h = (k - (a - b).abs()).max(0.0) / k;
    if a < b {
        (1.0 - 0.5 * h, 0.5 * h)
    } else {
        (0.5 * h, 1.0 - 0.5 * h)
    }
}

/// The gradients of [`smooth_max`]'s result by `a` and by `b`.
fn smooth_max_slopes(a: f64, b: f64, k: f64) -> (f64, f64) {
    smooth_min_slopes(-a, -b, k)
}

/// The shape of a river's valley on its two sides at one river point:
/// index 0 is the left side looking downstream, 1 the right. All distances
/// are in metres from the water's edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ValleyShape {
    /// How wide a flat floor, at the water's level, runs out from the water.
    pub floor: [f32; 2],
    /// How steeply the valley wall rises beyond the floor, as rise over run.
    pub wall: [f32; 2],
    /// How far out the valley reaches; beyond it the ground is untouched.
    /// Always more than `floor`.
    pub reach: [f32; 2],
    /// Whether gullies cut the valley walls. Off for valleys that may reach
    /// a region's border, such as crossing pieces': the neighbour couldn't
    /// cut the same gullies.
    pub gullies: bool,
}

impl ValleyShape {
    /// The same valley on both sides: no floor, a wall at [`RIVER_BANK`],
    /// reaching `reach` metres out, and no gullies.
    pub(crate) fn even(reach: f64) -> Self {
        Self {
            floor: [0.0; 2],
            wall: [RIVER_BANK as f32; 2],
            reach: [reach as f32; 2],
            gullies: false,
        }
    }

    /// The furthest either side reaches, in metres.
    fn extent(&self) -> f64 {
        f64::from(self.reach[0].max(self.reach[1]))
    }

    /// The shape a fraction `t` of the way from `self` to `other`, taking
    /// `left` (0 to 1) of the left side and the rest of the right: its
    /// floor, wall and reach.
    fn side_towards(&self, other: &Self, t: f64, left: f64) -> (f64, f64, f64) {
        let pick = |v: [f32; 2]| f64::from(v[1]) + (f64::from(v[0]) - f64::from(v[1])) * left;
        let lerp = |a: [f32; 2], b: [f32; 2]| pick(a) + (pick(b) - pick(a)) * t;
        (
            lerp(self.floor, other.floor),
            lerp(self.wall, other.wall),
            lerp(self.reach, other.reach),
        )
    }

    /// The gradients of [`side_towards`](Self::side_towards)'s floor, wall
    /// and reach, given the gradients `t_slope` of `t` and `left_slope` of
    /// `left`.
    fn side_towards_slopes(
        &self,
        other: &Self,
        t: f64,
        left: f64,
        t_slope: DVec2,
        left_slope: DVec2,
    ) -> [DVec2; 3] {
        let slope = |a: [f32; 2], b: [f32; 2]| {
            let pick = |v: [f32; 2]| f64::from(v[1]) + (f64::from(v[0]) - f64::from(v[1])) * left;
            let gap = |v: [f32; 2]| f64::from(v[0]) - f64::from(v[1]);
            let by_left = gap(a) + (gap(b) - gap(a)) * t;
            t_slope * (pick(b) - pick(a)) + left_slope * by_left
        };
        [
            slope(self.floor, other.floor),
            slope(self.wall, other.wall),
            slope(self.reach, other.reach),
        ]
    }
}

/// How far the corner between a valley's floor and its wall is rounded
/// off, in metres.
const FLOOR_ROUNDING: f64 = 8.0;

/// `max(t, 0)` with its corner rounded over [`FLOOR_ROUNDING`] metres, so
/// its slope is continuous.
fn rounded_ramp(t: f64) -> f64 {
    if t <= 0.0 {
        0.0
    } else if t < FLOOR_ROUNDING {
        t * t / (2.0 * FLOOR_ROUNDING)
    } else {
        t - 0.5 * FLOOR_ROUNDING
    }
}

/// The slope of [`rounded_ramp`] at `t`.
fn rounded_ramp_slope(t: f64) -> f64 {
    if t <= 0.0 {
        0.0
    } else if t < FLOOR_ROUNDING {
        t / FLOOR_ROUNDING
    } else {
        1.0
    }
}

/// What the gradients of the inputs of [`valley`] are: of the distance, the
/// level, and the floor, wall and reach. The half width and depth don't
/// change within one segment.
struct ValleySlopes {
    distance: DVec2,
    level: DVec2,
    shape: [DVec2; 3],
}

/// The gradients of [`valley`]'s profile and pull, for the same inputs and
/// their gradients `slopes`. Zero where `valley` gives `None`.
fn valley_slopes(
    d: f64,
    half_width: f64,
    depth: f32,
    (floor, wall, reach): (f64, f64, f64),
    slopes: &ValleySlopes,
) -> (DVec2, DVec2) {
    if d <= half_width {
        let r = d / half_width;
        let profile = slopes.level + slopes.distance * (2.0 * f64::from(depth) * r / half_width);
        return (profile, DVec2::ZERO);
    }
    let x = d - half_width;
    if x >= reach {
        return (DVec2::ZERO, DVec2::ZERO);
    }
    let [floor_slope, wall_slope, reach_slope] = slopes.shape;
    let width = reach - floor;
    let raw = (x - floor) / width;
    let (u, u_slope) = if raw > 0.0 {
        let along = slopes.distance - floor_slope;
        (
            raw,
            (along * width - (reach_slope - floor_slope) * (x - floor)) / (width * width),
        )
    } else {
        (0.0, DVec2::ZERO)
    };
    let fade = 1.0 - u * u;
    let pull = u_slope * (-4.0 * u * fade);
    let profile = slopes.level
        + wall_slope * rounded_ramp(x - floor)
        + (slopes.distance - floor_slope) * (wall * rounded_ramp_slope(x - floor));
    (profile, pull)
}

/// The shape a river or pond pulls the ground towards at distance `d` from
/// its centre line: the profile height and how strongly (0 to 1) the
/// ground is pulled to it, or `None` beyond its valley.
///
/// Inside the water the bed is a parabola `depth` deep, and the ground is
/// pulled all the way to it. Outside, the profile is a floor at the water's
/// level `floor` metres wide, then a wall rising at `wall` (rise over run).
/// The pull is full across the floor and fades to nothing `reach` metres
/// from the water's edge. Ground above the profile is lowered by the pull
/// times its height above the profile, so the valley keeps a growing share
/// of the ground's own relief away from the water. A deep cut needs a large
/// `reach` to open into a valley rather than a gorge.
fn valley(
    d: f64,
    half_width: f64,
    level: f32,
    depth: f32,
    (floor, wall, reach): (f64, f64, f64),
) -> Option<(f64, f64)> {
    let level = f64::from(level);
    if d <= half_width {
        let r = d / half_width;
        return Some((level - f64::from(depth) * (1.0 - r * r), 1.0));
    }
    let x = d - half_width;
    (x < reach).then(|| {
        let u = ((x - floor) / (reach - floor)).max(0.0);
        let fade = 1.0 - u * u;
        (level + wall * rounded_ramp(x - floor), fade * fade)
    })
}

/// About how far apart gullies on a valley wall are, in metres.
const GULLY_SPACING: f64 = 40.0;

/// How deep a gully cuts, as a share of how far the valley wall stands
/// above the water there.
const GULLY_SHARE: f64 = 0.15;

/// The deepest a gully cuts, in metres.
const GULLY_DEPTH: f64 = 6.0;

/// Gullies stay this far from a road's edge, in metres, and fade in over
/// [`GULLY_ROAD_FADE`] metres beyond that.
const GULLY_ROAD_CLEARANCE: f64 = 10.0;
const GULLY_ROAD_FADE: f64 = 30.0;

/// The deepest a river's gullies cut into a valley wall here, in metres:
/// a gully's floor; [`grooves`] says how much of it the gullies cut at a
/// given point. `x` is how far the point is from the water's edge,
/// `(floor, wall, reach)` the valley's shape on this side, and `above` how
/// high the valley wall stands above the water there.
///
/// Gullies are nothing on the floor and at the valley's reach and deepest
/// on the lower wall; they are at most [`GULLY_SHARE`] of `above` and
/// [`GULLY_DEPTH`] deep.
fn gully_depth(x: f64, (floor, _, reach): (f64, f64, f64), above: f64) -> f64 {
    if x <= floor || x >= reach || above <= 0.0 {
        return 0.0;
    }
    // Strongest a third of the way up the wall.
    let u = (x - floor) / (reach - floor);
    let mask = 6.75 * u * (1.0 - u) * (1.0 - u);
    (GULLY_SHARE * above).min(GULLY_DEPTH) * mask
}

/// How much of [`gully_depth`] the gullies cut at `p`, from 0 on the spurs
/// between gullies to 1 along a gully's floor. `flow` is the unit
/// direction towards the river.
fn grooves(p: DVec2, flow: DVec2) -> f64 {
    // Two octaves of stripes; narrow grooves, broad spurs.
    let v = (gully::stripes(p, flow, GULLY_SPACING, 0)
        + 0.5 * gully::stripes(p, flow, 0.5 * GULLY_SPACING, 1))
        / 1.5;
    let groove = 0.5 * (v + 1.0);
    groove * groove
}

/// The gradient of [`gully_depth`], given the gradients of `x`, of the
/// shape's floor and reach (`floor_slope`, `reach_slope`), and of `above`.
fn gully_depth_slope(
    x: f64,
    (floor, _, reach): (f64, f64, f64),
    above: f64,
    [x_slope, floor_slope, reach_slope, above_slope]: [DVec2; 4],
) -> DVec2 {
    if x <= floor || x >= reach || above <= 0.0 {
        return DVec2::ZERO;
    }
    let width = reach - floor;
    let u = (x - floor) / width;
    let u_slope = ((x_slope - floor_slope) * width - (reach_slope - floor_slope) * (x - floor))
        / (width * width);
    let mask = 6.75 * u * (1.0 - u) * (1.0 - u);
    let mask_slope = u_slope * (6.75 * (1.0 - u) * (1.0 - 3.0 * u));
    let (cap, cap_slope) = if GULLY_SHARE * above < GULLY_DEPTH {
        (GULLY_SHARE * above, above_slope * GULLY_SHARE)
    } else {
        (GULLY_DEPTH, DVec2::ZERO)
    };
    cap_slope * mask + mask_slope * cap
}

/// The gradient of [`grooves`], where `flow` changes with `p` by
/// `flow_slope` (along x, then along z).
fn grooves_slope(p: DVec2, flow: DVec2, flow_slope: [DVec2; 2]) -> DVec2 {
    let v = (gully::stripes(p, flow, GULLY_SPACING, 0)
        + 0.5 * gully::stripes(p, flow, 0.5 * GULLY_SPACING, 1))
        / 1.5;
    let v_slope = (gully::stripes_slope(p, flow, flow_slope, GULLY_SPACING, 0)
        + gully::stripes_slope(p, flow, flow_slope, 0.5 * GULLY_SPACING, 1) * 0.5)
        / 1.5;
    v_slope * (0.5 * (v + 1.0))
}

/// How far out a river's valley may reach at a point where it cuts `cut`
/// metres below the natural ground, beyond a floor `floor` metres wide, with
/// walls at `wall` (rise over run): far enough for its wall to climb out
/// with room to spare, at least [`BANK_REACH`] and at most
/// [`MAX_VALLEY_REACH`].
pub(crate) fn valley_reach(cut: f64, floor: f64, wall: f64) -> f64 {
    (floor + VALLEY_SPARE * cut.max(0.0) / wall).clamp(BANK_REACH, MAX_VALLEY_REACH)
}

/// How far a road lets the ground differ from its level at distance `d`
/// from its centre line, or `None` beyond its cuttings and embankments.
/// `bank` is the slope of its banks, as rise over run: a flat face at that
/// slope, steepening only close to [`BANK_REACH`] so that every bank meets
/// the ground within it.
fn road_tolerance(d: f64, half_width: f64, bank: f64) -> Option<f64> {
    let x = d - half_width;
    if x <= 0.0 {
        return Some(ROAD_TOLERANCE);
    }
    (x < BANK_REACH).then(|| ROAD_TOLERANCE + bank * x / (1.0 - (x / BANK_REACH).powi(8)))
}

/// The gradient of [`road_tolerance`] at `x` metres beyond the road's
/// edge, given the gradients of `x` and of `bank`.
fn road_tolerance_slope(x: f64, bank: f64, x_slope: DVec2, bank_slope: DVec2) -> DVec2 {
    if x <= 0.0 || x >= BANK_REACH {
        return DVec2::ZERO;
    }
    let u8 = (x / BANK_REACH).powi(8);
    let fall = 1.0 - u8;
    let fall_slope = x_slope * (-8.0 * u8 / x);
    bank_slope * (x / fall) + (x_slope * fall - fall_slope * x) * (bank / (fall * fall))
}

/// The slope, as rise over run, of a road's cutting or embankment bank at
/// distance `d` from its centre line, or `None` on the road itself or
/// beyond its banks. It is the slope of [`road_tolerance`] with the same
/// `bank`.
fn bank_slope(d: f64, half_width: f64, bank: f64) -> Option<f64> {
    let x = d - half_width;
    if x <= 0.0 || x >= BANK_REACH {
        return None;
    }
    let u8 = (x / BANK_REACH).powi(8);
    Some(bank * (1.0 + 7.0 * u8) / (1.0 - u8).powi(2))
}

/// How steeply a road's banks start, as rise over run, where the natural
/// ground around it has slope `ground`: twice as steep, so the bank always
/// meets the ground, and never gentler than [`ROAD_BANK`] or steeper than
/// [`MAX_BANK`].
pub(crate) fn bank_for(ground: f64) -> f64 {
    (BANK_FACTOR * ground).clamp(ROAD_BANK, MAX_BANK)
}

impl Features {
    /// Gathers features and builds the spatial index.
    ///
    /// `river_valleys` gives, for each of `rivers`, its valley's shape at
    /// each of its points; `road_banks` gives, for each of `roads`, how
    /// steeply its banks start at each of its points (see [`bank_for`]);
    /// `road_open`, for each road, whether it stops short at its start and
    /// at its end, holding no ground beyond (a few metres past such an end
    /// the ground is left as it is).
    ///
    /// # Panics
    ///
    /// If `river_valleys` doesn't have one shape per river point,
    /// `road_banks` one value per road point, or `road_open` one entry per
    /// road.
    pub(crate) fn new(
        rivers: Vec<FeatureLine>,
        river_valleys: Vec<Vec<ValleyShape>>,
        ponds: Vec<Pond>,
        roads: Vec<FeatureLine>,
        road_banks: Vec<Vec<f32>>,
        road_open: Vec<[bool; 2]>,
        points_of_interest: Vec<PointOfInterest>,
    ) -> Self {
        assert_eq!(road_open.len(), roads.len(), "one pair of ends per road");
        fn matches<T>(values: &[Vec<T>], lines: &[FeatureLine]) -> bool {
            values.len() == lines.len()
                && values
                    .iter()
                    .zip(lines)
                    .all(|(v, l)| v.len() == l.points.len())
        }
        assert!(
            matches(&river_valleys, &rivers),
            "one valley shape per river point"
        );
        assert!(
            matches(&road_banks, &roads),
            "one bank steepness per road point"
        );
        let parts = Self::parts(&rivers, &river_valleys, &ponds, &roads);
        let mut bounds: BTreeMap<FeatureId, ([DVec2; 2], u32)> = BTreeMap::new();
        for (part, [lo, hi]) in &parts {
            let (b, _) = bounds.entry(part.id).or_insert(([*lo, *hi], part.index));
            *b = [b[0].min(*lo), b[1].max(*hi)];
        }
        Self {
            rivers,
            river_valleys,
            ponds,
            roads,
            road_banks,
            road_open,
            points_of_interest,
            bounds,
            index: FeatureIndex::new(parts),
        }
    }

    /// Every piece of every feature, with the box outside which it changes
    /// nothing: rivers' segments (out to their valleys' reach), ponds, then
    /// roads' segments (out to their banks' reach), each in order.
    fn parts(
        rivers: &[FeatureLine],
        river_valleys: &[Vec<ValleyShape>],
        ponds: &[Pond],
        roads: &[FeatureLine],
    ) -> Vec<(Part, [DVec2; 2])> {
        let mut parts = Vec::new();
        let line_starts = |lines: &[FeatureLine]| -> Vec<[DVec2; 2]> {
            lines
                .iter()
                .map(|l| [l.points[0], *l.points.get(1).unwrap_or(&l.points[0])])
                .collect()
        };
        let pond_starts: Vec<[DVec2; 2]> = ponds.iter().map(|p| [p.centre; 2]).collect();
        let keys = [
            feature_keys(FeatureKind::River, &line_starts(rivers)),
            feature_keys(FeatureKind::Pond, &pond_starts),
            feature_keys(FeatureKind::Road, &line_starts(roads)),
        ];
        let id = |kind, index: usize| {
            let k = match kind {
                FeatureKind::River => 0,
                FeatureKind::Pond => 1,
                FeatureKind::Road => 2,
            };
            (
                FeatureId {
                    kind,
                    key: keys[k][index],
                },
                index as u32,
            )
        };
        for (line, river) in rivers.iter().enumerate() {
            let valleys = &river_valleys[line];
            for segment in 0..river.points.len().saturating_sub(1) {
                let valley = valleys[segment].extent().max(valleys[segment + 1].extent());
                let reach = half_width(river, segment) + valley;
                let (a, b) = (river.points[segment], river.points[segment + 1]);
                let (id, index) = id(FeatureKind::River, line);
                let part = Part {
                    id,
                    index,
                    segment: segment as u32,
                };
                parts.push((part, [a.min(b) - reach, a.max(b) + reach]));
            }
        }
        for (i, pond) in ponds.iter().enumerate() {
            let reach = DVec2::splat(pond.radius + BANK_REACH);
            let (id, index) = id(FeatureKind::Pond, i);
            let part = Part {
                id,
                index,
                segment: 0,
            };
            parts.push((part, [pond.centre - reach, pond.centre + reach]));
        }
        for (line, road) in roads.iter().enumerate() {
            for segment in 0..road.points.len().saturating_sub(1) {
                let reach = half_width(road, segment) + BANK_REACH;
                let (a, b) = (road.points[segment], road.points[segment + 1]);
                let (id, index) = id(FeatureKind::Road, line);
                let part = Part {
                    id,
                    index,
                    segment: segment as u32,
                };
                parts.push((part, [a.min(b) - reach, a.max(b) + reach]));
            }
        }
        parts
    }

    /// Every feature, in id order, with its bounds. Features without a
    /// piece (a line of one point) are left out: they change nothing.
    pub(crate) fn all(&self) -> impl Iterator<Item = TerrainFeature<'_>> + '_ {
        self.bounds
            .iter()
            .map(|(&id, &(bounds, index))| TerrainFeature {
                id,
                shape: self.shape(id.kind, index),
                bounds,
            })
    }

    /// The feature `id`, or `None` if there is no such feature or it has no
    /// piece.
    pub(crate) fn get(&self, id: FeatureId) -> Option<TerrainFeature<'_>> {
        let &(bounds, index) = self.bounds.get(&id)?;
        Some(TerrainFeature {
            id,
            shape: self.shape(id.kind, index),
            bounds,
        })
    }

    /// The shape of the feature of `kind` at `index` in its list.
    fn shape(&self, kind: FeatureKind, index: u32) -> FeatureShape<'_> {
        let i = index as usize;
        match kind {
            FeatureKind::River => FeatureShape::Line(&self.rivers[i]),
            FeatureKind::Road => FeatureShape::Line(&self.roads[i]),
            FeatureKind::Pond => FeatureShape::Pond(&self.ponds[i]),
        }
    }

    /// The features that may change something in the box from `lo` to
    /// `hi`: those with a piece whose box meets it, in id order.
    pub(crate) fn within(&self, lo: DVec2, hi: DVec2) -> Vec<FeatureId> {
        self.index.near(lo, hi).into_iter().collect()
    }

    /// Segment `segment` of road `line` seen from `p`: the distance to it,
    /// the road's half width, and its level and bank steepness at the
    /// closest point. Past an open end the distance grows
    /// [`OPEN_END`] times faster along the road than across it.
    fn road_at(&self, line: u32, segment: u32, p: DVec2) -> (f64, f64, f64, f64) {
        let (road, banks) = (&self.roads[line as usize], &self.road_banks[line as usize]);
        let s = segment as usize;
        let (mut d, t) = segment_position(road, s, p);
        if let Some((open, _)) = past_open_end(road, s, self.road_open[line as usize], p) {
            d = open;
        }
        let lerp = |a: f32, b: f32| f64::from(a) + (f64::from(b) - f64::from(a)) * t;
        (
            d,
            0.5 * lerp(road.widths[s], road.widths[s + 1]),
            lerp(road.levels[s], road.levels[s + 1]),
            lerp(banks[s], banks[s + 1]),
        )
    }

    /// The gradients of [`road_at`](Self::road_at)'s distance, half width,
    /// level and bank steepness.
    fn road_slopes(&self, line: u32, segment: u32, p: DVec2) -> [DVec2; 4] {
        let (road, banks) = (&self.roads[line as usize], &self.road_banks[line as usize]);
        let s = segment as usize;
        let (mut distance, t) = segment_slopes(road, s, p);
        if let Some((_, slope)) = past_open_end(road, s, self.road_open[line as usize], p) {
            distance = slope;
        }
        let change = |a: f32, b: f32| t * (f64::from(b) - f64::from(a));
        [
            distance,
            change(road.widths[s], road.widths[s + 1]) * 0.5,
            change(road.levels[s], road.levels[s + 1]),
            change(banks[s], banks[s + 1]),
        ]
    }

    /// Carves ground of height `h` at `p`: rivers and ponds cut it down,
    /// then roads cut or fill it to their level.
    #[cfg(test)]
    pub(crate) fn apply(&self, p: DVec2, h: f32) -> Carved {
        self.carve(p, h, None).0
    }

    /// [`apply`](Self::apply), with the gradient of the carved ground when
    /// `slope`, the gradient of the uncarved ground at `p`, is given;
    /// otherwise the gradient returned is zero.
    pub(crate) fn carve(&self, p: DVec2, h: f32, slope: Option<DVec2>) -> (Carved, DVec2) {
        self.carve_parts(p, h, slope, self.index.at(p))
    }

    /// The water and roads within `reach` metres of `p`, at most
    /// [`BANK_REACH`]: see [`Nearby`].
    pub(crate) fn nearby(&self, p: DVec2, reach: f64) -> Nearby {
        debug_assert!(reach > 0.0 && reach <= BANK_REACH);
        let mut nearby = Nearby {
            water_distance: reach,
            road_distance: reach,
            water_level: None,
            water_kind: None,
        };
        // Every piece within `reach` of `p` is listed at `p`: each piece's
        // box reaches at least `BANK_REACH` beyond its edge.
        let parts = self.index.at(p);
        let water_at = |part: &Part| -> Option<(f64, f32)> {
            let (line, s) = (part.index as usize, part.segment as usize);
            let (gap, level) = match part.id.kind {
                FeatureKind::River => {
                    let river = &self.rivers[line];
                    let (d, level) = segment_query(river, s, p);
                    (d - half_width(river, s), level)
                }
                FeatureKind::Pond => {
                    let pond = &self.ponds[line];
                    (p.distance(pond.centre) - pond.radius, pond.level)
                }
                FeatureKind::Road => return None,
            };
            Some((gap.max(0.0), level))
        };

        // The nearest road and water. Equal gaps go to a pond, whatever
        // order the pieces are listed in.
        for part in parts {
            if part.id.kind == FeatureKind::Road {
                let (d, half_width, _, _) = self.road_at(part.index, part.segment, p);
                nearby.road_distance = nearby.road_distance.min((d - half_width).max(0.0));
            } else if let Some((gap, _)) = water_at(part) {
                let nearer = gap < nearby.water_distance
                    || (gap == nearby.water_distance
                        && nearby.water_kind.is_some()
                        && part.id.kind == FeatureKind::Pond);
                if nearer {
                    nearby.water_kind = Some(part.id.kind);
                }
                nearby.water_distance = nearby.water_distance.min(gap);
            }
        }
        let nearest = nearby.water_distance;
        if nearest >= reach - WATER_MIX {
            return nearby;
        }

        // The surface: the nearest water's, mixed with those of waters
        // nearly as near by the inverse square of their distance, down to
        // nothing for those `WATER_MIX` further off, all within `reach`.
        // Each piece counts on its own, so a river that comes back near
        // itself mixes both stretches rather than jumping between them.
        let (mut weights, mut levels) = (0.0, 0.0);
        for (gap, level) in parts.iter().filter_map(water_at) {
            let weight = (1.0 - smootherstep((gap - nearest) / WATER_MIX)) / (1.0 + gap).powi(2);
            weights += weight;
            levels += weight * f64::from(level);
        }
        nearby.water_level = Some((levels / weights) as f32);
        nearby
    }

    /// Every piece of every feature, in the order the index lists them.
    #[cfg(test)]
    pub(crate) fn all_parts(&self) -> Vec<Part> {
        Self::parts(&self.rivers, &self.river_valleys, &self.ponds, &self.roads)
            .into_iter()
            .map(|(part, _)| part)
            .collect()
    }

    /// [`carve`](Self::carve) with only the feature pieces `parts`, in
    /// that order. Pieces that can't reach `p` change nothing.
    pub(crate) fn carve_parts(
        &self,
        p: DVec2,
        h: f32,
        slope: Option<DVec2>,
        parts: &[Part],
    ) -> (Carved, DVec2) {
        let h_slope = slope.unwrap_or_default();
        if parts.is_empty() {
            let carved = Carved {
                height: h,
                water: None,
                tags: RoadTags::NONE,
            };
            return (carved, h_slope);
        }
        let track = slope.is_some();

        // Roads near here: gullies keep away from them, then they hold the
        // ground to their level.
        let road_items: Vec<(u32, u32)> = parts
            .iter()
            .filter(|part| part.id.kind == FeatureKind::Road)
            .map(|part| (part.index, part.segment))
            .collect();
        let roads: Vec<(f64, f64, f64, f64)> = road_items
            .iter()
            .map(|&(line, segment)| self.road_at(line, segment, p))
            .collect();
        let road_slopes: Vec<[DVec2; 4]> = if track {
            road_items
                .iter()
                .map(|&(line, segment)| self.road_slopes(line, segment, p))
                .collect()
        } else {
            Vec::new()
        };
        let road_gap = roads
            .iter()
            .map(|&(d, half_width, _, _)| d - half_width)
            .fold(f64::INFINITY, f64::min);
        let road_fade = smootherstep((road_gap - GULLY_ROAD_CLEARANCE) / GULLY_ROAD_FADE);
        let road_fade_slope = if track && road_gap.is_finite() {
            // The nearest road's gap, as `fold` picks it.
            let nearest = roads
                .iter()
                .position(|&(d, half_width, _, _)| d - half_width == road_gap)
                .expect("the gap is one road's");
            let [distance, half_width, _, _] = road_slopes[nearest];
            let t = (road_gap - GULLY_ROAD_CLEARANCE) / GULLY_ROAD_FADE;
            (distance - half_width) * (smootherstep_slope(t) / GULLY_ROAD_FADE)
        } else {
            DVec2::ZERO
        };

        // Rivers and ponds: each lowers ground above its valley profile by
        // its pull, and a river's walls by its gullies; the deepest
        // lowering wins. Note the water surface wherever the point is
        // within the water's width.
        let mut water: Option<f32> = None;
        let mut lowering = 0.0_f64;
        let mut lowering_slope = DVec2::ZERO;
        let no_slopes = || ValleySlopes {
            distance: DVec2::ZERO,
            level: DVec2::ZERO,
            shape: [DVec2::ZERO; 3],
        };
        for part in parts {
            // Each piece's inputs to its valley, with their gradients when
            // tracked, and for a gullied river the way to it and how that
            // way turns.
            let (line, segment) = (part.index, part.segment);
            let (d, half_width, level, depth, shape, flow, slopes, flow_slope) = match part.id.kind
            {
                FeatureKind::River => {
                    let (river, valleys) = (
                        &self.rivers[line as usize],
                        &self.river_valleys[line as usize],
                    );
                    let s = segment as usize;
                    let (d, level) = segment_query(river, s, p);
                    let (_, t) = segment_position(river, s, p);
                    // How much `p` is on the river's left (looking
                    // downstream): all or nothing beside the segment,
                    // blending round its ends so the two sides' shapes
                    // meet without a step. And the way to the river.
                    let (a, b) = (river.points[s], river.points[s + 1]);
                    let direction = (b - a).normalize_or_zero();
                    let lateral = direction.perp_dot(p - a);
                    let raw_left = 0.5 + 0.5 * lateral / d;
                    let left = if d > 0.0 {
                        raw_left.clamp(0.0, 1.0)
                    } else {
                        0.5
                    };
                    let shape = valleys[s].side_towards(&valleys[s + 1], t, left);
                    let flow = (valleys[s].gullies && valleys[s + 1].gullies)
                        .then(|| (a.lerp(b, t) - p).normalize_or_zero());
                    let (mut slopes, mut flow_slope) = (no_slopes(), [DVec2::ZERO; 2]);
                    if track {
                        let (d_slope, t_slope) = segment_slopes(river, s, p);
                        let left_slope = if d > 0.0 && raw_left > 0.0 && raw_left < 1.0 {
                            (direction.perp() * d - d_slope * lateral) * (0.5 / (d * d))
                        } else {
                            DVec2::ZERO
                        };
                        let level_change =
                            f64::from(river.levels[s + 1]) - f64::from(river.levels[s]);
                        slopes = ValleySlopes {
                            distance: d_slope,
                            level: t_slope * level_change,
                            shape: valleys[s].side_towards_slopes(
                                &valleys[s + 1],
                                t,
                                left,
                                t_slope,
                                left_slope,
                            ),
                        };
                        // The way to the river's closest point, `n / |n|`.
                        let n = a.lerp(b, t) - p;
                        let length = n.length();
                        if flow.is_some() && length > 0.0 {
                            let f = n / length;
                            flow_slope = [DVec2::X, DVec2::Y].map(|e| {
                                let dn = (b - a) * t_slope.dot(e) - e;
                                (dn - f * f.dot(dn)) / length
                            });
                        }
                    }
                    let width = half_width(river, s);
                    (
                        d,
                        width,
                        level,
                        river.depth,
                        shape,
                        flow,
                        slopes,
                        flow_slope,
                    )
                }
                FeatureKind::Pond => {
                    let pond = &self.ponds[line as usize];
                    let shape = (0.0, RIVER_BANK, BANK_REACH);
                    let d = p.distance(pond.centre);
                    let mut slopes = no_slopes();
                    if track && d > 0.0 {
                        slopes.distance = (p - pond.centre) / d;
                    }
                    let flow_slope = [DVec2::ZERO; 2];
                    (
                        d,
                        pond.radius,
                        pond.level,
                        pond.depth,
                        shape,
                        None,
                        slopes,
                        flow_slope,
                    )
                }
                FeatureKind::Road => continue,
            };
            if let Some((profile, pull)) = valley(d, half_width, level, depth, shape) {
                let above = smooth_max(f64::from(h) - profile, 0.0, RIVER_BLEND);
                let mut lowered = pull * above;
                let mut lowered_slope = DVec2::ZERO;
                let (profile_slope, pull_slope) = if track {
                    valley_slopes(d, half_width, depth, shape, &slopes)
                } else {
                    (DVec2::ZERO, DVec2::ZERO)
                };
                if track {
                    let (by_above, _) = smooth_max_slopes(f64::from(h) - profile, 0.0, RIVER_BLEND);
                    let above_slope = (h_slope - profile_slope) * by_above;
                    lowered_slope = pull_slope * above + above_slope * pull;
                }
                // Gullies, unless even their deepest couldn't beat another
                // river's lowering here.
                if let Some(flow) = flow {
                    let walled = f64::from(h) - lowered;
                    let most = gully_depth(d - half_width, shape, walled - f64::from(level));
                    if road_fade > 0.0 && most > 0.0 && lowered + road_fade * most > lowering {
                        let groove = grooves(p, flow);
                        if track {
                            let above_slope = h_slope - lowered_slope - slopes.level;
                            let [floor_slope, _, reach_slope] = slopes.shape;
                            let most_slope = gully_depth_slope(
                                d - half_width,
                                shape,
                                walled - f64::from(level),
                                [slopes.distance, floor_slope, reach_slope, above_slope],
                            );
                            lowered_slope += road_fade_slope * (most * groove)
                                + (most_slope * groove + grooves_slope(p, flow, flow_slope) * most)
                                    * road_fade;
                        }
                        lowered += road_fade * (most * groove);
                    }
                }
                if lowered > lowering {
                    lowering_slope = lowered_slope;
                }
                lowering = lowering.max(lowered);
                if d <= half_width {
                    water = Some(water.map_or(level, |w| w.max(level)));
                }
            }
        }
        let mut height = (f64::from(h) - lowering) as f32;
        let mut height_slope = h_slope - lowering_slope;

        // Roads: hold the ground within reach of the road's level.
        let uncut = f64::from(height);
        for (i, &(d, half_width, level, bank)) in roads.iter().enumerate() {
            if let Some(e) = road_tolerance(d, half_width, bank) {
                let held = smooth_min(f64::from(height), level + e, ROAD_BLEND);
                if track {
                    let [d_slope, width_slope, level_slope, bank_slope] = road_slopes[i];
                    let e_slope = road_tolerance_slope(
                        d - half_width,
                        bank,
                        d_slope - width_slope,
                        bank_slope,
                    );
                    let (by_ground, by_top) =
                        smooth_min_slopes(f64::from(height), level + e, ROAD_BLEND);
                    let held_slope = height_slope * by_ground + (level_slope + e_slope) * by_top;
                    let (by_held, by_bottom) = smooth_max_slopes(held, level - e, ROAD_BLEND);
                    height_slope = held_slope * by_held + (level_slope - e_slope) * by_bottom;
                }
                height = smooth_max(held, level - e, ROAD_BLEND) as f32;
            }
        }

        // Tags: the road surface, and banks that are the ground here,
        // cut down or built up, with how steep they are.
        let mut tags = RoadTags::NONE;
        for &(d, half_width, level, bank) in &roads {
            if d <= half_width {
                tags |= RoadTags::ROAD;
                continue;
            }
            let (Some(e), Some(slope)) = (
                road_tolerance(d, half_width, bank),
                bank_slope(d, half_width, bank),
            ) else {
                continue;
            };
            let on = |edge: f64| (f64::from(height) - edge).abs() <= ROAD_BLEND;
            if uncut > level + e && on(level + e) {
                tags |= RoadTags::CUT;
            } else if uncut < level - e && on(level - e) {
                tags |= if slope >= CLIFF_SLOPE {
                    RoadTags::RETAINING_WALL
                } else {
                    RoadTags::EMBANKMENT
                };
            } else {
                continue;
            }
            if slope >= CLIFF_SLOPE {
                tags |= RoadTags::CLIFF;
            } else if slope >= BLUFF_SLOPE {
                tags |= RoadTags::BLUFF;
            }
        }
        let carved = Carved {
            height,
            water: water.filter(|&w| height < w),
            tags,
        };
        (carved, height_slope)
    }
}

/// Smooths a line by Chaikin corner cutting, `iterations` times, keeping its
/// end points. Levels are blended with the same weights, so a line whose
/// levels never rise keeps that property.
pub(crate) fn chaikin(
    points: &[DVec2],
    levels: &[f32],
    iterations: usize,
) -> (Vec<DVec2>, Vec<f32>) {
    let (mut points, mut levels) = (points.to_vec(), levels.to_vec());
    for _ in 0..iterations {
        if points.len() < 3 {
            break;
        }
        let mut p = vec![points[0]];
        let mut l = vec![levels[0]];
        for i in 0..points.len() - 1 {
            let (a, b) = (points[i], points[i + 1]);
            let (la, lb) = (levels[i], levels[i + 1]);
            if i > 0 {
                p.push(a.lerp(b, 0.25));
                l.push(la + (lb - la) * 0.25);
            }
            if i < points.len() - 2 {
                p.push(a.lerp(b, 0.75));
                l.push(la + (lb - la) * 0.75);
            }
        }
        p.push(points[points.len() - 1]);
        l.push(levels[levels.len() - 1]);
        (points, levels) = (p, l);
    }
    (points, levels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn river() -> FeatureLine {
        FeatureLine {
            kind: FeatureKind::River,
            points: vec![DVec2::ZERO, DVec2::new(100.0, 0.0)],
            levels: vec![10.0, 9.0],
            widths: vec![6.0; 2],
            depth: 1.5,
        }
    }

    #[test]
    fn rivers_cut_a_bed_and_hold_water() {
        let features = Features::new(
            vec![river()],
            vec![vec![ValleyShape::even(BANK_REACH); 2]],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let middle = features.apply(DVec2::new(50.0, 0.0), 20.0);
        assert!((middle.height - 8.0).abs() < 1e-4);
        assert_eq!(middle.water, Some(9.5));
        // Far away, nothing changes.
        let far = features.apply(DVec2::new(50.0, 200.0), 20.0);
        assert_eq!(far.height, 20.0);
        assert_eq!(far.water, None);
    }

    #[test]
    fn nearby_water_and_roads_are_measured_from_their_edges() {
        // The river along z = 0 from x = 0 to 100 (3 m half-width, surface
        // 10 to 9 m), a road along z = 30 (3 m half-width) and a pond of
        // radius 5 at (160, 0).
        let road = FeatureLine {
            kind: FeatureKind::Road,
            points: vec![DVec2::new(0.0, 30.0), DVec2::new(100.0, 30.0)],
            depth: 0.0,
            ..river()
        };
        let pond = Pond {
            centre: DVec2::new(160.0, 0.0),
            level: 4.0,
            radius: 5.0,
            depth: 1.0,
        };
        let features = Features::new(
            vec![river()],
            vec![vec![ValleyShape::even(BANK_REACH); 2]],
            vec![pond],
            vec![road],
            vec![vec![ROAD_BANK as f32; 2]],
            vec![[false; 2]],
            Vec::new(),
        );
        let near = |x: f64, z: f64| features.nearby(DVec2::new(x, z), BANK_REACH);

        // In the river: no distance, and its own surface.
        let wet = near(50.0, 1.0);
        assert_eq!(wet.water_distance, 0.0);
        assert_eq!(wet.water_level, Some(9.5));
        assert_eq!(wet.road_distance, 30.0 - 1.0 - 3.0);
        assert_eq!(wet.water_kind, Some(FeatureKind::River));

        // Between the two: each edge's distance.
        let between = near(50.0, 13.0);
        assert!((between.water_distance - 10.0).abs() < 1e-12);
        assert!((between.road_distance - 14.0).abs() < 1e-12);
        assert!((between.water_level.unwrap() - 9.5).abs() < 1e-6);

        // Beyond the reach: the reach, and no water.
        let far = near(50.0, -60.0);
        assert_eq!(far.water_distance, BANK_REACH);
        assert_eq!(far.road_distance, BANK_REACH);
        assert_eq!(far.water_level, None);
        assert_eq!(far.water_kind, None);

        // Beside the pond, beyond the river's reach: its edge and its
        // surface.
        let pond_side = near(172.0, 0.0);
        assert!((pond_side.water_distance - 7.0).abs() < 1e-12);
        assert_eq!(pond_side.water_level, Some(4.0));
        assert_eq!(pond_side.water_kind, Some(FeatureKind::Pond));

        // Between the river's end and the pond: the pond, 18 m further off
        // than the river (gaps 17 and 35 m), doesn't count; 6 m further
        // off (gaps 23 and 29 m), it counts, less than the river.
        let nearer = near(120.0, 0.0);
        assert!((nearer.water_distance - 17.0).abs() < 1e-12);
        assert_eq!(nearer.water_level, Some(9.0));
        let level = near(126.0, 0.0).water_level.unwrap();
        assert!(level > 6.5 && level < 9.0, "{level}");
    }

    #[test]
    fn banks_meet_the_ground_without_a_step() {
        let features = Features::new(
            vec![river()],
            vec![vec![ValleyShape::even(BANK_REACH); 2]],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let mut last = features.apply(DVec2::new(50.0, 0.0), 20.0).height;
        for k in 1..2000 {
            let h = features
                .apply(DVec2::new(50.0, f64::from(k) * 0.05), 20.0)
                .height;
            assert!((h - last).abs() < 0.2, "step at {}", f64::from(k) * 0.05);
            last = h;
        }
        assert_eq!(last, 20.0);
    }

    #[test]
    fn roads_cut_and_fill_to_their_level() {
        let road = FeatureLine {
            kind: FeatureKind::Road,
            depth: 0.0,
            ..river()
        };
        let features = Features::new(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![road],
            vec![vec![0.6; 2]],
            vec![[false; 2]],
            Vec::new(),
        );
        let cut = features.apply(DVec2::new(50.0, 0.0), 30.0).height;
        let fill = features.apply(DVec2::new(50.0, 0.0), 0.0).height;
        assert!((cut - 9.75).abs() < 1e-4);
        assert!((fill - 9.25).abs() < 1e-4);
    }

    #[test]
    fn a_road_holds_no_ground_a_few_metres_past_an_open_end() {
        let road = FeatureLine {
            kind: FeatureKind::Road,
            depth: 0.0,
            ..river()
        };
        let carve = |open: [bool; 2], p: DVec2| {
            let features = Features::new(
                Vec::new(),
                Vec::new(),
                Vec::new(),
                vec![road.clone()],
                vec![vec![0.6; 2]],
                vec![open],
                Vec::new(),
            );
            features.apply(p, 0.0).height
        };
        // On the road near its end, filled to its level either way.
        let on = DVec2::new(99.0, 0.0);
        assert!((carve([false, true], on) - 8.76).abs() < 0.05);
        // Five metres past the end: an open end leaves the ground alone,
        // a closed one still holds it up.
        let past = DVec2::new(105.0, 0.0);
        assert_eq!(carve([false, true], past), 0.0);
        assert!(carve([false, false], past) > 1.0);
        // Opening the far end changes nothing at the start.
        let start = DVec2::new(-5.0, 0.0);
        assert_eq!(carve([false, true], start), carve([false, false], start));
        // Just past the end the ground falls away, without a step.
        let heights: Vec<f32> = (0..=40)
            .map(|i| carve([false, true], DVec2::new(100.0 + 0.1 * f64::from(i), 0.0)))
            .collect();
        assert!(heights.windows(2).all(|w| w[1] <= w[0] + 1e-4));
        assert!(heights.windows(2).all(|w| w[0] - w[1] < 1.5), "{heights:?}");
    }

    #[test]
    fn road_banks_are_tagged_by_kind_and_steepness() {
        let road = FeatureLine {
            kind: FeatureKind::Road,
            depth: 0.0,
            ..river()
        };
        // Three copies of the road far apart, with banks of 0.6 (31°), 1.5
        // (56°) and 3 (72°).
        let roads: Vec<FeatureLine> = [0.0, 1000.0, 2000.0]
            .iter()
            .map(|&z| FeatureLine {
                points: road
                    .points
                    .iter()
                    .map(|p| *p + DVec2::new(0.0, z))
                    .collect(),
                ..road.clone()
            })
            .collect();
        let banks = vec![vec![0.6; 2], vec![1.5; 2], vec![3.0; 2]];
        let features = Features::new(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            roads,
            banks,
            vec![[false; 2]; 3],
            Vec::new(),
        );
        // 5 m out from a road's edge (3 m from its centre), on high ground
        // (cut) or low ground (fill).
        let tags = |z: f64, ground: f32| features.apply(DVec2::new(50.0, z + 8.0), ground).tags;
        let (cut, fill) = (1000.0, -1000.0);
        assert_eq!(tags(0.0, cut), RoadTags::CUT);
        assert_eq!(tags(1000.0, cut), RoadTags::CUT | RoadTags::BLUFF);
        assert_eq!(tags(2000.0, cut), RoadTags::CUT | RoadTags::CLIFF);
        assert_eq!(tags(0.0, fill), RoadTags::EMBANKMENT);
        assert_eq!(tags(1000.0, fill), RoadTags::EMBANKMENT | RoadTags::BLUFF);
        assert_eq!(
            tags(2000.0, fill),
            RoadTags::RETAINING_WALL | RoadTags::CLIFF
        );
        // The road itself, and ground the bank doesn't reach.
        assert_eq!(
            features.apply(DVec2::new(50.0, 1.0), cut).tags,
            RoadTags::ROAD
        );
        assert_eq!(tags(2000.0, 9.5), RoadTags::NONE);
        assert_eq!(
            (RoadTags::RETAINING_WALL | RoadTags::CLIFF).names(),
            ["retaining_wall", "cliff"]
        );
    }

    #[test]
    fn deep_cuts_open_into_valleys() {
        // A river 30 m below flat ground. With the narrowest reach the
        // ground 60 m from the water is untouched (a gorge); with the reach
        // a 30 m cut asks for, it is lowered there, though not all the way
        // down to the plain 0.35 wall, and untouched beyond the reach.
        let level = 9.5;
        let carve = |reach: f64, out: f64, ground: f32| {
            let features = Features::new(
                vec![river()],
                vec![vec![ValleyShape::even(reach); 2]],
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            );
            features
                .apply(DVec2::new(50.0, 3.0 + out), level + ground)
                .height
                - level
        };
        let wide = valley_reach(30.0, 0.0, RIVER_BANK);
        assert!(wide > 86.0 && wide <= MAX_VALLEY_REACH, "reach {wide}");
        assert!((carve(BANK_REACH, 60.0, 30.0) - 30.0).abs() < 1e-3);
        let at_60 = carve(wide, 60.0, 30.0);
        assert!(
            at_60 > 0.35 * 60.0 && at_60 < 27.0,
            "valley at 60 m: {at_60}"
        );
        assert_eq!(carve(wide, wide + 1.0, 30.0), 30.0);
        // At the water's edge the ground is pulled right down to the water.
        assert!(carve(wide, 0.0, 30.0).abs() < 1e-3);
        // A shallow cut keeps the narrowest valley.
        assert_eq!(valley_reach(2.0, 0.0, RIVER_BANK), BANK_REACH);
    }

    #[test]
    fn valley_sides_take_their_own_shape() {
        // The test river runs along +x, so its left side is +y. The left
        // side has a 20 m floor and reaches 60 m; the right side has no
        // floor and reaches 40 m.
        let shape = ValleyShape {
            floor: [20.0, 0.0],
            wall: [0.5, RIVER_BANK as f32],
            reach: [60.0, 40.0],
            gullies: false,
        };
        let features = Features::new(
            vec![river()],
            vec![vec![shape; 2]],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let level = 9.5;
        let carve = |y: f64| features.apply(DVec2::new(50.0, y), 30.0).height;
        // On the left floor the ground is down at the water's level; 10 m
        // out on the right it is already climbing the wall.
        assert!((carve(13.0) - level).abs() < 1e-3);
        assert!(carve(-13.0) > level + 3.0);
        // 45 m out the left is still in its valley, the right is not.
        assert!(carve(48.0) < 30.0);
        assert_eq!(carve(-48.0), 30.0);
    }

    #[test]
    fn carving_slopes_match_differences() {
        // Uneven gullied valleys round a bend and a confluence, a pond, and
        // a road across a valley wall, cutting and filling, with one open
        // end, all on a tilted
        // plane. At a grid of points the carved slope matches differences
        // of the carved heights.
        let main = FeatureLine {
            kind: FeatureKind::River,
            points: vec![
                DVec2::ZERO,
                DVec2::new(100.0, 0.0),
                DVec2::new(100.0, 100.0),
            ],
            levels: vec![10.0, 9.0, 8.0],
            widths: vec![6.0; 3],
            depth: 1.5,
        };
        let tributary = FeatureLine {
            kind: FeatureKind::River,
            points: vec![DVec2::new(50.0, -120.0), DVec2::new(50.0, -3.0)],
            levels: vec![20.0, 9.5],
            widths: vec![4.0; 2],
            depth: 1.0,
        };
        let road = FeatureLine {
            kind: FeatureKind::Road,
            points: vec![DVec2::new(-60.0, 60.0), DVec2::new(60.0, 75.0)],
            levels: vec![24.0, 27.0],
            widths: vec![6.0, 5.0],
            depth: 0.0,
        };
        let pond = Pond {
            centre: DVec2::new(-30.0, -60.0),
            level: 18.0,
            radius: 12.0,
            depth: 2.0,
        };
        let shape = |floor: [f32; 2], wall: [f32; 2], reach: [f32; 2]| ValleyShape {
            floor,
            wall,
            reach,
            gullies: true,
        };
        let features = Features::new(
            vec![main, tributary],
            vec![
                vec![
                    shape([0.0, 10.0], [0.6, 0.3], [90.0, 140.0]),
                    shape([20.0, 0.0], [0.3, 0.8], [150.0, 70.0]),
                    shape([5.0, 5.0], [0.5, 0.5], [110.0, 110.0]),
                ],
                vec![
                    shape([0.0, 0.0], [0.7, 0.7], [80.0, 80.0]),
                    shape([10.0, 10.0], [0.4, 0.4], [100.0, 100.0]),
                ],
            ],
            vec![pond],
            vec![road],
            vec![vec![0.8, 1.5]],
            // Its far end stops short, as at a river bank.
            vec![[false, true]],
            Vec::new(),
        );
        let tilt = DVec2::new(0.12, -0.05);
        let ground = |p: DVec2| (30.0 + tilt.dot(p)) as f32;
        let height = |p: DVec2| f64::from(features.apply(p, ground(p)).height);
        let slope = |p: DVec2| features.carve(p, ground(p), Some(tilt)).1;

        // Along x and z through each point, the difference of the heights
        // 0.5 m either side must match the slope averaged over that span
        // (Simpson's rule). Spans across a crease, where the slope jumps
        // between neighbouring samples 1/64 m apart (where the deepest
        // lowering changes, or a gully's cap starts), are skipped.
        const HALF: f64 = 0.5;
        const STEPS: i32 = 64;
        let (mut skipped, mut total, mut carved) = (0, 0, 0);
        for i in 0..40 {
            for j in 0..40 {
                let p = DVec2::new(-70.0 + 5.83 * f64::from(i), -140.0 + 7.31 * f64::from(j));
                let out = features.carve(p, ground(p), Some(tilt)).0;
                assert_eq!(out.height, features.apply(p, ground(p)).height);
                if out.height != ground(p) {
                    carved += 1;
                }
                for axis in [DVec2::X, DVec2::Y] {
                    total += 1;
                    let slopes: Vec<f64> = (0..=STEPS)
                        .map(|k| {
                            let at =
                                p + axis * HALF * (2.0 * f64::from(k) / f64::from(STEPS) - 1.0);
                            slope(at).dot(axis)
                        })
                        .collect();
                    if slopes.windows(2).any(|w| (w[1] - w[0]).abs() > 0.02) {
                        skipped += 1;
                        continue;
                    }
                    let mean = slopes
                        .iter()
                        .enumerate()
                        .map(|(k, &s)| match k as i32 {
                            0 | STEPS => s,
                            k if k % 2 == 1 => 4.0 * s,
                            _ => 2.0 * s,
                        })
                        .sum::<f64>()
                        / (3.0 * f64::from(STEPS));
                    let difference =
                        (height(p + axis * HALF) - height(p - axis * HALF)) / (2.0 * HALF);
                    assert!(
                        (difference - mean).abs() < 1e-4 + 1e-3 * mean.abs(),
                        "at {p} along {axis}: mean slope {mean} against difference {difference}"
                    );
                }
            }
        }
        assert!(
            carved * 2 > total / 2,
            "only {carved} of {} points carved",
            total / 2
        );
        assert!(skipped * 10 <= total, "{skipped} of {total} spans skipped");
    }

    #[test]
    fn gullied_valleys_have_no_steps_at_bends_joints_and_confluences() {
        // A river with a sharp bend, whose valley changes shape from point
        // to point, and a tributary joining it partway along a segment, all
        // with gullies, cut into high ground. Stepping finely across the
        // bend, the joints and the confluence, the ground never jumps.
        let main = FeatureLine {
            kind: FeatureKind::River,
            points: vec![
                DVec2::ZERO,
                DVec2::new(100.0, 0.0),
                DVec2::new(100.0, 100.0),
            ],
            levels: vec![10.0, 9.0, 8.0],
            widths: vec![6.0; 3],
            depth: 1.5,
        };
        let tributary = FeatureLine {
            kind: FeatureKind::River,
            points: vec![DVec2::new(50.0, -120.0), DVec2::new(50.0, -3.0)],
            levels: vec![20.0, 9.5],
            widths: vec![4.0; 2],
            depth: 1.0,
        };
        let shape = |floor: [f32; 2], wall: [f32; 2], reach: [f32; 2]| ValleyShape {
            floor,
            wall,
            reach,
            gullies: true,
        };
        let features = Features::new(
            vec![main, tributary],
            vec![
                vec![
                    shape([0.0, 10.0], [0.6, 0.3], [90.0, 140.0]),
                    shape([20.0, 0.0], [0.3, 0.8], [150.0, 70.0]),
                    shape([5.0, 5.0], [0.5, 0.5], [110.0, 110.0]),
                ],
                vec![
                    shape([0.0, 0.0], [0.7, 0.7], [80.0, 80.0]),
                    shape([10.0, 10.0], [0.4, 0.4], [100.0, 100.0]),
                ],
            ],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );

        // Rows and columns through the bend (100, 0), the confluence
        // (50, −3) and open wall, in 1 cm steps.
        let step = 0.01;
        let mut worst = 0.0_f64;
        for k in 0..12 {
            let across = -60.0 + 15.0 * f64::from(k);
            for (start, dir) in [
                (DVec2::new(-60.0, across), DVec2::X),
                (DVec2::new(across + 50.0, -140.0), DVec2::Y),
            ] {
                let height = |i: i32| {
                    f64::from(
                        features
                            .apply(start + dir * (f64::from(i) * step), 60.0)
                            .height,
                    )
                };
                let mut last = height(0);
                for i in 1..=30_000 {
                    let h = height(i);
                    worst = worst.max((h - last).abs());
                    last = h;
                }
            }
        }
        // The steepest the carving makes is a wall at 0.8 with gullies
        // in it, well under 3 cm per centimetre.
        assert!(worst < 0.03, "step {worst} m in 1 cm");
    }

    #[test]
    fn gullies_cut_into_valley_walls_and_never_raise_them() {
        // The same valley with and without gullies: across its walls the
        // gullies lower the ground by metres in places, and nowhere raise
        // it.
        let carve_with = |gullies: bool| {
            let shape = ValleyShape {
                gullies,
                ..ValleyShape::even(100.0)
            };
            Features::new(
                vec![river()],
                vec![vec![shape; 2]],
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
        };
        let (with, without) = (carve_with(true), carve_with(false));
        let mut deepest = 0.0_f32;
        for i in 0..100 {
            for j in -60..=60 {
                let p = DVec2::new(f64::from(i), f64::from(j) * 2.0);
                let cut = without.apply(p, 60.0).height - with.apply(p, 60.0).height;
                assert!(cut >= 0.0, "raised by {} at {p}", -cut);
                deepest = deepest.max(cut);
            }
        }
        assert!(deepest > 2.0, "deepest gully {deepest} m");
    }

    #[test]
    fn valley_walls_keep_the_grounds_relief() {
        // On a valley wall, a 2 m bump in the ground stays a smaller bump,
        // growing towards the valley's edge, and nothing is ever raised.
        let reach = valley_reach(30.0, 0.0, RIVER_BANK);
        let features = Features::new(
            vec![river()],
            vec![vec![ValleyShape::even(reach); 2]],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let carve =
            |out: f64, ground: f32| features.apply(DVec2::new(50.0, 3.0 + out), ground).height;
        let mut last_bump = 0.0;
        for out in [10.0, 40.0, 80.0, 120.0] {
            let bump = carve(out, 41.5) - carve(out, 39.5);
            assert!(bump > last_bump && bump <= 2.0, "bump {bump} at {out} m");
            last_bump = bump;
        }
        for ground in [5.0, 9.0, 15.0, 40.0] {
            for out in [0.0, 2.0, 10.0, 60.0] {
                assert!(
                    carve(out, ground) <= ground,
                    "raised at {out} m on {ground}"
                );
            }
        }
    }

    #[test]
    fn steep_banks_are_flat_faces() {
        // A bank keeps its slope, within 5%, out to half its reach.
        for bank in [0.6, 2.0, 20.0] {
            let near = bank_slope(3.0 + 1.0, 3.0, bank).unwrap();
            let far = bank_slope(3.0 + 0.5 * BANK_REACH, 3.0, bank).unwrap();
            assert!(
                (far / near - 1.0).abs() < 0.05,
                "bank {bank}: {near} then {far}"
            );
        }
    }

    #[test]
    fn smoothing_keeps_ends_and_falling_levels() {
        let points = [
            DVec2::ZERO,
            DVec2::new(10.0, 0.0),
            DVec2::new(10.0, 10.0),
            DVec2::new(20.0, 10.0),
        ];
        let levels = [5.0, 4.0, 4.0, 1.0];
        let (p, l) = chaikin(&points, &levels, 2);
        assert_eq!(p[0], points[0]);
        assert_eq!(p[p.len() - 1], points[3]);
        assert_eq!(l[0], 5.0);
        assert_eq!(l[l.len() - 1], 1.0);
        assert!(l.windows(2).all(|w| w[1] <= w[0]));
    }
}
