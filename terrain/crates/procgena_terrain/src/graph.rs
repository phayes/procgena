//! The region graph: an endless tiling of the plane into regions, worked
//! out from position alone, so any region is known without building the
//! others, in any order.
//!
//! Each region is the Voronoi cell of one site, jittered inside its cell
//! of a staggered (hexagonal) lattice. Corners closer together than a
//! minimum edge length are merged into one corner touched by four or more
//! regions.

use std::collections::{BTreeSet, HashMap};
use std::fmt;

use glam::{DVec2, I64Vec2, IVec2};

use crate::hash::{GRAPH_SITE_X, GRAPH_SITE_Z, hash2, seed_for};
use crate::ports::CORNER_CLEARANCE;
use crate::region::{Boundary, Region, RegionCorner, RegionEdge, RegionId};
use crate::warp::{BorderWarp, BorderWarpError, side_amplitude, warp_side};

/// The most a site may move from its lattice point, as a share of half
/// the lattice spacing along each axis (see
/// [`RegionGraphOptions::jitter`]).
pub const MAX_JITTER: f64 = 0.8;

/// The shortest [`RegionGraphOptions::min_edge`] allowed, in metres: an
/// edge needs this much length to carry a river or road crossing clear of
/// both its corners.
pub const MIN_EDGE: f64 = 2.0 * CORNER_CLEARANCE;

/// How many lattice cells either way are searched for the corners around a
/// site, at first.
///
/// A corner is a circle with no site inside, so its radius is at most the
/// farthest any point lies from its nearest site: for the lattice, half a
/// row spacing over cos 30° (1/√3 spacings), plus a site's largest jitter
/// (√2 × 0.4 spacings at [`MAX_JITTER`]), about 1.14 spacings. A corner's
/// circle passes through the site, so it lies within twice that, about
/// 2.3 spacings, of it. Four cells either way hold every site within about
/// 3.5 spacings (see [`RegionGraph::coverage`]); a circle reaching past what
/// the window holds makes the search widen (see [`MAX_WINDOW`]).
const WINDOW: i32 = 4;

/// The widest search, in lattice cells either way.
const MAX_WINDOW: i32 = 8;

/// The largest lattice spacing, in metres. Sites are compared exactly in
/// 128-bit integers of millimetres; within the widest search (8 cells
/// either way) at this spacing their differences stay below about 10⁹ mm,
/// so the in-circle test's terms stay below about 10³⁷, inside the
/// 1.7·10³⁸ an i128 holds.
pub const MAX_SIZE: f64 = 50_000.0;

/// Sites are placed on a grid of millimetres, so they can be compared
/// exactly: metres times this.
const PER_METRE: f64 = 1000.0;

/// The distance between lattice rows, as a share of the spacing along a
/// row: the rows of a hexagonal lattice.
const ROW: f64 = 0.866_025_403_784_438_6;

/// A region of a [`RegionGraph`]: the lattice cell its site belongs to.
/// Convert it to and from the region's [`RegionId`] with
/// [`id`](Self::id) and [`from_id`](Self::from_id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegionKey(pub IVec2);

impl Ord for RegionKey {
    /// By x, then by z.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.0.x, self.0.y).cmp(&(other.0.x, other.0.y))
    }
}

impl PartialOrd for RegionKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl RegionKey {
    /// The region's id. Different keys always give different ids.
    pub fn id(self) -> RegionId {
        RegionId((u64::from(self.0.x as u32) << 32) | u64::from(self.0.y as u32))
    }

    /// The key whose [`id`](Self::id) is `id`. Every id is some key's.
    pub fn from_id(id: RegionId) -> Self {
        Self(IVec2::new((id.0 >> 32) as u32 as i32, id.0 as u32 as i32))
    }
}

/// How the plane is split into regions.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionGraphOptions {
    /// The lattice spacing, about the width of a region, in metres.
    pub size: f64,
    /// How far each site moves from its lattice point, from 0 (a regular
    /// hexagonal tiling) to [`MAX_JITTER`].
    pub jitter: f64,
    /// Edges shorter than this, in metres, are merged away: their two
    /// corners become one corner, touched by four or more regions. At
    /// least [`MIN_EDGE`].
    pub min_edge: f64,
    /// How the borders between regions are made irregular. Corners stay
    /// where they are; the edges between them wiggle.
    pub warp: BorderWarp,
}

impl Default for RegionGraphOptions {
    fn default() -> Self {
        Self {
            size: 4000.0,
            jitter: 0.6,
            min_edge: MIN_EDGE,
            warp: BorderWarp::default(),
        }
    }
}

/// Why region graph settings were refused.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum RegionGraphError {
    /// The size must be finite, at least 5 times the minimum edge and at
    /// most [`MAX_SIZE`].
    Size(f64),
    /// The jitter must be from 0 to [`MAX_JITTER`].
    Jitter(f64),
    /// The minimum edge must be finite and at least [`MIN_EDGE`].
    MinEdge(f64),
    /// The border warp settings aren't valid.
    Warp(BorderWarpError),
}

