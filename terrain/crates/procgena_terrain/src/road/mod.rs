//! Routing roads over a region's grid and setting their levels.

mod align;
mod assemble;
mod earthworks;
mod level;
mod network;
mod quality;
mod search;
pub(crate) mod section;

pub(crate) use assemble::CarvedRoad;
pub use earthworks::{
    BreaklineKind, Earthwork, JunctionFootprint, RoadBreakline, RoadEarthworks, SegmentEarthworks,
    SideSection, StationSection,
};
pub(crate) use earthworks::{CarvedLine, earthworks};
pub use network::{
    Alignment, AlignmentPiece, RiverCrossing, RoadId, RoadNetwork, RoadNode, RoadNodeId,
    RoadNodeKind, RoadSegment, RoadStation,
};
pub use quality::{RoadQuality, road_quality};
pub use section::SectionShape;

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use glam::DVec2;

use crate::carve::{FeatureLine, PointOfInterest, Pond};
use crate::grid::Grid;
use crate::options::RoadGrades;
use crate::ports::FEATURE_CLEARANCE;
use crate::seam::smootherstep;

use assemble::{Planned, RiverIndex, assemble, gaps, nearest_on};
use level::{across_road, crowded, grade_limited, level_on_roads, smooth};
use search::{Bridges, Scratch, Terrain, network_distance, search, water};

/// How many joins a road may find out of reach before it gives up.
const MAX_REFUSED_JOINS: usize = 8;

/// How far apart a road's stations are, in metres: the points its level
/// and width are set at, and the points it is carved through.
const STATION: f64 = 2.0;

/// How far a road's fitted centre line may stray from its routed path
/// before a corner is kept, as a share of the routing grid's cell size.
const FIT_SHARE: f64 = 0.5;

/// How far along the road a road's narrowing is averaged, in metres.
const SLOPE_RADIUS: f64 = 20.0;

/// Over how many metres from its start a road's width and shape across
/// ease in from those of its border crossing piece.
const BANK_EASE: f64 = 80.0;

/// How deep a road cuts, in metres, before it starts to narrow.
const NARROW_FROM: f64 = 3.0;

/// How deep a road cuts, in metres, where it is at its narrowest.
const NARROW_TO: f64 = 10.0;

/// How close two legs of one road may come, as a multiple of its width.
const LEG_SPACING: f64 = 3.0;

/// How many times a road is routed again around a place where its legs
/// crowd each other, before it is kept as it is.
const MAX_CROWDED_RETRIES: usize = 8;

/// The cell size of the grid the road network is first outlined on, in
/// metres.
const COARSE_CELL: f64 = 16.0;

/// How far a refined road may stray from its coarse outline, in metres.
const CORRIDOR: f64 = 48.0;

/// A road's start: the end of a border crossing on this side, and the road
/// level there.
pub(crate) struct RoadStart {
    /// The road network's node there.
    pub node: RoadNodeId,
    pub point: DVec2,
    pub level: f32,
    pub width: f32,
    /// How narrow the road may get through deep cuttings.
    pub min_width: f32,
    /// The tightest bend the road should take, in metres.
    pub min_radius: f32,
    /// The road's shape across, away from its start: it eases in from
    /// [`SectionShape::PIECE`] over its first [`BANK_EASE`] metres.
    pub section: SectionShape,
}

/// The water roads cross or keep clear of: rivers, and the ponds where
/// they end.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Waters<'a> {
    pub rivers: &'a [FeatureLine],
    pub ponds: &'a [Pond],
}

/// Where a region's roads lead.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Hub {
    /// Near where, or `None` for a level place near the region's middle.
    pub at: Option<DVec2>,
    /// The road network's node there.
    pub node: RoadNodeId,
}

/// The roads of a region, and where they end.
pub(crate) struct Routes {
    /// Each road whole, from its start to where it joins the network,
    /// river gaps included, in the order planned.
    pub roads: Vec<FeatureLine>,
    /// The hub every road leads to, and the road level there, if any road
    /// reached it.
    pub end: Option<(DVec2, f32)>,
    /// The roads as a network, split at every junction and river gap.
    pub network: RoadNetwork,
    /// The network's segments, as lines to carve.
    pub carved: Vec<CarvedRoad>,
    /// The hub, the river crossings and the sharp bends.
    pub points_of_interest: Vec<PointOfInterest>,
    /// What planning them took.
    pub stats: RouteStats,
}

/// Diagnostics from planning one region's roads at one tier: how much work
/// the route search did, and where the time went. For measuring the road
/// builder only: the counts are the same on every run, the times are not,
/// and neither changes the roads.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RouteStats {
    /// Cells in the routing grid's bounding box.
    pub grid_cells: usize,
    /// Cells a road may pass through.
    pub active_cells: usize,
    /// Road starts: one per road crossing on the region's border.
    pub starts: usize,
    /// Starts that got no road.
    pub unconnected: usize,
    /// Route searches run, retries included.
    pub searches: usize,
    /// Search states queued, over every search.
    pub states_pushed: u64,
    /// Search states taken from the queue, over every search.
    pub states_popped: u64,
    /// Joins refused because the road couldn't reach the network's level.
    pub refused_joins: usize,
    /// Searches run again because a road's legs crowded each other.
    pub crowded_retries: usize,
    /// Roads kept although their legs crowd each other.
    pub crowded_kept: usize,
    /// The most memory one search held for its states, in bytes.
    pub peak_state_bytes: usize,
    /// Bends tighter than the roads' minimum radius.
    pub sharp_bends: usize,
    /// River crossings left open for a later pass to span.
    pub river_crossings: usize,
    /// Time spent building the routing grid: cells and ground heights.
    pub grid_time: Duration,
    /// Time spent on per-cell grades, water and bridges.
    pub setup_time: Duration,
    /// Time spent on the estimates of the cost still ahead.
    pub field_time: Duration,
    /// Time spent in route searches.
    pub search_time: Duration,
    /// Time spent setting road levels, widths and checking crowding.
    pub level_time: Duration,
}

