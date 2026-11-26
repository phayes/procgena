//! The route search: step costs over a region's grid, and the cheapest
//! route from a start to the road network.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::sync::OnceLock;

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

/// The headings a search state can have: one per move direction, and one
/// more for the start, which has none.
const HEADINGS: usize = DIRECTIONS + 1;

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

/// Whether a road heading in move direction `a` may next move in
/// direction `b`: the turn is at most [`MAX_TURN`].
fn turnable(a: usize, b: usize) -> bool {
    static TABLE: OnceLock<[[bool; DIRECTIONS]; DIRECTIONS]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [[false; DIRECTIONS]; DIRECTIONS];
        for (a, row) in table.iter_mut().enumerate() {
            for (b, ok) in row.iter_mut().enumerate() {
                *ok = direction(a).angle_to(direction(b)).abs() <= MAX_TURN + 1e-9;
            }
        }
        table
    })[a][b]
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

/// Memory the route search reuses from one search to the next over the
/// same grid. It holds state only for the cells a road may pass through,
/// and marks which states the current search has reached with a
/// generation number, so a new search needn't clear it.
pub(super) struct Scratch {
    /// For each grid cell, its index among the allowed cells, or
    /// `u32::MAX` if roads may not pass through it.
    index: Vec<u32>,
    /// For each allowed cell's index, the grid cell.
    cells: Vec<usize>,
    /// For each state, the search that last reached it.
    reached: Vec<u32>,
    /// The current search's number.
    generation: u32,
    /// For each state the current search reached: the cheapest cost to it,
    /// how far the road could climb on the way, and the state before it.
    cost: Vec<f64>,
    run: Vec<f64>,
    previous: Vec<u32>,
    queue: BinaryHeap<Entry>,
}

impl Scratch {
    /// Scratch for searches over a grid where `allowed` says which cells a
    /// road may pass through.
    pub(super) fn new(allowed: &[bool]) -> Self {
        let mut index = vec![u32::MAX; allowed.len()];
        let mut cells = Vec::new();
        for (cell, _) in allowed.iter().enumerate().filter(|(_, a)| **a) {
            index[cell] = u32::try_from(cells.len()).expect("fewer than 2^32 cells");
            cells.push(cell);
        }
        let states = cells.len() * HEADINGS;
        assert!(states < u32::MAX as usize, "fewer than 2^32 search states");
        Self {
            index,
            cells,
            reached: vec![0; states],
            generation: 0,
            cost: vec![0.0; states],
            run: vec![0.0; states],
            previous: vec![0; states],
            queue: BinaryHeap::new(),
        }
    }

    /// Memory held for search states, in bytes.
    fn bytes(&self) -> usize {
        self.reached.len() * (size_of::<u32>() + 2 * size_of::<f64>() + size_of::<u32>())
            + self.queue.capacity() * size_of::<Entry>()
    }

    /// Starts a new search: every state counts as not reached.
    fn begin(&mut self) {
        self.queue.clear();
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            // After 2^32 searches the numbers come round: clear them once.
            self.reached.fill(0);
            self.generation = 1;
        }
    }

    /// The cheapest cost found to `state` in this search, infinite if it
    /// hasn't been reached.
    fn cost(&self, state: usize) -> f64 {
        if self.reached[state] == self.generation {
            self.cost[state]
        } else {
            f64::INFINITY
        }
    }

    /// Records that `state` is reached at `cost`, from `previous`, having
    /// climbed up to `run`.
    fn reach(&mut self, state: usize, cost: f64, run: f64, previous: usize) {
        self.reached[state] = self.generation;
        self.cost[state] = cost;
        self.run[state] = run;
        self.previous[state] = previous as u32;
    }
}

/// The cheapest route over `terrain` from `start` to a cell of `network`
/// where `joinable(cell, reach)` allows joining, as cells from `start` to
/// the joining cell, avoiding `blocked` cells. `reach` is how far the road
/// could have climbed or fallen on its way there, at each step's steepest
/// grade. Other network cells may be crossed. The route never turns more
/// than [`MAX_TURN`] from one step to the next. `start` must be an allowed
/// cell.
///
/// `ahead[cell]` must never be more than the cost from `cell` to the first
/// network cell (see [`network_distance`]); the search uses it to head for
/// the network without missing the cheapest route. `scratch` must have been
/// made for `terrain.allowed`.
///
/// Counts the search, and the states it queued and took, in `stats`.
#[allow(clippy::too_many_arguments)]
pub(super) fn search(
    terrain: &Terrain,
    network: &[bool],
    ahead: &[f64],
    blocked: &[bool],
    start: usize,
    joinable: &dyn Fn(usize, f64) -> bool,
    scratch: &mut Scratch,
    stats: &mut RouteStats,
) -> Option<Vec<usize>> {
    let Terrain {
        grid,
        allowed,
        river,
        bridges,
        grades,
    } = *terrain;
    stats.searches += 1;

    // A search state is an allowed cell's index and the direction the route
    // entered it by; the start has no direction (`DIRECTIONS`). The queue
    // is ordered by the cost so far plus the least cost still ahead.
    scratch.begin();
    let first = scratch.index[start] as usize * HEADINGS + DIRECTIONS;
    scratch.reach(first, 0.0, 0.0, first);
    scratch.queue.push(Entry {
        cost: ahead[start],
        state: first,
    });
    stats.states_pushed += 1;
    let found = loop {
        let Some(Entry {
            cost: estimate,
            state,
        }) = scratch.queue.pop()
        else {
            break None;
        };
        stats.states_popped += 1;
        let (cell, heading) = (scratch.cells[state / HEADINGS], state % HEADINGS);
        let c = scratch.cost[state];
        if estimate > c + ahead[cell] {
            continue;
        }
        if network[cell] && joinable(cell, scratch.run[state]) {
            // The route, from the start to here.
            let mut states = vec![state];
            while states[states.len() - 1] != first {
                states.push(scratch.previous[states[states.len() - 1]] as usize);
            }
            states.reverse();
            break Some(
                states
                    .into_iter()
                    .map(|s| scratch.cells[s / HEADINGS])
                    .collect(),
            );
        }
        for (next, length, k) in grid.moves(cell) {
            if !allowed[next] || blocked[next] {
                continue;
            }
            if heading != DIRECTIONS && !turnable(heading, k) {
                continue;
            }
            let next_state = scratch.index[next] as usize * HEADINGS + k;
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
            let here = scratch.run[state] + steepest * length;
            if network[next] && joinable(next, here) {
                step *= REUSE_COST;
            }
            // A step across a river needs a bridge.
            if let Some(bridge) = bridges.cost(cell, k) {
                step += bridge;
            }
            let total = c + step;
            if total < scratch.cost(next_state) {
                scratch.reach(next_state, total, here, state);
                scratch.queue.push(Entry {
                    cost: total + ahead[next],
                    state: next_state,
                });
                stats.states_pushed += 1;
            }
        }
    };
    stats.peak_state_bytes = stats.peak_state_bytes.max(scratch.bytes());
    found
}
