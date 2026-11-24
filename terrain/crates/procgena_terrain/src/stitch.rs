//! A small reference world builder: joins terrains on polygon regions.
//!
//! The real world generator is a separate project. This one exists so the
//! crate can be tested and demonstrated end to end: it finds shared edges and
//! corners, collects offers, negotiates and builds every terrain.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use glam::DVec2;

use crate::converge::{
    BorderTarget, Convergence, EdgeTarget, blend_weights, relief_meeting_is_valid, relief_target,
};
use crate::elevation::Elevation;
use crate::options::{OptionsError, RoadGrades, TerrainOptions};
use crate::region::{Boundary, CornerId, Region, RegionCorner, RegionEdge, RegionError, RegionId};
use crate::seam::{CornerAgreement, EdgeAgreement, SeamError, negotiate, negotiate_corner};
use crate::terrain::{HeightField, RiverTrace, Terrain, TerrainError, TerrainGenerator};

/// One region of a [`StitchedWorld`]: its polygon and terrain settings.
#[derive(Debug, Clone)]
pub struct RegionSpec {
    /// The region's id, unique in the world.
    pub id: RegionId,
    /// The region's outline as a simple polygon, in metres, without
    /// repeating the first point. Neighbouring regions must use exactly the
    /// same points along their shared borders.
    pub polygon: Vec<DVec2>,
    /// The terrain settings for this region.
    pub options: TerrainOptions,
}

/// A world that could not be stitched.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum StitchError {
    /// The relief meeting exponent is above 1 or NaN.
    ReliefMeeting(f32),
    /// The relief blend is outside 0 to 1, or NaN.
    ReliefBlend(f32),
    /// Two regions have the same id.
    DuplicateId(RegionId),
    /// A polygon has fewer than three points.
    TooFewPoints(RegionId),
    /// One polygon side is shared by more than two regions.
    Overlap(RegionId),
    /// A region is entirely surrounded by one neighbour; regions with holes
    /// aren't supported.
    Enclosed(RegionId),
    /// A region's settings are invalid.
    Options(RegionId, OptionsError),
    /// A border edge could not be built.
    Region(RegionId, RegionError),
    /// Negotiation failed.
    Seam(SeamError),
    /// A terrain could not be built.
    Terrain(RegionId, TerrainError),
}

impl fmt::Display for StitchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReliefMeeting(p) => {
                write!(f, "relief meeting exponent {p} is above 1 or NaN")
            }
            Self::ReliefBlend(b) => write!(f, "relief blend {b} is outside 0 to 1"),
            Self::DuplicateId(id) => write!(f, "region id {} is used twice", id.0),
            Self::TooFewPoints(id) => write!(f, "region {} needs at least three points", id.0),
            Self::Overlap(id) => {
                write!(f, "region {} shares a side with more than one region", id.0)
            }
            Self::Enclosed(id) => write!(f, "region {} is surrounded by a single neighbour", id.0),
            Self::Options(id, e) => write!(f, "region {}: {e}", id.0),
            Self::Region(id, e) => write!(f, "region {}: {e}", id.0),
            Self::Seam(e) => write!(f, "negotiation failed: {e}"),
            Self::Terrain(id, e) => write!(f, "region {}: {e}", id.0),
        }
    }
}

impl std::error::Error for StitchError {}

/// Terrains on polygon regions, joined along their shared borders.
#[derive(Debug, Clone)]
pub struct StitchedWorld {
    terrains: Vec<Terrain>,
    agreements: Vec<EdgeAgreement>,
}

/// Points this close to a region's border (metres) belong to it even if the
/// inside test misses them.
const ON_BORDER: f64 = 1e-6;

/// A vertex position as exact bits, with −0.0 counted as +0.0.
type Key = (u64, u64);

fn key(p: DVec2) -> Key {
    let bits = |v: f64| if v == 0.0 { 0 } else { v.to_bits() };
    (bits(p.x), bits(p.y))
}

/// A polygon side, the same whichever direction it is walked.
fn side_key(a: DVec2, b: DVec2) -> (Key, Key) {
    let (ka, kb) = (key(a), key(b));
    if ka <= kb { (ka, kb) } else { (kb, ka) }
}