impl RouteStats {
    /// The total time recorded.
    pub fn total_time(&self) -> Duration {
        self.grid_time + self.setup_time + self.field_time + self.search_time + self.level_time
    }
}

/// Links every start to a hub near the middle of the region, or to a road
/// already linked, over `grid` (heights of the bent terrain, which
/// `ground` gives at any point). Roads cross the rivers of `waters` as
/// seldom as they reasonably can, and `grades` gives the road grades at a
/// point. Roads
/// stay at least [`FEATURE_CLEARANCE`] from the border, apart from the
/// straight first leg from each start.
///
/// The hub is the allowed cell nearest `hub.at` when given, otherwise a
/// level cell near the middle of the region.
///
/// Each road is fitted with straights and arcs of at least its start's
/// `min_radius` where the ground leaves room (tighter bends are listed as
/// sharp bends), and stops clear of the water (ponds included) on both
/// banks of each river it crosses.
pub(crate) fn route(
    grid: &Grid,
    starts: &[RoadStart],
    waters: Waters,
    hub: Hub,
    grades: &(dyn Fn(DVec2) -> RoadGrades + Sync),
    ground: &(dyn Fn(DVec2) -> f32 + Sync),
) -> Routes {
    let Hub {
        at: hub_at,
        node: hub_node,
    } = hub;
    let setup_started = Instant::now();
    let allowed: Vec<bool> = (0..grid.heights.len())
        .map(|i| grid.inside[i] && grid.clearance[i] > FEATURE_CLEARANCE)
        .collect();
    let mut stats = RouteStats {
        grid_cells: allowed.len(),
        active_cells: allowed.iter().filter(|&&a| a).count(),
        starts: starts.len(),
        ..RouteStats::default()
    };
    let hub = match hub_at {
        Some(p) => nearest_allowed(grid, &allowed, p),
        None => level_middle(grid, &allowed),
    };
    let Some(hub) = hub else {
        stats.unconnected = starts.len();
        return Routes::none(stats);
    };

    // Grades are read only where a road may pass; the search never steps
    // onto the other cells.
    let unused = RoadGrades {
        grade: 1.0,
        max_grade: 1.0,
    };
    let cell_grades: Vec<RoadGrades> = (0..grid.heights.len())
        .map(|i| {
            if allowed[i] {
                grades(grid.centre(i))
            } else {
                unused
            }
        })
        .collect();
    let rivers = waters.rivers;
    let river = water(grid, rivers);
    let bridges = Bridges::new(grid, rivers);
    let terrain = Terrain {
        grid,
        allowed: &allowed,
        river: &river,
        bridges: &bridges,
        grades: &cell_grades,
    };
    let mut scratch = Scratch::new(&allowed);
    stats.setup_time = setup_started.elapsed();

    let river_index = RiverIndex::new(rivers, waters.ponds);
    let mut network = vec![false; grid.heights.len()];
    network[hub] = true;
    // Cells over a river gap: no road may join there.
    let mut over_gaps = vec![false; grid.heights.len()];
    let mut planned: Vec<Planned> = Vec::new();
    let mut roads: Vec<FeatureLine> = Vec::new();
    for start in starts {
        let Some(first) = first_cell(grid, &allowed, start.point) else {
            stats.unconnected += 1;
            continue;
        };
        // Route, and set levels; a join that the smoothed road can't reach
        // within its grade is refused, and the search runs again.
        let lead_in =
            start.point.distance(grid.centre(first)) * f64::from(grades(start.point).max_grade);
        let mut refused: Vec<usize> = Vec::new();
        let spacing = LEG_SPACING * f64::from(start.width);
        let mut blocked = vec![false; grid.heights.len()];
        let mut retries = 0;
        let mut crowded_route = None;
        let field_started = Instant::now();
        let ahead = network_distance(grid, &network);
        stats.field_time += field_started.elapsed();
        // The network's level at each cell, looked up once per cell: the
        // roads don't change until this start's road is kept.
        let levels = RefCell::new(HashMap::new());
        let network_level = |cell: usize| {
            *levels
                .borrow_mut()
                .entry(cell)
                .or_insert_with(|| level_on_roads(&roads, grid.centre(cell)))
        };
        let routed = loop {
            if refused.len() > MAX_REFUSED_JOINS {
                stats.crowded_kept += usize::from(crowded_route.is_some());
                break crowded_route;
            }
            // The road may join the network only where it can reach the
            // network's level at its steepest grade (with a little to spare
            // for the smoothing).
            let joinable = |cell: usize, reach: f64| {
                !refused.contains(&cell)
                    && !over_gaps[cell]
                    && network_level(cell).is_none_or(|level| {
                        f64::from((level - start.level).abs()) <= 0.9 * (reach + lead_in)
                    })
            };
            let search_started = Instant::now();
            let found = search(
                &terrain,
                &network,
                &ahead,
                &blocked,
                first,
                &joinable,
                &mut scratch,
                &mut stats,
            );
            stats.search_time += search_started.elapsed();
            let Some(cells) = found else {
                stats.crowded_kept += usize::from(crowded_route.is_some());
                break crowded_route;
            };
            let level_started = Instant::now();

            // The path: the crossing end, the route, and the point where it
            // meets the network (the hub, for the first road).
            let join = *cells.last().expect("route has cells");
            let joined = nearest_on(&planned, grid.centre(join));
            let mut path = vec![start.point];
            path.extend(cells[..cells.len() - 1].iter().map(|&c| grid.centre(c)));
            path.push(joined.map_or(grid.centre(join), |(_, _, point, _)| point));

            // Fitted with straights and arcs, with stations along it, and
            // the river gaps.
            let fitted = align::fit(&path, f64::from(start.min_radius), FIT_SHARE * grid.cell);
            let ends = [path[0], path[path.len() - 1]];
            let (along, points): (Vec<f64>, Vec<DVec2>) =
                align::stations(&fitted.alignment, STATION, ends)
                    .into_iter()
                    .unzip();
            let river_gaps = gaps(&river_index, &points, &along);

            // Levels: follow the smoothed ground, from the crossing's level
            // to the network's level where the road joins it. Over a river
            // gap the ground doesn't count: there the level only keeps to
            // its grade, along a straight target from bank to bank.
            let heights: Vec<f32> = points.iter().map(|&p| ground(p)).collect();
            let middles: Vec<DVec2> = (0..points.len())
                .map(|i| points[i].lerp(points[i.saturating_sub(1)], 0.5))
                .collect();
            let steepest: Vec<f64> = middles
                .iter()
                .map(|&p| f64::from(grades(p).max_grade))
                .collect();
            let end_level = joined.map(|(_, _, _, level)| level);
            let mut target = smooth(&heights);
            let mut across = across_road(&points, &heights, f64::from(start.width) / 2.0, ground);
            for gap in &river_gaps {
                let (a0, a1) = (along[gap.from], along[gap.to]);
                let (t0, t1) = (target[gap.from], target[gap.to]);
                for i in gap.from + 1..gap.to {
                    let t = ((along[i] - a0) / (a1 - a0)) as f32;
                    target[i] = t0 + (t1 - t0) * t;
                    across[i] = (f32::INFINITY, f32::NEG_INFINITY);
                }
            }
            let Some(levels) =
                grade_limited(&points, &across, &target, start.level, end_level, &steepest)
            else {
                stats.level_time += level_started.elapsed();
                stats.refused_joins += 1;
                refused.push(join);
                continue;
            };

            // Where two legs crowd each other, route again around that
            // place, keeping this route in case no other is found.
            let crowding = crowded(&points, spacing);
            stats.level_time += level_started.elapsed();
            if crowding.is_empty() || retries >= MAX_CROWDED_RETRIES {
                if !crowding.is_empty() {
                    stats.crowded_kept += 1;
                }
                break Some(Route {
                    cells,
                    fitted,
                    along,
                    points,
                    levels,
                    across,
                    gaps: river_gaps,
                    joins: joined.map(|(road, a, _, _)| (road, a)),
                });
            }
            for &j in &crowding {
                block_around(grid, &mut blocked, points[j], 0.5 * spacing, first);
            }
            retries += 1;
            stats.crowded_retries += 1;
            crowded_route = Some(Route {
                cells,
                fitted,
                along,
                points,
                levels,
                across,
                gaps: river_gaps,
                joins: joined.map(|(road, a, _, _)| (road, a)),
            });
        };
        let Some(chosen) = routed else {
            stats.unconnected += 1;
            continue;
        };

        // Keep it: its cells join the network, those over its gaps closed
        // to joins.
        for &c in &chosen.cells {
            network[c] = true;
        }
        for gap in &chosen.gaps {
            for &p in &chosen.points[gap.from..=gap.to] {
                block_around(grid, &mut over_gaps, p, grid.cell, usize::MAX);
            }
        }
        let level_started = Instant::now();
        let Route {
            fitted,
            along,
            points,
            levels,
            across,
            gaps,
            joins,
            ..
        } = chosen;
        let road = Planned {
            start: start.node,
            widths: widths(&points, &across, &levels, start.width, start.min_width),
            sections: sections(&points, start.section),
            alignment: fitted.alignment,
            along,
            points,
            levels,
            gaps,
            joins,
            sharp: fitted.sharp,
        };
        roads.push(road.line());
        planned.push(road);
        stats.level_time += level_started.elapsed();
    }

    // The network, split at its junctions and river gaps.
    let end = level_on_roads(&roads, grid.centre(hub)).map(|level| (grid.centre(hub), level));
    let (network, carved, points_of_interest) = assemble(&planned, hub_node);
    stats.river_crossings = network.crossings.len();
    stats.sharp_bends = points_of_interest
        .iter()
        .filter(|p| p.kind == crate::carve::PoiKind::SharpBend)
        .count();
    Routes {
        roads,
        end,
        network,
        carved,
        points_of_interest,
        stats,
    }
}

