//! The region graph: an endless tiling of the plane into regions, worked
//! out from position alone, so any region is known without building the
//! others, in any order.
//!
//! Each region is the Voronoi cell of one site, jittered inside its cell
//! of a staggered (hexagonal) lattice. Corners closer together than a
//! minimum edge length are merged into one corner touched by four or more
//! regions.

// Used by WorldTerrain (phase W4); until then only the tests use it.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::{BTreeSet, HashMap};

use glam::{DVec2, IVec2};

use crate::hash::{derive_seed, hash2};
use crate::ports::CORNER_CLEARANCE;
use crate::region::{Boundary, Region, RegionCorner, RegionEdge, RegionId};

/// The most a site may move from its lattice point, as a share of half
/// the lattice spacing along each axis.
pub(crate) const MAX_JITTER: f64 = 0.8;

/// How many lattice cells either way are searched for a region's
/// neighbours. Enough for [`MAX_JITTER`]: a site's neighbours lie within
/// about 2.3 lattice spacings of it.
const WINDOW: i32 = 3;

/// The distance between lattice rows, as a share of the spacing along a
/// row: the rows of a hexagonal lattice.
const ROW: f64 = 0.866_025_403_784_438_6;

/// A region of the graph: the lattice cell of its site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RegionKey(pub IVec2);

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
    /// The region's id: its lattice cell, packed. Different keys give
    /// different ids.
    pub(crate) fn id(self) -> RegionId {
        RegionId((u64::from(self.0.x as u32) << 32) | u64::from(self.0.y as u32))
    }
}

/// How the plane is split into regions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RegionGraphOptions {
    /// The lattice spacing, about the width of a region, in metres.
    pub size: f64,
    /// How far each site moves from its lattice point, from 0 (a regular
    /// hexagonal tiling) to [`MAX_JITTER`].
    pub jitter: f64,
    /// Edges shorter than this, in metres, are merged away: their two
    /// corners become one.
    pub min_edge: f64,
}

impl Default for RegionGraphOptions {
    fn default() -> Self {
        Self {
            size: 4000.0,
            jitter: 0.6,
            min_edge: 2.0 * CORNER_CLEARANCE,
        }
    }
}

/// Why region graph settings were refused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum GraphError {
    /// The size must be finite and at least 5 times the minimum edge.
    Size(f64),
    /// The jitter must be from 0 to [`MAX_JITTER`].
    Jitter(f64),
    /// The minimum edge must be finite and not negative.
    MinEdge(f64),
}

impl RegionGraphOptions {
    /// Checks the settings.
    pub(crate) fn validate(&self) -> Result<(), GraphError> {
        if !self.min_edge.is_finite() || self.min_edge < 0.0 {
            return Err(GraphError::MinEdge(self.min_edge));
        }
        if !self.size.is_finite() || self.size <= 0.0 || self.size < 5.0 * self.min_edge {
            return Err(GraphError::Size(self.size));
        }
        if !(0.0..=MAX_JITTER).contains(&self.jitter) {
            return Err(GraphError::Jitter(self.jitter));
        }
        Ok(())
    }
}

/// The regions of a world.
#[derive(Debug, Clone)]
pub(crate) struct RegionGraph {
    options: RegionGraphOptions,
    seed: u32,
}

/// The three regions whose sites define a raw Voronoi corner, sorted.
type Triple = [RegionKey; 3];

impl RegionGraph {
    /// The graph for `options` and the world's `seed`.
    pub(crate) fn new(options: RegionGraphOptions, seed: u32) -> Result<Self, GraphError> {
        options.validate()?;
        Ok(Self { options, seed })
    }

