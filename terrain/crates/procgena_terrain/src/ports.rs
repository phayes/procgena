//! Where rivers and roads cross a shared border.
//!
//! Each terrain says where its own rivers and roads would like to leave
//! through an edge ([`PortWish`]). Negotiation turns the wishes of both sides
//! into agreed crossings ([`Port`]) that both terrains build to.

use glam::DVec2;

use crate::region::{RegionEdge, RegionId, point_at, segments};
use crate::seam::EdgeSample;

/// How far a channel's banks reach beyond its edge, in metres.
pub(crate) const BANK_REACH: f64 = 40.0;

/// The widest half-width of a river, road or pond, in metres.
pub(crate) const MAX_HALF_WIDTH: f64 = 20.0;

/// How far from a border, in metres, a terrain's own features must stay
/// (except at a crossing), so that their carving never reaches the border.
pub const FEATURE_CLEARANCE: f64 = MAX_HALF_WIDTH + BANK_REACH + 5.0;

/// The number of steps in the piece carved across the border at a crossing.
/// Odd, so the border falls in the middle of a step rather than on a point,
/// where the piece's slope would change.
const PIECE_POINTS: usize = 13;

/// How far the agreed cross-slope is carried away from an edge, in metres.
/// The slope holds at the edge and its effect levels off beyond this.
pub(crate) const SLOPE_REACH: f64 = 32.0;

/// How much of the agreed cross-slope applies at signed distance `across`
/// from an edge: `across` near the edge, levelling off at ±[`SLOPE_REACH`].
/// Multiply by the cross-slope for the height change.
pub(crate) fn slope_reach(across: f64) -> f64 {
    SLOPE_REACH * (across / SLOPE_REACH).tanh()
}

/// How far from either end of an edge a crossing must be, in metres, so its
/// carving stays clear of the other edges meeting at that corner. Corners
/// sharper than about 40° can still be reached.
pub(crate) const CORNER_CLEARANCE: f64 = 3.0 * FEATURE_CLEARANCE;

/// The widest river or road a wish may ask for, in metres.
pub const MAX_PORT_WIDTH: f32 = 2.0 * MAX_HALF_WIDTH as f32;

/// How far a wish may move along the edge to reach a low point of the agreed
/// profile, in metres.
const RIVER_SNAP: f64 = 120.0;

/// How far below the agreed ground a river's water surface sits at a
/// crossing, in metres.
pub(crate) const FREEBOARD: f32 = 1.0;

/// How much a river must fall between leaving its terrain's interior and
/// the crossing, in metres.
const MIN_DROP: f32 = 0.5;

/// The steepest cross-slope a border may have where a river crosses it, as
/// rise over run. Steeper borders are ramps a neighbour was bent into, not
/// valleys a river would follow.
const RIVER_MAX_CROSS_SLOPE: f32 = 0.25;

/// The closest two crossings on one edge may be, in metres.
const PORT_SPACING: f64 = 3.0 * FEATURE_CLEARANCE;

/// Road wishes from the two sides closer than this (metres) become one road.
const ROAD_MERGE: f64 = 300.0;

/// How far, in metres, a side's own ground at the inner end of a crossing
/// piece may depart from its own border profile carried inward.
const PIECE_GAP: f32 = 4.0;

/// The longest stretch of road, in metres, either side may need to climb
/// or descend from a crossing to its own ground.
const ROAD_RAMP: f64 = 500.0;

/// Which side of an edge a region lies on, looking along the edge's
/// canonical direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    /// On the left: the side the edge's canonical normal points to.
    Left,
    /// On the right.
    Right,
}

impl Side {
    /// +1 on the left, −1 on the right: the sign of distances measured
    /// along the edge's canonical normal into this side.
    pub fn sign(self) -> f64 {
        match self {
            Self::Left => 1.0,
            Self::Right => -1.0,
        }
    }
}