impl fmt::Display for RegionGraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Size(s) => write!(
                f,
                "the region size must be at least 5 times the minimum edge and at most \
                 {MAX_SIZE} m, not {s}"
            ),
            Self::Jitter(j) => write!(f, "the jitter must be from 0 to {MAX_JITTER}, not {j}"),
            Self::MinEdge(m) => write!(
                f,
                "the minimum edge must be finite and at least {MIN_EDGE} m, not {m}"
            ),
            Self::Warp(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for RegionGraphError {}

impl RegionGraphOptions {
    /// Checks the settings.
    pub fn validate(&self) -> Result<(), RegionGraphError> {
        if self.min_edge.is_nan() || self.min_edge < MIN_EDGE {
            return Err(RegionGraphError::MinEdge(self.min_edge));
        }
        self.validate_unfloored()
    }

    /// Checks the settings, allowing any minimum edge from 0, for tests of
    /// the raw cells.
    fn validate_unfloored(&self) -> Result<(), RegionGraphError> {
        if !self.min_edge.is_finite() || self.min_edge < 0.0 {
            return Err(RegionGraphError::MinEdge(self.min_edge));
        }
        if !self.size.is_finite()
            || self.size <= 0.0
            || self.size < 5.0 * self.min_edge
            || self.size > MAX_SIZE
        {
            return Err(RegionGraphError::Size(self.size));
        }
        if !(0.0..=MAX_JITTER).contains(&self.jitter) {
            return Err(RegionGraphError::Jitter(self.jitter));
        }
        self.warp.validate().map_err(RegionGraphError::Warp)
    }
}

/// The regions of an endless world: which region holds each point, and
/// each region's outline, edges and corners. Every region is worked out
/// on its own, from the settings and seed alone, so any region can be
/// asked for at any time, in any order, on any thread, with the same
/// result.
#[derive(Debug, Clone)]
pub struct RegionGraph {
    options: RegionGraphOptions,
    seed: u32,
    /// Sites placed by hand, in millimetres, for tests of awkward layouts.
    #[cfg(test)]
    placed: HashMap<RegionKey, I64Vec2>,
}

/// Every region whose site lies on one empty circle (a raw Voronoi
/// corner), sorted. Usually three; four or more where sites share a
/// circle exactly.
type Corner = Vec<RegionKey>;

/// A region's raw Voronoi cell: its neighbours in anticlockwise order, and
/// the raw corner between each neighbour and the one before it.
#[derive(Debug, Clone)]
struct Ring {
    neighbours: Vec<RegionKey>,
    /// `corners[i]` lies between `neighbours[i - 1]` and `neighbours[i]`.
    corners: Vec<Corner>,
}

impl RegionGraph {
    /// The graph for `options` and the world's `seed`.
    ///
    /// # Errors
    ///
    /// If `options` aren't valid (see [`RegionGraphOptions::validate`]).
    pub fn new(options: RegionGraphOptions, seed: u32) -> Result<Self, RegionGraphError> {
        options.validate()?;
        Ok(Self::unchecked(options, seed))
    }

    /// The graph for settings already checked.
    fn unchecked(options: RegionGraphOptions, seed: u32) -> Self {
        Self {
            options,
            seed,
            #[cfg(test)]
            placed: HashMap::new(),
        }
    }

    /// Like [`new`](Self::new), allowing any minimum edge from 0.
    #[cfg(test)]
    fn unfloored(options: RegionGraphOptions, seed: u32) -> Result<Self, RegionGraphError> {
        options.validate_unfloored()?;
        Ok(Self::unchecked(options, seed))
    }

    /// This graph with the sites of `placed` regions at the given points,
    /// in millimetres.
    #[cfg(test)]
    fn with_sites(mut self, placed: &[(RegionKey, I64Vec2)]) -> Self {
        self.placed.extend(placed.iter().copied());
        self
    }

    /// How far any hand-placed site lies from where its jittered site
    /// would be, in metres: the search must reach that much further.
    #[cfg(test)]
    fn placed_offset(&self) -> f64 {
        let unplaced = Self::unchecked(self.options, self.seed);
        self.placed
            .iter()
            .map(|(&k, &p)| (p - unplaced.site_mm(k)).as_dvec2().length() / PER_METRE)
            .fold(0.0, f64::max)
    }

    /// The settings.
    pub fn options(&self) -> &RegionGraphOptions {
        &self.options
    }

    /// The site of region `key`, in metres: the point its region is built
    /// around. Sites lie on a grid of millimetres.
    pub fn site(&self, key: RegionKey) -> DVec2 {
        self.site_mm(key).as_dvec2() / PER_METRE
    }

    /// The site of region `key`, in whole millimetres.
    fn site_mm(&self, key: RegionKey) -> I64Vec2 {
        #[cfg(test)]
        if let Some(&placed) = self.placed.get(&key) {
            return placed;
        }
        let IVec2 { x, y: z } = key.0;
        let size = self.options.size;
        let stagger = if z.rem_euclid(2) == 1 { 0.5 } else { 0.0 };
        let point = DVec2::new((f64::from(x) + stagger) * size, f64::from(z) * ROW * size);
        let unit = |key: &str| {
            let h = hash2(i64::from(x), i64::from(z), seed_for(self.seed, key));
            f64::from(h) / f64::from(u32::MAX) * 2.0 - 1.0
        };
        let jitter = DVec2::new(unit(GRAPH_SITE_X), unit(GRAPH_SITE_Z));
        let site = point + jitter * (0.5 * self.options.jitter * size);
        (site * PER_METRE).round().as_i64vec2()
    }

    /// How far from a site the sites within `window` lattice cells either
    /// way reach for certain, in metres: every site nearer than this is
    /// among them.
    fn coverage(&self, window: i32) -> f64 {
        let (size, jitter) = (self.options.size, self.options.jitter);
        // Rows beyond the window are at least this far off, less both
        // sites' jitter; columns likewise, less half a cell of stagger.
        let rows = f64::from(window + 1) * ROW * size;
        let columns = (f64::from(window) + 0.5) * size;
        let reach = rows.min(columns) - jitter * size - 1.0;
        #[cfg(test)]
        let reach = reach - 2.0 * self.placed_offset();
        reach
    }

    /// The lattice cell nearest `p`: the key of a region whose site is
    /// near `p`, though not always the region holding `p`.
    pub(crate) fn cell_near(&self, p: DVec2) -> RegionKey {
        let size = self.options.size;
        let z = (p.y / (ROW * size)).round() as i32;
        let stagger = if z.rem_euclid(2) == 1 { 0.5 } else { 0.0 };
        let x = (p.x / size - stagger).round() as i32;
        RegionKey(IVec2::new(x, z))
    }

    /// The region `key`: its outline, edges and corners, ready to pass to a
    /// terrain. Both regions on an edge get identical edges, and every
    /// region at a corner gets the identical corner.
    pub fn region(&self, key: RegionKey) -> Region {
        self.build(&mut Local::new(self), key)
    }

    /// The region `key`, with its borders warped, reusing the cells and
    /// corners already worked out in `local`.
    fn build(&self, local: &mut Local, key: RegionKey) -> Region {
        let sides = local.sides(key).to_vec();
        let mut points = Vec::new();
        let mut edges = Vec::with_capacity(sides.len());
        let mut corners: Vec<RegionCorner> = Vec::with_capacity(sides.len());
        for side in &sides {
            let (a, b) = (side.from.position, side.to.position);
            let amplitude = local.amplitude(side);
            let polyline = warp_side(a, b, amplitude, &self.options.warp, self.seed);
            points.extend_from_slice(&polyline[..polyline.len() - 1]);
            edges.push(
                RegionEdge::new(key.id(), Some(side.neighbour.id()), polyline)
                    .expect("two distinct corners make an edge"),
            );
            if !corners.iter().any(|c| c.id == side.from.id) {
                corners.push(side.from.clone());
            }
        }
        Region {
            id: key.id(),
            boundary: Boundary::Polygon(points),
            edges,
            corners,
        }
    }

    /// The region holding `p`. Points on a border may go to either side.
    pub fn region_at(&self, p: DVec2) -> RegionKey {
        let mut local = Local::new(self);

        // The regions nearest first; merged corners shift outlines away
        // from the plain Voronoi cells, so the owner isn't always the
        // nearest site, but it is close by.
        let near = self.cell_near(p);
        let mut candidates: Vec<(f64, RegionKey)> = (-2..=2)
            .flat_map(|dz| (-2..=2).map(move |dx| RegionKey(near.0 + IVec2::new(dx, dz))))
            .map(|k| (self.site(k).distance_squared(p), k))
            .collect();
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let straight = candidates
            .iter()
            .map(|&(_, k)| k)
            .find(|&k| local.outline(k).contains(p))
            .unwrap_or(candidates[0].1);

        // Warping moves a border by at most the amplitude, and only between
        // the two regions on it, so only a point that near a border can
        // belong to one of the owner's neighbours instead.
        let amplitude = self.options.warp.amplitude;
        if amplitude == 0.0 || local.outline(straight).distance_to_border(p) > amplitude {
            return straight;
        }
        let near: Vec<RegionKey> = std::iter::once(straight)
            .chain(local.sides(straight).iter().map(|s| s.neighbour))
            .collect();
        near.into_iter()
            .find(|&k| self.build(&mut local, k).boundary.contains(p))
            .unwrap_or(straight)
    }

    /// How far the edge between `key` and its `neighbour` may be warped,
    /// in metres: the amplitude of the warp, less wherever the edges
    /// around it leave less room. For tests that report how often edges
    /// are held back.
    #[cfg(test)]
    pub(crate) fn warp_amplitude(&self, key: RegionKey, neighbour: RegionKey) -> Option<f64> {
        let mut local = Local::new(self);
        let side = local
            .sides(key)
            .iter()
            .find(|s| s.neighbour == neighbour)?
            .clone();
        Some(local.amplitude(&side))
    }
}

/// One straight side of a region's unwarped outline.
#[derive(Debug, Clone)]
struct Side {
    /// The corner it starts at, going anticlockwise round the region.
    from: RegionCorner,
    /// The corner it ends at.
    to: RegionCorner,
    /// The region on the other side.
    neighbour: RegionKey,
}

/// Raw Voronoi cells, merged corners and unwarped outlines worked out while
/// building regions, kept so each is worked out once.
struct Local<'a> {
    graph: &'a RegionGraph,
    rings: HashMap<RegionKey, Ring>,
    corners: HashMap<Corner, RegionCorner>,
    sides: HashMap<RegionKey, Vec<Side>>,
}

