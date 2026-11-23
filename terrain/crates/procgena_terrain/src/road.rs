//! Routing roads over a region's grid and setting their levels.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use glam::DVec2;

use crate::carve::{FeatureKind, FeatureLine, bank_for, chaikin};
use crate::grid::{DIRECTIONS, Grid, direction};
use crate::options::RoadGrades;
use crate::ports::FEATURE_CLEARANCE;
use crate::seam::smootherstep;

/// Extra cost per unit of grade beyond the road's preferred grade, squared
/// (in units of that grade).
const PREFERRED_COST: f64 = 40.0;

/// Extra cost per unit of grade beyond the road's steepest grade, squared
/// (in units of that grade), on top of [`PREFERRED_COST`].
const STEEP_COST: f64 = 400.0;

/// Cost factor for a step onto a cell with water, so a bridge costs more
/// the longer it is.
const RIVER_COST: f64 = 6.0;

/// Cost of a bridge square to its river, charged on every step that
/// crosses a river, in metres of level road: a road will go this much
/// further round rather than cross a river once more. A slanting bridge is
/// longer, and costs this divided by the sine of its angle to the river.
const BRIDGE_COST: f64 = 300.0;

/// The smallest sine of a bridge's angle to its river that its cost is
/// worked out from, so a step along a river costs no more than this
/// allows.
const MIN_BRIDGE_SINE: f64 = 0.2;

/// How far a river's line is marked onto the road grid as water, as a
/// share of a cell: about every half cell.
const WATER_STEP: f64 = 0.5;

/// Cost factor for travelling along an existing road, so roads share.
const REUSE_COST: f64 = 0.4;

/// Cost per unit of grade, so roads prefer level ground.
const GRADE_COST: f64 = 4.0;

/// How many joins a road may find out of reach before it gives up.
const MAX_REFUSED_JOINS: usize = 8;

/// How many points either side are averaged into the smoothed ground that
/// road levels keep close to, where they have the choice.
const LEVEL_SMOOTHING: usize = 1;

/// Extra cost of a step per metre of expected cutting or filling, squared
/// (in units of [`CARVE_SCALE`]).
const CARVE_COST: f64 = 1.0;

/// The depth of cutting or filling that doubles a step's cost, in metres.
const CARVE_SCALE: f64 = 2.0;

/// How many metres of cutting road levels accept to save a metre of
/// filling: cuttings may steepen to cliffs, but embankments stay earth.
const CUT_SHARE: f64 = 3.0;

/// The sharpest turn a road may make from one grid step to the next, in
/// radians. On the road grid this gives hairpins about 10 m in radius.
const MAX_TURN: f64 = std::f64::consts::FRAC_PI_4;

/// How far around a road point the ground's slope is measured, and how far
/// along the road it is averaged, for the road's banks, in metres.
const SLOPE_RADIUS: f64 = 20.0;

/// Over how many metres from its start a road's banks ease in from the
/// gentlest.
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

/// A road's start: the end of a border crossing on this side, and the road
/// level there.
pub(crate) struct RoadStart {
    pub point: DVec2,
    pub level: f32,
    pub width: f32,
    /// How narrow the road may get through deep cuttings.
    pub min_width: f32,
}

/// An entry in the route search queue: lowest cost first.
#[derive(PartialEq)]
struct Entry {
    cost: f64,
    state: usize,
}

impl Eq for Entry {}

impl Ord for Entry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .total_cmp(&self.cost)
            .then(other.state.cmp(&self.state))
    }
}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The roads of a region, and where they end.
pub(crate) struct Routes {
    pub roads: Vec<FeatureLine>,
    /// The hub every road leads to, and the road level there, if any road
    /// reached it.
    pub end: Option<(DVec2, f32)>,
}