/// A river or road that one terrain would like to send through an edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PortWish {
    /// Where along the edge, in metres from its canonical start.
    pub along: f64,
    /// For a river, its water level in metres where it would start towards
    /// the edge. Not used for roads.
    pub level: f32,
    /// Width of the river or road, in metres. Positive, at most
    /// [`MAX_PORT_WIDTH`].
    pub width: f32,
    /// For a river, the lowest ground in metres between where it would
    /// start towards the edge and the end of its crossing piece; the
    /// crossing must sit below it, or the river would float above that
    /// ground. Infinite for roads.
    pub ground: f32,
}

/// What crosses at a [`Port`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortKind {
    /// A river, flowing out of region `from` into the other region.
    River {
        /// The region the river flows out of.
        from: RegionId,
    },
    /// A road, usable both ways.
    Road,
}

/// An agreed crossing of a shared edge.
///
/// Both terrains carve the same short straight piece of river or road
/// across the border here, so the crossing is seamless.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Port {
    /// What crosses.
    pub kind: PortKind,
    /// Where along the edge, in metres from its canonical start.
    pub along: f64,
    /// The river's water level, or the road's surface level, at the
    /// crossing, in metres.
    pub level: f32,
    /// Width of the river or road, in metres.
    pub width: f32,
    /// For a river, the `along` of the wish it came from, so the sending
    /// terrain can find its river. Equal to `along` for roads.
    pub wish: f64,
    /// The agreed rate of height change across the edge at the crossing,
    /// along the edge's canonical normal. The piece carved across the border
    /// follows it.
    pub slope: f32,
}

impl Port {
    /// The two ends of the short straight piece of river or road carved
    /// across the border at this crossing: first the end on the edge's left
    /// side, then the right. Each is [`FEATURE_CLEARANCE`] from the edge.
    /// `edge` must be the edge this port was agreed for.
    pub fn ends(&self, edge: &RegionEdge) -> (DVec2, DVec2) {
        let (q, normal) = point_at(&segments(&edge.polyline), self.along);
        (
            q + normal * FEATURE_CLEARANCE,
            q - normal * FEATURE_CLEARANCE,
        )
    }

    /// The crossing point on the edge itself.
    pub fn point(&self, edge: &RegionEdge) -> DVec2 {
        point_at(&segments(&edge.polyline), self.along).0
    }

    /// The water or road level at signed distance `across` from the edge
    /// along its canonical normal (positive on the left), on the piece
    /// carved across the border.
    pub fn level_at(&self, across: f64) -> f32 {
        (f64::from(self.level) + f64::from(self.slope) * slope_reach(across)) as f32
    }

    /// The piece carved across the border: points from the left end to the
    /// right end (see [`ends`](Self::ends)), with their levels.
    pub fn piece(&self, edge: &RegionEdge) -> (Vec<DVec2>, Vec<f32>) {
        let (q, normal) = point_at(&segments(&edge.polyline), self.along);
        (0..=PIECE_POINTS)
            .map(|k| {
                let across = FEATURE_CLEARANCE * (1.0 - 2.0 * k as f64 / PIECE_POINTS as f64);
                (q + normal * across, self.level_at(across))
            })
            .unzip()
    }
}

/// The data about one side that crossing negotiation needs.
pub(crate) struct SideWishes<'a> {
    pub region: RegionId,
    pub side: Side,
    pub rivers: &'a [PortWish],
    pub roads: &'a [PortWish],
    /// The side's own smoothed profile along the edge.
    pub natural: &'a [EdgeSample],
    /// The steepest grade the side's roads may climb.
    pub road_grade: f32,
    /// How far inside its region the side bends to meet the agreement, in
    /// metres.
    pub margin: f32,
    /// The side's own height [`FEATURE_CLEARANCE`] metres inside, at each
    /// profile sample.
    pub inner: &'a [f32],
}

impl SideWishes<'_> {
    /// Whether the side's own ground [`FEATURE_CLEARANCE`] metres inside
    /// stays within [`PIECE_GAP`] of its own smoothed border profile carried
    /// inward, at profile sample `i`. Where it does, a crossing piece
    /// following the agreed profile stays close to the ground at that end.
    fn piece_fits(&self, i: usize) -> bool {
        let own = self.natural[i];
        let carried = own.height
            + (f64::from(own.slope) * slope_reach(self.side.sign() * FEATURE_CLEARANCE)) as f32;
        (self.inner[i] - carried).abs() <= PIECE_GAP
    }
}