impl<'a> Local<'a> {
    fn new(graph: &'a RegionGraph) -> Self {
        Self {
            graph,
            rings: HashMap::new(),
            corners: HashMap::new(),
            sides: HashMap::new(),
        }
    }

    /// The straight sides of region `key`'s unwarped outline, anticlockwise.
    /// Sides whose two ends merged into one corner are left out.
    fn sides(&mut self, key: RegionKey) -> &[Side] {
        if !self.sides.contains_key(&key) {
            let Ring {
                neighbours,
                corners: raw,
            } = self.ring(key).clone();
            // Side `i`, shared with `neighbours[i]`, runs from corner `i` to
            // `i + 1`.
            let corners: Vec<RegionCorner> = raw.into_iter().map(|c| self.corner(c)).collect();
            let n = neighbours.len();
            let sides = (0..n)
                .filter(|&i| corners[i].id != corners[(i + 1) % n].id)
                .map(|i| Side {
                    from: corners[i].clone(),
                    to: corners[(i + 1) % n].clone(),
                    neighbour: neighbours[i],
                })
                .collect();
            self.sides.insert(key, sides);
        }
        &self.sides[&key]
    }

    /// Region `key`'s unwarped outline.
    fn outline(&mut self, key: RegionKey) -> Boundary {
        Boundary::Polygon(self.sides(key).iter().map(|s| s.from.position).collect())
    }

    /// How far `side` may be warped, from the sides of every region at
    /// either of its corners: the same whichever region on it asks.
    fn amplitude(&mut self, side: &Side) -> f64 {
        let warp = self.graph.options.warp;
        if warp.amplitude == 0.0 {
            return 0.0;
        }
        let around: BTreeSet<RegionId> = side
            .from
            .incident
            .iter()
            .chain(&side.to.incident)
            .copied()
            .collect();
        let mut pieces = Vec::new();
        for id in around {
            pieces.extend(
                self.sides(RegionKey::from_id(id))
                    .iter()
                    .map(|s| (s.from.position, s.to.position)),
            );
        }
        side_amplitude(side.from.position, side.to.position, pieces, &warp)
    }

