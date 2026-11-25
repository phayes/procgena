//! Terrain generators, and terrains bound to a region.

use std::fmt;
use std::sync::Arc;

use glam::{DVec2, Vec2};
use procgena_core::{Ground, GroundSample};

use crate::border::{Pull, SegmentIndex, pull_gradients, surely_beyond};
use crate::carve::{Carved, FeatureKind, FeatureLine, Features, PointOfInterest, Pond, RoadTags};
use crate::converge::{EdgeTarget, Fade, FadeOwn, total_spread};
use crate::elevation::Elevation;
use crate::feature::{FeatureId, FeatureTier, TerrainFeature};
use crate::hash::{SEAM_DETAIL, salt};
use crate::label::{Labels, Shape, Site, classify, principal_bends, shape_points};
use crate::measure::{
    CURVATURE_STEP, MEASURE_REACH, Measures, SampleDetail, curvature_of, slope_of,
};
use crate::noise::{Octaves, fbm, fbm_gradient};
use crate::options::{OptionsError, RoadGrades, TerrainOptions};
use crate::plan;
use crate::ports::{INNER_DEPTHS, Side, inner_depth, slope_reach};
use crate::region::{
    EdgeId, Region, RegionCorner, RegionEdge, RegionId, Segment, point_at, segments,
};
use crate::relief::Relief;
use crate::scale::WorldScale;
use crate::seam::{
    CornerOffer, EDGE_STEP, EdgeAgreement, EdgeOffer, EdgeSample, Profile, elevation_samples,
    relative_to, sample_count, smooth, smootherstep, smootherstep_slope,
};

/// Distances below this (metres) count as on the edge.
pub(crate) const ON_EDGE: f64 = 1e-9;

/// How far border edges pull on the ground, as a multiple of the margin.
/// Twice the margin keeps the pull's fade to zero well beyond where the
/// bending ends.
pub(crate) const BORDER_REACH: f64 = 2.0;
/// Width of the strip next to a border where a terrain's own fine detail
/// gives way to the shared border detail, in metres (at most the margin).
const DETAIL_BAND: f64 = 48.0;

/// Wavelength of the shared border detail, in metres.
const DETAIL_WAVELENGTH: f64 = 40.0;

/// Seed of the shared border detail. Every terrain uses the same one, so
/// both sides of a border add the same detail (see [`SEAM_DETAIL`]).
const DETAIL_SEED: u32 = salt(SEAM_DETAIL);

/// Octaves of the shared border detail.
const DETAIL_OCTAVES: Octaves = Octaves {
    count: 3,
    gain: 0.5,
};

/// A terrain type with validated settings, not yet bound to a region.
///
/// Use it to make offers for a region's borders, then
/// [`build`](Self::build) a [`Terrain`] once the offers are agreed.
#[derive(Debug, Clone)]
pub struct TerrainGenerator {
    options: TerrainOptions,
    relief: Relief,
}

/// A terrain bound to its region and bent to meet its border agreements.
#[derive(Debug, Clone)]
pub struct Terrain {
    generator: TerrainGenerator,
    region: Region,
    seams: Vec<Seam>,
    /// Which seam segments can pull on each part of the plane.
    index: SegmentIndex,
    margin: f64,
    /// Each shared edge's index in the region, with its agreement.
    agreements: Vec<(usize, EdgeAgreement)>,
    /// The rivers and ponds: the planning stage after the river trace.
    hydrology: Arc<plan::Hydrology>,
    /// The roads, once planned: the last stage.
    roads: Option<Arc<plan::Roads>>,
    /// Both stages, ready to carve the ground.
    features: Features,
    /// How far the features are planned.
    tier: FeatureTier,
    /// For each tier, in [`FeatureTier::ALL`] order, how far the features
    /// it adds move the ground (see [`Terrain::tier_difference`]).
    differences: [f32; FeatureTier::ALL.len()],
    /// The world's scale, which the labels read shapes at.
    scale: WorldScale,
}

/// One shared edge: the agreed profile and this terrain's own smoothed
/// profile along it, both as heights above the world elevation E.
#[derive(Debug, Clone)]
struct Seam {
    agreed: Profile,
    own: Profile,
}

/// A terrain that could not be built.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TerrainError {
    /// A shared border edge has no agreement.
    MissingAgreement(EdgeId),
    /// An agreement is for an edge the region doesn't share.
    UnknownAgreement(EdgeId),
    /// A relief target is for an edge the region doesn't share.
    UnknownTarget(EdgeId),
    /// An edge has more than one relief target.
    DuplicateTarget(EdgeId),
    /// A relief target has a negative or non-finite amplitude, blend shares
    /// that aren't sorted, not negative and adding up to 1, or road grades
    /// out of range.
    InvalidTarget(EdgeId),
    /// A target gives a blend share to a region that is neither this one
    /// nor one of the neighbours given.
    UnknownNeighbour(RegionId),
    /// A neighbour's settings are invalid.
    NeighbourOptions(RegionId, OptionsError),
    /// The blend share is outside 0 to 1, or NaN.
    InvalidBlend(f32),
    /// The river trace given is of another region, this one.
    TraceMismatch(RegionId),
    /// An agreement has the wrong number of samples for its edge.
    SampleCount {
        /// The edge.
        edge: EdgeId,
        /// The number of samples the edge needs.
        expected: usize,
        /// The number of samples the agreement has.
        found: usize,
    },
}

impl fmt::Display for TerrainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingAgreement(id) => write!(f, "shared edge {} has no agreement", id.0),
            Self::UnknownAgreement(id) => {
                write!(f, "edge {} isn't a shared edge of this region", id.0)
            }
            Self::UnknownTarget(id) => {
                write!(f, "relief target for edge {}, which isn't shared", id.0)
            }
            Self::DuplicateTarget(id) => {
                write!(f, "edge {} has more than one relief target", id.0)
            }
            Self::InvalidTarget(id) => write!(f, "the target for edge {} isn't valid", id.0),
            Self::UnknownNeighbour(id) => {
                write!(f, "a target blends in region {}, which isn't given", id.0)
            }
            Self::NeighbourOptions(id, e) => write!(f, "neighbour region {}: {e}", id.0),
            Self::InvalidBlend(b) => write!(f, "blend share {b} is outside 0 to 1"),
            Self::TraceMismatch(id) => {
                write!(f, "the river trace is of region {}, not this one", id.0)
            }
            Self::SampleCount {
                edge,
                expected,
                found,
            } => {
                write!(
                    f,
                    "agreement for edge {} has {found} samples, but the edge needs {expected}",
                    edge.0
                )
            }
        }
    }
}

impl std::error::Error for TerrainError {}

impl TerrainGenerator {
    /// Creates a generator, after checking `options`.
    pub fn new(options: TerrainOptions) -> Result<Self, OptionsError> {
        options.validate()?;
        let relief = Relief::new(&options);
        Ok(Self { options, relief })
    }