/// Links every start to a hub near the middle of the region, or to a road
/// already linked, over `grid` (heights of the bent terrain, which
/// `ground` gives at any point). Roads cross `rivers` as seldom as they
/// reasonably can, and `grades` gives the road grades at a point. Roads
/// stay at least [`FEATURE_CLEARANCE`] from the border, apart from the
/// straight first leg from each start.
pub(crate) fn route(
    grid: &Grid,
    starts: &[RoadStart],
    rivers: &[FeatureLine],
    grades: &dyn Fn(DVec2) -> RoadGrades,
    ground: &dyn Fn(DVec2) -> f32,
) -> Routes {
    let allowed: Vec<bool> = (0..grid.heights.len())
        .map(|i| grid.inside[i] && grid.clearance[i] > FEATURE_CLEARANCE)
        .collect();
    let Some(hub) = hub(grid, &allowed) else {
        return Routes {
            roads: Vec::new(),
            end: None,
        };
    };
    let cell_grades: Vec<RoadGrades> = (0..grid.heights.len())
        .map(|i| grades(grid.centre(i)))
        .collect();
    let river = water(grid, rivers);
    let bridges = Bridges::new(grid, rivers);
    let terrain = Terrain {
        grid,
        allowed: &allowed,
        river: &river,
        bridges: &bridges,
        grades: &cell_grades,
    };

    let mut network = vec![false; grid.heights.len()];
    network[hub] = true;
    let mut roads: Vec<FeatureLine> = Vec::new();
    for start in starts {
        let Some(first) = first_cell(grid, &allowed, start.point) else {
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
        let ahead = network_distance(grid, &network);
        let routed = loop {
            if refused.len() > MAX_REFUSED_JOINS {
                break crowded_route;
            }
            // The road may join the network only where it can reach the
            // network's level at its steepest grade (with a little to spare
            // for the smoothing).
            let joinable = |cell: usize, reach: f64| {
                !refused.contains(&cell)
                    && level_on_roads(&roads, grid.centre(cell)).is_none_or(|level| {
                        f64::from((level - start.level).abs()) <= 0.9 * (reach + lead_in)
                    })
            };
            let Some(cells) = search(&terrain, &network, &ahead, &blocked, first, &joinable) else {
                break crowded_route;
            };

            // Points: the crossing end, then the route, smoothed.
            let mut points = vec![start.point];
            points.extend(cells.iter().map(|&c| grid.centre(c)));
            let flat = vec![0.0; points.len()];
            let (points, _) = chaikin(&points, &flat, 2);

            // Levels: follow the smoothed ground, from the crossing's level
            // to the network's level where the road joins it.
            let heights: Vec<f32> = points.iter().map(|&p| ground(p)).collect();
            let steepest: Vec<f64> = points
                .iter()
                .enumerate()
                .map(|(i, &p)| {
                    let middle = if i == 0 {
                        p
                    } else {
                        p.lerp(points[i - 1], 0.5)
                    };
                    f64::from(grades(middle).max_grade)
                })
                .collect();
            let join = *cells.last().expect("route has cells");
            let end_level = level_on_roads(&roads, grid.centre(join));
            let target = smooth(&heights);
            let across = across_road(&points, &heights, f64::from(start.width) / 2.0, ground);
            let Some(levels) =
                grade_limited(&points, &across, &target, start.level, end_level, &steepest)
            else {
                refused.push(join);
                continue;
            };

            // Where two legs crowd each other, route again around that
            // place, keeping this route in case no other is found.
            let crowding = crowded(&points, spacing);
            if crowding.is_empty() || retries >= MAX_CROWDED_RETRIES {
                break Some((cells, points, levels, across));
            }
            for &j in &crowding {
                block_around(grid, &mut blocked, points[j], 0.5 * spacing, first);
            }
            retries += 1;
            crowded_route = Some((cells, points, levels, across));
        };
        let Some((cells, points, levels, across)) = routed else {
            continue;
        };
        for &c in &cells {
            network[c] = true;
        }
        roads.push(FeatureLine {
            kind: FeatureKind::Road,
            widths: widths(&points, &across, &levels, start.width, start.min_width),
            points,
            levels,
            depth: 0.0,
        });
    }
    let end = level_on_roads(&roads, grid.centre(hub)).map(|level| (grid.centre(hub), level));
    Routes { roads, end }
}

/// How steeply a road along `points` has its banks start at each point, as
/// rise over run (see [`bank_for`]), from the slope of `ground` around each
/// point, averaged along the road. Over its first [`BANK_EASE`] metres the
/// road eases in from the gentlest banks, which its crossing piece has.
pub(crate) fn banks(points: &[DVec2], ground: &dyn Fn(DVec2) -> f32) -> Vec<f32> {
    // The ground's slope around each point.
    let slope: Vec<f64> = points
        .iter()
        .map(|&p| {
            let (dx, dz) = (DVec2::new(SLOPE_RADIUS, 0.0), DVec2::new(0.0, SLOPE_RADIUS));
            let gx = f64::from(ground(p + dx) - ground(p - dx)) / (2.0 * SLOPE_RADIUS);
            let gz = f64::from(ground(p + dz) - ground(p - dz)) / (2.0 * SLOPE_RADIUS);
            gx.hypot(gz)
        })
        .collect();

    // Averaged along the road, then eased in from the start.
    let gentle = bank_for(0.0);
    eased(
        points,
        &along_mean(points, &slope, SLOPE_RADIUS),
        |mean, ease| (gentle + (bank_for(mean) - gentle) * ease) as f32,
    )
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

/// For each cell, the least cost of any route from it to the nearest cell
/// of `network`: the length of the shortest path over the grid's moves,
/// with the last step onto the network at [`REUSE_COST`] of its length, as
/// the route search may count it. Every step the search takes costs at
/// least that much, so this never overestimates.
fn network_distance(grid: &Grid, network: &[bool]) -> Vec<f64> {
    let mut distance = vec![f64::INFINITY; network.len()];
    let mut queue = BinaryHeap::new();
    for (cell, _) in network.iter().enumerate().filter(|(_, on)| **on) {
        distance[cell] = 0.0;
        queue.push(Entry {
            cost: 0.0,
            state: cell,
        });
    }
    while let Some(Entry {
        cost: d,
        state: cell,
    }) = queue.pop()
    {
        if d > distance[cell] {
            continue;
        }
        // Routes run from `next` to `cell`, so the step's discount depends
        // on whether `cell` is on the network.
        let share = if network[cell] { REUSE_COST } else { 1.0 };
        for (next, length) in grid.neighbours(cell) {
            let total = d + share * length;
            if total < distance[next] {
                distance[next] = total;
                queue.push(Entry {
                    cost: total,
                    state: next,
                });
            }
        }
    }
    distance
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

/// Every point of `points` that comes within `spacing` of an earlier part
/// of the same line more than twice `spacing` back along it: where two legs
/// of a road crowd each other. A hairpin's own bend is not counted.
fn crowded(points: &[DVec2], spacing: f64) -> Vec<usize> {
    // Distance along the line to each point.
    let mut along = vec![0.0f64; points.len()];
    for i in 1..points.len() {
        along[i] = along[i - 1] + points[i].distance(points[i - 1]);
    }

    // Points bucketed by square, `spacing` across, so only nearby squares
    // are searched.
    let key = |p: DVec2| {
        (
            (p.x / spacing).floor() as i64,
            (p.y / spacing).floor() as i64,
        )
    };
    let mut squares: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    let mut out = Vec::new();
    for (j, &p) in points.iter().enumerate() {
        let (kx, kz) = key(p);
        let near = (-1..=1)
            .flat_map(|dx| (-1..=1).map(move |dz| (kx + dx, kz + dz)))
            .filter_map(|k| squares.get(&k))
            .flatten()
            .any(|&i| along[j] - along[i] > 2.0 * spacing && points[i].distance(p) < spacing);
        if near {
            out.push(j);
        }
        squares.entry((kx, kz)).or_default().push(j);
    }
    out
}

/// The lowest and highest ground across a road at each of `points`: at its
/// centre (`centre`, the ground there) and at its two edges, `half_width`
/// to either side.
fn across_road(
    points: &[DVec2],
    centre: &[f32],
    half_width: f64,
    ground: &dyn Fn(DVec2) -> f32,
) -> Vec<(f32, f32)> {
    let last = points.len() - 1;
    (0..points.len())
        .map(|i| {
            let along = points[(i + 1).min(last)] - points[i.saturating_sub(1)];
            let side = along.perp().normalize_or_zero() * half_width;
            let (left, right) = (ground(points[i] + side), ground(points[i] - side));
            (
                centre[i].min(left).min(right),
                centre[i].max(left).max(right),
            )
        })
        .collect()
}

/// Road levels along `points`, starting at `start` and, if given, ending at
/// `end`, that keep the deepest fill, and the deepest cut divided by
/// [`CUT_SHARE`], as small as the grades allow: where the road can't follow
/// a hump or a dip, it cuts deep sooner than it builds a high embankment.
/// `across[i]` is the lowest and highest ground across the road at point
/// `i`; fill is measured from the lowest, cut from the highest, so on a
/// side slope the road sinks into the hillside rather than standing out
/// on a fill. Within that, each level keeps as close to `target` (the
/// smoothed ground along the road's centre) as it can. `steepest[i]` is
/// the steepest grade allowed from point `i - 1` to point `i`
/// (`steepest[0]` is unused).
///
/// The two end levels are fixed and don't count. Returns `None` if the two
/// ends are too far apart in height for the grades.
fn grade_limited(
    points: &[DVec2],
    across: &[(f32, f32)],
    target: &[f32],
    start: f32,
    end: Option<f32>,
    steepest: &[f64],
) -> Option<Vec<f32>> {
    // How far the road may rise or fall along each step.
    let rise: Vec<f64> = (0..points.len())
        .map(|i| {
            if i == 0 {
                0.0
            } else {
                steepest[i] * points[i].distance(points[i - 1])
            }
        })
        .collect();
    let (start, end) = (f64::from(start), end.map(f64::from));
    let last = points.len() - 1;
    if let Some(e) = end
        && (e - start).abs() > rise.iter().sum::<f64>()
    {
        return None;
    }

    // The levels each point can take with every gap at most `gap`, passing
    // forward from the start; `None` if the end can't be reached.
    let bands = |gap: f64| -> Option<Vec<(f64, f64)>> {
        let mut out = vec![(start, start); points.len()];
        for i in 1..points.len() {
            let (lo, hi) = (out[i - 1].0 - rise[i], out[i - 1].1 + rise[i]);
            let (lo, hi) = match (i == last, end) {
                (true, Some(e)) => (lo.max(e), hi.min(e)),
                _ => {
                    let (low, high) = (f64::from(across[i].0), f64::from(across[i].1));
                    (lo.max(high - CUT_SHARE * gap), hi.min(low + gap))
                }
            };
            if lo > hi {
                return None;
            }
            out[i] = (lo, hi);
        }
        Some(out)
    };

    // The smallest gap that works, by bisection: the end is reachable, so
    // a gap as large as the ground's whole height range plus the climb
    // always works.
    let (lo_ground, hi_ground) = across.iter().fold((start, start), |(lo, hi), &(l, h)| {
        (lo.min(f64::from(l)), hi.max(f64::from(h)))
    });
    let mut wide = hi_ground - lo_ground + rise.iter().sum::<f64>() + 1.0;
    let mut narrow = 0.0;
    let mut best = bands(wide).expect("a wide enough gap always works");
    for _ in 0..40 {
        let gap = 0.5 * (narrow + wide);
        match bands(gap) {
            Some(b) => {
                wide = gap;
                best = b;
            }
            None => narrow = gap,
        }
    }

    // Levels from the end backward: each as near its target as its band
    // and the grade to the next level allow. Every band was reached from
    // the one before, so the choice is never empty.
    let mut levels = vec![0.0f64; points.len()];
    levels[last] = f64::from(target[last]).clamp(best[last].0, best[last].1);
    for i in (0..last).rev() {
        let lo = best[i].0.max(levels[i + 1] - rise[i + 1]);
        let hi = best[i].1.min(levels[i + 1] + rise[i + 1]);
        levels[i] = f64::from(target[i]).clamp(lo, hi.max(lo));
    }
    Some(levels.into_iter().map(|l| l as f32).collect())
}

/// The flattest allowed cell near the middle of the region.
fn hub(grid: &Grid, allowed: &[bool]) -> Option<usize> {
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

/// Which cells of `grid` the lines of `rivers` pass through.
fn water(grid: &Grid, rivers: &[FeatureLine]) -> Vec<bool> {
    let mut water = vec![false; grid.heights.len()];
    for river in rivers {
        for w in river.points.windows(2) {
            let steps = (w[0].distance(w[1]) / (WATER_STEP * grid.cell))
                .ceil()
                .max(1.0) as usize;
            for k in 0..=steps {
                if let Some(c) = grid.cell_of(w[0].lerp(w[1], k as f64 / steps as f64)) {
                    water[c] = true;
                }
            }
        }
    }
    water
}

/// The moves over a grid that cross a river, from one cell's centre to the
/// next cell's. Unlike the water cells, these include steps that jump over
/// a narrow river without landing in it.
struct Bridges {
    /// For each cell, which of its moves cross a river: bit `k` for
    /// direction `k` of [`Grid::moves`].
    moves: Vec<u16>,
    /// The bridge cost of each crossing move, by `cell * DIRECTIONS + k`.
    costs: HashMap<usize, f64>,
}

impl Bridges {
    /// The moves over `grid` that cross a line of `rivers`.
    fn new(grid: &Grid, rivers: &[FeatureLine]) -> Self {
        // The longest move is a knight's move, about 2.24 cells.
        let reach = 2.5 * grid.cell;
        let mut bridges = Self {
            moves: vec![0; grid.heights.len()],
            costs: HashMap::new(),
        };
        for river in rivers {
            for w in river.points.windows(2) {
                let along = (w[1] - w[0]).normalize_or_zero();
                for cell in grid.cells_near(w[0], w[1], reach) {
                    let from = grid.centre(cell);
                    for (next, _, k) in grid.moves(cell) {
                        if !segments_meet(from, grid.centre(next), w[0], w[1]) {
                            continue;
                        }
                        // Where the move meets two pieces of river at once,
                        // the dearer bridge counts.
                        let sine = direction(k).perp_dot(along).abs().max(MIN_BRIDGE_SINE);
                        let cost = bridges.costs.entry(cell * DIRECTIONS + k).or_default();
                        *cost = cost.max(BRIDGE_COST / sine);
                        bridges.moves[cell] |= 1 << k;
                    }
                }
            }
        }
        bridges
    }

    /// The bridge cost of move `k` from `cell`, if it crosses a river.
    fn cost(&self, cell: usize, k: usize) -> Option<f64> {
        (self.moves[cell] & (1 << k) != 0).then(|| self.costs[&(cell * DIRECTIONS + k)])
    }
}

/// Whether segments `a`–`b` and `c`–`d` cross or touch.
fn segments_meet(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let side = |p: DVec2, q: DVec2, r: DVec2| (q - p).perp_dot(r - p);
    side(c, d, a) * side(c, d, b) <= 0.0 && side(a, b, c) * side(a, b, d) <= 0.0
}

/// What the route search knows about each cell of a grid, apart from the
/// road network built so far.
struct Terrain<'a> {
    grid: &'a Grid,
    /// Whether a road may pass through the cell.
    allowed: &'a [bool],
    /// Whether the cell has water.
    river: &'a [bool],
    /// The moves that cross a river.
    bridges: &'a Bridges,
    /// The road grades in the cell.
    grades: &'a [RoadGrades],
}

/// The cheapest route over `terrain` from `start` to a cell of `network`
/// where `joinable(cell, reach)` allows joining, as cells from `start` to
/// the joining cell, avoiding `blocked` cells. `reach` is how far the road
/// could have climbed or fallen on its way there, at each step's steepest
/// grade. Other network cells may be crossed. The route never turns more
/// than [`MAX_TURN`] from one step to the next.
///
/// `ahead[cell]` must never be more than the cost from `cell` to the first
/// network cell (see [`network_distance`]); the search uses it to head for
/// the network without missing the cheapest route.
fn search(
    terrain: &Terrain,
    network: &[bool],
    ahead: &[f64],
    blocked: &[bool],
    start: usize,
    joinable: &dyn Fn(usize, f64) -> bool,
) -> Option<Vec<usize>> {
    let Terrain {
        grid,
        allowed,
        river,
        bridges,
        grades,
    } = *terrain;

    // A search state is a cell and the direction the route entered it by;
    // the start has no direction (`DIRECTIONS`). The queue is ordered by
    // the cost so far plus the least cost still ahead.
    let headings = DIRECTIONS + 1;
    let n = allowed.len() * headings;
    let mut turnable = [[false; DIRECTIONS]; DIRECTIONS];
    for (a, row) in turnable.iter_mut().enumerate() {
        for (b, ok) in row.iter_mut().enumerate() {
            *ok = direction(a).angle_to(direction(b)).abs() <= MAX_TURN + 1e-9;
        }
    }
    let mut cost = vec![f64::INFINITY; n];
    let mut run = vec![0.0f64; n];
    let mut previous = vec![usize::MAX; n];
    let mut queue = BinaryHeap::new();
    let first = start * headings + DIRECTIONS;
    cost[first] = 0.0;
    queue.push(Entry {
        cost: ahead[start],
        state: first,
    });
    while let Some(Entry {
        cost: estimate,
        state,
    }) = queue.pop()
    {
        let (cell, heading) = (state / headings, state % headings);
        let c = cost[state];
        if estimate > c + ahead[cell] {
            continue;
        }
        if network[cell] && joinable(cell, run[state]) {
            let mut states = vec![state];
            while states[states.len() - 1] != first {
                states.push(previous[states[states.len() - 1]]);
            }
            states.reverse();
            return Some(states.into_iter().map(|s| s / headings).collect());
        }
        for (next, length, k) in grid.moves(cell) {
            if !allowed[next] || blocked[next] {
                continue;
            }
            if heading != DIRECTIONS && !turnable[heading][k] {
                continue;
            }
            let next_state = next * headings + k;
            // The step's grades: the mean of its two cells'.
            let (a, b) = (grades[cell], grades[next]);
            let preferred = 0.5 * f64::from(a.grade + b.grade);
            let steepest = 0.5 * f64::from(a.max_grade + b.max_grade);
            let grade = f64::from(grid.heights[next] - grid.heights[cell]).abs() / length;
            let over_preferred = (grade / preferred - 1.0).max(0.0);
            let over_steepest = (grade / steepest - 1.0).max(0.0);
            // Cutting and filling: about half the climb beyond the steepest
            // grade.
            let carve = 0.5 * (grade - steepest).max(0.0) * length / CARVE_SCALE;
            let mut step = length
                * (1.0
                    + GRADE_COST * grade
                    + PREFERRED_COST * over_preferred * over_preferred
                    + STEEP_COST * over_steepest * over_steepest
                    + CARVE_COST * carve * carve);
            if river[next] {
                step *= RIVER_COST;
            }
            // Only roads this one could join are cheap to follow; others it
            // may only cross.
            let here = run[state] + steepest * length;
            if network[next] && joinable(next, here) {
                step *= REUSE_COST;
            }
            // A step across a river needs a bridge.
            if let Some(bridge) = bridges.cost(cell, k) {
                step += bridge;
            }
            let total = c + step;
            if total < cost[next_state] {
                cost[next_state] = total;
                run[next_state] = here;
                previous[next_state] = state;
                queue.push(Entry {
                    cost: total + ahead[next],
                    state: next_state,
                });
            }
        }
    }
    None
}

/// A moving average over [`LEVEL_SMOOTHING`] points either side.
fn smooth(values: &[f32]) -> Vec<f32> {
    let last = values.len() as isize - 1;
    (0..values.len() as isize)
        .map(|i| {
            let r = LEVEL_SMOOTHING as isize;
            let sum: f32 = (-r..=r)
                .map(|k| values[(i + k).clamp(0, last) as usize])
                .sum();
            sum / (2 * r + 1) as f32
        })
        .collect()
}

/// The level of the nearest point on `roads` to `p`, if there are roads.
fn level_on_roads(roads: &[FeatureLine], p: DVec2) -> Option<f32> {
    roads
        .iter()
        .flat_map(|r| r.points.iter().zip(&r.levels))
        .min_by(|a, b| a.0.distance(p).total_cmp(&b.0.distance(p)))
        .map(|(_, &l)| l)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            RoadStart {
                point: DVec2::new(1000.0, 70.0),
                level: 10.0,
                width: 6.0,
                min_width: 4.0,
            },
            RoadStart {
                point: DVec2::new(70.0, 1500.0),
                level: 1.0,
                width: 6.0,
                min_width: 4.0,
            },
        ];
        let grades = |_| RoadGrades {
            grade: 0.04,
            max_grade: 0.08,
        };
        let Routes { roads, end } = route(&grid, &starts, &[], &grades, &ground);
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
    /// in the middle, past `river`.
    fn road_past(river: Vec<DVec2>) -> (FeatureLine, FeatureLine) {
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
        let start = RoadStart {
            point: DVec2::new(1000.0, 70.0),
            level: 0.0,
            width: 6.0,
            min_width: 4.0,
        };
        let grades = |_| RoadGrades {
            grade: 0.04,
            max_grade: 0.08,
        };
        let Routes { mut roads, .. } = route(
            &grid,
            &[start],
            std::slice::from_ref(&river),
            &grades,
            &ground,
        );
        assert_eq!(roads.len(), 1);
        (roads.remove(0), river)
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
    fn levels_keep_to_each_steps_grade() {
        // Flat ground, a 20 m climb, then flat: the road may climb 0.1 on the
        // first half and 0.3 on the second.
        let points: Vec<DVec2> = (0..=10)
            .map(|i| DVec2::new(10.0 * f64::from(i), 0.0))
            .collect();
        let ground = vec![0.0; points.len()];
        let across = vec![(0.0, 0.0); points.len()];
        let steepest: Vec<f64> = (0..=10).map(|i| if i <= 5 { 0.1 } else { 0.3 }).collect();
        let levels = grade_limited(&points, &across, &ground, 0.0, Some(20.0), &steepest).unwrap();
        for i in 1..levels.len() {
            let grade = f64::from(levels[i] - levels[i - 1]).abs() / 10.0;
            assert!(grade <= steepest[i] * 1.001, "step {i}: grade {grade}");
        }
        assert_eq!(*levels.last().unwrap(), 20.0);
        // 5 m on the gentle half and 15 m on the steep half is all there is.
        assert!(grade_limited(&points, &across, &ground, 0.0, Some(20.5), &steepest).is_none());
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

    #[test]
    fn levels_sink_into_a_side_slope() {
        // A level road along a side slope: the ground across it runs from
        // 0 m at its downhill edge to 6 m at its uphill edge, 3 m at the
        // centre. Three metres of cut count as one of fill, so the road sits
        // at 1.5 m: 1.5 m of fill under the downhill edge and 4.5 m of cut at
        // the uphill edge, not on the centre's 3 m with 3 m of fill.
        let points: Vec<DVec2> = (0..=10)
            .map(|i| DVec2::new(10.0 * f64::from(i), 0.0))
            .collect();
        let across = vec![(0.0, 6.0); points.len()];
        let centre = vec![3.0; points.len()];
        let steepest = vec![0.1; points.len()];
        let levels = grade_limited(&points, &across, &centre, 1.5, None, &steepest).unwrap();
        for (i, l) in levels.iter().enumerate() {
            assert!((l - 1.5).abs() < 0.01, "point {i}: level {l}");
        }
    }

    #[test]
    fn crowded_legs_are_found_but_a_hairpin_bend_is_not() {
        // Up the z axis, round a hairpin, and back down 20 m away: legs
        // 20 m apart are fine with a spacing of 18 m, not with 25 m.
        let mut points: Vec<DVec2> = (0..=10)
            .map(|i| DVec2::new(0.0, 10.0 * f64::from(i)))
            .collect();
        points.extend((1..10).map(|i| {
            let a = std::f64::consts::PI * f64::from(i) / 10.0;
            DVec2::new(10.0 - 10.0 * a.cos(), 100.0 + 10.0 * a.sin())
        }));
        points.extend((0..=10).map(|i| DVec2::new(20.0, 100.0 - 10.0 * f64::from(i))));
        assert!(crowded(&points, 18.0).is_empty());
        let crowding = crowded(&points, 25.0);
        assert!(!crowding.is_empty(), "legs 20 m apart crowd at 25 m");
        assert!(crowding.iter().all(|&j| points[j].x == 20.0));
    }

    #[test]
    fn levels_cut_a_hump_sooner_than_fill_beside_it() {
        // Flat ground with a 10 m spike in the middle, a road free to end
        // anywhere, and a grade of 0.1 over 10 m steps: the road can rise
        // only 1 m per step, so it climbs early and cuts the spike's top.
        let points: Vec<DVec2> = (0..=20)
            .map(|i| DVec2::new(10.0 * f64::from(i), 0.0))
            .collect();
        let mut ground = vec![0.0; points.len()];
        ground[10] = 10.0;
        let steepest = vec![0.1; points.len()];
        let across: Vec<(f32, f32)> = ground.iter().map(|&g| (g, g)).collect();
        let levels = grade_limited(&points, &across, &ground, 0.0, None, &steepest).unwrap();
        let (cut, fill) = ground
            .iter()
            .zip(&levels)
            .fold((0.0f32, 0.0f32), |(c, f), (g, l)| {
                (c.max(g - l), f.max(l - g))
            });
        // At level L on the spike, its neighbours can be at L − 1: a cut of
        // 10 − L and a fill of L − 1. Three metres of cut count as one of
        // fill, so L = 3.25: a 6.75 m cut and a 2.25 m fill. Following the
        // ground and then clamping would leave a 9 m cut.
        assert!((cut - 6.75).abs() < 0.01, "cut {cut}");
        assert!((fill - 2.25).abs() < 0.01, "fill {fill}");
        for i in 1..levels.len() {
            assert!((levels[i] - levels[i - 1]).abs() <= 1.0 + 1e-4, "step {i}");
        }
    }
}