/// A road found for one start, before it is kept.
struct Route {
    /// The grid cells it was routed through.
    cells: Vec<usize>,
    fitted: align::Fit,
    along: Vec<f64>,
    points: Vec<DVec2>,
    levels: Vec<f32>,
    across: Vec<(f32, f32)>,
    gaps: Vec<assemble::Gap>,
    joins: Option<(usize, f64)>,
}

impl Routes {
    /// No roads, with `stats`.
    fn none(stats: RouteStats) -> Self {
        Self {
            roads: Vec::new(),
            end: None,
            network: RoadNetwork::default(),
            carved: Vec::new(),
            points_of_interest: Vec::new(),
            stats,
        }
    }
}

/// Like [`route`], on a grid of [`COARSE_CELL`] metres over `polygon`: the
/// road network in outline, cheap to plan, with the same starts and levels
/// at the crossings. [`refined_routes`] follows it at full detail.
pub(crate) fn coarse_routes(
    polygon: &[DVec2],
    starts: &[RoadStart],
    waters: Waters,
    hub: RoadNodeId,
    grades: &(dyn Fn(DVec2) -> RoadGrades + Sync),
    ground: &(dyn Fn(DVec2) -> f32 + Sync),
) -> Routes {
    let started = Instant::now();
    let grid = Grid::with_cell(polygon, ground, COARSE_CELL);
    let grid_time = started.elapsed();
    let hub = Hub {
        at: None,
        node: hub,
    };
    let mut routes = route(&grid, starts, waters, hub, grades, ground);
    routes.stats.grid_time = grid_time;
    routes
}