    /// The site of region `key`, in metres.
    pub(crate) fn site(&self, key: RegionKey) -> DVec2 {
        let IVec2 { x, y: z } = key.0;
        let size = self.options.size;
        let stagger = if z.rem_euclid(2) == 1 { 0.5 } else { 0.0 };
        let point = DVec2::new((f64::from(x) + stagger) * size, f64::from(z) * ROW * size);
        let unit = |salt: u32| {
            let h = hash2(i64::from(x), i64::from(z), derive_seed(self.seed, salt));
            f64::from(h) / f64::from(u32::MAX) * 2.0 - 1.0
        };
        point + DVec2::new(unit(1), unit(2)) * (0.5 * self.options.jitter * size)
    }

    /// The lattice cell nearest `p`: the key of a region whose site is
    /// near `p`, though not always the region holding `p`.
    fn cell_near(&self, p: DVec2) -> RegionKey {
        let size = self.options.size;
        let z = (p.y / (ROW * size)).round() as i32;
        let stagger = if z.rem_euclid(2) == 1 { 0.5 } else { 0.0 };
        let x = (p.x / size - stagger).round() as i32;
        RegionKey(IVec2::new(x, z))
    }

    /// The region `key`: its polygon, edges and corners. Both regions on an
    /// edge get identical edges, and every region at a corner gets the
    /// identical corner.
    pub(crate) fn region(&self, key: RegionKey) -> Region {
        let mut local = Local::new(self);
        let ring = local.ring(key).to_vec();
        let n = ring.len();

        // Each raw corner of the cell, merged with any corners near it.
        let corners: Vec<RegionCorner> = (0..n)
            .map(|i| {
                let triple = sorted([key, ring[(i + n - 1) % n], ring[i]]);
                local.corner(triple)
            })
            .collect();

        // Sides whose two ends merged into one corner vanish.
        let mut points = Vec::with_capacity(n);
        let mut edges = Vec::with_capacity(n);
        let mut kept = Vec::with_capacity(n);
        for i in 0..n {
            let (a, b) = (&corners[i], &corners[(i + 1) % n]);
            if a.id == b.id {
                continue;
            }
            points.push(a.position);
            edges.push(
                RegionEdge::new(key.id(), Some(ring[i].id()), [a.position, b.position])
                    .expect("two distinct corners make an edge"),
            );
            if !kept.iter().any(|c: &RegionCorner| c.id == a.id) {
                kept.push(a.clone());
            }
        }
        Region {
            id: key.id(),
            boundary: Boundary::Polygon(points),
            edges,
            corners: kept,
        }
    }

    /// The region holding `p`. Points on a border may go to either side.
    pub(crate) fn region_at(&self, p: DVec2) -> RegionKey {
        // The few regions with the nearest sites; one of them holds `p`.
        let near = self.cell_near(p);
        let mut candidates: Vec<(f64, RegionKey)> = (-2..=2)
            .flat_map(|dz| (-2..=2).map(move |dx| RegionKey(near.0 + IVec2::new(dx, dz))))
            .map(|k| (self.site(k).distance_squared(p), k))
            .collect();
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        candidates
            .iter()
            .take(4)
            .map(|&(_, k)| k)
            .find(|&k| self.region(k).boundary.contains(p))
            .unwrap_or(candidates[0].1)
    }
}

/// Raw Voronoi cells worked out while building one region, kept so each
/// is worked out once.
struct Local<'a> {
    graph: &'a RegionGraph,
    rings: HashMap<RegionKey, Vec<RegionKey>>,
}

impl<'a> Local<'a> {
    fn new(graph: &'a RegionGraph) -> Self {
        Self {
            graph,
            rings: HashMap::new(),
        }
    }

    /// The regions whose raw Voronoi cells share a side with `key`'s, in
    /// counterclockwise order around it.
    fn ring(&mut self, key: RegionKey) -> &[RegionKey] {
        let graph = self.graph;
        self.rings
            .entry(key)
            .or_insert_with(|| raw_ring(graph, key, WINDOW))
    }

    /// The raw Voronoi corner of `triple`, from its three sites in sorted
    /// order, so every region computes the same bits.
    fn position(&self, triple: Triple) -> DVec2 {
        let [a, b, c] = triple.map(|k| self.graph.site(k));
        circumcentre(a, b, c)
    }