    /// The raw Voronoi cell of `key`: the same for whichever region asks,
    /// since it depends only on the sites.
    fn ring(&mut self, key: RegionKey) -> &Ring {
        let graph = self.graph;
        self.rings.entry(key).or_insert_with(|| {
            (WINDOW..=MAX_WINDOW)
                .step_by(2)
                .find_map(|window| walk(graph, key, window))
                .expect("a site's corners lie within the widest search")
        })
    }

    /// Where raw corner `corner` is: the centre of the circle through its
    /// first three sites in sorted order, so every region computes the
    /// same bits.
    fn position(&self, corner: &Corner) -> DVec2 {
        let [a, b, c] = [0, 1, 2].map(|i| self.graph.site(corner[i]));
        circumcentre(a, b, c)
    }

    /// The corner that raw corner `corner` belongs to, once every raw
    /// corner joined to it by a chain of edges shorter than the minimum is
    /// merged in: at the mean of the merged raw corners (summed in sorted
    /// order), touched by every region touching any of them.
    fn corner(&mut self, corner: Corner) -> RegionCorner {
        if let Some(known) = self.corners.get(&corner) {
            return known.clone();
        }
        let merged = self.merge(corner.clone());
        self.corners.insert(corner, merged.clone());
        merged
    }

    /// [`corner`](Self::corner), worked out afresh.
    fn merge(&mut self, corner: Corner) -> RegionCorner {
        let min_edge = self.graph.options.min_edge;
        let mut merged: BTreeSet<Corner> = BTreeSet::from([corner.clone()]);
        let mut todo = vec![corner];
        while let Some(c) = todo.pop() {
            let here = self.position(&c);
            for other in self.adjacent(&c) {
                if !merged.contains(&other) && here.distance(self.position(&other)) < min_edge {
                    merged.insert(other.clone());
                    todo.push(other);
                }
            }
        }
        let sum = merged
            .iter()
            .fold(DVec2::ZERO, |sum, c| sum + self.position(c));
        let incident = merged.iter().flatten().map(|k| k.id());
        RegionCorner::new(sum / merged.len() as f64, incident)
    }

    /// The raw corners at the far ends of the sides that meet at raw corner
    /// `corner`: around each of its sites, the corners before and after it.
    fn adjacent(&mut self, corner: &Corner) -> Vec<Corner> {
        let mut out = Vec::new();
        for &key in corner {
            let ring = self.ring(key);
            let n = ring.corners.len();
            let k = ring
                .corners
                .iter()
                .position(|c| c == corner)
                .expect("every site on a corner has it in its ring");
            out.push(ring.corners[(k + n - 1) % n].clone());
            out.push(ring.corners[(k + 1) % n].clone());
        }
        out
    }
}

/// The raw Voronoi cell of `key`, from the sites within `window` lattice
/// cells either way, or `None` if one of its corners might be affected by
/// a site beyond them (search wider then).
///
/// A raw corner is an empty circle (no site strictly inside) through `key`
/// and two or more other sites. The walk starts at the nearest site, which
/// always shares a corner with `key`, and goes round anticlockwise: from
/// each neighbour, the next corner is the circle through `key`, that
/// neighbour and the site to its left that leaves every other site on the
/// left outside; every site on that circle belongs to the corner; and the
/// next neighbour is the corner's most anticlockwise site seen from `key`.
/// The walk ends back at the first corner. Every test is exact.
fn walk(graph: &RegionGraph, key: RegionKey, window: i32) -> Option<Ring> {
    let a = graph.site_mm(key);
    let others: Vec<(RegionKey, I64Vec2)> = (-window..=window)
        .flat_map(|dz| (-window..=window).map(move |dx| IVec2::new(dx, dz)))
        .filter(|&d| d != IVec2::ZERO)
        .map(|d| {
            let k = RegionKey(key.0 + d);
            (k, graph.site_mm(k))
        })
        .collect();
    let reach = graph.coverage(window) * PER_METRE;

    // The nearest site, ties to the smaller key.
    let distance = |p: I64Vec2| {
        let (x, z) = (i128::from(p.x - a.x), i128::from(p.y - a.y));
        x * x + z * z
    };
    let start = *others
        .iter()
        .min_by(|p, q| distance(p.1).cmp(&distance(q.1)).then(p.0.cmp(&q.0)))?;

    // Round the corners, until back at the first.
    let mut neighbours = Vec::new();
    let mut corners: Vec<Corner> = Vec::new();
    let mut from = start;
    loop {
        let (corner, next) = corner_after(key, a, from, &others, reach)?;
        if corners.first() == Some(&corner) {
            break;
        }
        assert!(corners.len() < 64, "the corners round {key:?} don't close");
        corners.push(corner);
        neighbours.push(next.0);
        from = next;
    }
    Some(Ring {
        neighbours,
        corners,
    })
}

/// The raw corner after neighbour `from` going anticlockwise round site
/// `a` (region `key`), with the neighbour after it, or `None` if the
/// corner's circle reaches more than `reach` millimetres from `a`, where a
/// site not among `others` might lie inside it.
fn corner_after(
    key: RegionKey,
    a: I64Vec2,
    from: (RegionKey, I64Vec2),
    others: &[(RegionKey, I64Vec2)],
    reach: f64,
) -> Option<(Corner, (RegionKey, I64Vec2))> {
    // The site to the left of `a` → `from` whose circle with them holds no
    // other site on the left.
    let mut best: Option<(RegionKey, I64Vec2)> = None;
    for &(k, p) in others {
        if k == from.0 || orient(a, from.1, p) <= 0 {
            continue;
        }
        if best.is_none_or(|(_, c)| in_circle(a, from.1, c, p) > 0) {
            best = Some((k, p));
        }
    }
    let third = best?;

    // The circle must lie within the sites searched.
    let (centre, point) = (
        circumcentre(a.as_dvec2(), from.1.as_dvec2(), third.1.as_dvec2()),
        a.as_dvec2(),
    );
    if 2.0 * centre.distance(point) >= reach {
        return None;
    }

    // Every site on the circle, and the most anticlockwise of them seen
    // from `a`: sites on a circle through `a` lie within a half-turn of
    // each other seen from `a`, so the exact orientation orders them.
    let mut members = vec![from, third];
    members.extend(
        others.iter().filter(|&&(k, p)| {
            k != from.0 && k != third.0 && in_circle(a, from.1, third.1, p) == 0
        }),
    );
    let next = *members
        .iter()
        .max_by(|p, q| 0.cmp(&orient(a, p.1, q.1)))
        .expect("a corner has sites");
    let mut corner: Corner = members.iter().map(|&(k, _)| k).collect();
    corner.push(key);
    corner.sort_unstable();
    Some((corner, next))
}