    /// This generator standing on the world elevation `elevation`: every
    /// height becomes E plus the terrain's relief, and the settings'
    /// [`base.elevation`](crate::BaseOptions::elevation) is an offset above
    /// E. Replaces any earlier elevation; a new generator stands on
    /// [`Elevation::flat`].
    ///
    /// Every region of a world must be given the same elevation. It may be
    /// set before or after [`with_edge_targets`](Self::with_edge_targets),
    /// with the same result, but before making offers.
    pub fn with_elevation(mut self, elevation: Elevation) -> Self {
        self.relief = self.relief.with_elevation(elevation);
        self
    }

    /// The generator's settings.
    pub fn options(&self) -> &TerrainOptions {
        &self.options
    }

    /// The terrain's own height at world position `p` (x and z, metres),
    /// before any bending to meet neighbours, but after fading towards the
    /// relief targets (see [`with_edge_targets`](Self::with_edge_targets)).
    /// Defined everywhere.
    pub fn natural_height(&self, p: DVec2) -> f32 {
        self.relief.height(p)
    }

    /// The road grades at world position `p`: the settings' own (see
    /// [`RoadOptions`](crate::RoadOptions)), rising towards a steeper
    /// neighbour's near a shared edge once edge targets are set (see
    /// [`with_edge_targets`](Self::with_edge_targets)).
    pub fn road_grades(&self, p: DVec2) -> RoadGrades {
        self.relief.road_grades(p)
    }

    /// The typical size of this terrain's relief in metres: about the
    /// standard deviation of its height about the mean, from its settings
    /// alone. Pass it to [`relief_target`](crate::relief_target) to agree
    /// on targets with the neighbours.
    pub fn relief_amplitude(&self) -> f32 {
        total_spread(self.relief.spreads())
    }

    /// This generator with its relief and road grades fading towards
    /// `targets` near the matching edges of `region`. Deep inside the region
    /// both are unchanged. Edges without a target are left as they are.
    /// Replaces any earlier targets.
    ///
    /// Near each edge the relief changes in two ways, mixed by `blend`
    /// (see [`Convergence::blend`](crate::Convergence::blend)): its own
    /// layers scale towards the target's amplitude, and it fades into the
    /// target's blend of the regions' own ground. `neighbours` gives the
    /// settings of every other region with a blend share; the blend uses
    /// their ground without any fading.
    ///
    /// Call it before making offers, and use the same region for the
    /// offers and for [`build`](Self::build). Both terrains on an edge
    /// must be given the same target and blend, or their borders have more
    /// to bend.
    ///
    /// # Errors
    ///
    /// If a target is for an edge that isn't a shared edge of `region`, if
    /// two targets are for one edge, if a target isn't valid, if a target
    /// blends in a region missing from `neighbours`, if a neighbour's
    /// settings are invalid, or if `blend` is outside 0 to 1.
    pub fn with_edge_targets(
        mut self,
        region: &Region,
        targets: &[EdgeTarget],
        neighbours: &[(RegionId, TerrainOptions)],
        blend: f32,
    ) -> Result<Self, TerrainError> {
        if !(0.0..=1.0).contains(&blend) {
            return Err(TerrainError::InvalidBlend(blend));
        }
        for (i, target) in targets.iter().enumerate() {
            let shared = region
                .edges
                .iter()
                .any(|e| e.id == target.edge && e.neighbor.is_some());
            if !shared {
                return Err(TerrainError::UnknownTarget(target.edge));
            }
            if targets[..i].iter().any(|t| t.edge == target.edge) {
                return Err(TerrainError::DuplicateTarget(target.edge));
            }
            if !target.is_valid() {
                return Err(TerrainError::InvalidTarget(target.edge));
            }
        }

        // The other regions' reliefs, for the blend.
        let mut reliefs: Vec<(RegionId, Relief)> = Vec::new();
        for target in targets {
            for other in target.blend_regions() {
                if other == region.id || reliefs.iter().any(|(r, _)| *r == other) {
                    continue;
                }
                let (_, options) = neighbours
                    .iter()
                    .find(|(r, _)| *r == other)
                    .ok_or(TerrainError::UnknownNeighbour(other))?;
                options
                    .validate()
                    .map_err(|e| TerrainError::NeighbourOptions(other, e))?;
                reliefs.push((other, Relief::new(options)));
            }
        }

        let own = FadeOwn {
            region: region.id,
            spreads: self.relief.spreads(),
            roads: self.options.features.roads.grades(),
            mean: self.relief.mean_height(),
        };
        let min_width = f64::from(self.options.edges.margin);
        let fade = Fade::new(region, targets, own, reliefs, f64::from(blend), min_width);
        self.relief = self.relief.with_fade(fade);
        Ok(self)
    }

    /// Traces `region`'s own rivers over this terrain's unbent relief: the
    /// first planning stage, which both [`offers_traced`](Self::offers_traced)
    /// and [`build_traced`](Self::build_traced) read. Keep it to make the
    /// offers and build the terrain without tracing twice. Set the edge
    /// targets and elevation first, as for the offers.
    pub fn trace_rivers(&self, region: &Region) -> RiverTrace {
        RiverTrace {
            region: region.id,
            own: plan::own_rivers(&self.options, &self.relief, region).map(Arc::new),
        }
    }

    /// Makes this terrain's offers for every shared edge of `region`, in the
    /// order of `region.edges`.
    ///
    /// The offers depend only on this generator and the region, never on
    /// other offers or agreements. They include the region's own rivers
    /// that reach each edge, so this traces the region's rivers once (see
    /// [`trace_rivers`](Self::trace_rivers) to keep the trace).
    pub fn offers(&self, region: &Region) -> Vec<EdgeOffer> {
        self.offers_traced(region, &self.trace_rivers(region))
    }

    /// [`offers`](Self::offers), from `trace`, which must be this
    /// generator's trace of `region` (see [`trace_rivers`](Self::trace_rivers)).
    ///
    /// # Panics
    ///
    /// If `trace` is of another region.
    pub fn offers_traced(&self, region: &Region, trace: &RiverTrace) -> Vec<EdgeOffer> {
        assert_eq!(trace.region, region.id, "the trace is of this region");
        let own = trace.own.as_deref();
        let edges = &self.options.edges;
        region
            .edges
            .iter()
            .enumerate()
            .filter(|(_, e)| e.neighbor.is_some())
            .map(|(i, edge)| {
                let natural = self.natural_samples(edge);
                let step = edge.length() / (natural.len() - 1) as f64;
                let (rivers, roads) =
                    plan::wishes(&self.options, &self.relief, region, own, i, &natural, step);
                let side = plan::side_of(region, edge);
                EdgeOffer {
                    edge: edge.id,
                    region: region.id,
                    natural,
                    stiffness: edges.stiffness,
                    margin: edges.margin,
                    roughness: edges.roughness,
                    side,
                    rivers,
                    roads,
                    road_grades: self.options.features.roads.grades(),
                    inner: self.inner_samples(edge, side),
                    unfaded: self.unfaded_samples(edge),
                    fade_width: self.relief.fade_width().unwrap_or(f64::from(edges.margin)) as f32,
                }
            })
            .collect()
    }

