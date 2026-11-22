//! Tracing rivers downhill over a region's grid.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use glam::DVec2;

use crate::grid::{Grid, polygon_area};
use crate::ports::{FEATURE_CLEARANCE, FREEBOARD};
use crate::region::{Region, segments};

/// How a traced river ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum End {
    /// It reached the border zone of shared edge `edge` (an index into the
    /// region's edges), `along` metres along it, with water level `level`.
    Edge { edge: usize, along: f64, level: f32 },
    /// It stopped in a hollow, or reached a border with nothing beyond.
    Sink,
    /// It joined an earlier river; its last point is on that river.
    Join,
}

/// A river traced over grid cells: its points, falling water levels, and
/// how it ends.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Traced {
    pub points: Vec<DVec2>,
    pub levels: Vec<f32>,
    pub end: End,
}

/// For each grid cell, the river (index) and point on it that passes
/// through, if any.
pub(crate) type Occupied = Vec<Option<(usize, usize)>>;

/// The water level after moving onto ground of height `ground` from water at
/// `level`: never higher than before, and below the ground.
fn next_level(level: f32, ground: f32) -> f32 {
    level.min(ground - FREEBOARD)
}

/// Settings for tracing a region's own rivers.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Tracing {
    /// Sources lie more than this far from the border, in metres.
    pub source_zone: f64,
    /// Sources per square kilometre.
    pub density: f32,
    /// The least distance between sources, in metres.
    pub spacing: f32,
    /// Rivers shorter than this (metres) that end in a hollow are dropped.
    pub min_length: f32,
    /// How far, in metres, a river may cut through ground above its water
    /// level (beyond the usual freeboard) to escape a hollow.
    pub breach: f32,
}

/// The cell of an earlier river that a move from `from` to `to` meets, if
/// any: one it lands on or slips between, or one next to where it lands.
/// Rivers that close merge.
fn meets(grid: &Grid, occupied: &Occupied, from: usize, to: usize) -> Option<usize> {
    grid.passes(from, to)
        .chain(grid.adjacent(to))
        .find(|&c| occupied[c].is_some())
}

/// The most cells searched to escape one hollow.
const MAX_ESCAPE: usize = 4096;

/// A cell waiting in the hollow-escape search: lowest spill height first.
#[derive(PartialEq)]
struct Spill {
    height: f32,
    cell: usize,
}

impl Eq for Spill {}

impl Ord for Spill {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .height
            .total_cmp(&self.height)
            .then(other.cell.cmp(&self.cell))
    }
}