impl StitchedWorld {
    /// Builds every region's border graph, negotiates all edges and corners,
    /// and builds each terrain.
    ///
    /// Before negotiating, neighbouring terrains become alike near each
    /// shared edge and corner, as `convergence` says
    /// ([`Convergence::default`] unless you are experimenting). Every
    /// terrain stands on `elevation`; pass [`Elevation::flat`] for land at
    /// sea level.
    ///
    /// # Errors
    ///
    /// If `convergence.meeting` isn't valid (see
    /// [`relief_meeting_is_valid`](crate::relief_meeting_is_valid)), if
    /// `convergence.blend` is outside 0 to 1, or for any of the other
    /// [`StitchError`] cases.
    pub fn build(
        mut specs: Vec<RegionSpec>,
        convergence: &Convergence,
        elevation: &Elevation,
    ) -> Result<Self, StitchError> {
        if !relief_meeting_is_valid(convergence.meeting) {
            return Err(StitchError::ReliefMeeting(convergence.meeting));
        }
        if !(0.0..=1.0).contains(&convergence.blend) {
            return Err(StitchError::ReliefBlend(convergence.blend));
        }
        specs.sort_by_key(|s| s.id);
        for pair in specs.windows(2) {
            if pair[0].id == pair[1].id {
                return Err(StitchError::DuplicateId(pair[0].id));
            }
        }

        // Index vertices and sides by the regions that use them.
        let mut vertex_regions: HashMap<Key, Vec<RegionId>> = HashMap::new();
        let mut side_regions: HashMap<(Key, Key), Vec<RegionId>> = HashMap::new();
        for spec in &specs {
            let n = spec.polygon.len();
            if n < 3 {
                return Err(StitchError::TooFewPoints(spec.id));
            }
            for i in 0..n {
                let (a, b) = (spec.polygon[i], spec.polygon[(i + 1) % n]);
                vertex_regions.entry(key(a)).or_default().push(spec.id);
                side_regions
                    .entry(side_key(a, b))
                    .or_default()
                    .push(spec.id);
            }
        }

        // Each region's edges and corners.
        let mut regions = Vec::with_capacity(specs.len());
        for spec in &specs {
            regions.push(region_graph(spec, &vertex_regions, &side_regions)?);
        }

        // Generators, with their relief converging towards their neighbours'.
        let generators = specs
            .iter()
            .map(|s| {
                TerrainGenerator::new(s.options.clone())
                    .map(|g| g.with_elevation(elevation.clone()))
                    .map_err(|e| StitchError::Options(s.id, e))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let index: BTreeMap<RegionId, usize> =
            specs.iter().enumerate().map(|(i, s)| (s.id, i)).collect();
        let generators = converge(&regions, generators, &index, convergence)?;

        // Offers and agreements, from each region's river trace, kept for
        // the build.
        let traces: Vec<RiverTrace> = regions
            .iter()
            .zip(&generators)
            .map(|(region, generator)| generator.trace_rivers(region))
            .collect();
        let corners = negotiate_corners(&regions, &generators, &index)?;
        let agreements =
            negotiate_edges(&regions, &generators, &traces, &index, &corners, elevation)?;

        // Build each terrain with the agreements for its edges, on several
        // threads with the `rayon` feature (the results are the same).
        let build = |((generator, region), trace): ((TerrainGenerator, Region), RiverTrace)| {
            let mine: Vec<_> = region
                .edges
                .iter()
                .filter_map(|e| agreements.get(&e.id).cloned())
                .collect();
            let id = region.id;
            generator
                .build_traced(region, &mine, &trace)
                .map(Terrain::with_roads)
                .map_err(|e| StitchError::Terrain(id, e))
        };
        let pairs: Vec<((TerrainGenerator, Region), RiverTrace)> =
            generators.into_iter().zip(regions).zip(traces).collect();
        #[cfg(feature = "rayon")]
        let terrains = {
            use rayon::prelude::*;
            pairs
                .into_par_iter()
                .map(build)
                .collect::<Result<Vec<_>, _>>()?
        };
        #[cfg(not(feature = "rayon"))]
        let terrains = pairs
            .into_iter()
            .map(build)
            .collect::<Result<Vec<_>, _>>()?;
        let mut agreements: Vec<EdgeAgreement> = agreements.into_values().collect();
        agreements.sort_by_key(|a| a.edge);
        Ok(Self {
            terrains,
            agreements,
        })
    }

    /// Every shared edge's agreement, sorted by edge id.
    pub fn agreements(&self) -> &[EdgeAgreement] {
        &self.agreements
    }

    /// The world's terrains, sorted by region id.
    pub fn terrains(&self) -> &[Terrain] {
        &self.terrains
    }

    /// The terrain whose region contains `p`, if any. On a shared border
    /// either side may be returned; both give the same height there.
    pub fn owner(&self, p: DVec2) -> Option<&Terrain> {
        if let Some(inside) = self
            .terrains
            .iter()
            .find(|t| t.region().boundary.contains(p))
        {
            return Some(inside);
        }
        // Points exactly on a border can fail every inside test; give them to
        // the nearest region.
        self.terrains
            .iter()
            .map(|t| (t.region().boundary.distance_to_border(p), t))
            .filter(|(d, _)| *d <= ON_BORDER)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, t)| t)
    }
}