    /// This terrain's own height and cross-slope at each profile sample of
    /// `edge`.
    fn natural_samples(&self, edge: &RegionEdge) -> Vec<EdgeSample> {
        let segments = segments(&edge.polyline);
        let length = edge.length();
        let count = sample_count(length);
        let step = length / (count - 1) as f64;
        (0..count)
            .map(|i| {
                let (point, normal) = point_at(&segments, i as f64 * step);
                let slope = self.relief.slope(point).dot(normal) as f32;
                EdgeSample {
                    height: self.relief.height(point),
                    slope,
                    detail: 0.0,
                }
            })
            .collect()
    }

    /// This terrain's own height at each of the [`INNER_DEPTHS`] depths in
    /// from each profile sample of `edge`, towards `side` (see
    /// [`inner_depth`]).
    fn inner_samples(&self, edge: &RegionEdge, side: Side) -> Vec<[f32; INNER_DEPTHS]> {
        let segments = segments(&edge.polyline);
        let length = edge.length();
        let count = sample_count(length);
        let step = length / (count - 1) as f64;
        (0..count)
            .map(|i| {
                let (point, normal) = point_at(&segments, i as f64 * step);
                std::array::from_fn(|k| {
                    self.relief
                        .height(point + normal * (side.sign() * inner_depth(k)))
                })
            })
            .collect()
    }

    /// This terrain's height without its fade at each profile sample of
    /// `edge`.
    fn unfaded_samples(&self, edge: &RegionEdge) -> Vec<f32> {
        let segments = segments(&edge.polyline);
        let length = edge.length();
        let count = sample_count(length);
        let step = length / (count - 1) as f64;
        (0..count)
            .map(|i| {
                self.relief
                    .unfaded_height(point_at(&segments, i as f64 * step).0)
            })
            .collect()
    }

    /// Makes this terrain's offer for `corner`, one of `region`'s corners.
    pub fn corner_offer(&self, region: &Region, corner: &RegionCorner) -> CornerOffer {
        CornerOffer {
            corner: corner.id,
            region: region.id,
            height: self.relief.height(corner.position),
            gradient: self.relief.slope(corner.position).as_vec2(),
            stiffness: self.options.edges.stiffness,
        }
    }

    /// Binds the generator to `region`, bending it to meet `agreements`,
    /// and plans every feature: its rivers, ponds and roads (the
    /// [`Fine`](FeatureTier::Fine) tier).
    ///
    /// There must be exactly one agreement for every edge of the region that
    /// has a neighbour. Edges without a neighbour are left as they are.
    pub fn build(
        self,
        region: Region,
        agreements: &[EdgeAgreement],
    ) -> Result<Terrain, TerrainError> {
        let trace = self.trace_rivers(&region);
        Ok(self
            .build_traced(region, agreements, &trace)?
            .with_tier(FeatureTier::Fine))
    }

    /// [`build`](Self::build) from the river trace `trace` (see
    /// [`trace_rivers`](Self::trace_rivers)), planning only the features of
    /// the roughest tier (see [`FeatureTier`]); [`Terrain::with_tier`] adds
    /// the rest later. Today that leaves out the roads, whose crossings'
    /// pieces aren't carved either: routing roads is the slowest stage, so
    /// a distant view can skip it.
    ///
    /// # Errors
    ///
    /// As for [`build`](Self::build), and
    /// [`TerrainError::TraceMismatch`] if `trace` is of another region.
    pub fn build_traced(
        self,
        region: Region,
        agreements: &[EdgeAgreement],
        trace: &RiverTrace,
    ) -> Result<Terrain, TerrainError> {
        if trace.region != region.id {
            return Err(TerrainError::TraceMismatch(trace.region));
        }

        // Every agreement must belong to a shared edge.
        for agreement in agreements {
            let shared = region
                .edges
                .iter()
                .any(|e| e.id == agreement.edge && e.neighbor.is_some());
            if !shared {
                return Err(TerrainError::UnknownAgreement(agreement.edge));
            }
        }

        // Every shared edge needs one agreement with the right sample count.
        let mut seams = Vec::new();
        let mut paired = Vec::new();
        for (index, edge) in region
            .edges
            .iter()
            .enumerate()
            .filter(|(_, e)| e.neighbor.is_some())
        {
            let agreement = agreements
                .iter()
                .find(|a| a.edge == edge.id)
                .ok_or(TerrainError::MissingAgreement(edge.id))?;
            let expected = sample_count(edge.length());
            if agreement.profile.len() != expected {
                return Err(TerrainError::SampleCount {
                    edge: edge.id,
                    expected,
                    found: agreement.profile.len(),
                });
            }

            // The agreed profile and this terrain's own, smoothed the same
            // way as in negotiation, both as heights above E.
            let e = elevation_samples(edge, expected, self.relief.elevation());
            let natural = self.natural_samples(edge);
            let step = edge.length() / (expected - 1) as f64;
            let own = EdgeAgreement {
                edge: edge.id,
                profile: smooth(&relative_to(&natural, e.as_deref(), -1.0), step),
                ports: Vec::new(),
            };
            let agreed = EdgeAgreement {
                profile: relative_to(&agreement.profile, e.as_deref(), -1.0),
                ..agreement.clone()
            };
            seams.push(Seam {
                agreed: Profile::new(edge, &agreed),
                own: Profile::new(edge, &own),
            });
            paired.push((index, agreement.clone()));
        }
        let margin = f64::from(self.options.edges.margin);
        let index = SegmentIndex::new(
            seams.iter().map(|s| s.agreed.segments()),
            BORDER_REACH * margin,
        );
        let mut terrain = Terrain {
            generator: self,
            region,
            seams,
            index,
            margin,
            agreements: paired,
            hydrology: Arc::default(),
            roads: None,
            features: Features::default(),
            tier: FeatureTier::ALL[0],
            differences: [0.0; FeatureTier::ALL.len()],
            scale: WorldScale::default(),
        };

        // Rivers and ponds, planned on the bent terrain.
        let hydrology = plan::hydrology(
            &terrain.generator.options,
            &terrain.region,
            trace.own.as_deref(),
            &terrain.paired(),
            &|p| terrain.base_height(p),
        );
        terrain.hydrology = Arc::new(hydrology);
        terrain.features = terrain.gather();

        // Any other feature drawn at the roughest tier.
        Ok(terrain.with_tier(FeatureTier::ALL[0]))
    }
}

/// A region's own rivers, traced over its terrain's unbent relief: the first
/// planning stage, shared by the offers and the build (see
/// [`TerrainGenerator::trace_rivers`]). Cheap to clone.
#[derive(Debug, Clone)]
pub struct RiverTrace {
    region: RegionId,
    own: Option<Arc<plan::OwnRivers>>,
}

impl RiverTrace {
    /// The region traced.
    pub fn region(&self) -> RegionId {
        self.region
    }
}