impl PartialOrd for Spill {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The highest ground a move from cell `from` to cell `to` crosses: the
/// height at `to`, or where a diagonal or knight's move runs between two
/// cells, the mean of their heights if that is higher.
fn crest(grid: &Grid, from: usize, to: usize) -> f32 {
    let between: Vec<f32> = grid
        .passes(from, to)
        .skip(1)
        .map(|c| grid.heights[c])
        .collect();
    let midway = match between.as_slice() {
        [a, b] => 0.5 * (a + b),
        _ => f32::NEG_INFINITY,
    };
    grid.heights[to].max(midway)
}

/// The next cells for a river at `cell` with water level `level`, among
/// cells that pass `allowed` and aren't already on the river.
///
/// Usually this is one cell: the lowest neighbour lower than `cell`. In a
/// hollow it is the path over the lowest rim to the nearest cell lower than
/// `cell`, found by searching outward in order of rim height. The rim may be
/// at most `breach` metres above where the water would sit (the river cuts
/// through it); a deeper hollow returns `None`.
fn next_steps(
    grid: &Grid,
    cell: usize,
    level: f32,
    breach: f32,
    on_river: impl Fn(usize) -> bool,
    allowed: impl Fn(usize) -> bool,
) -> Option<Vec<usize>> {
    // A move must not land on, slip between, or come next to cells of the
    // river itself (apart from the cell it leaves), so the river never
    // doubles back on itself.
    let usable = |from: usize, j: usize| {
        allowed(j)
            && !grid.passes(from, j).any(&on_river)
            && !grid.adjacent(j).any(|k| k != from && on_river(k))
    };

    // Downhill, without passing over higher ground on the way.
    let lower = grid
        .neighbours(cell)
        .map(|(j, _)| j)
        .filter(|&j| usable(cell, j) && crest(grid, cell, j) < grid.heights[cell])
        .min_by(|&a, &b| grid.heights[a].total_cmp(&grid.heights[b]).then(a.cmp(&b)));
    if let Some(j) = lower {
        return Some(vec![j]);
    }

    // A hollow: search outward over the lowest rims.
    let limit = level + FREEBOARD + breach;
    let mut came_from = HashMap::from([(cell, cell)]);
    let mut queue = BinaryHeap::from([Spill {
        height: grid.heights[cell],
        cell,
    }]);
    while let Some(Spill { height, cell: c }) = queue.pop() {
        if c != cell && grid.heights[c] < grid.heights[cell] {
            let mut path = vec![c];
            while came_from[&path[path.len() - 1]] != cell {
                path.push(came_from[&path[path.len() - 1]]);
            }
            path.reverse();
            return Some(path);
        }
        if came_from.len() > MAX_ESCAPE {
            return None;
        }
        for (n, _) in grid.neighbours(c) {
            if !usable(c, n) || came_from.contains_key(&n) {
                continue;
            }
            let spill = height.max(crest(grid, c, n));
            if spill <= limit {
                came_from.insert(n, c);
                queue.push(Spill {
                    height: spill,
                    cell: n,
                });
            }
        }
    }
    None
}

/// Traces the region's own rivers over `grid` (heights of the terrain's own,
/// unbent relief).
///
/// Sources are the highest cells more than `tracing.source_zone` metres from
/// the border and `tracing.spacing` apart, `tracing.density` per km². Each
/// river runs downhill until its next step would come within
/// [`FEATURE_CLEARANCE`] of the border, it stops in a hollow, or it joins an
/// earlier river. Rivers shorter than `tracing.min_length` that end in a
/// hollow are dropped. The result is the same for the same inputs.
pub(crate) fn trace_own(
    grid: &Grid,
    region: &Region,
    polygon: &[DVec2],
    tracing: &Tracing,
) -> (Vec<Traced>, Occupied) {
    let Tracing {
        source_zone,
        density,
        spacing,
        min_length,
        breach,
    } = *tracing;
    let mut rivers: Vec<Traced> = Vec::new();
    let mut occupied: Occupied = vec![None; grid.heights.len()];
    let count = (f64::from(density) * polygon_area(polygon) / 1.0e6).round() as usize;
    if count == 0 {
        return (rivers, occupied);
    }

    // Candidate sources, highest first.
    let mut candidates: Vec<usize> = (0..grid.heights.len())
        .filter(|&i| grid.inside[i] && grid.clearance[i] > source_zone)
        .collect();
    candidates.sort_by(|&a, &b| grid.heights[b].total_cmp(&grid.heights[a]).then(a.cmp(&b)));

    let mut sources: Vec<DVec2> = Vec::new();
    let max_steps = 4 * (grid.width + grid.depth);
    // Which trace last passed each cell, so a river never loops back.
    let mut on_river = vec![0usize; grid.heights.len()];
    let mut attempt = 0;
    for start in candidates {
        if sources.len() >= count {
            break;
        }
        let at = grid.centre(start);
        if occupied[start].is_some() || sources.iter().any(|s| s.distance(at) < f64::from(spacing))
        {
            continue;
        }
        sources.push(at);

        // Run downhill.
        let mut cells = vec![start];
        let mut levels = vec![grid.heights[start] - FREEBOARD];
        let mut end = End::Sink;
        attempt += 1;
        on_river[start] = attempt;
        'trace: while cells.len() < max_steps {
            let cell = cells[cells.len() - 1];
            let level = levels[levels.len() - 1];
            let Some(steps) = next_steps(
                grid,
                cell,
                level,
                breach,
                |j| on_river[j] == attempt,
                |_| true,
            ) else {
                break;
            };
            for next in steps {
                let (cell, level) = (cells[cells.len() - 1], levels[levels.len() - 1]);
                on_river[next] = attempt;
                // Stop before coming too close to the border, so the river's
                // carving never reaches it.
                if grid.clearance[next] <= FEATURE_CLEARANCE {
                    end = edge_end(region, grid.centre(cell), level);
                    break 'trace;
                }
                // Meeting an earlier river, including one this move would
                // slip between cells past.
                if let Some(hit) = meets(grid, &occupied, cell, next) {
                    let (river, point) = occupied[hit].expect("occupied");
                    let joined = rivers[river].levels[point];
                    if level >= joined {
                        cells.push(hit);
                        levels.push(joined);
                        end = End::Join;
                    }
                    break 'trace;
                }
                cells.push(next);
                levels.push(next_level(level, grid.heights[next]));
            }
        }

        // Keep it unless it is a short river ending in a hollow.
        let length = (cells.len() - 1) as f64 * grid.cell;
        if end == End::Sink && length < f64::from(min_length) {
            continue;
        }
        let index = rivers.len();
        let mut points: Vec<DVec2> = cells.iter().map(|&c| grid.centre(c)).collect();
        if end == End::Join {
            // The joining cell belongs to the other river.
            points.pop();
            let (river, point) = occupied[cells[cells.len() - 1]].expect("joined cell is occupied");
            points.push(rivers[river].points[point]);
        }
        let last = if end == End::Join {
            cells.len() - 1
        } else {
            cells.len()
        };
        for (k, &c) in cells[..last].iter().enumerate() {
            occupied[c] = Some((index, k));
        }
        rivers.push(Traced {
            points,
            levels,
            end,
        });
    }
    (rivers, occupied)
}