impl HeightField for StitchedWorld {
    /// The owning terrain's height at `p`, or NaN outside every region.
    fn height(&self, p: DVec2) -> f32 {
        self.owner(p).map_or(f32::NAN, |t| t.height(p))
    }
}

/// Splits one region's outline into edges, one per neighbour run, and finds
/// their corners.
fn region_graph(
    spec: &RegionSpec,
    vertex_regions: &HashMap<Key, Vec<RegionId>>,
    side_regions: &HashMap<(Key, Key), Vec<RegionId>>,
) -> Result<Region, StitchError> {
    // The neighbour across each side.
    let poly = &spec.polygon;
    let n = poly.len();
    let mut across = Vec::with_capacity(n);
    for i in 0..n {
        let users = &side_regions[&side_key(poly[i], poly[(i + 1) % n])];
        let others: Vec<RegionId> = users.iter().copied().filter(|&r| r != spec.id).collect();
        if others.len() > 1 || users.len() > 2 {
            return Err(StitchError::Overlap(spec.id));
        }
        across.push(others.first().copied());
    }

    // Start at a side where the neighbour changes, so runs don't wrap.
    let boundary = Boundary::Polygon(poly.clone());
    let Some(start) = (0..n).find(|&i| across[(i + n - 1) % n] != across[i]) else {
        if across[0].is_some() {
            return Err(StitchError::Enclosed(spec.id));
        }
        // Nothing borders this region at all.
        return Ok(Region {
            id: spec.id,
            boundary,
            edges: Vec::new(),
            corners: Vec::new(),
        });
    };

    // Walk the runs.
    let mut edges = Vec::new();
    let mut corners: Vec<RegionCorner> = Vec::new();
    let mut i = 0;
    while i < n {
        let first = (start + i) % n;
        let neighbor = across[first];
        let mut points = vec![poly[first]];
        while i < n && across[(start + i) % n] == neighbor {
            points.push(poly[(start + i + 1) % n]);
            i += 1;
        }
        for end in [points[0], points[points.len() - 1]] {
            let corner = RegionCorner::new(end, vertex_regions[&key(end)].iter().copied());
            if !corners.iter().any(|c| c.id == corner.id) {
                corners.push(corner);
            }
        }
        edges.push(
            RegionEdge::new(spec.id, neighbor, points)
                .map_err(|e| StitchError::Region(spec.id, e))?,
        );
    }
    Ok(Region {
        id: spec.id,
        boundary,
        edges,
        corners,
    })
}