impl Terrain {
    /// This terrain with every feature drawn at `tier` or before planned
    /// and carved in. At [`Fine`](FeatureTier::Fine) it is the terrain
    /// [`TerrainGenerator::build`] gives. Features already planned are
    /// kept as they are, so asking for an earlier tier than the terrain
    /// has changes nothing.
    pub fn with_tier(mut self, tier: FeatureTier) -> Self {
        // Rivers and ponds are planned when the terrain is built; roads at
        // the first tier they are drawn at.
        if self.roads.is_none() && tier >= FeatureKind::Road.first_tier() {
            let before = self.clone();
            let roads = plan::roads(
                &self.generator.options,
                &self.generator.relief,
                &self.region,
                &self.paired(),
                &self.hydrology.rivers,
                &|p| self.base_height(p),
            );
            self.roads = Some(Arc::new(roads));
            self.features = self.gather();

            // How far the roads moved the ground.
            let index = FeatureKind::Road.first_tier().position();
            self.differences[index] =
                self.differences[index].max(moved(&before, &self, self.roads()));
        }
        self.tier = self.tier.max(tier);
        self
    }

    /// How far this terrain's features are planned (see
    /// [`with_tier`](Self::with_tier)). Only a terrain at
    /// [`Fine`](FeatureTier::Fine) is the real ground.
    pub fn tier(&self) -> FeatureTier {
        self.tier
    }

    /// This terrain with its world's `scale`, which its
    /// [`annotate`](Self::annotate) labels read shapes at. A terrain is
    /// built at [scale 1](WorldScale::default). Every terrain of one world
    /// must have the same scale, or labels jump at their borders.
    ///
    /// # Panics
    ///
    /// If either part of `scale` isn't finite and positive.
    pub fn with_scale(mut self, scale: WorldScale) -> Self {
        assert!(
            scale.is_valid(),
            "the scale must be finite and positive, not {} × {}",
            scale.horizontal,
            scale.vertical
        );
        self.scale = scale;
        self
    }

    /// This terrain's world scale (see [`with_scale`](Self::with_scale)).
    pub fn scale(&self) -> WorldScale {
        self.scale
    }

    /// The most, in metres, this terrain's ground differs from the same
    /// terrain at tier `from`: how far the features of the later tiers
    /// move the ground, measured along each of them when they are
    /// planned, with a margin. 0 if `from` is this terrain's tier or later.
    /// It sizes the skirts that hide gaps between chunks drawn at
    /// different tiers.
    pub fn tier_difference(&self, from: FeatureTier) -> f32 {
        FeatureTier::ALL
            .iter()
            .zip(self.differences)
            .filter(|&(&t, _)| t > from && t <= self.tier)
            .map(|(_, d)| d)
            .fold(0.0, f32::max)
    }

    /// The agreements, each with its edge's index in the region.
    fn paired(&self) -> Vec<(usize, &EdgeAgreement)> {
        self.agreements.iter().map(|(i, a)| (*i, a)).collect()
    }

    /// Every planned feature, ready to carve.
    fn gather(&self) -> Features {
        let h = &*self.hydrology;
        let roads = self.roads.as_deref().cloned().unwrap_or_default();
        Features::new(
            h.rivers.clone(),
            h.valleys.clone(),
            h.ponds.clone(),
            roads.roads,
            roads.banks,
            roads.points_of_interest,
        )
    }

    /// The terrain's region.
    pub fn region(&self) -> &Region {
        &self.region
    }

    /// The generator this terrain was built from.
    pub fn generator(&self) -> &TerrainGenerator {
        &self.generator
    }

    /// Ground height at world position `p` (x and z, metres), with rivers,
    /// ponds and roads cut in.
    ///
    /// Inside the region's border margin the terrain is bent towards the
    /// agreed border profiles, which it meets exactly on each shared edge.
    /// The result is defined for any `p`, but is meaningful only inside the
    /// region.
    pub fn height(&self, p: DVec2) -> f32 {
        self.point(p, false).0.height
    }

    /// The water surface height at `p` (metres) if a river or pond covers
    /// it, otherwise `None`.
    pub fn water_level(&self, p: DVec2) -> Option<f32> {
        self.point(p, false).0.water
    }

    /// What the roads made of the ground at `p`: the road surface, a
    /// cutting or an embankment, and how steep the bank is (see
    /// [`RoadTags`]). Empty where no road changed the ground.
    pub fn road_tags(&self, p: DVec2) -> RoadTags {
        self.point(p, false).0.tags
    }

    /// The terrain's rivers. Each runs downstream from its first point,
    /// including the short straight pieces across borders.
    pub fn rivers(&self) -> &[FeatureLine] {
        &self.features.rivers
    }

    /// The ponds where rivers end.
    pub fn ponds(&self) -> &[Pond] {
        &self.features.ponds
    }

    /// The terrain's roads, including the short straight pieces across
    /// borders.
    pub fn roads(&self) -> &[FeatureLine] {
        &self.features.roads
    }

    /// Every river, road and pond of the terrain, each with its id and its
    /// bounds, in id order: rivers, roads, then ponds, each kind in the
    /// order of its keys. A line of a single point changes nothing and is
    /// left out.
    pub fn features(&self) -> impl Iterator<Item = TerrainFeature<'_>> + '_ {
        self.features.all()
    }