/// How a river at `point` (inside the border zone) ends: at the nearest
/// edge if it has a neighbour, otherwise in a hollow.
fn edge_end(region: &Region, point: DVec2, level: f32) -> End {
    let mut best: Option<(f64, usize, f64)> = None;
    for (i, edge) in region.edges.iter().enumerate() {
        for segment in segments(&edge.polyline) {
            let projection = segment.project(point);
            if best.is_none_or(|(d, _, _)| projection.distance < d) {
                best = Some((projection.distance, i, projection.along));
            }
        }
    }
    match best {
        Some((_, edge, along)) if region.edges[edge].neighbor.is_some() => {
            End::Edge { edge, along, level }
        }
        _ => End::Sink,
    }
}

/// Traces a river that flows in from a neighbour, starting at `start` with
/// water level `level`, over `grid` (heights of the bent terrain).
///
/// It runs downhill (cutting through rises up to `breach` metres, as for
/// [`Tracing::breach`]), never closer to the border than
/// [`FEATURE_CLEARANCE`], until it stops in a hollow (ending [`End::Sink`])
/// or joins a river in
/// `occupied` that is no higher ([`End::Join`]). Cells it passes are added
/// to `occupied` as river `index`.
pub(crate) fn trace_inbound(
    grid: &Grid,
    start: DVec2,
    level: f32,
    breach: f32,
    rivers: &[Traced],
    occupied: &mut Occupied,
    index: usize,
) -> Traced {
    let mut points = vec![start];
    let mut levels = vec![level];
    let mut end = End::Sink;
    let allowed = |j: usize| grid.clearance[j] > FEATURE_CLEARANCE;
    let mut cell = match grid.cell_of(start) {
        Some(c) => c,
        None => {
            return Traced {
                points,
                levels,
                end,
            };
        }
    };
    let mut visited = vec![cell];
    let max_steps = 4 * (grid.width + grid.depth);
    'trace: while visited.len() < max_steps {
        let level = levels[levels.len() - 1];
        let Some(steps) = next_steps(grid, cell, level, breach, |j| visited.contains(&j), allowed)
        else {
            break;
        };
        for next in steps {
            let level = levels[levels.len() - 1];
            let hit = meets(grid, occupied, cell, next);
            if let Some((river, point)) = hit.and_then(|c| occupied[c]) {
                let joined = rivers.get(river).map(|r| r.levels[point]);
                if let Some(joined) = joined.filter(|&j| level >= j) {
                    points.push(rivers[river].points[point]);
                    levels.push(joined);
                    end = End::Join;
                }
                break 'trace;
            }
            points.push(grid.centre(next));
            levels.push(next_level(level, grid.heights[next]));
            visited.push(next);
            cell = next;
        }
    }
    for (k, &c) in visited.iter().enumerate().skip(1) {
        occupied[c] = Some((index, k));
    }
    Traced {
        points,
        levels,
        end,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::{Boundary, RegionId};

    fn bowl_region() -> (Region, Vec<DVec2>) {
        let polygon = vec![
            DVec2::ZERO,
            DVec2::new(2000.0, 0.0),
            DVec2::new(2000.0, 2000.0),
            DVec2::new(0.0, 2000.0),
        ];
        let region = Region {
            id: RegionId(1),
            boundary: Boundary::Polygon(polygon.clone()),
            edges: Vec::new(),
            corners: Vec::new(),
        };
        (region, polygon)
    }

    #[test]
    fn rivers_run_downhill_into_a_hollow() {
        let (region, polygon) = bowl_region();
        let centre = DVec2::splat(1000.0);
        let grid = Grid::new(&polygon, |p| (p.distance(centre) * 0.05) as f32);
        let tracing = Tracing {
            source_zone: 100.0,
            density: 2.0,
            spacing: 400.0,
            min_length: 100.0,
            breach: 0.0,
        };
        let (rivers, _) = trace_own(&grid, &region, &polygon, &tracing);
        assert!(!rivers.is_empty());
        for (i, river) in rivers.iter().enumerate() {
            assert!(river.levels.windows(2).all(|w| w[1] <= w[0]));
            let last = *river.points.last().unwrap();
            match river.end {
                // Rivers that stop do so at the bottom of the bowl.
                End::Sink => assert!(last.distance(centre) < 60.0),
                // Rivers that join end on an earlier river.
                End::Join => assert!(rivers[..i].iter().any(|r| r.points.contains(&last))),
                End::Edge { .. } => panic!("no shared edges here"),
            }
        }
    }

    /// Ground falling gently towards +x, with a dam `dam` metres high across
    /// it at x = 1000 m.
    fn dammed(dam: f32) -> Grid {
        let (_, polygon) = bowl_region();
        Grid::new(&polygon, move |p| {
            let wall = if (p.x - 1000.0).abs() < 30.0 {
                dam
            } else {
                0.0
            };
            (100.0 - 0.05 * p.x) as f32 + wall
        })
    }

    #[test]
    fn rivers_cut_through_a_low_dam() {
        let grid = dammed(2.0);
        let start = DVec2::new(300.0, 1000.0);
        let river = trace_inbound(
            &grid,
            start,
            84.0,
            4.0,
            &[],
            &mut vec![None; grid.heights.len()],
            0,
        );
        assert!(river.levels.windows(2).all(|w| w[1] <= w[0]));
        assert!(
            river.points.last().unwrap().x > 1200.0,
            "stopped at {}",
            river.points.last().unwrap()
        );
    }

    #[test]
    fn rivers_stop_at_a_high_dam() {
        let grid = dammed(50.0);
        let start = DVec2::new(300.0, 1000.0);
        let river = trace_inbound(
            &grid,
            start,
            84.0,
            4.0,
            &[],
            &mut vec![None; grid.heights.len()],
            0,
        );
        assert_eq!(river.end, End::Sink);
        assert!(river.points.last().unwrap().x < 1000.0);
        assert!(river.levels.windows(2).all(|w| w[1] <= w[0]));
    }

    #[test]
    fn knight_moves_dont_jump_over_ridges() {
        // A low cell whose only lower neighbour is a knight's move away,
        // across a ridge between the two cells it passes.
        let (_, polygon) = bowl_region();
        let mut grid = Grid::new(&polygon, |_| 50.0);
        let from = grid.cell_of(DVec2::splat(1000.0)).expect("inside");
        let to = from + 2 + grid.width;
        let ridge: Vec<usize> = grid.passes(from, to).skip(1).collect();
        assert_eq!(ridge.len(), 2);
        grid.heights[from] = 10.0;
        grid.heights[to] = 5.0;
        for &c in &ridge {
            grid.heights[c] = 100.0;
        }
        assert_eq!(crest(&grid, from, to), 100.0);
        assert_eq!(
            next_steps(&grid, from, 10.0, 0.0, |_| false, |_| true),
            None
        );
    }

    #[test]
    fn inbound_rivers_join_rivers_no_higher() {
        let grid = dammed(0.0);
        let mut occupied = vec![None; grid.heights.len()];
        // An earlier river crossing the slope at x = 800 m, well below.
        let crossing: Vec<usize> = (0..grid.heights.len())
            .filter(|&i| grid.inside[i] && (grid.centre(i).x - 812.5).abs() < 1.0)
            .collect();
        let trunk = Traced {
            points: crossing.iter().map(|&c| grid.centre(c)).collect(),
            levels: vec![50.0; crossing.len()],
            end: End::Sink,
        };
        for (k, &c) in crossing.iter().enumerate() {
            occupied[c] = Some((0, k));
        }
        let start = DVec2::new(300.0, 1000.0);
        let river = trace_inbound(&grid, start, 84.0, 4.0, &[trunk], &mut occupied, 1);
        assert_eq!(river.end, End::Join);
        assert_eq!(*river.levels.last().unwrap(), 50.0);
    }
}