/// Returns true if every wish is usable on an edge `length` metres long.
pub(crate) fn wishes_valid(wishes: &[PortWish], length: f64) -> bool {
    wishes.iter().all(|w| {
        (0.0..=length).contains(&w.along)
            && w.level.is_finite()
            && !w.ground.is_nan()
            && w.width > 0.0
            && w.width <= MAX_PORT_WIDTH
    })
}

/// Agrees on the crossings of one edge, from both sides' wishes and the
/// agreed profile (samples `step` metres apart). `a` must be the side with
/// the lower region id. A crossing may be placed only where `usable(along)`
/// is true.
pub(crate) fn agree_ports(
    a: &SideWishes,
    b: &SideWishes,
    profile: &[EdgeSample],
    step: f64,
    usable: &dyn Fn(f64) -> bool,
) -> Vec<Port> {
    let mut candidates = Vec::new();

    // Rivers: snap to a low point, then check they run downhill all the way.
    for (from, to) in [(a, b), (b, a)] {
        let mut wishes = from.rivers.to_vec();
        wishes.sort_by(|x, y| x.along.total_cmp(&y.along));
        for wish in wishes {
            let along = snap_to_low_point(profile, step, wish.along);
            let sample = ((along / step).round() as usize).min(profile.len() - 1);
            if !usable(along) || !from.piece_fits(sample) || !to.piece_fits(sample) {
                continue;
            }
            let ground = interpolate(profile, step, along);
            let level = ground.height - FREEBOARD;
            // The receiving side's ground must fall away gently: at the
            // border, and over its margin, where it is bent from the agreed
            // height back to its own.
            let own = interpolate(to.natural, step, along).height;
            let ramp = (ground.height - own).abs() / to.margin;
            let falls_away = f64::from(ground.slope) * to.side.sign() < 0.0
                && ground.slope.abs() <= RIVER_MAX_CROSS_SLOPE
                && ramp <= RIVER_MAX_CROSS_SLOPE;
            // The crossing piece rises towards the sending side; the river
            // must arrive above its upper end.
            let upper =
                level + (f64::from(ground.slope.abs()) * slope_reach(FEATURE_CLEARANCE)) as f32;
            // The river must also stay below the ground on its way to the
            // crossing piece.
            // Near the border the sender's ground is bent towards the agreed
            // height; if that lowers it, the river's way down is lower too.
            let bent_down =
                (ground.height - interpolate(from.natural, step, along).height).min(0.0);
            let under_ground = upper <= wish.ground + bent_down - FREEBOARD;
            if wish.level >= upper + MIN_DROP && falls_away && under_ground {
                candidates.push(Port {
                    kind: PortKind::River { from: from.region },
                    along,
                    level,
                    width: wish.width,
                    wish: wish.along,
                    slope: ground.slope,
                });
            }
        }
    }

    // Roads: wishes from both sides that are close together become one.
    let mut roads: Vec<(f64, f32)> = Vec::new();
    let mut taken = vec![false; b.roads.len()];
    for wa in a.roads {
        let partner = b
            .roads
            .iter()
            .enumerate()
            .filter(|(i, wb)| !taken[*i] && (wb.along - wa.along).abs() < ROAD_MERGE)
            .min_by(|x, y| {
                (x.1.along - wa.along)
                    .abs()
                    .total_cmp(&(y.1.along - wa.along).abs())
            });
        match partner {
            Some((i, wb)) => {
                taken[i] = true;
                roads.push(((wa.along + wb.along) / 2.0, wa.width.max(wb.width)));
            }
            None => roads.push((wa.along, wa.width)),
        }
    }
    for (i, wb) in b.roads.iter().enumerate() {
        if !taken[i] {
            roads.push((wb.along, wb.width));
        }
    }
    for (wished, width) in roads {
        if let Some(along) = road_crossing(a, b, profile, step, wished, &usable) {
            let ground = interpolate(profile, step, along);
            candidates.push(Port {
                kind: PortKind::Road,
                along,
                level: ground.height,
                width,
                wish: along,
                slope: ground.slope,
            });
        }
    }

    // Keep crossings apart: rivers first, then roads, each in order along
    // the edge.
    candidates.sort_by(|x, y| {
        let rank = |p: &Port| match p.kind {
            PortKind::River { from } => (0, from.0),
            PortKind::Road => (1, 0),
        };
        rank(x).cmp(&rank(y)).then(x.along.total_cmp(&y.along))
    });
    let mut ports: Vec<Port> = Vec::new();
    for candidate in candidates {
        if ports
            .iter()
            .all(|p| (p.along - candidate.along).abs() >= PORT_SPACING)
        {
            ports.push(candidate);
        }
    }
    ports.sort_by(|x, y| x.along.total_cmp(&y.along));
    ports
}