    /// The corner that raw corner `triple` belongs to, once every raw
    /// corner joined to it by an edge shorter than the minimum is merged
    /// in: at the mean of the merged raw corners, touched by every region
    /// touching any of them.
    fn corner(&mut self, triple: Triple) -> RegionCorner {
        let min_edge = self.graph.options.min_edge;
        let mut merged: BTreeSet<Triple> = BTreeSet::from([triple]);
        let mut todo = vec![triple];
        while let Some(t) = todo.pop() {
            let here = self.position(t);
            for (a, b, c) in [(t[0], t[1], t[2]), (t[1], t[2], t[0]), (t[0], t[2], t[1])] {
                let other = self.across(a, b, c);
                if !merged.contains(&other) && here.distance(self.position(other)) < min_edge {
                    merged.insert(other);
                    todo.push(other);
                }
            }
        }
        let sum = merged
            .iter()
            .fold(DVec2::ZERO, |sum, &t| sum + self.position(t));
        let incident = merged.iter().flatten().map(|k| k.id());
        RegionCorner::new(sum / merged.len() as f64, incident)
    }

    /// The raw corner at the other end of the side between `a` and `b`
    /// from the raw corner of `a`, `b` and `c`.
    fn across(&mut self, a: RegionKey, b: RegionKey, c: RegionKey) -> Triple {
        let ring = self.ring(a);
        let n = ring.len();
        let j = ring
            .iter()
            .position(|&k| k == b)
            .expect("neighbouring cells list each other");
        let (before, after) = (ring[(j + n - 1) % n], ring[(j + 1) % n]);
        sorted([a, b, if before == c { after } else { before }])
    }
}

/// The raw Voronoi cell of `key`, as the regions whose cells share a side
/// with it, in counterclockwise order, from the sites within `window`
/// lattice cells either way.
fn raw_ring(graph: &RegionGraph, key: RegionKey, window: i32) -> Vec<RegionKey> {
    // A square well beyond the cell, cut down by the half-plane nearer
    // `key` than each other site. Each point carries the region whose
    // bisector the side starting there lies on.
    let site = graph.site(key);
    let r = 4.0 * graph.options.size;
    let mut cell: Vec<(DVec2, Option<RegionKey>)> =
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(x, z)| (site + DVec2::new(x, z) * r, None))
            .to_vec();
    for dz in -window..=window {
        for dx in -window..=window {
            if (dx, dz) == (0, 0) {
                continue;
            }
            let other = RegionKey(key.0 + IVec2::new(dx, dz));
            cell = clip(&cell, site, graph.site(other), other);
        }
    }
    cell.into_iter()
        .map(|(_, by)| by.expect("the starting square lies far outside the cell"))
        .collect()
}

/// `cell` cut down to the points nearer `site` than `other_site`, whose
/// region is `other`.
fn clip(
    cell: &[(DVec2, Option<RegionKey>)],
    site: DVec2,
    other_site: DVec2,
    other: RegionKey,
) -> Vec<(DVec2, Option<RegionKey>)> {
    let middle = 0.5 * (site + other_site);
    let normal = other_site - site;
    let side = |p: DVec2| (p - middle).dot(normal);
    let mut out = Vec::with_capacity(cell.len() + 1);
    for (i, &(a, by)) in cell.iter().enumerate() {
        let b = cell[(i + 1) % cell.len()].0;
        let (sa, sb) = (side(a), side(b));
        let crossing = || a + (b - a) * (sa / (sa - sb));
        match (sa <= 0.0, sb <= 0.0) {
            (true, true) => out.push((a, by)),
            (true, false) => {
                out.push((a, by));
                out.push((crossing(), Some(other)));
            }
            (false, true) => out.push((crossing(), by)),
            (false, false) => {}
        }
    }
    out
}

