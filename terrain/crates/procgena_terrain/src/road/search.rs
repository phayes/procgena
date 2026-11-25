//! The route search: step costs over a region's grid, and the cheapest
//! route from a start to the road network.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use glam::DVec2;

use crate::carve::FeatureLine;
use crate::grid::{DIRECTIONS, Grid, direction};
use crate::options::RoadGrades;

use super::RouteStats;

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

/// Extra cost of a step per metre of expected cutting or filling, squared
/// (in units of [`CARVE_SCALE`]).
const CARVE_COST: f64 = 1.0;

/// The depth of cutting or filling that doubles a step's cost, in metres.
const CARVE_SCALE: f64 = 2.0;

/// The sharpest turn a road may make from one grid step to the next, in
/// radians. On the road grid this gives hairpins about 10 m in radius.
const MAX_TURN: f64 = std::f64::consts::FRAC_PI_4;

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

/// For each cell, the least cost of any route from it to the nearest cell
/// of `network`: the length of the shortest path over the grid's moves,
/// with the last step onto the network at [`REUSE_COST`] of its length, as
/// the route search may count it. Every step the search takes costs at
/// least that much, so this never overestimates.
pub(super) fn network_distance(grid: &Grid, network: &[bool]) -> Vec<f64> {
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

/// Which cells of `grid` the lines of `rivers` pass through.
pub(super) fn water(grid: &Grid, rivers: &[FeatureLine]) -> Vec<bool> {
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
pub(super) struct Bridges {
    /// For each cell, which of its moves cross a river: bit `k` for
    /// direction `k` of [`Grid::moves`].
    moves: Vec<u16>,
    /// The bridge cost of each crossing move, by `cell * DIRECTIONS + k`.
    costs: HashMap<usize, f64>,
}

impl Bridges {
    /// The moves over `grid` that cross a line of `rivers`.
    pub(super) fn new(grid: &Grid, rivers: &[FeatureLine]) -> Self {
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
pub(super) fn segments_meet(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let side = |p: DVec2, q: DVec2, r: DVec2| (q - p).perp_dot(r - p);
    side(c, d, a) * side(c, d, b) <= 0.0 && side(a, b, c) * side(a, b, d) <= 0.0
}

/// What the route search knows about each cell of a grid, apart from the
/// road network built so far.
pub(super) struct Terrain<'a> {
    pub grid: &'a Grid,
    /// Whether a road may pass through the cell.
    pub allowed: &'a [bool],
    /// Whether the cell has water.
    pub river: &'a [bool],
    /// The moves that cross a river.
    pub bridges: &'a Bridges,
    /// The road grades in the cell.
    pub grades: &'a [RoadGrades],
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
///
/// Counts the search, and the states it queued and took, in `stats`.
pub(super) fn search(
    terrain: &Terrain,
    network: &[bool],
    ahead: &[f64],
    blocked: &[bool],
    start: usize,
    joinable: &dyn Fn(usize, f64) -> bool,
    stats: &mut RouteStats,
) -> Option<Vec<usize>> {
    stats.searches += 1;
    let mut counts = SearchCounts::default();
    let found = search_counted(
        terrain,
        network,
        ahead,
        blocked,
        start,
        joinable,
        &mut counts,
    );
    stats.states_pushed += counts.pushed;
    stats.states_popped += counts.popped;
    stats.peak_state_bytes = stats.peak_state_bytes.max(counts.peak_bytes);
    found
}

/// The work one search did.
#[derive(Default)]
struct SearchCounts {
    pushed: u64,
    popped: u64,
    /// Memory held for the search's states at its largest, in bytes.
    peak_bytes: usize,
}

/// [`search`], counting its work in `counts`.
fn search_counted(
    terrain: &Terrain,
    network: &[bool],
    ahead: &[f64],
    blocked: &[bool],
    start: usize,
    joinable: &dyn Fn(usize, f64) -> bool,
    counts: &mut SearchCounts,
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
    counts.pushed += 1;
    let state_bytes = n * (2 * size_of::<f64>() + size_of::<usize>());
    let mut longest_queue = 1;
    while let Some(Entry {
        cost: estimate,
        state,
    }) = queue.pop()
    {
        counts.popped += 1;
        let (cell, heading) = (state / headings, state % headings);
        let c = cost[state];
        if estimate > c + ahead[cell] {
            continue;
        }
        if network[cell] && joinable(cell, run[state]) {
            counts.peak_bytes = state_bytes + longest_queue * size_of::<Entry>();
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
                counts.pushed += 1;
                longest_queue = longest_queue.max(queue.len());
            }
        }
    }
    counts.peak_bytes = state_bytes + longest_queue * size_of::<Entry>();
    None
}
