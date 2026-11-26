//! Turning a region's planned roads into a [`RoadNetwork`]: river gaps
//! found along each road, junctions where roads join, and the segments,
//! carved lines and points of interest between them.

use std::collections::{HashMap, HashSet};

use glam::DVec2;

use crate::carve::{FeatureKind, FeatureLine, PoiKind, PointOfInterest, Pond};
use crate::hash::mix64;

use super::network::{
    Alignment, RiverCrossing, RoadId, RoadNetwork, RoadNode, RoadNodeId, RoadNodeKind, RoadSegment,
    RoadStation,
};

/// How far beyond a river's water a road stops for a crossing, in metres.
const CROSSING_MARGIN: f64 = 6.0;

/// How far either side of where it crosses a river a road looks for dry
/// ground to stop on, in metres. A road that runs along the water further
/// than this keeps its crossing filled.
const CROSSING_SEARCH: f64 = 300.0;

/// Bucket size of the river index, in metres.
const RIVER_BUCKET: f64 = 32.0;

/// Positions closer than this along a road, in metres, count as one.
const SAME_PLACE: f64 = 1e-6;

/// Tags mixed into the ids, one per kind of id.
const JUNCTION_TAG: u64 = 0x6a75_6e63;
const BANK_TAG: u64 = 0x6261_6e6b;
const SEGMENT_TAG: u64 = 0x7365_676d;

/// A stretch of a road over a river, left open: the stations at its two
/// ends (where the road stops on each bank) and the river crossed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Gap {
    /// Station index of the near bank.
    pub from: usize,
    /// Station index of the far bank.
    pub to: usize,
    /// The river, as an index into the rivers.
    pub river: usize,
    /// The river's water level where the road crosses it.
    pub water_level: f32,
}

impl Gap {
    /// Whether the step from station `i` to station `i + 1` lies over the
    /// gap.
    pub fn spans_step(&self, i: usize) -> bool {
        self.from <= i && i < self.to
    }

    /// Whether station `i` lies strictly inside the gap.
    pub fn holds(&self, i: usize) -> bool {
        self.from < i && i < self.to
    }
}

/// One road as planned: from its start to the hub or to the road it
/// joins, with its stations.
pub(super) struct Planned {
    /// The node at its start.
    pub start: RoadNodeId,
    /// The centre line.
    pub alignment: Alignment,
    /// Distance along the centre line of each station.
    pub along: Vec<f64>,
    pub points: Vec<DVec2>,
    pub levels: Vec<f32>,
    pub widths: Vec<f32>,
    /// How steeply its banks start at each station.
    pub banks: Vec<f32>,
    /// The river gaps, in order along the road.
    pub gaps: Vec<Gap>,
    /// The road it joins and how far along that road, or `None` for the
    /// road that ends at the hub.
    pub joins: Option<(usize, f64)>,
    /// Bends tighter than asked: middle and radius.
    pub sharp: Vec<(DVec2, f64)>,
}

impl Planned {
    /// The road as one line over its full length, gaps included.
    pub fn line(&self) -> FeatureLine {
        FeatureLine {
            kind: FeatureKind::Road,
            points: self.points.clone(),
            levels: self.levels.clone(),
            widths: self.widths.clone(),
            depth: 0.0,
        }
    }