/// Like [`route`], on a grid of `cell` metres over `polygon` that covers
/// only the ground within [`CORRIDOR`] metres of the `coarse` routes (see
/// [`coarse_routes`], planned with the same arguments), with the same hub.
/// Every refined road stays inside that corridor, and the hub keeps its
/// node. With no coarse network there are no roads.
pub(crate) fn refined_routes(
    polygon: &[DVec2],
    coarse: &Routes,
    cell: f64,
    starts: &[RoadStart],
    waters: Waters,
    grades: &(dyn Fn(DVec2) -> RoadGrades + Sync),
    ground: &(dyn Fn(DVec2) -> f32 + Sync),
) -> Routes {
    let hub = coarse
        .network
        .nodes
        .iter()
        .find(|n| n.kind == RoadNodeKind::Hub)
        .map(|n| n.id);
    let (Some((hub_at, _)), Some(hub)) = (coarse.end, hub) else {
        return Routes::none(RouteStats {
            starts: starts.len(),
            unconnected: starts.len(),
            ..RouteStats::default()
        });
    };
    let started = Instant::now();
    let corridor = Corridor::new(&coarse.roads, hub_at, CORRIDOR);
    let grid = Grid::with_cell_where(polygon, ground, cell, |p| corridor.contains(p));
    let grid_time = started.elapsed();
    let hub = Hub {
        at: Some(hub_at),
        node: hub,
    };
    let mut routes = route(&grid, starts, waters, hub, grades, ground);
    routes.stats.grid_time = grid_time;
    routes
}

/// The ground within a set distance of some lines and a point, bucketed so
/// a point is tested only against the segments nearby.
struct Corridor {
    /// Bucket size, the corridor's half-width.
    reach: f64,
    /// For each bucket, the segments that may lie within `reach` of a point
    /// in it.
    buckets: HashMap<(i64, i64), Vec<(DVec2, DVec2)>>,
}

impl Corridor {
    /// The ground within `reach` metres of `lines` or of `point`.
    fn new(lines: &[FeatureLine], point: DVec2, reach: f64) -> Self {
        let mut corridor = Self {
            reach,
            buckets: HashMap::new(),
        };
        let segments = lines
            .iter()
            .flat_map(|l| l.points.windows(2).map(|w| (w[0], w[1])))
            .chain(std::iter::once((point, point)));
        for (a, b) in segments {
            let (lo, hi) = (corridor.bucket(a.min(b)), corridor.bucket(a.max(b)));
            for x in lo.0 - 1..=hi.0 + 1 {
                for z in lo.1 - 1..=hi.1 + 1 {
                    corridor.buckets.entry((x, z)).or_default().push((a, b));
                }
            }
        }
        corridor
    }

    /// The bucket holding `p`.
    fn bucket(&self, p: DVec2) -> (i64, i64) {
        let q = (p / self.reach).floor();
        (q.x as i64, q.y as i64)
    }

    /// Whether `p` lies in the corridor.
    fn contains(&self, p: DVec2) -> bool {
        self.buckets.get(&self.bucket(p)).is_some_and(|segments| {
            segments
                .iter()
                .any(|&(a, b)| distance_to_segment(p, a, b) <= self.reach)
        })
    }
}

