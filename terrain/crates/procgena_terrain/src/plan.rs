//! Planning a region's rivers and roads: wishes before negotiation, and the
//! finished features after it.

use glam::DVec2;

use crate::carve::{FeatureKind, FeatureLine, Features, Pond, chaikin};
use crate::grid::Grid;
use crate::options::TerrainOptions;
use crate::ports::{FEATURE_CLEARANCE, FREEBOARD, MAX_HALF_WIDTH, Port, PortKind, PortWish, Side};
use crate::region::{Boundary, Region, RegionEdge, point_at, segments};
use crate::relief::Relief;
use crate::river::{End, Occupied, Traced, Tracing, trace_inbound, trace_own};
use crate::road::{RoadStart, route};
use crate::seam::{EdgeAgreement, EdgeSample, smooth};

/// Chaikin smoothing passes applied to rivers.
const RIVER_SMOOTHING: usize = 3;

/// The smallest depth a river may cut through a rise, in metres.
const BREACH_BASE: f32 = 4.0;

/// The share of the terrain's hill and mountain height a river may cut
/// through on top of [`BREACH_BASE`].
const BREACH_SHARE: f32 = 0.03;

/// The region's own rivers, traced over its unbent relief.
#[derive(Debug, Clone)]
pub(crate) struct OwnRivers {
    grid: Grid,
    rivers: Vec<Traced>,
    occupied: Occupied,
}

/// How far from the border a region's own rivers may start: outside its
/// margin, and never closer than [`FEATURE_CLEARANCE`].
fn source_zone(options: &TerrainOptions) -> f64 {
    f64::from(options.edges.margin).max(FEATURE_CLEARANCE)
}

/// How deep a river may cut through a rise to escape a hollow, in metres:
/// a few metres, plus a share of the terrain's relief, so mountain rivers
/// cut gorges through the rims of large hollows.
fn breach(options: &TerrainOptions) -> f32 {
    let b = &options.base;
    BREACH_BASE + BREACH_SHARE * (b.hills.height + b.mountains.height)
}

/// Traces the region's own rivers, or `None` for a region without a polygon.
pub(crate) fn own_rivers(
    options: &TerrainOptions,
    relief: &Relief,
    region: &Region,
) -> Option<OwnRivers> {
    let Boundary::Polygon(polygon) = &region.boundary else {
        return None;
    };
    let grid = Grid::new(polygon, |p| relief.height(p));
    let r = &options.features.rivers;
    let tracing = Tracing {
        source_zone: source_zone(options),
        density: r.density,
        spacing: r.spacing,
        min_length: r.min_length,
        breach: breach(options),
    };
    let (rivers, occupied) = trace_own(&grid, region, polygon, &tracing);
    Some(OwnRivers {
        grid,
        rivers,
        occupied,
    })
}

/// Which side of `edge` the region lies on.
pub(crate) fn side_of(region: &Region, edge: &RegionEdge) -> Side {
    let segments = segments(&edge.polyline);
    let middle = &segments[segments.len() / 2];
    let probe = middle.start + middle.direction * (middle.length / 2.0) + middle.normal * 0.5;
    if region.boundary.contains(probe) {
        Side::Left
    } else {
        Side::Right
    }
}

/// The end on `side` of the piece carved across the border at `port`, with
/// its level.
fn piece_end(edge: &RegionEdge, port: &Port, side: Side) -> (DVec2, f32) {
    let (left, right) = port.ends(edge);
    let point = if side == Side::Left { left } else { right };
    (point, port.level_at(side.sign() * FEATURE_CLEARANCE))
}

/// The other side of an edge.
fn opposite(side: Side) -> Side {
    match side {
        Side::Left => Side::Right,
        Side::Right => Side::Left,
    }
}

/// Keeps a river's water below the uncarved ground at every point, never
/// rising downstream. With `fixed_start` the first level is kept as it is;
/// with `floor`, no level drops below it (the river must still reach a
/// crossing at that level).
fn fit_to_ground(
    points: &[DVec2],
    levels: &mut [f32],
    base: &dyn Fn(DVec2) -> f32,
    fixed_start: bool,
    floor: Option<f32>,
) {
    for i in usize::from(fixed_start)..levels.len() {
        let mut level = levels[i].min(base(points[i]) - FREEBOARD);
        if i > 0 {
            level = level.min(levels[i - 1]);
        }
        levels[i] = floor.map_or(level, |f| level.max(f));
    }
}