    /// The point, level, width and bank `a` metres along the road,
    /// interpolated between its stations.
    fn at(&self, a: f64) -> (DVec2, f32, f32, f32) {
        let i = self
            .along
            .partition_point(|&s| s <= a)
            .clamp(1, self.along.len() - 1);
        let span = self.along[i] - self.along[i - 1];
        let t = if span > 0.0 {
            ((a - self.along[i - 1]) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let lerp = |v: &[f32]| v[i - 1] + (v[i] - v[i - 1]) * t as f32;
        (
            self.points[i - 1].lerp(self.points[i], t),
            lerp(&self.levels),
            lerp(&self.widths),
            lerp(&self.banks),
        )
    }
}

/// The rivers' segments, bucketed by square so a point is tested only
/// against the segments nearby.
pub(super) struct RiverIndex<'a> {
    rivers: &'a [FeatureLine],
    buckets: HashMap<(i64, i64), Vec<(u32, u32)>>,
    /// Each pond, with the river that ends in it, if any.
    ponds: Vec<(&'a Pond, Option<usize>)>,
}

impl<'a> RiverIndex<'a> {
    /// Indexes `rivers`, each segment in every bucket within its half width
    /// plus [`CROSSING_MARGIN`], and `ponds`, each with the river that ends
    /// in it (the first whose last point lies in the pond).
    pub fn new(rivers: &'a [FeatureLine], ponds: &'a [Pond]) -> Self {
        let ponds = ponds
            .iter()
            .map(|pond| {
                let river = rivers.iter().position(|r| {
                    r.points
                        .last()
                        .is_some_and(|&end| end.distance(pond.centre) <= pond.radius)
                });
                (pond, river)
            })
            .collect();
        let mut buckets: HashMap<(i64, i64), Vec<(u32, u32)>> = HashMap::new();
        for (r, river) in rivers.iter().enumerate() {
            for s in 0..river.points.len().saturating_sub(1) {
                let reach = DVec2::splat(half_width(river, s) + CROSSING_MARGIN);
                let (a, b) = (river.points[s], river.points[s + 1]);
                let (lo, hi) = (bucket(a.min(b) - reach), bucket(a.max(b) + reach));
                for x in lo.0..=hi.0 {
                    for z in lo.1..=hi.1 {
                        buckets
                            .entry((x, z))
                            .or_default()
                            .push((r as u32, s as u32));
                    }
                }
            }
        }
        Self {
            rivers,
            buckets,
            ponds,
        }
    }

    /// Whether `p` is within [`CROSSING_MARGIN`] of the water of river
    /// `river` or of any pond.
    fn wet(&self, river: usize, p: DVec2) -> bool {
        let line = &self.rivers[river];
        let near_river = self.buckets.get(&bucket(p)).is_some_and(|parts| {
            parts.iter().any(|&(r, s)| {
                let s = s as usize;
                r as usize == river && {
                    let (d, _) = closest(line.points[s], line.points[s + 1], p);
                    d - half_width(line, s) < CROSSING_MARGIN
                }
            })
        });
        near_river
            || self
                .ponds
                .iter()
                .any(|(pond, _)| p.distance(pond.centre) - pond.radius < CROSSING_MARGIN)
    }

    /// Where the step from `a` to `b` first meets water: the first river
    /// centre line it crosses, or failing that a pond it enters (as the
    /// river that ends there), with the water level there.
    fn crossing(&self, a: DVec2, b: DVec2) -> Option<(usize, f32)> {
        self.river_crossing(a, b).or_else(|| {
            self.ponds.iter().find_map(|&(pond, river)| {
                let (d, _) = closest(a, b, pond.centre);
                (d < pond.radius).then_some(river?).map(|r| (r, pond.level))
            })
        })
    }

    /// Where the step from `a` to `b` crosses a river's centre line: the
    /// first crossing along the step, as the river and its water level
    /// there.
    fn river_crossing(&self, a: DVec2, b: DVec2) -> Option<(usize, f32)> {
        let (lo, hi) = (bucket(a.min(b)), bucket(a.max(b)));
        let mut best: Option<(f64, usize, usize, f32)> = None;
        for x in lo.0..=hi.0 {
            for z in lo.1..=hi.1 {
                for &(r, s) in self.buckets.get(&(x, z)).into_iter().flatten() {
                    let (r, s) = (r as usize, s as usize);
                    let line = &self.rivers[r];
                    let (c, d) = (line.points[s], line.points[s + 1]);
                    let Some((t, u)) = intersection(a, b, c, d) else {
                        continue;
                    };
                    let (la, lb) = (line.levels[s], line.levels[s + 1]);
                    let level = la + (lb - la) * u as f32;
                    let candidate = (t, r, s, level);
                    if best.is_none_or(|b| (t, r, s) < (b.0, b.1, b.2)) {
                        best = Some(candidate);
                    }
                }
            }
        }
        best.map(|(_, r, _, level)| (r, level))
    }
}

/// The bucket holding `p`.
fn bucket(p: DVec2) -> (i64, i64) {
    let q = (p / RIVER_BUCKET).floor();
    (q.x as i64, q.y as i64)
}

/// Half the wider of a river's widths at the ends of segment `s`.
fn half_width(line: &FeatureLine, s: usize) -> f64 {
    0.5 * f64::from(line.widths[s].max(line.widths[s + 1]))
}

/// The distance from `p` to the segment from `a` to `b`, and how far along
/// it (0 to 1) the closest point is.
fn closest(a: DVec2, b: DVec2, p: DVec2) -> (f64, f64) {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 {
        ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p.distance(a + ab * t), t)
}

/// Where the segments from `a` to `b` and from `c` to `d` cross: how far
/// along each (0 to 1). `None` if they don't cross or are parallel.
fn intersection(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> Option<(f64, f64)> {
    let (r, s) = (b - a, d - c);
    let denominator = r.perp_dot(s);
    if denominator == 0.0 {
        return None;
    }
    let t = (c - a).perp_dot(s) / denominator;
    let u = (c - a).perp_dot(r) / denominator;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((t, u))
}

/// The river gaps along a road through `points` (stations, `along` metres
/// along it): wherever it crosses a river or enters the pond at a river's
/// end, from the last station before that is at least [`CROSSING_MARGIN`]
/// from the river and every pond to the
/// first such station after it. Gaps that would overlap are merged; a gap
/// that would reach the road's first or last station is left out, and the
/// road there keeps the ground filled.
pub(super) fn gaps(rivers: &RiverIndex, points: &[DVec2], along: &[f64]) -> Vec<Gap> {
    let last = points.len() - 1;
    let mut out: Vec<Gap> = Vec::new();
    let mut i = 0;
    while i < last {
        let Some((river, water_level)) = rivers.crossing(points[i], points[i + 1]) else {
            i += 1;
            continue;
        };

        // Back and on to dry ground, within reach.
        let mut from = i;
        while from > 0
            && along[i] - along[from] < CROSSING_SEARCH
            && rivers.wet(river, points[from])
        {
            from -= 1;
        }
        let mut to = i + 1;
        while to < last
            && along[to] - along[i + 1] < CROSSING_SEARCH
            && rivers.wet(river, points[to])
        {
            to += 1;
        }
        let dry = |k: usize| !rivers.wet(river, points[k]);
        if from == 0 || to == last || !dry(from) || !dry(to) {
            i = to.max(i + 1);
            continue;
        }

        // Merged with the gap before, if they meet.
        match out.last_mut() {
            Some(gap) if gap.to >= from => gap.to = gap.to.max(to),
            _ => out.push(Gap {
                from,
                to,
                river,
                water_level,
            }),
        }
        i = to;
    }
    out
}

/// The nearest point to `p` on the planned roads, outside their gaps: the
/// road, how far along it, the point and the road's level there. Ties go
/// to the earliest road, then the earliest point along it.
pub(super) fn nearest_on(roads: &[Planned], p: DVec2) -> Option<(usize, f64, DVec2, f32)> {
    let mut best: Option<(f64, usize, f64, DVec2, f32)> = None;
    for (r, road) in roads.iter().enumerate() {
        for i in 0..road.points.len().saturating_sub(1) {
            if road.gaps.iter().any(|g| g.spans_step(i)) {
                continue;
            }
            let (d, t) = closest(road.points[i], road.points[i + 1], p);
            if best.is_none_or(|b| d < b.0) {
                let a = road.along[i] + (road.along[i + 1] - road.along[i]) * t;
                let (point, level, _, _) = road.at(a);
                best = Some((d, r, a, point, level));
            }
        }
    }
    best.map(|(_, r, a, point, level)| (r, a, point, level))
}

/// A road's piece to carve: one segment of the network as a line, with
/// its banks, and whether each end stops at a river bank (and so must not
/// carve beyond it).
pub(crate) struct CarvedRoad {
    pub line: FeatureLine,
    pub banks: Vec<f32>,
    pub open: [bool; 2],
}

/// The network built from `roads` (in the order they were planned) and
/// the hub node `hub`: nodes, segments split at every junction and river
/// bank, the river crossings, the segments as lines to carve, and the
/// points of interest along the roads: the hub, each sharp bend and each
/// river bank.
pub(super) fn assemble(
    roads: &[Planned],
    hub: RoadNodeId,
) -> (RoadNetwork, Vec<CarvedRoad>, Vec<PointOfInterest>) {
    let mut nodes: HashMap<RoadNodeId, RoadNode> = HashMap::new();
    let mut crossings = Vec::new();
    let mut points_of_interest = Vec::new();

    // Each road's breaks: where it starts and ends, each river bank and
    // each junction, with the node there.
    let mut breaks: Vec<Vec<(f64, RoadNodeId)>> = Vec::with_capacity(roads.len());
    for road in roads {
        let length = *road.along.last().expect("a road has stations");
        let node = |id, kind, a: f64| {
            let (position, level, width, _) = road.at(a);
            RoadNode {
                id,
                kind,
                position,
                level,
                heading: road.alignment.heading_at(a).unwrap_or(DVec2::X),
                width,
            }
        };
        nodes.insert(road.start, node(road.start, RoadNodeKind::Border, 0.0));
        let mut own = vec![(0.0, road.start)];

        // River banks, linked in pairs.
        for (k, gap) in road.gaps.iter().enumerate() {
            let ids = [0, 1]
                .map(|side| RoadNodeId(mix64(mix64(road.start.0, BANK_TAG), 2 * k as u64 + side)));
            for (&id, station) in ids.iter().zip([gap.from, gap.to]) {
                let a = road.along[station];
                let bank = node(id, RoadNodeKind::RiverBank, a);
                nodes.insert(id, bank);
                own.push((a, id));
            }
            let first = points_of_interest.len();
            for (side, &id) in ids.iter().enumerate() {
                let bank = nodes[&id];
                points_of_interest.push(PointOfInterest {
                    kind: PoiKind::RiverCrossing,
                    position: bank.position,
                    level: bank.level,
                    partner: Some(first + 1 - side),
                });
            }
            crossings.push(RiverCrossing {
                banks: ids,
                origin: road.start,
                river: gap.river,
                water_level: gap.water_level,
            });
        }

        // The end: the hub, or a node of the road it joins.
        let end = match road.joins {
            None => {
                let hub_node = node(hub, RoadNodeKind::Hub, length);
                nodes.insert(hub, hub_node);
                points_of_interest.push(PointOfInterest {
                    kind: PoiKind::RoadEnd,
                    position: hub_node.position,
                    level: hub_node.level,
                    partner: None,
                });
                hub
            }
            Some((joined, a)) => {
                let existing = breaks[joined]
                    .iter()
                    .find(|(b, _)| (b - a).abs() <= SAME_PLACE)
                    .map(|&(_, id)| id);
                existing.unwrap_or_else(|| {
                    let id = RoadNodeId(mix64(road.start.0, JUNCTION_TAG));
                    let mut junction = node(id, RoadNodeKind::Junction, length);
                    junction.position = roads[joined].at(a).0;
                    nodes.insert(id, junction);
                    breaks[joined].push((a, id));
                    id
                })
            }
        };
        own.push((length, end));
        breaks.push(own);

        // Bends tighter than asked, away from the gaps.
        for &(middle, _) in &road.sharp {
            let nearest = (0..road.points.len())
                .min_by(|&a, &b| {
                    road.points[a]
                        .distance(middle)
                        .total_cmp(&road.points[b].distance(middle))
                })
                .expect("a road has stations");
            if road.gaps.iter().all(|g| !g.holds(nearest)) {
                points_of_interest.push(PointOfInterest {
                    kind: PoiKind::SharpBend,
                    position: middle,
                    level: road.levels[nearest],
                    partner: None,
                });
            }
        }
    }

    // The segments between consecutive breaks, leaving out the gaps.
    let mut segments = Vec::new();
    let mut carved = Vec::new();
    let mut taken = HashSet::new();
    for (road, own) in roads.iter().zip(&mut breaks) {
        own.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        own.dedup_by(|b, a| (b.0 - a.0).abs() <= SAME_PLACE);
        for pair in own.windows(2) {
            let ((a0, n0), (a1, n1)) = (pair[0], pair[1]);
            let over_gap = road.gaps.iter().any(|g| {
                (road.along[g.from] - a0).abs() <= SAME_PLACE
                    && (road.along[g.to] - a1).abs() <= SAME_PLACE
            });
            if over_gap || a1 - a0 <= SAME_PLACE {
                continue;
            }
            let (segment, line) = cut(road, a0, a1, n0, n1, &mut taken);
            let open = [n0, n1].map(|n| nodes[&n].kind == RoadNodeKind::RiverBank);
            segments.push(segment);
            carved.push(CarvedRoad {
                banks: line.1,
                line: line.0,
                open,
            });
        }
    }

    // Everything in id order.
    let mut nodes: Vec<RoadNode> = nodes.into_values().collect();
    nodes.sort_by_key(|n| n.id);
    segments.sort_by_key(|s: &RoadSegment| s.id);
    crossings.sort_by_key(|c: &RiverCrossing| c.banks[0]);
    (
        RoadNetwork {
            nodes,
            segments,
            crossings,
        },
        carved,
        points_of_interest,
    )
}

/// The part of `road` from `a0` to `a1` metres along it, between nodes
/// `n0` and `n1`: as a segment, and as a line to carve with its banks.
/// `taken` holds the segment ids given out so far.
fn cut(
    road: &Planned,
    a0: f64,
    a1: f64,
    n0: RoadNodeId,
    n1: RoadNodeId,
    taken: &mut HashSet<RoadId>,
) -> (RoadSegment, (FeatureLine, Vec<f32>)) {
    // Stations: the two ends, and every station between.
    let mut stations = Vec::new();
    let mut banks = Vec::new();
    let mut push = |a: f64, (position, level, width, bank): (DVec2, f32, f32, f32)| {
        stations.push(RoadStation {
            along: a - a0,
            position,
            level,
            width,
        });
        banks.push(bank);
    };
    push(a0, road.at(a0));
    for (i, &a) in road.along.iter().enumerate() {
        if a > a0 + SAME_PLACE && a < a1 - SAME_PLACE {
            let station = (
                road.points[i],
                road.levels[i],
                road.widths[i],
                road.banks[i],
            );
            push(a, station);
        }
    }
    push(a1, road.at(a1));

    // A fresh id from its two ends.
    let mut id = RoadId(mix64(mix64(SEGMENT_TAG, n0.0), n1.0));
    while !taken.insert(id) {
        id = RoadId(id.0.wrapping_add(1));
    }
    let line = FeatureLine {
        kind: FeatureKind::Road,
        points: stations.iter().map(|s| s.position).collect(),
        levels: stations.iter().map(|s| s.level).collect(),
        widths: stations.iter().map(|s| s.width).collect(),
        depth: 0.0,
    };
    (
        RoadSegment {
            id,
            origin: road.start,
            start: n0,
            end: n1,
            alignment: road.alignment.between(a0, a1),
            stations,
        },
        (line, banks),
    )
}
