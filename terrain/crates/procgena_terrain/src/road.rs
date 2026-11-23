//! Routing roads over a region's grid and setting their levels.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use glam::DVec2;

use crate::carve::{FeatureKind, FeatureLine, bank_for, chaikin};
use crate::seam::smootherstep;
use crate::grid::{DIRECTIONS, Grid, direction};
use crate::options::RoadGrades;
use crate::ports::FEATURE_CLEARANCE;

/// Extra cost per unit of grade beyond the road's preferred grade, squared
/// (in units of that grade).
const PREFERRED_COST: f64 = 40.0;

/// Extra cost per unit of grade beyond the road's steepest grade, squared
/// (in units of that grade), on top of [`PREFERRED_COST`].
const STEEP_COST: f64 = 400.0;

/// Cost factor for crossing a river cell.
const RIVER_COST: f64 = 6.0;

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
/// `ground` gives at any point). `river` marks cells with water, and
/// `grades` gives the road grades at a point. Roads stay at least
/// [`FEATURE_CLEARANCE`] from the border, apart from the straight first leg
/// from each start.
pub(crate) fn route(
    grid: &Grid,
    starts: &[RoadStart],
    river: &[bool],
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
    let terrain = Terrain {
        grid,
        allowed: &allowed,
        river,
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
            let Some(cells) = search(&terrain, &network, &blocked, first, &joinable) else {
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
            let Some(levels) =
                grade_limited(&points, &heights, &target, start.level, end_level, &steepest)
            else {
                refused.push(join);
                continue;
            };

            // Where two legs crowd each other, route again around that
            // place, keeping this route in case no other is found.
            match crowded(&points, spacing) {
                Some(j) if retries < MAX_CROWDED_RETRIES => {
                    block_around(grid, &mut blocked, points[j], 0.5 * spacing, first);
                    retries += 1;
                    crowded_route = Some((cells, points, levels));
                }
                _ => break Some((cells, points, levels)),
            }
        };
        let Some((cells, points, levels)) = routed else {
            continue;
        };
        for &c in &cells {
            network[c] = true;
        }
        roads.push(FeatureLine {
            kind: FeatureKind::Road,
            points,
            levels,
            width: start.width,
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

    // Averaged over the points within `SLOPE_RADIUS` along the road.
    let mut along = vec![0.0f64; points.len()];
    for i in 1..points.len() {
        along[i] = along[i - 1] + points[i].distance(points[i - 1]);
    }
    let (mut lo, mut hi) = (0, 0);
    (0..points.len())
        .map(|i| {
            while along[i] - along[lo] > SLOPE_RADIUS {
                lo += 1;
            }
            while hi + 1 < points.len() && along[hi + 1] - along[i] <= SLOPE_RADIUS {
                hi += 1;
            }
            let mean = slope[lo..=hi].iter().sum::<f64>() / (hi - lo + 1) as f64;
            let ease = smootherstep((along[i] / BANK_EASE).min(1.0));
            let gentle = bank_for(0.0);
            (gentle + (bank_for(mean) - gentle) * ease) as f32
        })
        .collect()
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

/// The first point of `points` that comes within `spacing` of an earlier
/// part of the same line more than twice `spacing` back along it: where
/// two legs of a road crowd each other. A hairpin's own bend is not
/// counted.
fn crowded(points: &[DVec2], spacing: f64) -> Option<usize> {
    // Distance along the line to each point.
    let mut along = vec![0.0f64; points.len()];
    for i in 1..points.len() {
        along[i] = along[i - 1] + points[i].distance(points[i - 1]);
    }

    // Points bucketed by square, `spacing` across, so only nearby squares
    // are searched.
    let key = |p: DVec2| ((p.x / spacing).floor() as i64, (p.y / spacing).floor() as i64);
    let mut squares: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (j, &p) in points.iter().enumerate() {
        let (kx, kz) = key(p);
        let near = (-1..=1)
            .flat_map(|dx| (-1..=1).map(move |dz| (kx + dx, kz + dz)))
            .filter_map(|k| squares.get(&k))
            .flatten()
            .any(|&i| along[j] - along[i] > 2.0 * spacing && points[i].distance(p) < spacing);
        if near {
            return Some(j);
        }
        squares.entry((kx, kz)).or_default().push(j);
    }
    None
}

/// Road levels along `points`, starting at `start` and, if given, ending at
/// `end`, that keep the deepest fill, and the deepest cut divided by
/// [`CUT_SHARE`], as small as the grades allow: where the road can't follow
/// a hump or a dip, it cuts deep sooner than it builds a high embankment.
/// Within that, each level keeps as close to `target` (a smoothed version
/// of `ground`) as it can. `steepest[i]` is the steepest grade allowed from
/// point `i - 1` to point `i` (`steepest[0]` is unused).
///
/// The two end levels are fixed and don't count. Returns `None` if the two
/// ends are too far apart in height for the grades.
fn grade_limited(
    points: &[DVec2],
    ground: &[f32],
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
                    let g = f64::from(ground[i]);
                    (lo.max(g - CUT_SHARE * gap), hi.min(g + gap))
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
    let (lo_ground, hi_ground) = ground
        .iter()
        .fold((start, start), |(lo, hi), &g| (lo.min(f64::from(g)), hi.max(f64::from(g))));
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

/// What the route search knows about each cell of a grid, apart from the
/// road network built so far.
struct Terrain<'a> {
    grid: &'a Grid,
    /// Whether a road may pass through the cell.
    allowed: &'a [bool],
    /// Whether the cell has water.
    river: &'a [bool],
    /// The road grades in the cell.
    grades: &'a [RoadGrades],
}

/// The cheapest route over `terrain` from `start` to a cell of `network`
/// where `joinable(cell, reach)` allows joining, as cells from `start` to
/// the joining cell, avoiding `blocked` cells. `reach` is how far the road
/// could have climbed or fallen on its way there, at each step's steepest
/// grade. Other network cells may be crossed. The route never turns more
/// than [`MAX_TURN`] from one step to the next.
fn search(
    terrain: &Terrain,
    network: &[bool],
    blocked: &[bool],
    start: usize,
    joinable: &dyn Fn(usize, f64) -> bool,
) -> Option<Vec<usize>> {
    let Terrain {
        grid,
        allowed,
        river,
        grades,
    } = *terrain;

    // A search state is a cell and the direction the route entered it by;
    // the start has no direction (`DIRECTIONS`).
    let headings = DIRECTIONS + 1;
    let n = allowed.len() * headings;
    let mut cost = vec![f64::INFINITY; n];
    let mut run = vec![0.0f64; n];
    let mut previous = vec![usize::MAX; n];
    let mut queue = BinaryHeap::new();
    let first = start * headings + DIRECTIONS;
    cost[first] = 0.0;
    queue.push(Entry {
        cost: 0.0,
        state: first,
    });
    while let Some(Entry { cost: c, state }) = queue.pop() {
        if c > cost[state] {
            continue;
        }
        let (cell, heading) = (state / headings, state % headings);
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
            if heading != DIRECTIONS
                && direction(heading).angle_to(direction(k)).abs() > MAX_TURN + 1e-9
            {
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
            let total = c + step;
            if total < cost[next_state] {
                cost[next_state] = total;
                run[next_state] = here;
                previous[next_state] = state;
                queue.push(Entry {
                    cost: total,
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
            },
            RoadStart {
                point: DVec2::new(70.0, 1500.0),
                level: 1.0,
                width: 6.0,
            },
        ];
        let river = vec![false; grid.heights.len()];
        let grades = |_| RoadGrades {
            grade: 0.04,
            max_grade: 0.08,
        };
        let Routes { roads, end } = route(&grid, &starts, &river, &grades, &ground);
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

    #[test]
    fn levels_keep_to_each_steps_grade() {
        // Flat ground, a 20 m climb, then flat: the road may climb 0.1 on the
        // first half and 0.3 on the second.
        let points: Vec<DVec2> = (0..=10)
            .map(|i| DVec2::new(10.0 * f64::from(i), 0.0))
            .collect();
        let ground = vec![0.0; points.len()];
        let steepest: Vec<f64> = (0..=10).map(|i| if i <= 5 { 0.1 } else { 0.3 }).collect();
        let levels = grade_limited(&points, &ground, &ground, 0.0, Some(20.0), &steepest).unwrap();
        for i in 1..levels.len() {
            let grade = f64::from(levels[i] - levels[i - 1]).abs() / 10.0;
            assert!(grade <= steepest[i] * 1.001, "step {i}: grade {grade}");
        }
        assert_eq!(*levels.last().unwrap(), 20.0);
        // 5 m on the gentle half and 15 m on the steep half is all there is.
        assert!(grade_limited(&points, &ground, &ground, 0.0, Some(20.5), &steepest).is_none());
    }

    #[test]
    fn crowded_legs_are_found_but_a_hairpin_bend_is_not() {
        // Up the z axis, round a hairpin, and back down 20 m away: legs
        // 20 m apart are fine with a spacing of 18 m, not with 25 m.
        let mut points: Vec<DVec2> = (0..=10).map(|i| DVec2::new(0.0, 10.0 * f64::from(i))).collect();
        points.extend((1..10).map(|i| {
            let a = std::f64::consts::PI * f64::from(i) / 10.0;
            DVec2::new(10.0 - 10.0 * a.cos(), 100.0 + 10.0 * a.sin())
        }));
        points.extend((0..=10).map(|i| DVec2::new(20.0, 100.0 - 10.0 * f64::from(i))));
        assert_eq!(crowded(&points, 18.0), None);
        let j = crowded(&points, 25.0).expect("legs 20 m apart crowd at 25 m");
        assert_eq!(points[j].x, 20.0);
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
        let levels = grade_limited(&points, &ground, &ground, 0.0, None, &steepest).unwrap();
        let (cut, fill) = ground
            .iter()
            .zip(&levels)
            .fold((0.0f32, 0.0f32), |(c, f), (g, l)| (c.max(g - l), f.max(l - g)));
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