/// Twice the signed area of triangle `a`, `b`, `c`: positive if they turn
/// anticlockwise, 0 if they lie on a line. Exact.
fn orient(a: I64Vec2, b: I64Vec2, c: I64Vec2) -> i128 {
    let (bx, bz) = (i128::from(b.x - a.x), i128::from(b.y - a.y));
    let (cx, cz) = (i128::from(c.x - a.x), i128::from(c.y - a.y));
    bx * cz - bz * cx
}

/// Positive if `d` lies inside the circle through `a`, `b` and `c` (which
/// must turn anticlockwise), 0 if on it, negative if outside. Exact for
/// points within [`MAX_SIZE`]'s bound of each other.
fn in_circle(a: I64Vec2, b: I64Vec2, c: I64Vec2, d: I64Vec2) -> i128 {
    let relative = |p: I64Vec2| {
        let (x, z) = (i128::from(p.x - d.x), i128::from(p.y - d.y));
        (x, z, x * x + z * z)
    };
    let ((ax, az, al), (bx, bz, bl), (cx, cz, cl)) = (relative(a), relative(b), relative(c));
    ax * (bz * cl - bl * cz) - az * (bx * cl - bl * cx) + al * (bx * cz - bz * cx)
}

/// The centre of the circle through `a`, `b` and `c`.
fn circumcentre(a: DVec2, b: DVec2, c: DVec2) -> DVec2 {
    let (b, c) = (b - a, c - a);
    let d = 2.0 * b.perp_dot(c);
    let (bb, cc) = (b.length_squared(), c.length_squared());
    a + DVec2::new(c.y * bb - b.y * cc, b.x * cc - c.x * bb) / d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::warp::segment_distance;

    /// No border warp.
    const STRAIGHT: BorderWarp = BorderWarp {
        amplitude: 0.0,
        wavelength: 800.0,
    };

    /// The strongest border warp allowed at the default wavelength.
    fn strongest_warp() -> BorderWarp {
        let wavelength = BorderWarp::default().wavelength;
        let Err(BorderWarpError::TooSteep { most, .. }) = (BorderWarp {
            amplitude: 1e6,
            wavelength,
        })
        .validate() else {
            unreachable!("a huge amplitude is too steep")
        };
        BorderWarp {
            amplitude: most,
            wavelength,
        }
    }

    /// Graphs with no jitter, the default and the most, each with no
    /// merging (raw cells) and the default minimum edge, and each with
    /// straight borders and the strongest warp.
    fn graphs() -> Vec<RegionGraph> {
        let mut out = Vec::new();
        for jitter in [0.0, 0.6, MAX_JITTER] {
            for min_edge in [0.0, MIN_EDGE] {
                for warp in [STRAIGHT, strongest_warp()] {
                    let options = RegionGraphOptions {
                        size: 2000.0,
                        jitter,
                        min_edge,
                        warp,
                    };
                    out.push(RegionGraph::unfloored(options, 7).unwrap());
                }
            }
        }
        out
    }

    /// The keys of a block of regions around the origin.
    fn block(radius: i32) -> impl Iterator<Item = RegionKey> {
        (-radius..=radius)
            .flat_map(move |z| (-radius..=radius).map(move |x| RegionKey(IVec2::new(x, z))))
    }

    fn key_of(id: RegionId) -> RegionKey {
        RegionKey::from_id(id)
    }

    /// Every raw corner touching the middle 3 × 3 regions, by testing every
    /// triple of sites in the 9 × 9 around them against every site there:
    /// each empty circle, with every site on it.
    fn brute_force_corners(graph: &RegionGraph) -> BTreeSet<Corner> {
        let sites: Vec<(RegionKey, I64Vec2)> = block(4).map(|k| (k, graph.site_mm(k))).collect();
        let middle: Vec<RegionKey> = block(1).collect();
        let mut corners = BTreeSet::new();
        for i in 0..sites.len() {
            for j in i + 1..sites.len() {
                for k in j + 1..sites.len() {
                    let (a, mut b, mut c) = (sites[i], sites[j], sites[k]);
                    match orient(a.1, b.1, c.1) {
                        0 => continue,
                        o if o < 0 => std::mem::swap(&mut b, &mut c),
                        _ => {}
                    }
                    if sites.iter().any(|d| in_circle(a.1, b.1, c.1, d.1) > 0) {
                        continue;
                    }
                    let mut corner: Corner = sites
                        .iter()
                        .filter(|d| in_circle(a.1, b.1, c.1, d.1) == 0)
                        .map(|d| d.0)
                        .collect();
                    corner.sort_unstable();
                    if corner.iter().any(|k| middle.contains(k)) {
                        corners.insert(corner);
                    }
                }
            }
        }
        corners
    }

    /// The raw corners the walk finds round the middle 3 × 3 regions.
    fn walked_corners(graph: &RegionGraph) -> BTreeSet<Corner> {
        let mut local = Local::new(graph);
        block(1)
            .flat_map(|k| local.ring(k).corners.clone())
            .collect()
    }

    #[test]
    fn corners_match_a_brute_force_search() {
        for graph in graphs() {
            assert_eq!(walked_corners(&graph), brute_force_corners(&graph));
        }
    }

    /// Checks that every edge of the regions `keys` is identical from both
    /// sides, bit for bit, and every corner from every region touching it.
    fn assert_agree(graph: &RegionGraph, keys: impl Iterator<Item = RegionKey>) {
        for key in keys {
            let region = graph.region(key);
            for edge in &region.edges {
                let other = graph.region(key_of(edge.neighbor.unwrap()));
                let theirs = other.edges.iter().find(|e| e.id == edge.id);
                assert_eq!(theirs.map(|e| &e.polyline), Some(&edge.polyline), "{key:?}");
                assert_eq!(theirs.and_then(|e| e.neighbor), Some(region.id));
            }
            for corner in &region.corners {
                assert!(corner.incident.contains(&region.id));
                for &id in &corner.incident {
                    let other = graph.region(key_of(id));
                    assert!(other.corners.contains(corner), "{key:?}: {corner:?}");
                }
            }
        }
    }

    #[test]
    fn neighbours_agree_on_every_edge_and_corner() {
        for graph in graphs() {
            assert_agree(&graph, block(6));
        }
    }

    #[test]
    fn no_edge_is_shorter_than_the_minimum() {
        for graph in graphs() {
            let mut merged = 0;
            for key in block(10) {
                let region = graph.region(key);
                for edge in &region.edges {
                    assert!(
                        edge.length() >= graph.options.min_edge,
                        "{key:?}: {}",
                        edge.length()
                    );
                }
                merged += region
                    .corners
                    .iter()
                    .filter(|c| c.incident.len() > 3)
                    .count();
            }
            // With no jitter every edge is as long as the rest, so none merge.
            if graph.options.min_edge > 0.0 && graph.options.jitter > 0.0 {
                assert!(merged > 0, "the test graphs should have merged corners");
            }
        }
    }

    /// Checks that the regions within 5 cells of the middle cover the
    /// square `half` metres either way of the origin with no gaps or
    /// overlaps, and that `region_at` finds each point's region.
    fn assert_tiles(graph: &RegionGraph, half: f64) {
        let regions: Vec<Region> = block(5).map(|k| graph.region(k)).collect();
        let steps = 100;
        let step = 2.0 * half / f64::from(steps);
        for i in 0..steps {
            for j in 0..steps {
                let p = DVec2::new(f64::from(i) * step - half, f64::from(j) * step - half)
                    + DVec2::splat(0.37 * step);
                let holders: Vec<&Region> =
                    regions.iter().filter(|r| r.boundary.contains(p)).collect();
                if holders
                    .iter()
                    .any(|r| r.boundary.distance_to_border(p) < 1e-6)
                {
                    continue;
                }
                assert_eq!(holders.len(), 1, "{p} is in {} regions", holders.len());
                assert_eq!(graph.region_at(p).id(), holders[0].id, "{p}");
            }
        }
    }

    #[test]
    fn regions_tile_the_plane_and_region_at_finds_the_owner() {
        for graph in graphs() {
            assert_tiles(&graph, 4800.0);
        }
    }

    /// Checks that the outline of each of the regions `keys` is a simple
    /// polygon going anticlockwise, with no repeated points.
    fn assert_simple(graph: &RegionGraph, keys: impl Iterator<Item = RegionKey>) {
        let cross = |a: DVec2, b: DVec2, c: DVec2, d: DVec2| {
            let side = |p: DVec2, q: DVec2, r: DVec2| (q - p).perp_dot(r - p);
            let (s1, s2) = (side(c, d, a), side(c, d, b));
            let (s3, s4) = (side(a, b, c), side(a, b, d));
            if [s1, s2, s3, s4].iter().all(|&s| s == 0.0) {
                // On one line: they meet only if they overlap.
                let (lo, hi) = (a.min(b), a.max(b));
                let (lo2, hi2) = (c.min(d), c.max(d));
                return lo.cmple(hi2).all() && lo2.cmple(hi).all();
            }
            s1 * s2 <= 0.0 && s3 * s4 <= 0.0
        };
        for key in keys {
            let Boundary::Polygon(points) = graph.region(key).boundary else {
                panic!("lattice regions have polygons");
            };
            let n = points.len();
            assert!(n >= 3, "{key:?}: {n} points");
            // Anticlockwise, with no repeated points.
            let area: f64 = (0..n)
                .map(|i| points[i].perp_dot(points[(i + 1) % n]))
                .sum();
            assert!(area > 0.0, "{key:?}: area {area}");
            for i in 0..n {
                for j in i + 1..n {
                    assert_ne!(points[i], points[j], "{key:?}");
                }
            }
            // No side meets another, except its neighbours at their shared
            // corner.
            for i in 0..n {
                for j in i + 2..n {
                    if (j + 1) % n == i {
                        continue;
                    }
                    let (a, b) = (points[i], points[(i + 1) % n]);
                    let (c, d) = (points[j], points[(j + 1) % n]);
                    assert!(
                        !cross(a, b, c, d),
                        "{key:?}: sides {i} and {j} meet; {:?}; {:?}",
                        graph.options,
                        &points[i.saturating_sub(2)..(j + 3).min(n)]
                    );
                }
            }
        }
    }

    #[test]
    fn polygons_stay_simple_after_corners_merge() {
        for graph in graphs() {
            assert_simple(&graph, block(10));
        }
    }

    /// A regular lattice 2 km wide (no jitter, no merging) with the sites
    /// of the regions round the origin moved onto the circle of radius
    /// 1 km round it, at `points` (in millimetres, each 1 km from the
    /// origin unless moved off on purpose). No other site lies within 2 km
    /// of the origin, so the circle holds no site.
    fn on_a_circle(points: &[(i64, i64)]) -> (RegionGraph, Vec<RegionKey>) {
        let keys: Vec<RegionKey> = [(0, 0), (1, 0), (-1, 0), (0, 1), (-1, 1), (0, -1)]
            .iter()
            .take(points.len())
            .map(|&(x, z)| RegionKey(IVec2::new(x, z)))
            .collect();
        let placed: Vec<(RegionKey, I64Vec2)> = keys
            .iter()
            .zip(points)
            .map(|(&k, &(x, z))| (k, I64Vec2::new(x, z)))
            .collect();
        let options = RegionGraphOptions {
            size: 2000.0,
            jitter: 0.0,
            min_edge: 0.0,
            warp: STRAIGHT,
        };
        let graph = RegionGraph::unfloored(options, 1)
            .unwrap()
            .with_sites(&placed);
        (graph, keys)
    }

    /// Points on the circle of radius 1 km (1 000 000 mm) round the origin,
    /// at whole millimetres.
    const CIRCLE: [(i64, i64); 6] = [
        (1_000_000, 0),
        (600_000, 800_000),
        (-600_000, 800_000),
        (-1_000_000, 0),
        (-600_000, -800_000),
        (600_000, -800_000),
    ];

    #[test]
    fn sites_on_one_circle_make_one_corner() {
        for count in [5, 6] {
            let (graph, keys) = on_a_circle(&CIRCLE[..count]);
            // Exactly one raw corner holds all of them, and no other holds
            // three or more of them (no corner cuts across the circle).
            let corners = brute_force_corners(&graph);
            let together = |c: &Corner| keys.iter().filter(|k| c.contains(k)).count();
            let all: Vec<&Corner> = corners.iter().filter(|c| together(c) == count).collect();
            assert_eq!(all.len(), 1, "{count} sites: {all:?}");
            assert_eq!(all[0].len(), count);
            assert!(
                corners
                    .iter()
                    .all(|c| together(c) == count || together(c) <= 2)
            );
            assert_eq!(walked_corners(&graph), corners);
            assert_agree(&graph, block(3));
            assert_simple(&graph, block(3));
            assert_tiles(&graph, 4000.0);
        }
    }

    #[test]
    fn sites_nearly_on_one_circle_agree_everywhere() {
        // One site moved a millimetre (the smallest step sites take) off
        // the circle, out or in, or two moved opposite ways.
        let moves: [&[(usize, i64)]; 4] = [&[(0, 1)], &[(0, -1)], &[(2, 1), (4, -1)], &[(1, -1)]];
        for moved in moves {
            let mut points = CIRCLE;
            for &(i, by) in moved {
                points[i].0 += by;
            }
            let (graph, _) = on_a_circle(&points);
            assert_eq!(
                walked_corners(&graph),
                brute_force_corners(&graph),
                "{moved:?}"
            );
            assert_agree(&graph, block(3));
            assert_simple(&graph, block(3));
            assert_tiles(&graph, 4000.0);
            // And with corners merged.
            let merged = RegionGraph::unfloored(
                RegionGraphOptions {
                    min_edge: MIN_EDGE,
                    ..*graph.options()
                },
                1,
            )
            .unwrap()
            .with_sites(
                &graph
                    .placed
                    .iter()
                    .map(|(&k, &p)| (k, p))
                    .collect::<Vec<_>>(),
            );
            assert_agree(&merged, block(3));
            assert_simple(&merged, block(3));
        }
    }

    #[test]
    fn regions_are_the_same_built_on_several_threads_in_any_order() {
        let graph = RegionGraph::new(RegionGraphOptions::default(), 11).unwrap();
        let keys: Vec<RegionKey> = block(4).collect();
        let alone: Vec<Region> = keys.iter().map(|&k| graph.region(k)).collect();
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|t| {
                    let (graph, keys) = (&graph, &keys);
                    scope.spawn(move || {
                        // Each thread in its own order.
                        let mut order: Vec<usize> = (0..keys.len()).collect();
                        order.rotate_left(t * keys.len() / 4);
                        if t % 2 == 1 {
                            order.reverse();
                        }
                        let mut out = vec![None; keys.len()];
                        for i in order {
                            out[i] = Some(graph.region(keys[i]));
                        }
                        out
                    })
                })
                .collect();
            for handle in handles {
                let built = handle.join().unwrap();
                for (region, alone) in built.into_iter().zip(&alone) {
                    assert_eq!(region.as_ref(), Some(alone));
                }
            }
        });
    }

    #[test]
    fn the_graph_depends_only_on_its_settings_and_seed() {
        let options = RegionGraphOptions::default();
        let (a, b) = (
            RegionGraph::new(options, 5).unwrap(),
            RegionGraph::new(options, 5).unwrap(),
        );
        let key = RegionKey(IVec2::new(-3, 8));
        assert_eq!(a.region(key), b.region(key));
        let other = RegionGraph::new(options, 6).unwrap();
        assert_ne!(a.site(key), other.site(key));
        let unjittered = RegionGraph::new(
            RegionGraphOptions {
                jitter: 0.0,
                warp: STRAIGHT,
                ..options
            },
            5,
        )
        .unwrap();
        // With no jitter every region is a regular hexagon, to within the
        // half millimetre sites move to sit on whole millimetres.
        let region = unjittered.region(key);
        assert_eq!(region.edges.len(), 6);
        for edge in &region.edges {
            assert!((edge.length() - options.size / 3f64.sqrt()).abs() < 1e-3);
        }
    }

    #[test]
    fn settings_are_checked() {
        let ok = RegionGraphOptions::default();
        assert_eq!(ok.validate(), Ok(()));
        for (bad, error) in [
            (
                RegionGraphOptions { jitter: 0.9, ..ok },
                RegionGraphError::Jitter(0.9),
            ),
            (
                RegionGraphOptions { size: 1900.0, ..ok },
                RegionGraphError::Size(1900.0),
            ),
            (
                RegionGraphOptions {
                    min_edge: 300.0,
                    ..ok
                },
                RegionGraphError::MinEdge(300.0),
            ),
            (
                RegionGraphOptions {
                    size: f64::INFINITY,
                    ..ok
                },
                RegionGraphError::Size(f64::INFINITY),
            ),
            (
                RegionGraphOptions {
                    size: 60_000.0,
                    ..ok
                },
                RegionGraphError::Size(60_000.0),
            ),
        ] {
            assert_eq!(bad.validate(), Err(error));
        }
        // NaN anywhere is refused (and compares unequal, so match on it).
        for bad in [
            RegionGraphOptions {
                size: f64::NAN,
                ..ok
            },
            RegionGraphOptions {
                jitter: f64::NAN,
                ..ok
            },
            RegionGraphOptions {
                min_edge: f64::NAN,
                ..ok
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
        // Raw cells for tests may go below the floor, but not below 0.
        let raw = RegionGraphOptions {
            min_edge: 0.0,
            ..ok
        };
        assert_eq!(raw.validate_unfloored(), Ok(()));
        assert!(
            RegionGraphOptions {
                min_edge: -1.0,
                ..ok
            }
            .validate_unfloored()
            .is_err()
        );
        // So are the warp's settings.
        assert!(matches!(
            RegionGraphOptions {
                warp: BorderWarp {
                    amplitude: -1.0,
                    ..ok.warp
                },
                ..ok
            }
            .validate(),
            Err(RegionGraphError::Warp(BorderWarpError::Amplitude(_)))
        ));
    }

    /// The default graph at the most jitter, with the strongest warp.
    fn warped(seed: u32) -> RegionGraph {
        let options = RegionGraphOptions {
            jitter: MAX_JITTER,
            warp: strongest_warp(),
            ..Default::default()
        };
        RegionGraph::new(options, seed).unwrap()
    }

    /// The distance from `p` to the line through `a` and `b`.
    fn off_line(p: DVec2, a: DVec2, b: DVec2) -> f64 {
        (p - a).perp_dot(b - a).abs() / a.distance(b)
    }

    #[test]
    fn both_regions_get_the_same_warped_polyline_for_an_edge() {
        let graph = warped(3);
        let bits = |p: &[DVec2]| {
            p.iter()
                .map(|q| (q.x.to_bits(), q.y.to_bits()))
                .collect::<Vec<_>>()
        };
        let (mut edges, mut moved) = (0, 0);
        for key in block(3) {
            let region = graph.region(key);
            for edge in &region.edges {
                let other = graph.region(key_of(edge.neighbor.unwrap()));
                let theirs = other
                    .edges
                    .iter()
                    .find(|e| e.id == edge.id)
                    .expect("the neighbour has the edge");
                assert_eq!(bits(&theirs.polyline), bits(&edge.polyline), "{key:?}");
                // The edge really is warped, not left straight.
                let (a, b) = (edge.polyline[0], *edge.polyline.last().unwrap());
                let furthest = edge
                    .polyline
                    .iter()
                    .map(|&p| off_line(p, a, b))
                    .fold(0.0, f64::max);
                edges += 1;
                moved += usize::from(furthest > 10.0);
            }
        }
        assert!(
            2 * moved > edges,
            "only {moved} of {edges} edges are warped"
        );
    }

    #[test]
    fn warped_borders_keep_their_distance() {
        // Borders that don't meet at a corner stay at least the shortest
        // edge apart, or as far apart as they were if that was less, so no
        // region closes up.
        for seed in [3, 8] {
            let graph = warped(seed);
            for key in block(2) {
                let region = graph.region(key);
                for (i, e) in region.edges.iter().enumerate() {
                    for g in &region.edges[i + 1..] {
                        if [e.start, e.end]
                            .iter()
                            .any(|c| [g.start, g.end].contains(c))
                        {
                            continue;
                        }
                        let chord = |r: &RegionEdge| (r.polyline[0], *r.polyline.last().unwrap());
                        let ((a, b), (c, d)) = (chord(e), chord(g));
                        let straight = segment_distance(a, b, c, d);
                        let warped = e
                            .polyline
                            .windows(2)
                            .flat_map(|p| {
                                g.polyline
                                    .windows(2)
                                    .map(move |q| segment_distance(p[0], p[1], q[0], q[1]))
                            })
                            .fold(f64::INFINITY, f64::min);
                        assert!(
                            warped >= straight.min(MIN_EDGE) - 1e-6,
                            "{key:?}: {warped} m apart, {straight} m unwarped"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn edges_held_back_by_the_edges_around_them_are_counted() {
        // How often an edge gets less than the full warp, and how often
        // none, since short or sharply angled edges are held back.
        for (name, warp) in [
            ("default", BorderWarp::default()),
            ("strongest", strongest_warp()),
        ] {
            for jitter in [0.6, MAX_JITTER] {
                let options = RegionGraphOptions {
                    jitter,
                    warp,
                    ..Default::default()
                };
                let graph = RegionGraph::new(options, 3).unwrap();
                let (mut edges, mut half, mut none) = (0, 0, 0);
                for key in block(5) {
                    let mut local = Local::new(&graph);
                    let neighbours: Vec<RegionKey> =
                        local.sides(key).iter().map(|s| s.neighbour).collect();
                    for neighbour in neighbours.into_iter().filter(|&n| key < n) {
                        let amplitude = graph.warp_amplitude(key, neighbour).unwrap();
                        edges += 1;
                        half += usize::from(amplitude < 0.5 * warp.amplitude);
                        none += usize::from(amplitude == 0.0);
                    }
                }
                println!(
                    "{name} warp, jitter {jitter}: {edges} edges, {half} under half the \
                     amplitude, {none} not warped"
                );
                // Measured: 6–11% held under half, none unwarped.
                assert!(
                    100 * half <= 15 * edges,
                    "{half} of {edges} held under half"
                );
                assert!(50 * none <= edges, "{none} of {edges} not warped");
            }
        }
    }
}