/// Depth of a river's bed at a crossing, from its width, in metres.
fn port_depth(width: f32) -> f32 {
    (0.25 * width).clamp(0.5, 5.0)
}

/// The river and road wishes for edge number `edge_index` of `region`, given
/// this terrain's own profile along it (`natural`, samples `step` metres
/// apart) and its own relief.
pub(crate) fn wishes(
    options: &TerrainOptions,
    relief: &Relief,
    region: &Region,
    own: Option<&OwnRivers>,
    edge_index: usize,
    natural: &[EdgeSample],
    step: f64,
) -> (Vec<PortWish>, Vec<PortWish>) {
    // Rivers that reached this edge's zone, with the lowest ground between
    // their last point and where their crossing piece would end.
    let width = options.features.rivers.width;
    let edge = &region.edges[edge_index];
    let inward = side_of(region, edge).sign() * FEATURE_CLEARANCE;
    let segments = segments(&edge.polyline);
    let rivers = own
        .map(|own| {
            own.rivers
                .iter()
                .filter_map(|r| match r.end {
                    End::Edge { edge, along, level } if edge == edge_index => {
                        let start = r.points[r.points.len() - 1];
                        let (q, normal) = point_at(&segments, along);
                        let ground = lowest_ground(relief, start, q + normal * inward);
                        Some(PortWish {
                            along,
                            level,
                            width,
                            ground,
                        })
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();

    // One road, at the most level point of the middle third of the edge.
    let mut roads = Vec::new();
    if options.features.roads.enabled && natural.len() >= 7 {
        let smoothed = smooth(natural, step);
        let third = natural.len() / 3;
        let best = (third.max(1)..(2 * third).min(natural.len() - 1)).min_by(|&a, &b| {
            let grade = |i: usize| (smoothed[i + 1].height - smoothed[i - 1].height).abs();
            grade(a).total_cmp(&grade(b)).then(a.cmp(&b))
        });
        if let Some(i) = best {
            roads.push(PortWish {
                along: i as f64 * step,
                level: 0.0,
                width: options.features.roads.width,
                ground: f32::INFINITY,
            });
        }
    }
    (rivers, roads)
}

/// The lowest height of `relief` on the straight line from `a` to `b`,
/// sampled every few metres.
fn lowest_ground(relief: &Relief, a: DVec2, b: DVec2) -> f32 {
    let steps = (a.distance(b) / 5.0).ceil().max(1.0) as usize;
    (0..=steps)
        .map(|k| relief.height(a.lerp(b, k as f64 / steps as f64)))
        .fold(f32::INFINITY, f32::min)
}

/// Where segment `a`–`b` first meets the line through `points` (touching
/// counts): the fraction of the way from `a` to `b`, the index of the line's
/// segment, and the fraction of the way along that segment.
fn meeting(a: DVec2, b: DVec2, points: &[DVec2]) -> Option<(f64, usize, f64)> {
    let ab = b - a;
    points
        .windows(2)
        .enumerate()
        .filter_map(|(k, w)| {
            let cd = w[1] - w[0];
            let denominator = ab.perp_dot(cd);
            if denominator == 0.0 {
                return None;
            }
            let ac = w[0] - a;
            let t = ac.perp_dot(cd) / denominator;
            let u = ac.perp_dot(ab) / denominator;
            ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((t, k, u))
        })
        .min_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)))
}

/// The point of the line through `points` nearest to `p`: the index of its
/// segment and the fraction of the way along it.
fn nearest_on(points: &[DVec2], p: DVec2) -> (usize, f64) {
    points
        .windows(2)
        .enumerate()
        .map(|(k, w)| {
            let cd = w[1] - w[0];
            let u = ((p - w[0]).dot(cd) / cd.length_squared().max(1e-12)).clamp(0.0, 1.0);
            (k, u, p.distance(w[0] + cd * u))
        })
        .min_by(|x, y| x.2.total_cmp(&y.2).then(x.0.cmp(&y.0)))
        .map(|(k, u, _)| (k, u))
        .expect("a river has at least two points")
}

/// Ends `tributary` exactly on `trunk`'s line: where it first meets that
/// line, or else at the point of the line nearest to its end. The meeting
/// point becomes a point of both lines, with `trunk`'s level there
/// interpolated.
fn join(tributary: &mut FeatureLine, trunk: &mut FeatureLine) {
    // Cut the tributary where it first meets the trunk, or carry its end on
    // to the trunk at its last level.
    let first = tributary
        .points
        .windows(2)
        .enumerate()
        .find_map(|(i, w)| meeting(w[0], w[1], &trunk.points).map(|(t, k, _)| (i, t, k)));
    let (point, segment) = match first {
        Some((i, t, k)) => {
            let (a, b) = (tributary.points[i], tributary.points[i + 1]);
            let (la, lb) = (tributary.levels[i], tributary.levels[i + 1]);
            let point = a.lerp(b, t);
            tributary.points.truncate(i + 1);
            tributary.levels.truncate(i + 1);
            if point != a {
                tributary.points.push(point);
                tributary.levels.push(la + (lb - la) * t as f32);
            }
            (point, k)
        }
        None => {
            let end = *tributary.points.last().expect("a river has points");
            let level = *tributary.levels.last().expect("a river has levels");
            let (k, u) = nearest_on(&trunk.points, end);
            let point = trunk.points[k].lerp(trunk.points[k + 1], u);
            if point != end {
                tributary.points.push(point);
                tributary.levels.push(level);
            }
            (point, k)
        }
    };

    // Give the trunk a point there too, unless it already has one.
    let (c, d) = (trunk.points[segment], trunk.points[segment + 1]);
    if point != c && point != d {
        let u = c.distance(point) / c.distance(d);
        let (lc, ld) = (trunk.levels[segment], trunk.levels[segment + 1]);
        trunk.points.insert(segment + 1, point);
        trunk.levels.insert(segment + 1, lc + (ld - lc) * u as f32);
    }
}

/// Builds the region's rivers, ponds and roads after negotiation.
///
/// `agreements` pairs each shared edge's index in the region with its
/// agreement. `base` is the terrain's bent height before any carving.
pub(crate) fn features(
    options: &TerrainOptions,
    relief: &Relief,
    region: &Region,
    agreements: &[(usize, &EdgeAgreement)],
    base: &dyn Fn(DVec2) -> f32,
) -> Features {
    let Some(own) = own_rivers(options, relief, region) else {
        return Features::default();
    };
    let bent = own.grid.with_heights(base);
    let r = &options.features.rivers;
    let pond = |centre: DVec2, level: f32, width: f32| Pond {
        centre,
        level,
        radius: f64::from(2.0 * width).clamp(12.0, MAX_HALF_WIDTH),
        depth: 1.5 * r.depth,
    };
    /// What part of a river a line is, which decides how it is finished.
    enum Part {
        /// A crossing piece: kept exactly as agreed.
        Piece,
        /// The region's own river; it may have to reach a crossing at a
        /// level (`floor`).
        Own { floor: Option<f32> },
        /// A river flowing in from a crossing at its first level.
        Inbound,
    }
    let mut rivers = Vec::new();
    let mut ponds = Vec::new();
    let mut push_river = |points: &[DVec2], levels: &[f32], width: f32, depth: f32, part: Part| {
        let (points, levels) = match part {
            Part::Piece => (points.to_vec(), levels.to_vec()),
            Part::Own { floor } => {
                let (points, mut levels) = chaikin(points, levels, RIVER_SMOOTHING);
                fit_to_ground(&points, &mut levels, base, false, floor);
                (points, levels)
            }
            Part::Inbound => {
                let (points, mut levels) = chaikin(points, levels, RIVER_SMOOTHING);
                fit_to_ground(&points, &mut levels, base, true, None);
                (points, levels)
            }
        };
        rivers.push(FeatureLine {
            kind: FeatureKind::River,
            points,
            levels,
            width,
            depth,
        });
        rivers.len() - 1
    };
    // Each traced river's index in `rivers`, and every (joining river,
    // river it joins) pair of traced indices.
    let mut line_of = Vec::new();
    let mut joins = Vec::new();

    // The region's own rivers, fitted to the bent ground: on to their
    // crossing, or into a pond. Rivers are visited in tracing order, so a
    // river's junction is always fitted before the river joining it.
    let mut traced = own.rivers.clone();
    let mut occupied = own.occupied.clone();
    let breach = breach(options);
    for k in 0..traced.len() {
        let river = traced[k].clone();
        let port = match river.end {
            End::Edge { edge, along, .. } => agreements
                .iter()
                .find(|(i, _)| *i == edge)
                .and_then(|(_, a)| {
                    a.ports
                        .iter()
                        .find(|p| p.kind == PortKind::River { from: region.id } && p.wish == along)
                })
                .map(|p| (edge, *p)),
            _ => None,
        };

        // Near the border the ground was bent after tracing. Keep the water
        // below it, and stop where the ground rises too far above the water
        // to cut through. A river with a crossing keeps its course, never
        // falling below the level where it meets the crossing piece.
        let arrival = port.map(|(edge, p)| {
            let edge = &region.edges[edge];
            piece_end(edge, &p, side_of(region, edge))
        });
        let mut levels = river.levels.clone();
        let mut kept = river.points.len();
        for i in 1..levels.len() {
            let ground = bent
                .cell_of(river.points[i])
                .map_or(f32::INFINITY, |c| bent.heights[c]);
            if port.is_none() && ground > levels[i - 1] + FREEBOARD + breach {
                kept = i;
                break;
            }
            let lowered = levels[i].min(levels[i - 1]).min(ground - FREEBOARD);
            levels[i] = arrival.map_or(lowered, |(_, level)| lowered.max(level));
        }
        let mut points = river.points[..kept].to_vec();
        levels.truncate(kept);

        // A shortened river, or one whose junction was cut off, ends in a pond.
        let last = (points[kept - 1], levels[kept - 1]);
        let mut end = if kept < river.points.len() {
            End::Sink
        } else {
            river.end
        };
        if end == End::Join {
            match traced[..k].iter().position(|t| t.points.contains(&last.0)) {
                Some(trunk) => joins.push((k, trunk)),
                None => end = End::Sink,
            }
        }
        traced[k] = Traced {
            points: points.clone(),
            levels: levels.clone(),
            end,
        };
        for cell in occupied.iter_mut() {
            if matches!(*cell, Some((river, point)) if river == k && point >= kept) {
                *cell = None;
            }
        }

        let mut floor = None;
        match (end, arrival) {
            (End::Edge { .. }, Some((point, level))) => {
                points.push(point);
                levels.push(level);
                floor = Some(level);
            }
            (End::Join, _) => {}
            _ => ponds.push(pond(last.0, last.1, r.width)),
        }
        line_of.push(push_river(
            &points,
            &levels,
            r.width,
            r.depth,
            Part::Own { floor },
        ));
    }

    // Rivers flowing in from neighbours, and every crossing's straight piece.
    let mut road_starts = Vec::new();
    for &(index, agreement) in agreements {
        let edge = &region.edges[index];
        let side = side_of(region, edge);
        for port in &agreement.ports {
            match port.kind {
                PortKind::River { from } => {
                    // The piece runs downstream, from the sending side.
                    let depth = port_depth(port.width);
                    let (mut points, mut levels) = port.piece(edge);
                    let sending = if from == region.id {
                        side
                    } else {
                        opposite(side)
                    };
                    if sending == Side::Right {
                        points.reverse();
                        levels.reverse();
                    }
                    push_river(&points, &levels, port.width, depth, Part::Piece);
                    if from != region.id {
                        let (start, level) = piece_end(edge, port, side);
                        let inbound = trace_inbound(
                            &bent,
                            start,
                            level,
                            breach,
                            &traced,
                            &mut occupied,
                            traced.len(),
                        );
                        let last = inbound.points.len() - 1;
                        match inbound.end {
                            End::Sink => ponds.push(pond(
                                inbound.points[last],
                                inbound.levels[last],
                                port.width,
                            )),
                            End::Join => {
                                let end = inbound.points[last];
                                let trunk = traced
                                    .iter()
                                    .position(|t| t.points.contains(&end))
                                    .expect("a joining river ends on a traced river");
                                joins.push((traced.len(), trunk));
                            }
                            End::Edge { .. } => {}
                        }
                        line_of.push(push_river(
                            &inbound.points,
                            &inbound.levels,
                            port.width,
                            depth,
                            Part::Inbound,
                        ));
                        traced.push(inbound);
                    }
                }
                PortKind::Road => {
                    let (point, level) = piece_end(edge, port, side);
                    road_starts.push(RoadStart {
                        point,
                        level,
                        width: port.width,
                    });
                }
            }
        }
    }

    // Smoothing moved every river off its traced cells, so end each joining
    // river on the smoothed line of the river it joins. Joins are visited in
    // tracing order, so the river joined is already finished.
    for &(tributary, trunk) in &joins {
        let (t, k) = (line_of[tributary], line_of[trunk]);
        let [tributary, trunk] = rivers
            .get_disjoint_mut([t, k])
            .expect("a river never joins itself");
        join(tributary, trunk);
    }

    // Roads: the crossing pieces, then routes linking them.
    let mut water = vec![false; bent.heights.len()];
    for river in &rivers {
        for &p in &river.points {
            if let Some(c) = bent.cell_of(p) {
                water[c] = true;
            }
        }
    }
    let mut roads = route(&bent, &road_starts, &water, &|p| relief.road_grades(p));
    for &(index, agreement) in agreements {
        let edge = &region.edges[index];
        for port in agreement.ports.iter().filter(|p| p.kind == PortKind::Road) {
            let (points, levels) = port.piece(edge);
            roads.push(FeatureLine {
                kind: FeatureKind::Road,
                points,
                levels,
                width: port.width,
                depth: 0.0,
            });
        }
    }
    Features::new(rivers, ponds, roads)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A river through `points`, with `levels`.
    fn river(points: &[(f64, f64)], levels: &[f32]) -> FeatureLine {
        FeatureLine {
            kind: FeatureKind::River,
            points: points.iter().map(|&(x, z)| DVec2::new(x, z)).collect(),
            levels: levels.to_vec(),
            width: 6.0,
            depth: 1.0,
        }
    }

    #[test]
    fn a_tributary_crossing_its_trunk_is_cut_where_they_meet() {
        // The trunk runs along x = 0; the tributary crosses it.
        let mut trunk = river(&[(0.0, -100.0), (0.0, 100.0)], &[10.0, 8.0]);
        let mut tributary = river(
            &[(-50.0, 0.0), (-10.0, 0.0), (30.0, 0.0)],
            &[20.0, 12.0, 11.0],
        );
        join(&mut tributary, &mut trunk);
        let meeting = DVec2::new(0.0, 0.0);
        assert_eq!(tributary.points.last(), Some(&meeting));
        assert_eq!(tributary.points.len(), 3);
        assert_eq!(tributary.levels.last(), Some(&11.75));
        assert_eq!(trunk.points[1], meeting);
        assert_eq!(trunk.levels, vec![10.0, 9.0, 8.0]);
    }

    #[test]
    fn a_tributary_short_of_its_trunk_is_carried_on_to_it() {
        let mut trunk = river(&[(0.0, -100.0), (0.0, 100.0)], &[10.0, 8.0]);
        let mut tributary = river(&[(-50.0, 50.0), (-10.0, 50.0)], &[20.0, 12.0]);
        join(&mut tributary, &mut trunk);
        let meeting = DVec2::new(0.0, 50.0);
        assert_eq!(tributary.points.last(), Some(&meeting));
        assert_eq!(tributary.levels, vec![20.0, 12.0, 12.0]);
        assert_eq!(trunk.points[1], meeting);
        assert_eq!(trunk.levels, vec![10.0, 8.5, 8.0]);
    }

    #[test]
    fn a_tributary_ending_on_a_trunk_point_adds_no_point() {
        let mut trunk = river(
            &[(0.0, -100.0), (0.0, 0.0), (0.0, 100.0)],
            &[10.0, 9.0, 8.0],
        );
        let mut tributary = river(&[(-50.0, 0.0), (0.0, 0.0)], &[20.0, 12.0]);
        join(&mut tributary, &mut trunk);
        assert_eq!(tributary.points.len(), 2);
        assert_eq!(trunk.points.len(), 3);
    }
}