    /// The feature `id`, or `None` if the terrain has no such feature (see
    /// [`features`](Self::features)).
    pub fn feature(&self, id: FeatureId) -> Option<TerrainFeature<'_>> {
        self.features.get(id)
    }

    /// The ids of the features that may change the ground, water or road
    /// tags inside the box between corners `min` and `max` (x and z,
    /// metres), in id order. Each feature's own bounds meet the box, but a
    /// bent feature's bounds can meet a box it doesn't reach.
    pub fn features_within(&self, min: DVec2, max: DVec2) -> Vec<FeatureId> {
        self.features.within(min, max)
    }

    /// Places in the terrain left for the caller to fill, such as the end
    /// of the region's road network. The terrain leaves the ground there
    /// as it is.
    pub fn points_of_interest(&self) -> &[PointOfInterest] {
        &self.features.points_of_interest
    }

    /// Ground height at `p` (metres) before rivers, ponds and roads are cut
    /// in, but after bending to meet the neighbours.
    pub fn uncarved_height(&self, p: DVec2) -> f32 {
        self.base_height(p)
    }

    /// The bent height before rivers and roads are cut in.
    fn base_height(&self, p: DVec2) -> f32 {
        self.base(p, false).0
    }

    /// The bent height before rivers and roads are cut in, and with
    /// `slope` its gradient (else zero): analytic through the terrain's own
    /// relief, E and the shared detail, with the border's pull (its soft
    /// distance and mixed targets) taken by differences (see
    /// [`pull_gradients`]).
    fn base(&self, p: DVec2, slope: bool) -> (f32, DVec2) {
        let relief = &self.generator.relief;
        let own = f64::from(relief.height(p));
        let own_slope = || {
            if slope { relief.slope(p) } else { DVec2::ZERO }
        };
        if self.seams.is_empty() {
            return (own as f32, own_slope());
        }

        // Mix what every border edge within reach asks for, each pulling
        // along its whole length, and find the (soft) distance to the
        // border.
        let reach = BORDER_REACH * self.margin;
        let mut pull = Pull::<3>::default();
        let mut on_edge = None;
        // First the distance alone, which is cheap; the targets only where
        // the bending reaches.
        let near = self.index.near(p);
        // Cheaper still: points the bending surely can't reach.
        let segments = near
            .iter()
            .map(|&(e, k)| &self.seams[e].agreed.segments()[k]);
        if surely_beyond(segments, p, reach, self.margin) {
            return (own as f32, own_slope());
        }
        for &(e, k) in near {
            let seam = &self.seams[e];
            let segment = &seam.agreed.segments()[k];
            let (targets, distance) = Targets::at(seam, segment, p);
            if distance < ON_EDGE {
                on_edge = Some(targets);
                break;
            }
            pull.add_distance(segment, p, reach);
        }
        let distance = pull.distance(reach);
        if on_edge.is_none() && distance < self.margin {
            for &(e, k) in near {
                let seam = &self.seams[e];
                let target = |along: f64, across: f64| {
                    let carry = slope_reach(across);
                    let at = |s: EdgeSample| f64::from(s.height) + f64::from(s.slope) * carry;
                    let (agreed, own) = (seam.agreed.sample(along), seam.own.sample(along));
                    [at(agreed), at(own), f64::from(agreed.detail)]
                };
                pull.add_targets(&seam.agreed.segments()[k], p, reach, EDGE_STEP, &target);
            }
        }
        // The targets are heights above E; E here is added back.
        let elevation = self.generator.relief.elevation();
        let on_e = |targets: Targets| {
            if elevation.is_flat() {
                return targets;
            }
            targets.raised(f64::from(elevation.height(p)))
        };
        let border = match (on_edge, pull.mixed()) {
            (Some(targets), _) => {
                let targets = on_e(targets);
                let slope = if slope {
                    self.bend_slopes(p, targets.detail).0
                } else {
                    DVec2::ZERO
                };
                return (targets.seam(p) as f32, slope);
            }
            (None, None) => return (own as f32, own_slope()),
            (None, Some(_)) if distance >= self.margin => return (own as f32, own_slope()),
            (None, Some([agreed, own, detail])) => on_e(Targets {
                agreed,
                own,
                detail,
            }),
        };

        // Across the margin: shift by how far the agreement is from this
        // terrain's own profile, which keeps the terrain's detail.
        let t = distance / self.margin;
        let broad = own + (border.agreed - border.own) * (1.0 - smootherstep(t));

        // Next to the border: give way to the agreed profile and shared detail.
        let band = DETAIL_BAND.min(self.margin);
        let seam = border.seam(p);
        let u = distance / band;
        let height = (seam + (broad - seam) * smootherstep(u)) as f32;
        if !slope {
            return (height, DVec2::ZERO);
        }

        // The same, differentiated.
        let (seam_slope, [distance_slope, agreed_slope, own_target_slope]) =
            self.bend_slopes(p, border.detail);
        let broad_slope = own_slope() + (agreed_slope - own_target_slope) * (1.0 - smootherstep(t))
            - distance_slope * ((border.agreed - border.own) * smootherstep_slope(t) / self.margin);
        let slope = seam_slope * (1.0 - smootherstep(u))
            + broad_slope * smootherstep(u)
            + distance_slope * ((broad - seam) * smootherstep_slope(u) / band);
        (height, slope)
    }

    /// At `p` near the border: the gradient of the seam height (the agreed
    /// height plus E plus the shared detail, whose height is `detail`
    /// there), and the gradients of the soft distance to the border and of
    /// the agreed and own targets, these three from differences of the
    /// border's pull (see [`pull_gradients`]).
    fn bend_slopes(&self, p: DVec2, detail: f64) -> (DVec2, [DVec2; 3]) {
        let (distance_slope, [agreed_slope, own_target_slope, detail_slope]) =
            pull_gradients(p, |q| self.bend_at(q));
        let (noise, noise_slope) = fbm_gradient(p / DETAIL_WAVELENGTH, DETAIL_SEED, DETAIL_OCTAVES);
        let (noise, noise_slope) = (f64::from(noise), noise_slope / DETAIL_WAVELENGTH);
        let elevation = self.generator.relief.elevation();
        let e_slope = if elevation.is_flat() {
            DVec2::ZERO
        } else {
            elevation.gradient(p)
        };
        let seam_slope =
            agreed_slope + e_slope + (detail_slope * noise + noise_slope * detail) * 0.5;
        (seam_slope, [distance_slope, agreed_slope, own_target_slope])
    }

    /// The soft distance from `p` to the border and the border's targets
    /// (agreed height, own height and detail, as heights above E) mixed by
    /// their pull, or `None` where no edge is within reach. Unlike
    /// [`base_height`](Self::base_height) it always mixes the targets.
    fn bend_at(&self, p: DVec2) -> Option<(f64, [f64; 3])> {
        let reach = BORDER_REACH * self.margin;
        let near = self.index.near(p);
        let mut pull = Pull::<3>::default();
        for &(e, k) in near {
            pull.add_distance(&self.seams[e].agreed.segments()[k], p, reach);
        }
        for &(e, k) in near {
            let seam = &self.seams[e];
            let target = |along: f64, across: f64| {
                let carry = slope_reach(across);
                let at = |s: EdgeSample| f64::from(s.height) + f64::from(s.slope) * carry;
                let (agreed, own) = (seam.agreed.sample(along), seam.own.sample(along));
                [at(agreed), at(own), f64::from(agreed.detail)]
            };
            pull.add_targets(&seam.agreed.segments()[k], p, reach, EDGE_STEP, &target);
        }
        let distance = pull.distance(reach);
        let mixed = pull.mixed()?;
        distance.is_finite().then_some((distance, mixed))
    }

    /// Everything the terrain says about `p` in one pass: the carved
    /// ground, its water and road tags, and with `slope` its gradient
    /// (else zero).
    fn point(&self, p: DVec2, slope: bool) -> (Carved, DVec2) {
        let (base, base_slope) = self.base(p, slope);
        self.features.carve(p, base, slope.then_some(base_slope))
    }

    /// Everything the terrain says about `p` (x and z, metres) at once, as
    /// far as `detail` asks:
    ///
    /// - always the ground's height, its water and road tags, the same as
    ///   [`height`](Self::height), [`water_level`](Self::water_level) and
    ///   [`road_tags`](Self::road_tags) give;
    /// - from [`SampleDetail::Gradient`], how much the ground rises per
    ///   metre along x and z, as [`sample`](Self::sample) gives it;
    /// - from [`SampleDetail::Measures`], the [`Measures`]: slope,
    ///   curvature, and the nearby water and roads;
    /// - at [`SampleDetail::Labels`], the [`Labels`], as
    ///   [`annotate`](Self::annotate) gives them.
    ///
    /// Cheaper than asking for each in turn. The gradient costs the most
    /// near borders, several times the height; the measures cost about four
    /// heights more, and the labels about eight more again.
    pub fn query(&self, p: DVec2, detail: SampleDetail) -> TerrainSample {
        let (carved, slope) = self.point(p, detail >= SampleDetail::Gradient);
        let gradient = slope.as_vec2();
        let measured =
            (detail >= SampleDetail::Measures).then(|| self.measures(p, carved.height, gradient));
        let labels =
            measured
                .filter(|_| detail >= SampleDetail::Labels)
                .map(|(measures, water_kind)| {
                    let shape = Shape::at(self.scale);
                    let around = shape_points(p, shape.step).map(|q| self.height(q));
                    let site = Site {
                        water: carved.water,
                        tags: carved.tags,
                        measures,
                        water_kind,
                        bends: principal_bends(carved.height, around, shape.step),
                    };
                    classify(&site, shape)
                });
        TerrainSample {
            height: carved.height,
            gradient: (detail >= SampleDetail::Gradient).then_some(gradient),
            water: carved.water,
            tags: carved.tags,
            measures: measured.map(|(measures, _)| measures),
            labels,
        }
    }

    /// The labels at `p` (x and z, metres): what kind of land it is, and
    /// what else holds there. The same as [`query`](Self::query) gives at
    /// [`SampleDetail::Labels`].
    pub fn annotate(&self, p: DVec2) -> Labels {
        self.query(p, SampleDetail::Labels)
            .labels
            .expect("a query at SampleDetail::Labels gives labels")
    }

    /// The measures at `p`, where the carved ground is `height` high and
    /// rises by `gradient`, and what the nearest water within their reach
    /// is.
    fn measures(&self, p: DVec2, height: f32, gradient: Vec2) -> (Measures, Option<FeatureKind>) {
        let reach = f64::from(MEASURE_REACH);
        let nearby = self.features.nearby(p, reach);
        let around = [DVec2::X, DVec2::NEG_X, DVec2::Y, DVec2::NEG_Y]
            .map(|axis| self.height(p + axis * CURVATURE_STEP));
        let measures = Measures {
            slope: slope_of(gradient),
            curvature: curvature_of(height, around),
            height_above_water: nearby.water_level.map(|level| height - level),
            water_distance: nearby.water_distance as f32,
            road_distance: nearby.road_distance as f32,
        };
        (measures, nearby.water_kind)
    }

    /// The ground's height at `p` with its gradient, the same as
    /// [`height`](Self::height) gives, and how much it rises per metre
    /// along x and z.
    pub fn sample(&self, p: DVec2) -> GroundSample {
        let (carved, slope) = self.point(p, true);
        GroundSample {
            height: carved.height,
            gradient: slope.as_vec2(),
        }
    }

    /// How much the ground rises per metre along x and z at `p`, as
    /// [`sample`](Self::sample) gives it.
    pub fn gradient(&self, p: DVec2) -> Vec2 {
        self.sample(p).gradient
    }
}