/// The centre of the circle through `a`, `b` and `c`.
fn circumcentre(a: DVec2, b: DVec2, c: DVec2) -> DVec2 {
    let (b, c) = (b - a, c - a);
    let d = 2.0 * b.perp_dot(c);
    let (bb, cc) = (b.length_squared(), c.length_squared());
    a + DVec2::new(c.y * bb - b.y * cc, b.x * cc - c.x * bb) / d
}

/// `keys` in ascending order.
fn sorted(mut keys: Triple) -> Triple {
    keys.sort_unstable();
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Graphs at the default jitter and at the most, with a short and the
    /// default minimum edge.
    fn graphs() -> Vec<RegionGraph> {
        let mut out = Vec::new();
        for jitter in [0.6, MAX_JITTER] {
            for min_edge in [0.0, 2.0 * CORNER_CLEARANCE] {
                let options = RegionGraphOptions {
                    size: 2000.0,
                    jitter,
                    min_edge,
                };
                out.push(RegionGraph::new(options, 7).unwrap());
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
        RegionKey(IVec2::new((id.0 >> 32) as u32 as i32, id.0 as u32 as i32))
    }

    #[test]
    fn the_neighbour_window_is_wide_enough() {
        let graph = RegionGraph::new(
            RegionGraphOptions {
                jitter: MAX_JITTER,
                ..Default::default()
            },
            3,
        )
        .unwrap();
        for key in block(12) {
            assert_eq!(
                raw_ring(&graph, key, WINDOW),
                raw_ring(&graph, key, 7),
                "{key:?}"
            );
        }
    }

    #[test]
    fn neighbours_agree_on_every_edge_and_corner() {
        for graph in graphs() {
            for key in block(6) {
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
            if graph.options.min_edge > 0.0 {
                assert!(merged > 0, "the test graphs should have merged corners");
            }
        }
    }

    #[test]
    fn regions_tile_the_plane_and_region_at_finds_the_owner() {
        for graph in graphs() {
            let regions: Vec<Region> = block(5).map(|k| graph.region(k)).collect();
            let step = 97.0;
            for i in 0..100 {
                for j in 0..100 {
                    let p = DVec2::new(f64::from(i) * step - 4800.0, f64::from(j) * step - 4800.0);
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
    }

    #[test]
    fn polygons_stay_simple_after_corners_merge() {
        let cross = |a: DVec2, b: DVec2, c: DVec2, d: DVec2| {
            let side = |p: DVec2, q: DVec2, r: DVec2| (q - p).perp_dot(r - p);
            side(c, d, a) * side(c, d, b) <= 0.0 && side(a, b, c) * side(a, b, d) <= 0.0
        };
        for graph in graphs() {
            for key in block(10) {
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
                // No side meets another, except its neighbours at their
                // shared corner.
                for i in 0..n {
                    for j in i + 2..n {
                        if (j + 1) % n == i {
                            continue;
                        }
                        let (a, b) = (points[i], points[(i + 1) % n]);
                        let (c, d) = (points[j], points[(j + 1) % n]);
                        assert!(!cross(a, b, c, d), "{key:?}: sides {i} and {j} meet");
                    }
                }
            }
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
                ..options
            },
            5,
        )
        .unwrap();
        // With no jitter every region is a regular hexagon.
        let region = unjittered.region(key);
        assert_eq!(region.edges.len(), 6);
        for edge in &region.edges {
            assert!((edge.length() - options.size / 3f64.sqrt()).abs() < 1e-6);
        }
    }

    #[test]
    fn settings_are_checked() {
        let ok = RegionGraphOptions::default();
        assert_eq!(ok.validate(), Ok(()));
        for (bad, error) in [
            (
                RegionGraphOptions { jitter: 0.9, ..ok },
                GraphError::Jitter(0.9),
            ),
            (
                RegionGraphOptions { size: 1900.0, ..ok },
                GraphError::Size(1900.0),
            ),
            (
                RegionGraphOptions {
                    min_edge: -1.0,
                    ..ok
                },
                GraphError::MinEdge(-1.0),
            ),
        ] {
            assert_eq!(bad.validate(), Err(error));
        }
    }
}