/// The distance from `p` to the segment from `a` to `b`.
fn distance_to_segment(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// The road's shape across at each of `points`: `shape`, eased in over its
/// first [`BANK_EASE`] metres from [`SectionShape::PIECE`], which its
/// border crossing piece has.
pub(crate) fn sections(points: &[DVec2], shape: SectionShape) -> Vec<SectionShape> {
    distance_along(points)
        .iter()
        .map(|&along| {
            let ease = smootherstep((along / BANK_EASE).min(1.0));
            SectionShape::PIECE.lerp(shape, ease as f32)
        })
        .collect()
}

/// How wide a road along `points` is at each point: `width` where it
/// cuts less than [`NARROW_FROM`] into the ground (the highest ground
/// across it, `across[i].1`, above its level), narrowing to `min_width`
/// where it cuts [`NARROW_TO`] or more, averaged over [`SLOPE_RADIUS`]
/// along the road. Over its first [`BANK_EASE`] metres the road eases in
/// from `width`, which its crossing piece has.
pub(crate) fn widths(
    points: &[DVec2],
    across: &[(f32, f32)],
    levels: &[f32],
    width: f32,
    min_width: f32,
) -> Vec<f32> {
    // How far towards the narrowest each point is.
    let narrowing: Vec<f64> = across
        .iter()
        .zip(levels)
        .map(|(&(_, high), &level)| {
            let cut = f64::from(high - level);
            smootherstep(((cut - NARROW_FROM) / (NARROW_TO - NARROW_FROM)).clamp(0.0, 1.0))
        })
        .collect();

    // Averaged along the road, then eased in from the start.
    let (width, min_width) = (f64::from(width), f64::from(min_width));
    eased(
        points,
        &along_mean(points, &narrowing, SLOPE_RADIUS),
        |mean, ease| (width + (min_width - width) * mean * ease) as f32,
    )
}

/// The mean of `values` over the points of `points` within `radius` metres
/// of each point, measured along the line.
pub(crate) fn along_mean(points: &[DVec2], values: &[f64], radius: f64) -> Vec<f64> {
    let along = distance_along(points);
    let (mut lo, mut hi) = (0, 0);
    (0..points.len())
        .map(|i| {
            while along[i] - along[lo] > radius {
                lo += 1;
            }
            while hi + 1 < points.len() && along[hi + 1] - along[i] <= radius {
                hi += 1;
            }
            values[lo..=hi].iter().sum::<f64>() / (hi - lo + 1) as f64
        })
        .collect()
}

/// `value(means[i], ease)` for each point of `points`, where `ease` rises
/// smoothly from 0 at the line's start to 1 at [`BANK_EASE`] metres along
/// it.
fn eased(points: &[DVec2], means: &[f64], value: impl Fn(f64, f64) -> f32) -> Vec<f32> {
    distance_along(points)
        .iter()
        .zip(means)
        .map(|(&along, &mean)| value(mean, smootherstep((along / BANK_EASE).min(1.0))))
        .collect()
}

/// The distance along `points` from the first point to each point.
pub(crate) fn distance_along(points: &[DVec2]) -> Vec<f64> {
    let mut along = vec![0.0f64; points.len()];
    for i in 1..points.len() {
        along[i] = along[i - 1] + points[i].distance(points[i - 1]);
    }
    along
}

/// Marks every cell within `radius` of `p` as `blocked`, except `keep`.
fn block_around(grid: &Grid, blocked: &mut [bool], p: DVec2, radius: f64, keep: usize) {
    let reach = (radius / grid.cell).ceil() as i64;
    for dx in -reach..=reach {
        for dz in -reach..=reach {
            let q = p + DVec2::new(dx as f64, dz as f64) * grid.cell;
            if let Some(c) = grid.cell_of(q)
                && c != keep
                && grid.centre(c).distance(p) <= radius
            {
                blocked[c] = true;
            }
        }
    }
}

/// The flattest allowed cell near the middle of the region.
fn level_middle(grid: &Grid, allowed: &[bool]) -> Option<usize> {
    let cells: Vec<usize> = (0..allowed.len()).filter(|&i| allowed[i]).collect();
    let middle = cells.iter().map(|&c| grid.centre(c)).sum::<DVec2>() / cells.len().max(1) as f64;
    let size = (grid.width.max(grid.depth) as f64) * grid.cell;
    cells.into_iter().min_by(|&a, &b| {
        let score = |c: usize| {
            let slope = grid
                .neighbours(c)
                .map(|(j, d)| (f64::from(grid.heights[j] - grid.heights[c]) / d).abs())
                .fold(0.0, f64::max);
            slope + 2.0 * grid.centre(c).distance(middle) / size
        };
        score(a).total_cmp(&score(b)).then(a.cmp(&b))
    })
}

/// The first allowed cell going inward from a crossing end at `point`: the
/// nearest allowed cell to it.
fn first_cell(grid: &Grid, allowed: &[bool], point: DVec2) -> Option<usize> {
    (0..allowed.len())
        .filter(|&i| {
            allowed[i] && grid.centre(i).distance(point) < 3.0 * grid.cell + FEATURE_CLEARANCE
        })
        .min_by(|&a, &b| {
            grid.centre(a)
                .distance(point)
                .total_cmp(&grid.centre(b).distance(point))
                .then(a.cmp(&b))
        })
}

/// The allowed cell nearest `point`, if any cell is allowed.
fn nearest_allowed(grid: &Grid, allowed: &[bool], point: DVec2) -> Option<usize> {
    (0..allowed.len()).filter(|&i| allowed[i]).min_by(|&a, &b| {
        grid.centre(a)
            .distance(point)
            .total_cmp(&grid.centre(b).distance(point))
            .then(a.cmp(&b))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carve::{FeatureKind, PoiKind};
    use search::segments_meet;

    /// No rivers or ponds.
    const DRY: Waters = Waters {
        rivers: &[],
        ponds: &[],
    };

    /// The hub of the test roads: near the middle, node 1.
    const HUB: Hub = Hub {
        at: None,
        node: RoadNodeId(1),
    };

    /// A road start at `point` and `level`, node `node`, 6 m wide down to
    /// 4 m, with bends of at least 12 m.
    fn start(node: u64, point: DVec2, level: f32) -> RoadStart {
        RoadStart {
            node: RoadNodeId(node),
            point,
            level,
            width: 6.0,
            min_width: 4.0,
            min_radius: 12.0,
            section: SectionShape::PIECE,
        }
    }

    #[test]
    fn roads_link_starts_within_their_grade() {
        let polygon = [
            DVec2::ZERO,
            DVec2::new(2000.0, 0.0),
            DVec2::new(2000.0, 2000.0),
            DVec2::new(0.0, 2000.0),
        ];
        // A gentle slope with a steep hill in the middle.
        let hill = DVec2::splat(1000.0);
        let ground =
            |p: DVec2| (0.01 * p.x + 80.0 * (-(p.distance(hill) / 200.0).powi(2)).exp()) as f32;
        let grid = Grid::new(&polygon, ground);
        let starts = [
            start(10, DVec2::new(1000.0, 70.0), 10.0),
            start(11, DVec2::new(70.0, 1500.0), 1.0),
        ];
        let grades = |_| RoadGrades {
            grade: 0.04,
            max_grade: 0.08,
        };
        let Routes { roads, end, .. } = route(&grid, &starts, DRY, HUB, &grades, &ground);
        assert_eq!(roads.len(), 2);
        // The network ends at the hub, where the first road arrives.
        let (hub, level) = end.expect("the roads reach the hub");
        assert_eq!(*roads[0].points.last().unwrap(), hub);
        assert_eq!(*roads[0].levels.last().unwrap(), level);
        for road in &roads {
            assert_eq!(
                road.points[0],
                starts[roads.iter().position(|r| r == road).unwrap()].point
            );
            for w in road.points.windows(2).zip(road.levels.windows(2)) {
                let ((a, b), (la, lb)) = ((w.0[0], w.0[1]), (w.1[0], w.1[1]));
                let grade = f64::from((lb - la).abs()) / a.distance(b);
                assert!(grade <= 0.08 * 1.001, "grade {grade}");
            }
        }
    }

    /// A road over flat ground, 2 km square, from its south edge to the hub
    /// in the middle, past `river`: the road whole, and the river.
    fn road_past(river: Vec<DVec2>) -> (FeatureLine, FeatureLine) {
        let (mut routes, river) = routes_past(river);
        assert_eq!(routes.roads.len(), 1);
        (routes.roads.remove(0), river)
    }

    /// The routes of [`road_past`], and the river.
    fn routes_past(river: Vec<DVec2>) -> (Routes, FeatureLine) {
        let polygon = [
            DVec2::ZERO,
            DVec2::new(2000.0, 0.0),
            DVec2::new(2000.0, 2000.0),
            DVec2::new(0.0, 2000.0),
        ];
        let ground = |_: DVec2| 0.0;
        let grid = Grid::with_cell(&polygon, ground, 8.0);
        let river = FeatureLine {
            kind: FeatureKind::River,
            levels: vec![0.0; river.len()],
            widths: vec![6.0; river.len()],
            points: river,
            depth: 1.0,
        };
        let start = start(10, DVec2::new(1000.0, 70.0), 0.0);
        let grades = |_| RoadGrades {
            grade: 0.04,
            max_grade: 0.08,
        };
        let routes = route(
            &grid,
            &[start],
            Waters {
                rivers: std::slice::from_ref(&river),
                ponds: &[],
            },
            HUB,
            &grades,
            &ground,
        );
        (routes, river)
    }

    /// Where `road` crosses `river`: the angle between them there, in
    /// degrees (90 is square), for each crossing.
    fn crossing_angles(road: &FeatureLine, river: &FeatureLine) -> Vec<f64> {
        let mut angles = Vec::new();
        for a in road.points.windows(2) {
            for b in river.points.windows(2) {
                if segments_meet(a[0], a[1], b[0], b[1]) {
                    let turn = (a[1] - a[0]).angle_to(b[1] - b[0]).abs().to_degrees();
                    angles.push(if turn > 90.0 { 180.0 - turn } else { turn });
                }
            }
        }
        angles
    }

    #[test]
    fn roads_go_round_a_river_bend_rather_than_cross_it_twice() {
        // A river in from the west edge that bends back 1.2 km in, so both
        // its arms lie between the road's start and the hub. Crossing both
        // saves the road about 400 m; going round costs no bridge.
        let mut river = vec![DVec2::new(0.0, 600.0), DVec2::new(1200.0, 600.0)];
        river.extend((1..12).map(|i| {
            let a = std::f64::consts::PI * (f64::from(i) / 12.0 - 0.5);
            DVec2::new(1200.0, 700.0) + 100.0 * DVec2::new(a.cos(), a.sin())
        }));
        river.extend([DVec2::new(1200.0, 800.0), DVec2::new(0.0, 800.0)]);
        let (road, river) = road_past(river);
        assert_eq!(crossing_angles(&road, &river), Vec::<f64>::new());
    }

    #[test]
    fn roads_cross_a_river_in_their_way_once_and_near_square() {
        // A river from edge to edge, slanting across the road's way.
        let (road, river) = road_past(vec![DVec2::new(0.0, 300.0), DVec2::new(2000.0, 700.0)]);
        let angles = crossing_angles(&road, &river);
        assert_eq!(angles.len(), 1, "crossings at {angles:?}°");
        assert!(angles[0] > 60.0, "crosses at {}°", angles[0]);
    }

    #[test]
    fn a_road_stops_on_both_banks_of_a_river_it_crosses() {
        let (routes, river) = routes_past(vec![DVec2::new(0.0, 300.0), DVec2::new(2000.0, 700.0)]);
        let network = &routes.network;

        // One crossing, its two banks linked as points of interest too.
        assert_eq!(network.crossings.len(), 1);
        assert_eq!(routes.stats.river_crossings, 1);
        let crossing = network.crossings[0];
        assert_eq!(crossing.river, 0);
        let banks = crossing
            .banks
            .map(|id| *network.node(id).expect("bank node"));
        for bank in banks {
            assert_eq!(bank.kind, RoadNodeKind::RiverBank);
            // On dry ground, clear of the 6 m wide river.
            let (a, b) = (river.points[0], river.points[1]);
            let t = ((bank.position - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
            let gap = bank.position.distance(a + (b - a) * t) - 3.0;
            assert!(gap >= 6.0 - 1e-9, "bank {gap} m from the water");
        }
        let pois: Vec<&PointOfInterest> = routes
            .points_of_interest
            .iter()
            .filter(|p| p.kind == PoiKind::RiverCrossing)
            .collect();
        assert_eq!(pois.len(), 2);
        let partner = pois[0].partner.expect("linked");
        assert_eq!(
            routes.points_of_interest[partner].position,
            pois[1].position
        );

        // The carved lines stop at the banks: none crosses the river, and
        // the ends at the banks are open.
        assert_eq!(routes.carved.len(), 2);
        for road in &routes.carved {
            assert!(crossing_angles(&road.line, &river).is_empty());
        }
        assert_eq!(
            routes
                .carved
                .iter()
                .filter(|r| r.open.contains(&true))
                .count(),
            2
        );

        // Segments run start to bank and bank to hub.
        assert_eq!(network.segments.len(), 2);
        let ends: Vec<[RoadNodeKind; 2]> = network
            .segments
            .iter()
            .map(|s| [s.start, s.end].map(|n| network.node(n).unwrap().kind))
            .collect();
        assert!(ends.contains(&[RoadNodeKind::Border, RoadNodeKind::RiverBank]));
        assert!(ends.contains(&[RoadNodeKind::RiverBank, RoadNodeKind::Hub]));
    }

    #[test]
    fn a_road_keeps_clear_of_a_pond_in_its_way() {
        // A river from the west edge ending in a pond right on the road's
        // way north to the hub.
        let polygon = [
            DVec2::ZERO,
            DVec2::new(2000.0, 0.0),
            DVec2::new(2000.0, 2000.0),
            DVec2::new(0.0, 2000.0),
        ];
        let ground = |_: DVec2| 0.0;
        let grid = Grid::with_cell(&polygon, ground, 8.0);
        let pond = Pond {
            centre: DVec2::new(1000.0, 500.0),
            level: 0.0,
            radius: 16.0,
            depth: 2.0,
        };
        let river = FeatureLine {
            kind: FeatureKind::River,
            points: vec![DVec2::new(0.0, 500.0), pond.centre],
            levels: vec![0.0; 2],
            widths: vec![6.0; 2],
            depth: 1.0,
        };
        let grades = |_| RoadGrades {
            grade: 0.04,
            max_grade: 0.08,
        };
        let waters = Waters {
            rivers: std::slice::from_ref(&river),
            ponds: std::slice::from_ref(&pond),
        };
        let start = start(10, DVec2::new(1000.0, 70.0), 0.0);
        let routes = route(&grid, &[start], waters, HUB, &grades, &ground);

        // The road passes the pond close by...
        let closest = routes.roads[0]
            .points
            .iter()
            .map(|p| p.distance(pond.centre) - pond.radius)
            .fold(f64::INFINITY, f64::min);
        assert!(closest < 6.0, "the road keeps {closest} m from the pond");

        // ...but no carved road comes within 6 m of it.
        assert!(!routes.carved.is_empty());
        for road in &routes.carved {
            for &p in &road.line.points {
                let gap = p.distance(pond.centre) - pond.radius;
                assert!(gap >= 6.0 - 1e-9, "{p} is {gap} m from the pond");
            }
        }
    }

    #[test]
    fn a_road_that_joins_another_splits_it_at_a_junction() {
        let polygon = [
            DVec2::ZERO,
            DVec2::new(2000.0, 0.0),
            DVec2::new(2000.0, 2000.0),
            DVec2::new(0.0, 2000.0),
        ];
        let ground = |p: DVec2| (0.002 * p.x) as f32;
        let grid = Grid::with_cell(&polygon, ground, 8.0);
        // Two starts close together on the south edge: the second joins
        // the first on its way to the hub.
        let starts = [
            start(10, DVec2::new(1000.0, 70.0), 2.0),
            start(11, DVec2::new(1150.0, 70.0), 2.3),
        ];
        let grades = |_| RoadGrades {
            grade: 0.04,
            max_grade: 0.08,
        };
        let routes = route(&grid, &starts, DRY, HUB, &grades, &ground);
        let network = &routes.network;
        let junctions: Vec<&RoadNode> = network
            .nodes
            .iter()
            .filter(|n| n.kind == RoadNodeKind::Junction)
            .collect();
        assert_eq!(junctions.len(), 1, "nodes {:?}", network.nodes);
        let junction = junctions[0].id;

        // Three segments meet there, each ending exactly on it.
        let meeting: Vec<&RoadSegment> = network
            .segments
            .iter()
            .filter(|s| s.start == junction || s.end == junction)
            .collect();
        assert_eq!(meeting.len(), 3);
        let at = network.node(junction).unwrap().position;
        for segment in meeting {
            let station = if segment.start == junction {
                segment.stations[0]
            } else {
                *segment.stations.last().unwrap()
            };
            assert!(station.position.distance(at) < 1e-6);
        }
    }

    #[test]
    fn the_network_is_the_same_every_time() {
        let river = vec![DVec2::new(0.0, 300.0), DVec2::new(2000.0, 700.0)];
        let (a, _) = routes_past(river.clone());
        let (b, _) = routes_past(river);
        assert_eq!(a.network, b.network);
        assert_eq!(a.network.content_hash(), b.network.content_hash());
        assert_ne!(
            a.network.content_hash(),
            RoadNetwork::default().content_hash()
        );
    }

    #[test]
    fn fitted_roads_keep_their_grade_and_radius() {
        let polygon = [
            DVec2::ZERO,
            DVec2::new(2000.0, 0.0),
            DVec2::new(2000.0, 2000.0),
            DVec2::new(0.0, 2000.0),
        ];
        let hill = DVec2::splat(1000.0);
        let ground =
            |p: DVec2| (0.01 * p.x + 80.0 * (-(p.distance(hill) / 200.0).powi(2)).exp()) as f32;
        let grid = Grid::new(&polygon, ground);
        let starts = [
            start(10, DVec2::new(1000.0, 70.0), 10.0),
            start(11, DVec2::new(70.0, 1500.0), 1.0),
        ];
        let grades = |_| RoadGrades {
            grade: 0.04,
            max_grade: 0.08,
        };
        let routes = route(&grid, &starts, DRY, HUB, &grades, &ground);
        let sharp: Vec<DVec2> = routes
            .points_of_interest
            .iter()
            .filter(|p| p.kind == PoiKind::SharpBend)
            .map(|p| p.position)
            .collect();
        assert_eq!(sharp.len(), routes.stats.sharp_bends);
        for segment in &routes.network.segments {
            // Every arc is at least 12 m across, or listed as a sharp bend.
            for piece in &segment.alignment.pieces {
                if let AlignmentPiece::Arc { radius, .. } = *piece
                    && radius < 12.0 - 1e-9
                {
                    let middle = piece.point_at(0.5 * piece.length());
                    assert!(
                        sharp.iter().any(|s| s.distance(middle) < 1e-6),
                        "an unlisted {radius} m bend at {middle}"
                    );
                }
            }
            // The grade holds between stations.
            for w in segment.stations.windows(2) {
                let run = w[0].position.distance(w[1].position);
                if run > 1e-6 {
                    let grade = f64::from((w[1].level - w[0].level).abs()) / run;
                    assert!(grade <= 0.08 * 1.001, "grade {grade}");
                }
            }
        }
    }

    #[test]
    fn roads_narrow_through_deep_cuttings() {
        // A level road, 400 m long: no cut over its first 200 m, then a 20 m
        // cut. Full width (6 m) at the start and on the open stretch; the
        // narrowest (4 m) well inside the cutting.
        let points: Vec<DVec2> = (0..=40)
            .map(|i| DVec2::new(10.0 * f64::from(i), 0.0))
            .collect();
        let across: Vec<(f32, f32)> = (0..=40)
            .map(|i| if i < 20 { (0.0, 0.0) } else { (0.0, 20.0) })
            .collect();
        let levels = vec![0.0; points.len()];
        let w = widths(&points, &across, &levels, 6.0, 4.0);
        assert_eq!(w[0], 6.0);
        assert_eq!(w[15], 6.0);
        assert!((w[35] - 4.0).abs() < 1e-5, "inside the cutting: {}", w[35]);
        // It narrows smoothly, never below the narrowest.
        assert!(w.iter().all(|&x| (4.0..=6.0).contains(&x)));
        assert!(w.windows(2).all(|p| p[1] <= p[0] + 1e-6));
    }
}