impl Ground for Terrain {
    fn height(&self, p: DVec2) -> f32 {
        Terrain::height(self, p)
    }

    fn gradient(&self, p: DVec2) -> Vec2 {
        Terrain::gradient(self, p)
    }

    fn sample(&self, p: DVec2) -> GroundSample {
        Terrain::sample(self, p)
    }
}

/// What the border asks for at one point, mixed from nearby edges.
#[derive(Debug, Clone, Copy, Default)]
struct Targets {
    /// The agreed height.
    agreed: f64,
    /// This terrain's own smoothed height, extended the same way.
    own: f64,
    /// Height of the shared border detail.
    detail: f64,
}

impl Targets {
    /// The targets of one segment at `p`, with the distance to it. Each
    /// profile sample at the closest point is carried away from the edge
    /// along its cross-slope, which levels off a few tens of metres out.
    fn at(seam: &Seam, segment: &Segment, p: DVec2) -> (Self, f64) {
        let projection = segment.project(p);
        let reach = slope_reach(projection.across);
        let carry = |s: EdgeSample| f64::from(s.height) + f64::from(s.slope) * reach;
        let agreed = seam.agreed.sample(projection.along);
        let own = seam.own.sample(projection.along);
        let targets = Self {
            agreed: carry(agreed),
            own: carry(own),
            detail: f64::from(agreed.detail),
        };
        (targets, projection.distance)
    }

    /// These targets with `height` added to the agreed and own heights.
    fn raised(self, height: f64) -> Self {
        Self {
            agreed: self.agreed + height,
            own: self.own + height,
            detail: self.detail,
        }
    }

    /// The height both sides meet at the border: the agreed height plus the
    /// shared detail, which is a function of world position alone.
    fn seam(self, p: DVec2) -> f64 {
        let noise = fbm(p / DETAIL_WAVELENGTH, DETAIL_SEED, DETAIL_OCTAVES, 0.0);
        self.agreed + 0.5 * self.detail * f64::from(noise)
    }
}

/// The most the ground of `after` differs from that of `before` along
/// `lines`, with a margin for the ground between the points measured: on
/// each point, at both edges and a few metres beyond them.
fn moved(before: &Terrain, after: &Terrain, lines: &[FeatureLine]) -> f32 {
    // Measured points miss the ground between them; this much more covers
    // that.
    const MARGIN: f32 = 1.0;
    const SCALE: f32 = 1.1;
    let mut most = 0.0f32;
    for line in lines {
        for (i, &p) in line.points.iter().enumerate() {
            let next = line
                .points
                .get(i + 1)
                .or(i.checked_sub(1).map(|j| &line.points[j]));
            let across = next.map_or(DVec2::X, |&q| (q - p).perp().normalize_or(DVec2::X));
            let half = 0.5 * f64::from(line.widths[i]);
            for offset in [0.0, half, -half, half + 4.0, -half - 4.0] {
                let at = p + across * offset;
                most = most.max((after.height(at) - before.height(at)).abs());
            }
        }
    }
    if most > 0.0 {
        most * SCALE + MARGIN
    } else {
        0.0
    }
}

/// Everything a [`Terrain`] says about one point (see [`Terrain::query`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerrainSample {
    /// Ground height in metres, with rivers, ponds and roads cut in.
    pub height: f32,
    /// How much the ground rises per metre along x and z, if asked for.
    pub gradient: Option<Vec2>,
    /// The water surface height in metres, if a river or pond covers the
    /// point.
    pub water: Option<f32>,
    /// What the roads made of the ground there (see [`RoadTags`]).
    pub tags: RoadTags,
    /// The ground's measures, if asked for.
    pub measures: Option<Measures>,
    /// The ground's labels, if asked for.
    pub labels: Option<Labels>,
}

/// Anything with a ground height at every world position.
pub trait HeightField: Sync {
    /// Ground height at world position `p` (x and z, metres). NaN where
    /// there is no ground.
    fn height(&self, p: DVec2) -> f32;

