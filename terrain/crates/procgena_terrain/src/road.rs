//! Routing roads over a region's grid and setting their levels.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use glam::DVec2;

use crate::carve::{FeatureKind, FeatureLine, chaikin};
use crate::grid::Grid;
use crate::ports::FEATURE_CLEARANCE;

/// Extra cost per unit of grade beyond the road's steepest grade, squared.
const STEEP_COST: f64 = 40.0;

/// Cost factor for crossing a river cell.
const RIVER_COST: f64 = 6.0;

/// Cost factor for travelling along an existing road, so roads share.
const REUSE_COST: f64 = 0.4;

/// Cost per unit of grade, so roads prefer level ground.
const GRADE_COST: f64 = 4.0;

/// How many joins a road may find out of reach before it gives up.
const MAX_REFUSED_JOINS: usize = 8;

/// How many points either side are averaged when smoothing road heights.
const LEVEL_SMOOTHING: usize = 3;

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
    cell: usize,
}

impl Eq for Entry {}

impl Ord for Entry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .total_cmp(&self.cost)
            .then(other.cell.cmp(&self.cell))
    }
}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Links every start to a hub near the middle of the region, or to a road
/// already linked, over `grid` (heights of the bent terrain). `river` marks
/// cells with water. Roads stay at least [`FEATURE_CLEARANCE`] from the
/// border, apart from the straight first leg from each start.
pub(crate) fn route(
    grid: &Grid,
    starts: &[RoadStart],
    river: &[bool],
    max_grade: f32,
) -> Vec<FeatureLine> {
    let allowed: Vec<bool> = (0..grid.heights.len())
        .map(|i| grid.inside[i] && grid.clearance[i] > FEATURE_CLEARANCE)
        .collect();
    let Some(hub) = hub(grid, &allowed) else {
        return Vec::new();
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
        let lead_in = start.point.distance(grid.centre(first));
        let mut refused: Vec<usize> = Vec::new();
        let routed = loop {
            if refused.len() > MAX_REFUSED_JOINS {
                break None;
            }
            // The road may join the network only where it can reach the
            // network's level within its grade (with a little to spare for
            // the smoothing).
            let joinable = |cell: usize, run: f64| {
                !refused.contains(&cell)
                    && level_on_roads(&roads, grid.centre(cell)).is_none_or(|level| {
                        f64::from((level - start.level).abs())
                            <= 0.9 * f64::from(max_grade) * (run + lead_in)
                    })
            };
            let Some(cells) = search(
                grid,
                &allowed,
                &network,
                river,
                first,
                f64::from(max_grade),
                &joinable,
            ) else {
                break None;
            };

            // Points: the crossing end, then the route, smoothed.
            let mut points = vec![start.point];
            points.extend(cells.iter().map(|&c| grid.centre(c)));
            let flat = vec![0.0; points.len()];
            let (points, _) = chaikin(&points, &flat, 2);

            // Levels: follow the smoothed ground, from the crossing's level
            // to the network's level where the road joins it.
            let ground: Vec<f32> = points.iter().map(|&p| height_near(grid, p)).collect();
            let join = *cells.last().expect("route has cells");
            let end_level = level_on_roads(&roads, grid.centre(join));
            match grade_limited(
                &points,
                &smooth(&ground),
                start.level,
                end_level,
                f64::from(max_grade),
            ) {
                Some(levels) => break Some((cells, points, levels)),
                None => refused.push(join),
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
    roads
}

/// Road levels along `points` that follow `ground` as closely as the grade
/// allows, starting at `start` and, if given, ending at `end`.
///
/// Each level stays within `max_grade` of both ends (measured along the
/// road), then passes forward and backward hold every step within the
/// grade. Returns `None` if the two ends are too far apart in height for
/// the grade.
fn grade_limited(
    points: &[DVec2],
    ground: &[f32],
    start: f32,
    end: Option<f32>,
    max_grade: f64,
) -> Option<Vec<f32>> {
    // Distance along the road to each point.
    let mut run = vec![0.0f64; points.len()];
    for i in 1..points.len() {
        run[i] = run[i - 1] + points[i].distance(points[i - 1]);
    }
    let total = run[run.len() - 1];
    let (start, end_level) = (f64::from(start), end.map(f64::from));
    if let Some(e) = end_level
        && (e - start).abs() > max_grade * total
    {
        return None;
    }

    // Keep each level reachable from both ends.
    let mut levels: Vec<f64> = (0..points.len())
        .map(|i| {
            let mut lo = start - max_grade * run[i];
            let mut hi = start + max_grade * run[i];
            if let Some(e) = end_level {
                lo = lo.max(e - max_grade * (total - run[i]));
                hi = hi.min(e + max_grade * (total - run[i]));
            }
            f64::from(ground[i]).clamp(lo, hi)
        })
        .collect();
    levels[0] = start;
    if let Some(e) = end_level {
        levels[points.len() - 1] = e;
    }

    // Hold every step within the grade, forward then backward.
    for i in 1..levels.len() {
        let rise = max_grade * (run[i] - run[i - 1]);
        levels[i] = levels[i].clamp(levels[i - 1] - rise, levels[i - 1] + rise);
    }
    for i in (0..levels.len() - 1).rev() {
        let rise = max_grade * (run[i + 1] - run[i]);
        levels[i] = levels[i].clamp(levels[i + 1] - rise, levels[i + 1] + rise);
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

/// The cheapest route from `start` to a cell of `network` where
/// `joinable(cell, metres travelled)` allows joining, as cells from `start`
/// to the joining cell. Other network cells may be crossed.
fn search(
    grid: &Grid,
    allowed: &[bool],
    network: &[bool],
    river: &[bool],
    start: usize,
    max_grade: f64,
    joinable: &dyn Fn(usize, f64) -> bool,
) -> Option<Vec<usize>> {
    let n = allowed.len();
    let mut cost = vec![f64::INFINITY; n];
    let mut run = vec![0.0f64; n];
    let mut previous = vec![usize::MAX; n];
    let mut queue = BinaryHeap::new();
    cost[start] = 0.0;
    queue.push(Entry {
        cost: 0.0,
        cell: start,
    });
    while let Some(Entry { cost: c, cell }) = queue.pop() {
        if c > cost[cell] {
            continue;
        }
        if network[cell] && joinable(cell, run[cell]) {
            let mut cells = vec![cell];
            while cells[cells.len() - 1] != start {
                cells.push(previous[cells[cells.len() - 1]]);
            }
            cells.reverse();
            return Some(cells);
        }
        for (next, length) in grid.neighbours(cell) {
            if !allowed[next] {
                continue;
            }
            let grade = f64::from(grid.heights[next] - grid.heights[cell]).abs() / length;
            let steep = (grade / max_grade - 1.0).max(0.0);
            let mut step = length * (1.0 + GRADE_COST * grade + STEEP_COST * steep * steep);
            if river[next] {
                step *= RIVER_COST;
            }
            // Only roads this one could join are cheap to follow; others it
            // may only cross.
            let here = run[cell] + length;
            if network[next] && joinable(next, here) {
                step *= REUSE_COST;
            }
            let total = c + step;
            if total < cost[next] {
                cost[next] = total;
                run[next] = here;
                previous[next] = cell;
                queue.push(Entry {
                    cost: total,
                    cell: next,
                });
            }
        }
    }
    None
}

/// The grid height at the cell nearest to `p`, or at the nearest inside
/// cell along the way.
fn height_near(grid: &Grid, p: DVec2) -> f32 {
    match grid.cell_of(p) {
        Some(c) => grid.heights[c],
        None => (0..grid.heights.len())
            .filter(|&i| grid.inside[i])
            .min_by(|&a, &b| {
                grid.centre(a)
                    .distance(p)
                    .total_cmp(&grid.centre(b).distance(p))
            })
            .map_or(0.0, |c| grid.heights[c]),
    }
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
        let grid = Grid::new(&polygon, |p| {
            (0.01 * p.x + 80.0 * (-(p.distance(hill) / 200.0).powi(2)).exp()) as f32
        });
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
        let roads = route(&grid, &starts, &river, 0.08);
        assert_eq!(roads.len(), 2);
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
}