/// Where near `wished` (within [`ROAD_MERGE`]) a road can cross, if
/// anywhere.
///
/// At the crossing the agreed ground may differ from each side's own ground;
/// each side's road must climb or descend that difference within
/// [`ROAD_RAMP`] metres at its steepest grade. The point needing the least
/// of that is chosen; if even it needs more, there is no crossing.
fn road_crossing(
    a: &SideWishes,
    b: &SideWishes,
    profile: &[EdgeSample],
    step: f64,
    wished: f64,
    usable: &impl Fn(f64) -> bool,
) -> Option<f64> {
    let last = profile.len() - 1;
    let lo = ((wished - ROAD_MERGE) / step).ceil().max(0.0) as usize;
    let hi = (((wished + ROAD_MERGE) / step).floor() as usize).min(last);
    let ramp = |i: usize| {
        [a, b]
            .iter()
            .map(|s| f64::from((profile[i].height - s.natural[i].height).abs() / s.road_grade))
            .fold(0.0, f64::max)
    };
    let off = |i: usize| (i as f64 * step - wished).abs();
    // The road climbs the border's cross-slope on its crossing piece.
    let gentle = |i: usize| profile[i].slope.abs() <= a.road_grade.min(b.road_grade);
    let fits = |i: usize| a.piece_fits(i) && b.piece_fits(i);
    (lo..=hi)
        .filter(|&i| usable(i as f64 * step) && gentle(i) && fits(i))
        .min_by(|&i, &j| {
            ramp(i)
                .total_cmp(&ramp(j))
                .then(off(i).total_cmp(&off(j)))
                .then(i.cmp(&j))
        })
        .filter(|&i| ramp(i) <= ROAD_RAMP)
        .map(|i| i as f64 * step)
}

/// The lowest point of the profile within [`RIVER_SNAP`] of `along`, if it is
/// a local low point; otherwise `along` itself.
fn snap_to_low_point(profile: &[EdgeSample], step: f64, along: f64) -> f64 {
    let last = profile.len() - 1;
    let lo = ((along - RIVER_SNAP) / step).ceil().max(1.0) as usize;
    let hi = (((along + RIVER_SNAP) / step).floor() as usize).min(last.saturating_sub(1));
    (lo..=hi)
        .filter(|&i| {
            profile[i].height <= profile[i - 1].height && profile[i].height <= profile[i + 1].height
        })
        .min_by(|&i, &j| {
            profile[i]
                .height
                .total_cmp(&profile[j].height)
                .then(i.cmp(&j))
        })
        .map_or(along, |i| i as f64 * step)
}