    /// The height at `p` with how much it rises per metre along x and z.
    /// The default takes central differences of [`height`](Self::height)
    /// [`GRADIENT_STEP`](procgena_core::GRADIENT_STEP) metres either side.
    fn sample(&self, p: DVec2) -> GroundSample {
        let step = procgena_core::GRADIENT_STEP;
        let slope = |axis: DVec2| {
            (self.height(p + axis * step) - self.height(p - axis * step)) / (2.0 * step) as f32
        };
        GroundSample {
            height: self.height(p),
            gradient: Vec2::new(slope(DVec2::X), slope(DVec2::Y)),
        }
    }
}

impl HeightField for Terrain {
    fn height(&self, p: DVec2) -> f32 {
        Terrain::height(self, p)
    }

    fn sample(&self, p: DVec2) -> GroundSample {
        Terrain::sample(self, p)
    }
}

impl HeightField for TerrainGenerator {
    fn height(&self, p: DVec2) -> f32 {
        self.natural_height(p)
    }

    fn sample(&self, p: DVec2) -> GroundSample {
        GroundSample {
            height: self.natural_height(p),
            gradient: self.relief.slope(p).as_vec2(),
        }
    }
}

/// A grid of heights, and if asked for the ground's gradient at each, on
/// the world lattice.
///
/// Sample `(i, j)` is at world position `((origin.x + i)·step, (origin.y +
/// j)·step)`. Positions are computed from integers, so two tiles with the
/// same `step` that share lattice points have identical values there.
#[derive(Debug, Clone, PartialEq)]
pub struct HeightTile {
    /// Lattice index of the first sample.
    pub origin: glam::I64Vec2,
    /// Lattice spacing, in metres.
    pub step: f64,
    /// Number of samples along x and z.
    pub size: glam::UVec2,
    /// Heights in metres, row by row (x fastest).
    pub heights: Vec<f32>,
    /// How much the ground rises per metre along x and z at each sample,
    /// in the same order as `heights`, for a tile made by
    /// [`sample_with_gradients`](Self::sample_with_gradients); otherwise
    /// `None`. Each is the field's own gradient at that point (see
    /// [`HeightField::sample`]), not a difference of neighbouring samples.
    pub gradients: Option<Vec<Vec2>>,
    /// The labels at each sample, in the same order as `heights`, for a
    /// tile made by [`sample_labelled`](Self::sample_labelled); otherwise
    /// `None`. A sample is `None` where the field has no labels (see
    /// [`LabelField::annotate`]).
    pub labels: Option<Vec<Option<Labels>>>,
}

/// Anything with [`Labels`] at every world position where it has ground.
pub trait LabelField: HeightField {
    /// The labels at world position `p` (x and z, metres), or `None` where
    /// there is no ground.
    fn annotate(&self, p: DVec2) -> Option<Labels>;
}

impl LabelField for Terrain {
    fn annotate(&self, p: DVec2) -> Option<Labels> {
        Some(Terrain::annotate(self, p))
    }
}

impl HeightTile {
    /// Samples the heights of `field` on a `size` grid of lattice points
    /// starting at `origin`, `step` metres apart. `step` must be positive.
    pub fn sample(
        field: &impl HeightField,
        origin: glam::I64Vec2,
        size: glam::UVec2,
        step: f64,
    ) -> Self {
        let heights = Self::each(origin, size, step, |p| field.height(p));
        Self {
            origin,
            step,
            size,
            heights,
            gradients: None,
            labels: None,
        }
    }

    /// [`sample`](Self::sample), with the gradient at every sample too.
    /// For a [`Terrain`] this costs several times as much as the heights
    /// alone.
    pub fn sample_with_gradients(
        field: &impl HeightField,
        origin: glam::I64Vec2,
        size: glam::UVec2,
        step: f64,
    ) -> Self {
        let samples = Self::each(origin, size, step, |p| field.sample(p));
        Self {
            origin,
            step,
            size,
            heights: samples.iter().map(|s| s.height).collect(),
            gradients: Some(samples.iter().map(|s| s.gradient).collect()),
            labels: None,
        }
    }

    /// [`sample`](Self::sample), with the labels at every sample too. For
    /// a [`Terrain`] this costs about ten times as much as the heights
    /// alone.
    pub fn sample_labelled(
        field: &impl LabelField,
        origin: glam::I64Vec2,
        size: glam::UVec2,
        step: f64,
    ) -> Self {
        let samples = Self::each(origin, size, step, |p| (field.height(p), field.annotate(p)));
        Self {
            origin,
            step,
            size,
            heights: samples.iter().map(|s| s.0).collect(),
            gradients: None,
            labels: Some(samples.iter().map(|s| s.1).collect()),
        }
    }

    /// `at` of every lattice point of the tile, row by row (x fastest).
    fn each<T: Send>(
        origin: glam::I64Vec2,
        size: glam::UVec2,
        step: f64,
        at: impl Fn(DVec2) -> T + Sync,
    ) -> Vec<T> {
        let row = |j: u32| -> Vec<T> {
            let z = (origin.y + i64::from(j)) as f64 * step;
            (0..size.x)
                .map(|i| at(DVec2::new((origin.x + i64::from(i)) as f64 * step, z)))
                .collect()
        };
        #[cfg(feature = "rayon")]
        let rows: Vec<Vec<T>> = {
            use rayon::prelude::*;
            (0..size.y).into_par_iter().map(row).collect()
        };
        #[cfg(not(feature = "rayon"))]
        let rows: Vec<Vec<T>> = (0..size.y).map(row).collect();
        rows.into_iter().flatten().collect()
    }

    /// The height at sample `(i, j)`.
    pub fn get(&self, i: u32, j: u32) -> f32 {
        self.heights[(j * self.size.x + i) as usize]
    }

    /// The gradient at sample `(i, j)`: how much the ground rises per metre
    /// along x and z; `None` for a tile without gradients.
    pub fn gradient(&self, i: u32, j: u32) -> Option<Vec2> {
        let gradients = self.gradients.as_ref()?;
        Some(gradients[(j * self.size.x + i) as usize])
    }