/// Gives each generator targets for its shared edges, from the regions on
/// each edge and touching each corner: the [`relief_target`] and
/// [`blend_weights`] of their amplitudes, and the steepest of their road
/// grades.
fn converge(
    regions: &[Region],
    generators: Vec<TerrainGenerator>,
    index: &BTreeMap<RegionId, usize>,
    convergence: &Convergence,
) -> Result<Vec<TerrainGenerator>, StitchError> {
    let meeting = convergence.meeting;
    let amplitudes: Vec<f32> = generators.iter().map(|g| g.relief_amplitude()).collect();
    let roads: Vec<RoadGrades> = generators
        .iter()
        .map(|g| g.options().features.roads.grades())
        .collect();
    let options: Vec<(RegionId, TerrainOptions)> = regions
        .iter()
        .zip(&generators)
        .map(|(r, g)| (r.id, g.options().clone()))
        .collect();
    let target = |touching: &[RegionId]| {
        let mut touching = touching.to_vec();
        touching.sort();
        let amplitudes: Vec<f32> = touching.iter().map(|r| amplitudes[index[r]]).collect();
        let roads = touching
            .iter()
            .map(|r| roads[index[r]])
            .reduce(RoadGrades::steepest)
            .expect("a border has regions");
        BorderTarget {
            amplitude: relief_target(&amplitudes, meeting),
            blend: touching
                .iter()
                .copied()
                .zip(blend_weights(&amplitudes, meeting))
                .collect(),
            roads,
        }
    };

    // Corner targets, from every region touching the corner.
    let mut corners: HashMap<CornerId, BorderTarget> = HashMap::new();
    for corner in regions.iter().flat_map(|r| &r.corners) {
        corners
            .entry(corner.id)
            .or_insert_with(|| target(&corner.incident));
    }

    // Edge targets, and each generator converged towards its own.
    generators
        .into_iter()
        .zip(regions)
        .map(|(generator, region)| {
            let targets: Vec<EdgeTarget> = region
                .edges
                .iter()
                .filter_map(|edge| {
                    let neighbor = edge.neighbor?;
                    Some(EdgeTarget {
                        edge: edge.id,
                        middle: target(&[region.id, neighbor]),
                        start: corners[&edge.start].clone(),
                        end: corners[&edge.end].clone(),
                    })
                })
                .collect();
            generator
                .with_edge_targets(region, &targets, &options, convergence.blend)
                .map_err(|e| StitchError::Terrain(region.id, e))
        })
        .collect()
}

/// Agrees on every corner, once, from all regions touching it.
fn negotiate_corners(
    regions: &[Region],
    generators: &[TerrainGenerator],
    index: &BTreeMap<RegionId, usize>,
) -> Result<HashMap<crate::region::CornerId, CornerAgreement>, StitchError> {
    let mut agreements = HashMap::new();
    for region in regions {
        for corner in &region.corners {
            if agreements.contains_key(&corner.id) {
                continue;
            }
            let offers: Vec<_> = corner
                .incident
                .iter()
                .map(|r| {
                    let i = index[r];
                    generators[i].corner_offer(&regions[i], corner)
                })
                .collect();
            let agreed = negotiate_corner(corner, &offers).map_err(StitchError::Seam)?;
            agreements.insert(corner.id, agreed);
        }
    }
    Ok(agreements)
}

/// Agrees on every shared edge, once, from the two regions on it.
fn negotiate_edges(
    regions: &[Region],
    generators: &[TerrainGenerator],
    traces: &[RiverTrace],
    index: &BTreeMap<RegionId, usize>,
    corners: &HashMap<crate::region::CornerId, CornerAgreement>,
    elevation: &Elevation,
) -> Result<HashMap<crate::region::EdgeId, crate::seam::EdgeAgreement>, StitchError> {
    // Every region's offers, once each, keyed by edge.
    let offers: Vec<HashMap<_, _>> = regions
        .iter()
        .zip(generators)
        .zip(traces)
        .map(|((region, generator), trace)| {
            generator
                .offers_traced(region, trace)
                .into_iter()
                .map(|o| (o.edge, o))
                .collect()
        })
        .collect();

    let mut agreements = HashMap::new();
    for (i, region) in regions.iter().enumerate() {
        for edge in &region.edges {
            let Some(neighbor) = edge.neighbor else {
                continue;
            };
            if agreements.contains_key(&edge.id) {
                continue;
            }
            let j = index[&neighbor];
            let (a, b) = (&offers[i][&edge.id], &offers[j][&edge.id]);
            let agreed = negotiate(
                edge,
                a,
                b,
                &corners[&edge.start],
                &corners[&edge.end],
                elevation,
            )
            .map_err(StitchError::Seam)?;
            agreements.insert(edge.id, agreed);
        }
    }
    Ok(agreements)
}