/// The profile at `along`, by linear interpolation between samples.
pub(crate) fn interpolate(profile: &[EdgeSample], step: f64, along: f64) -> EdgeSample {
    let last = profile.len() - 1;
    let t = (along / step).clamp(0.0, last as f64);
    let i = (t.floor() as usize).min(last.saturating_sub(1));
    let f = (t - i as f64) as f32;
    let (s, e) = (profile[i], profile[(i + 1).min(last)]);
    EdgeSample {
        height: s.height + (e.height - s.height) * f,
        slope: s.slope + (e.slope - s.slope) * f,
        detail: s.detail + (e.detail - s.detail) * f,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Crossings anywhere more than 100 m from the ends of the 1000 m test
    /// edge.
    fn away_from_ends(along: f64) -> bool {
        (100.0..=900.0).contains(&along)
    }

    #[test]
    fn crossings_only_where_usable() {
        let (profile, step) = valley();
        let fit = fit_both(&profile);
        let wish = [PortWish {
            along: 350.0,
            level: 40.0,
            width: 6.0,
            ground: f32::INFINITY,
        }];
        let (a, b) = sides(&wish, &[], &[], &[], &profile, &fit);
        // The low point at 400 m is ruled out.
        let not_there = |along: f64| away_from_ends(along) && (along - 400.0).abs() > 50.0;
        assert!(agree_ports(&a, &b, &profile, step, &not_there).is_empty());
    }

    /// A V-shaped profile, lowest at 400 m, falling towards the left side.
    fn valley() -> (Vec<EdgeSample>, f64) {
        let step = 8.0;
        let profile = (0..=125)
            .map(|i| {
                let along = f64::from(i) * step;
                EdgeSample {
                    height: (20.0 + 0.05 * (along - 400.0).abs()) as f32,
                    slope: 0.1,
                    detail: 0.0,
                }
            })
            .collect();
        (profile, step)
    }

    /// Both sides, each with its own ground equal to `natural`.
    /// Inland heights that fit `natural` exactly on `side`: its own profile
    /// carried inward.
    fn fitting(natural: &[EdgeSample], side: Side) -> Vec<f32> {
        let reach = slope_reach(side.sign() * FEATURE_CLEARANCE);
        natural
            .iter()
            .map(|s| s.height + (f64::from(s.slope) * reach) as f32)
            .collect()
    }

    /// Both sides, each with its own ground equal to `natural` and inland
    /// heights `inner` (left side first).
    fn sides<'a>(
        a_rivers: &'a [PortWish],
        b_rivers: &'a [PortWish],
        a_roads: &'a [PortWish],
        b_roads: &'a [PortWish],
        natural: &'a [EdgeSample],
        inner: &'a [Vec<f32>; 2],
    ) -> (SideWishes<'a>, SideWishes<'a>) {
        (
            SideWishes {
                region: RegionId(1),
                side: Side::Left,
                rivers: a_rivers,
                roads: a_roads,
                natural,
                road_grade: 0.1,
                margin: 200.0,
                inner: &inner[0],
            },
            SideWishes {
                region: RegionId(2),
                side: Side::Right,
                rivers: b_rivers,
                roads: b_roads,
                natural,
                road_grade: 0.1,
                margin: 200.0,
                inner: &inner[1],
            },
        )
    }

    /// Inland heights fitting `natural` on both sides.
    fn fit_both(natural: &[EdgeSample]) -> [Vec<f32>; 2] {
        [fitting(natural, Side::Left), fitting(natural, Side::Right)]
    }

    #[test]
    fn a_river_snaps_to_the_low_point_and_flows_downhill() {
        let (profile, step) = valley();
        let fit = fit_both(&profile);
        // Region 1 is on the left, where the ground rises (slope +0.1 along
        // the normal), so its river can flow out to the right.
        let wish = [PortWish {
            along: 350.0,
            level: 40.0,
            width: 6.0,
            ground: f32::INFINITY,
        }];
        let (a, b) = sides(&wish, &[], &[], &[], &profile, &fit);
        let ports = agree_ports(&a, &b, &profile, step, &away_from_ends);
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].along, 400.0);
        assert_eq!(ports[0].level, 20.0 - FREEBOARD);
        assert_eq!(ports[0].kind, PortKind::River { from: RegionId(1) });
        assert_eq!(ports[0].wish, 350.0);
    }

    #[test]
    fn uphill_rivers_and_rivers_into_rising_ground_are_dropped() {
        let (profile, step) = valley();
        let fit = fit_both(&profile);
        let low = [PortWish {
            along: 400.0,
            level: 19.2,
            width: 6.0,
            ground: f32::INFINITY,
        }];
        let (a, b) = sides(&low, &[], &[], &[], &profile, &fit);
        assert!(agree_ports(&a, &b, &profile, step, &away_from_ends).is_empty());
        // Region 2's river would flow into region 1, where the ground rises.
        let high = [PortWish {
            along: 400.0,
            level: 40.0,
            width: 6.0,
            ground: f32::INFINITY,
        }];
        let (a, b) = sides(&[], &high, &[], &[], &profile, &fit);
        assert!(agree_ports(&a, &b, &profile, step, &away_from_ends).is_empty());
    }

    #[test]
    fn rivers_dont_cross_over_a_dip_they_would_float_above() {
        let (profile, step) = valley();
        let fit = fit_both(&profile);
        // High enough, but the ground on the way to the crossing dips to
        // 15 m, below the 19 m crossing level.
        let wish = [PortWish {
            along: 350.0,
            level: 40.0,
            width: 6.0,
            ground: 15.0,
        }];
        let (a, b) = sides(&wish, &[], &[], &[], &profile, &fit);
        assert!(agree_ports(&a, &b, &profile, step, &away_from_ends).is_empty());
    }

    #[test]
    fn no_crossing_where_a_side_departs_from_its_border_profile() {
        let (profile, step) = valley();
        let mut fit = fit_both(&profile);
        // The right side's ground 65 m in is 10 m below its profile carried
        // inward: a crossing piece would float there.
        for h in &mut fit[1] {
            *h -= 10.0;
        }
        let road = [PortWish {
            along: 500.0,
            level: 0.0,
            width: 6.0,
            ground: f32::INFINITY,
        }];
        let (a, b) = sides(&[], &[], &road, &[], &profile, &fit);
        assert!(agree_ports(&a, &b, &profile, step, &away_from_ends).is_empty());
    }

    #[test]
    fn no_wishes_means_no_crossings() {
        let (profile, step) = valley();
        let fit = fit_both(&profile);
        let (a, b) = sides(&[], &[], &[], &[], &profile, &fit);
        assert!(agree_ports(&a, &b, &profile, step, &away_from_ends).is_empty());
    }

    #[test]
    fn roads_dont_cross_where_the_border_is_far_above_a_side() {
        let (profile, step) = valley();
        let fit = fit_both(&profile);
        // One side's own ground is 150 m below the agreed border: at grade
        // 0.1 its road would need 1.5 km to get down.
        let low: Vec<EdgeSample> = profile
            .iter()
            .map(|s| EdgeSample {
                height: s.height - 150.0,
                ..*s
            })
            .collect();
        let road = [PortWish {
            along: 500.0,
            level: 0.0,
            width: 6.0,
            ground: f32::INFINITY,
        }];
        let (a, mut b) = sides(&[], &[], &road, &[], &profile, &fit);
        let low_inner = fitting(&low, Side::Right);
        b.natural = &low;
        b.inner = &low_inner;
        assert!(agree_ports(&a, &b, &profile, step, &away_from_ends).is_empty());
        // 50 m below is reachable.
        let close: Vec<EdgeSample> = profile
            .iter()
            .map(|s| EdgeSample {
                height: s.height - 50.0,
                ..*s
            })
            .collect();
        let close_inner = fitting(&close, Side::Right);
        b.natural = &close;
        b.inner = &close_inner;
        assert_eq!(
            agree_ports(&a, &b, &profile, step, &away_from_ends).len(),
            1
        );
    }

    #[test]
    fn nearby_road_wishes_merge_and_crossings_keep_apart() {
        let (profile, step) = valley();
        let fit = fit_both(&profile);
        let a_roads = [PortWish {
            along: 500.0,
            level: 0.0,
            width: 6.0,
            ground: f32::INFINITY,
        }];
        let b_roads = [PortWish {
            along: 600.0,
            level: 0.0,
            width: 8.0,
            ground: f32::INFINITY,
        }];
        let (a, b) = sides(&[], &[], &a_roads, &b_roads, &profile, &fit);
        let ports = agree_ports(&a, &b, &profile, step, &away_from_ends);
        assert_eq!(ports.len(), 1);
        // Midway between the wishes, on the nearest profile sample.
        assert_eq!(ports[0].along, 552.0);
        assert_eq!(ports[0].width, 8.0);
    }
}