    /// The labels at sample `(i, j)`; `None` for a tile without labels, or
    /// where the field has none.
    pub fn labels(&self, i: u32, j: u32) -> Option<Labels> {
        self.labels.as_ref()?[(j * self.size.x + i) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carve::FeatureKind;
    use crate::feature::FeatureShape;
    use crate::{Convergence, Elevation, RegionSpec, StitchedWorld, TerrainPreset};

    /// Plains in the west and rolling hills in the east (hill seed 102),
    /// split by a bent border: the plains get rivers, a pond and roads.
    fn hills_into_plains() -> StitchedWorld {
        let points = |v: &[(f64, f64)]| v.iter().map(|&(x, z)| DVec2::new(x, z)).collect();
        let options = |preset: &TerrainPreset, seed| {
            let mut o = preset.terrain.clone();
            o.default_seed = seed;
            o
        };
        StitchedWorld::build(
            vec![
                RegionSpec {
                    id: RegionId(1),
                    polygon: points(&[
                        (-2000.0, -1500.0),
                        (0.0, -1500.0),
                        (150.0, -700.0),
                        (-200.0, 0.0),
                        (100.0, 800.0),
                        (0.0, 1500.0),
                        (-2000.0, 1500.0),
                    ]),
                    options: options(TerrainPreset::plains(), 102),
                },
                RegionSpec {
                    id: RegionId(2),
                    polygon: points(&[
                        (0.0, -1500.0),
                        (2000.0, -1500.0),
                        (2000.0, 1500.0),
                        (0.0, 1500.0),
                        (100.0, 800.0),
                        (-200.0, 0.0),
                        (150.0, -700.0),
                    ]),
                    options: options(TerrainPreset::rolling_hills(), 102),
                },
            ],
            &Convergence::default(),
            &Elevation::flat(),
        )
        .expect("the world builds")
    }

    /// Points every 9.7 m by 11.3 m over the terrain's region and a little
    /// beyond.
    fn points() -> impl Iterator<Item = DVec2> {
        (0..440).flat_map(|i| {
            (0..280).map(move |j| {
                DVec2::new(-2100.0 + 9.7 * f64::from(i), -1580.0 + 11.3 * f64::from(j))
            })
        })
    }

    #[test]
    fn the_index_gives_what_every_feature_gives() {
        let world = hills_into_plains();
        let t = &world.terrains()[0];
        for kind in [FeatureKind::River, FeatureKind::Road, FeatureKind::Pond] {
            assert!(t.features().any(|f| f.id.kind == kind), "a {kind:?}");
        }
        let all = t.features.all_parts();
        for p in points() {
            let h = t.base_height(p);
            let (indexed, slope) = t.features.carve(p, h, Some(DVec2::new(0.1, 0.2)));
            let (every, every_slope) =
                t.features
                    .carve_parts(p, h, Some(DVec2::new(0.1, 0.2)), &all);
            assert_eq!(
                indexed.height.to_bits(),
                every.height.to_bits(),
                "height at {p}"
            );
            assert_eq!(indexed.water, every.water, "water at {p}");
            assert_eq!(indexed.tags, every.tags, "tags at {p}");
            assert_eq!(slope, every_slope, "slope at {p}");
        }
    }

    #[test]
    fn no_feature_changes_anything_outside_its_bounds() {
        let world = hills_into_plains();
        let t = &world.terrains()[0];
        let all = t.features.all_parts();
        for feature in t.features() {
            let parts: Vec<_> = all.iter().copied().filter(|p| p.id == feature.id).collect();
            let [lo, hi] = feature.bounds;
            for p in points().filter(|p| p.x < lo.x || p.y < lo.y || p.x > hi.x || p.y > hi.y) {
                let h = t.base_height(p);
                let (carved, slope) = t.features.carve_parts(p, h, Some(DVec2::X), &parts);
                assert_eq!(carved.height, h, "{:?} at {p}", feature.id);
                assert_eq!(carved.water, None);
                assert!(carved.tags.is_empty());
                assert_eq!(slope, DVec2::X);
            }
            // Boxes around its points find it; boxes off its bounds don't.
            let start = match feature.shape {
                FeatureShape::Line(line) => line.points[0],
                FeatureShape::Pond(pond) => pond.centre,
            };
            let around = [start - DVec2::ONE, start + DVec2::ONE];
            assert!(
                t.features_within(around[0], around[1])
                    .contains(&feature.id)
            );
            let off = [hi + DVec2::ONE, hi + DVec2::splat(50.0)];
            assert!(!t.features_within(off[0], off[1]).contains(&feature.id));
            assert_eq!(t.feature(feature.id), Some(feature));
        }
    }

    #[test]
    fn roads_come_at_the_fine_tier_and_ids_stay() {
        let world = hills_into_plains();
        let t = &world.terrains()[0];
        assert!(t.tier() == FeatureTier::Fine && !t.roads().is_empty());

        // The same terrain built from its trace at the rough tier: the same
        // rivers and ponds, with the same ids, and no roads.
        let generator = t.generator().clone();
        let region = t.region().clone();
        let mine: Vec<EdgeAgreement> = world
            .agreements()
            .iter()
            .filter(|a| region.edges.iter().any(|e| e.id == a.edge))
            .cloned()
            .collect();
        let trace = generator.trace_rivers(&region);
        assert_eq!(
            generator.offers(&region),
            generator.offers_traced(&region, &trace)
        );
        let bare = generator
            .build_traced(region, &mine, &trace)
            .expect("builds");
        assert!(bare.tier() == FeatureTier::Rough && bare.roads().is_empty());
        // Asking for an earlier tier changes nothing.
        assert!(t.clone().with_tier(FeatureTier::Rough).tier() == FeatureTier::Fine);
        assert_eq!(bare.rivers(), t.rivers());
        assert_eq!(bare.ponds(), t.ponds());
        let ids = |t: &Terrain, kind| {
            t.features()
                .filter(|f| f.id.kind == kind)
                .map(|f| f.id)
                .collect::<Vec<_>>()
        };
        for kind in [FeatureKind::River, FeatureKind::Pond] {
            assert_eq!(ids(&bare, kind), ids(t, kind));
        }

        // The fine tier adds the roads, giving the terrain as built at once.
        let full = bare.with_tier(FeatureTier::Fine);
        assert_eq!(full.roads(), t.roads());
        for p in points().step_by(97) {
            assert_eq!(full.height(p).to_bits(), t.height(p).to_bits());
        }
        // Ids are unique.
        let mut all: Vec<FeatureId> = t.features().map(|f| f.id).collect();
        let count = all.len();
        all.dedup();
        assert_eq!(all.len(), count);
    }

    #[test]
    fn one_query_gives_what_each_accessor_gives() {
        let world = hills_into_plains();
        let t = &world.terrains()[0];
        for p in points().step_by(41) {
            let plain = t.query(p, SampleDetail::Height);
            let full = t.query(p, SampleDetail::Gradient);
            let measured = t.query(p, SampleDetail::Measures);
            let sample = t.sample(p);
            assert_eq!(plain.height.to_bits(), t.height(p).to_bits());
            assert_eq!(plain.water, t.water_level(p));
            assert_eq!(plain.tags, t.road_tags(p));
            assert_eq!(plain.gradient, None);
            assert_eq!(plain.measures, None);
            assert_eq!(full.height, plain.height);
            assert_eq!(full.gradient, Some(sample.gradient));
            assert_eq!(full.measures, None);
            assert_eq!(sample.height, plain.height);
            assert_eq!(
                (
                    measured.height,
                    measured.gradient,
                    measured.water,
                    measured.tags
                ),
                (full.height, full.gradient, full.water, full.tags)
            );
            let measures = measured.measures.expect("measures were asked for");
            assert_eq!(measures.slope, slope_of(sample.gradient));
            assert_eq!(measured.labels, None);
            let labelled = t.query(p, SampleDetail::Labels);
            assert_eq!(
                TerrainSample {
                    labels: None,
                    ..labelled
                },
                measured
            );
            assert_eq!(labelled.labels, Some(t.annotate(p)));
        }
    }
}
