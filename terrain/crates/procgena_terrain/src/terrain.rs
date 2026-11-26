//! Terrain generators, and terrains bound to a region.

use std::fmt;
use std::sync::Arc;

use glam::{DVec2, Vec2};
use procgena_core::{Ground, GroundSample};

use crate::border::{
    BorderField, BorderProfile, EdgeScales, FieldEdge, Knots, ProfileShape, middle_scales,
    pin_ramp, pin_shares,
};
use crate::carve::{Carved, FeatureKind, FeatureLine, Features, PointOfInterest, Pond, RoadTags};
use crate::converge::{BLEND_RATE, BorderTarget, EdgeTarget, fade_width, total_spread};
use crate::elevation::Elevation;
use crate::feature::{FeatureId, FeatureTier, TerrainFeature};
use crate::label::{Labels, Shape, Site, classify, principal_bends, shape_points};
use crate::measure::{
    CURVATURE_STEP, MEASURE_REACH, Measures, NearbyMeasures, SampleDetail, curvature_of, slope_of,
};
use crate::options::{OptionsError, RoadGrades, TerrainOptions};
use crate::plan;
use crate::ports::{INNER_DEPTHS, Side, inner_depth};
use crate::region::{EdgeId, Region, RegionEdge, RegionId, point_at, segments};
use crate::relief::Relief;
use crate::road::{CarvedLine, RoadEarthworks, RoadNetwork, RouteStats, earthworks};
use crate::scale::WorldScale;
use crate::seam::{CrossingSamples, EdgeAgreement, EdgeOffer, EdgeSample, sample_count};

/// Distances below this (metres) count as on the edge.
pub(crate) const ON_EDGE: f64 = 1e-9;

/// A terrain type with validated settings, not yet bound to a region.
///
/// Use it to make offers for a region's borders, then
/// [`build`](Self::build) a [`Terrain`] once the offers are agreed.
#[derive(Debug, Clone)]
pub struct TerrainGenerator {
    options: TerrainOptions,
    relief: Relief,
}

/// A terrain bound to its region and meeting its border agreements.
#[derive(Debug, Clone)]
pub struct Terrain {
    generator: TerrainGenerator,
    region: Region,
    /// Each shared edge's index in the region, with its agreement.
    agreements: Vec<(usize, EdgeAgreement)>,
    /// The rivers and ponds: the planning stage after the river trace.
    hydrology: Arc<plan::Hydrology>,
    /// The roads drawn at this terrain's tier, once planned: the last
    /// stage.
    roads: Option<Arc<plan::Roads>>,
    /// The roads in outline, once planned, which the full roads follow.
    outline: Option<Arc<plan::RoadOutline>>,
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
    /// A target is for an edge whose neighbour isn't one of the
    /// neighbours given.
    UnknownNeighbour(RegionId),
    /// A neighbour's settings are invalid.
    NeighbourOptions(RegionId, OptionsError),
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
                write!(f, "a target's neighbour, region {}, isn't given", id.0)
            }
            Self::NeighbourOptions(id, e) => write!(f, "neighbour region {}: {e}", id.0),
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
    /// before meeting its neighbours' border profiles, but after fading
    /// towards the relief targets (see
    /// [`with_edge_targets`](Self::with_edge_targets)). Defined everywhere.
    pub fn natural_height(&self, p: DVec2) -> f32 {
        self.relief.own_height(p)
    }

    /// The ground's height at `p` before carving, meeting the border
    /// profiles: what the built terrain's
    /// [`uncarved_height`](Terrain::uncarved_height) gives.
    #[cfg(test)]
    pub(crate) fn ground_height(&self, p: DVec2) -> f32 {
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

    /// Whether this generator and `other` have the same relief: the same
    /// seed and the same base settings, so their heights before carving are
    /// the same function, bit for bit. Two regions with the same relief
    /// need no border between them.
    pub fn same_relief(&self, other: &TerrainGenerator) -> bool {
        self.options.default_seed == other.options.default_seed
            && self.options.base == other.options.base
    }

    /// This generator with its relief and road grades fading towards
    /// `targets` near the matching edges of `region`: its relief layers
    /// scale towards each target's amplitude, and its road grades towards
    /// the target's. Deep inside the region both are unchanged. Edges
    /// without a target are left as they are, and need no agreement to
    /// build. Replaces any earlier targets.
    ///
    /// `neighbours` gives the settings of the region across each targeted
    /// edge; how far the terrain gives way to the agreed border profile
    /// follows from how far its mean height lies from theirs.
    ///
    /// Call it before making offers, and use the same region for the
    /// offers and for [`build`](Self::build). Both terrains on an edge
    /// must be given the same target.
    ///
    /// # Errors
    ///
    /// If a target is for an edge that isn't a shared edge of `region`, if
    /// two targets are for one edge, if a target isn't valid, if a
    /// targeted edge's neighbour is missing from `neighbours`, or if a
    /// neighbour's settings are invalid.
    pub fn with_edge_targets(
        mut self,
        region: &Region,
        targets: &[EdgeTarget],
        neighbours: &[(RegionId, TerrainOptions)],
    ) -> Result<Self, TerrainError> {
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

        // Each targeted edge, with this terrain's layer scales along it, and
        // how far its mean height lies from the border profile's.
        let spreads = self.relief.spreads();
        let margin = f64::from(self.options.edges.margin);
        let mut strip = margin;
        let mut edges = Vec::with_capacity(targets.len());
        for edge in &region.edges {
            let Some(target) = targets.iter().find(|t| t.edge == edge.id) else {
                continue;
            };
            let neighbor = edge.neighbor.expect("targets are for shared edges");
            let (_, options) = neighbours
                .iter()
                .find(|(r, _)| *r == neighbor)
                .ok_or(TerrainError::UnknownNeighbour(neighbor))?;
            options
                .validate()
                .map_err(|e| TerrainError::NeighbourOptions(neighbor, e))?;
            let theirs = Relief::new(options);
            let amplitude = target.middle.amplitude;
            let mean =
                |relief: &Relief| relief.mean_height(middle_scales(relief.spreads(), amplitude));
            let (own_mean, their_mean) = (mean(&self.relief), mean(&theirs));
            let (k_own, k_theirs) = (
                f64::from(self.options.edges.stiffness),
                f64::from(options.edges.stiffness),
            );
            // Summed in region order, as the profile is mixed.
            let (first, second) = if region.id < neighbor {
                ((k_own, own_mean), (k_theirs, their_mean))
            } else {
                ((k_theirs, their_mean), (k_own, own_mean))
            };
            let profile_mean = (first.0 * first.1 + second.0 * second.1) / (first.0 + second.0);
            strip = strip.max((profile_mean - own_mean).abs() / BLEND_RATE);

            let length = edge.length();
            edges.push(FieldEdge {
                id: edge.id,
                segments: segments(&edge.polyline),
                length,
                target: target.clone(),
                scales: EdgeScales::new(spreads, target, length),
            });
        }
        let fade = fade_width(spreads, targets, margin);
        let field = BorderField::new(edges, fade, strip).map(Arc::new);
        let borders = match &field {
            Some(field) => self.border_profiles(region, field, neighbours)?,
            None => Vec::new(),
        };
        self.relief = self.relief.with_field(field, borders.into());
        Ok(self)
    }

    /// The agreed profile of each of `field`'s edges, in order, from the
    /// settings of this terrain and of `neighbours` alone: every region on
    /// the edges and at their corners must be among them. Both regions on
    /// an edge compute the same profile, bit for bit.
    fn border_profiles(
        &self,
        region: &Region,
        field: &BorderField,
        neighbours: &[(RegionId, TerrainOptions)],
    ) -> Result<Vec<BorderProfile>, TerrainError> {
        let elevation = self.relief.elevation();
        // Any region's relief (built the same way whichever region asks),
        // stiffness and settings.
        let side = |id: RegionId| -> Result<(Relief, f64, &TerrainOptions), TerrainError> {
            let options = if id == region.id {
                &self.options
            } else {
                &neighbours
                    .iter()
                    .find(|(r, _)| *r == id)
                    .ok_or(TerrainError::UnknownNeighbour(id))?
                    .1
            };
            let relief = Relief::new(options).with_elevation(elevation.clone());
            Ok((relief, f64::from(options.edges.stiffness), options))
        };

        // A corner's height: the stiffness-weighted mean of every region
        // there, each scaled to the corner's amplitude, in region order.
        let corner_height = |position: DVec2, corner: &BorderTarget| {
            let (mut sum, mut total) = (0.0, 0.0);
            for &(id, _) in &corner.blend {
                let (relief, k, _) = side(id)?;
                let scales = middle_scales(relief.spreads(), corner.amplitude).map(f64::from);
                sum += k * relief.layer_values(position).combine(&scales);
                total += k;
            }
            Ok::<f32, TerrainError>((sum / total + f64::from(elevation.height(position))) as f32)
        };

        let mut profiles = Vec::with_capacity(field.edges().len());
        for edge in field.edges() {
            let polyline = &region
                .edges
                .iter()
                .find(|e| e.id == edge.id)
                .expect("field edges are region edges")
                .polyline;
            let neighbor = region
                .edges
                .iter()
                .find(|e| e.id == edge.id)
                .and_then(|e| e.neighbor)
                .expect("field edges are shared");
            let length = edge.length;
            let ramp = pin_ramp(length);
            let positions = [polyline[0], polyline[polyline.len() - 1]];
            let corners = [
                corner_height(positions[0], &edge.target.start)?,
                corner_height(positions[1], &edge.target.end)?,
            ];
            // The corners' heights above E: profiles mix and pin heights
            // above E, so E passes through them exactly.
            let ends =
                [0, 1].map(|i| f64::from(corners[i]) - f64::from(elevation.height(positions[i])));

            // Each side's relief along the edge, above E, scaled as the
            // edge's target asks of it.
            let along = |relief: &Relief, scales: &EdgeScales, s: f64| {
                let (point, _) = point_at(&edge.segments, s);
                relief.layer_values(point).combine(&scales.at(s).0)
            };
            let (own, k_own, own_options) = side(region.id)?;
            let (theirs, k_theirs, their_options) = side(neighbor)?;
            let shape = if own_options.default_seed == their_options.default_seed
                && own_options.base == their_options.base
            {
                // The same relief on both sides: the relief itself, offset
                // at the corners.
                let offsets = [
                    ends[0] - along(&own, &edge.scales, 0.0),
                    ends[1] - along(&own, &edge.scales, length),
                ];
                ProfileShape::Relative { offsets, ramp }
            } else {
                // Different reliefs: the stiffness-weighted mean of both
                // sides, in region order, pinned to the corners.
                let their_scales = EdgeScales::new(theirs.spreads(), &edge.target, length);
                let mine = (&own, &edge.scales, k_own);
                let other = (&theirs, &their_scales, k_theirs);
                let (low, high) = if region.id < neighbor {
                    (mine, other)
                } else {
                    (other, mine)
                };
                let mixed = |s: f64| {
                    (low.2 * along(low.0, low.1, s) + high.2 * along(high.0, high.1, s))
                        / (low.2 + high.2)
                };
                let offsets = [ends[0] - mixed(0.0), ends[1] - mixed(length)];
                ProfileShape::Shared(Knots::sample(length, ends, |s| {
                    let (shares, _) = pin_shares(s, length, ramp);
                    mixed(s) + offsets[0] * shares[0] + offsets[1] * shares[1]
                }))
            };
            profiles.push(BorderProfile { shape, corners });
        }
        Ok(profiles)
    }

    /// The height profile this terrain meets along `edge`, if it is one of
    /// its targeted edges (see [`with_edge_targets`](Self::with_edge_targets)).
    /// The neighbour on the edge has the same profile, bit for bit.
    pub fn border(&self, edge: EdgeId) -> Option<&BorderProfile> {
        self.relief.border(edge)
    }

    /// How far, in metres, the terrain gives way to its agreed border
    /// profiles: the strip width (or its margin without targets).
    pub(crate) fn strip_width(&self) -> f64 {
        self.relief
            .field()
            .map_or(f64::from(self.options.edges.margin), |f| f.strip())
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
        region
            .edges
            .iter()
            .enumerate()
            .filter(|(_, e)| e.neighbor.is_some())
            .map(|(i, _)| self.edge_offer(region, i, trace, true))
            .collect()
    }

    /// The offer for edge number `index` of `region`, a shared edge, from
    /// this generator's `trace` of `region`. Without `crossing`, the offer's
    /// `inner` and `unfaded` are left empty and its natural cross-slopes are
    /// 0: [`crossing_samples`] gives them, and an agreement needs them only
    /// where either side wishes for a crossing.
    ///
    /// [`crossing_samples`]: Self::crossing_samples
    pub(crate) fn edge_offer(
        &self,
        region: &Region,
        index: usize,
        trace: &RiverTrace,
        crossing: bool,
    ) -> EdgeOffer {
        let edge = &region.edges[index];
        let edges = &self.options.edges;
        let natural = self.natural_heights(edge);
        let step = edge.length() / (natural.len() - 1) as f64;
        let own = trace.own.as_deref();
        let (rivers, roads) = plan::wishes(
            &self.options,
            &self.relief,
            region,
            own,
            index,
            &natural,
            step,
        );
        let side = plan::side_of(region, edge);
        let offer = EdgeOffer {
            edge: edge.id,
            region: region.id,
            natural,
            stiffness: edges.stiffness,
            margin: self.strip_width() as f32,
            side,
            rivers,
            roads,
            road_grades: self.options.features.roads.grades(),
            inner: Vec::new(),
            unfaded: Vec::new(),
            fade_width: self
                .relief
                .field()
                .map_or(f64::from(edges.margin), |f| f.fade()) as f32,
        };
        if crossing {
            offer.with_crossing(self.crossing_samples(region, index))
        } else {
            offer
        }
    }

    /// The samples of the offer for edge number `index` of `region` that
    /// only crossings need (see [`EdgeOffer`]).
    pub(crate) fn crossing_samples(&self, region: &Region, index: usize) -> CrossingSamples {
        let edge = &region.edges[index];
        let side = plan::side_of(region, edge);
        CrossingSamples {
            inner: self.inner_samples(edge, side),
            unfaded: self.unfaded_samples(edge),
            slopes: self.natural_slopes(edge),
        }
    }

    /// This terrain's own height at each profile sample of `edge`, with a
    /// cross-slope of 0.
    fn natural_heights(&self, edge: &RegionEdge) -> Vec<EdgeSample> {
        let segments = segments(&edge.polyline);
        let length = edge.length();
        let count = sample_count(length);
        let step = length / (count - 1) as f64;
        (0..count)
            .map(|i| EdgeSample {
                height: self
                    .relief
                    .own_height(point_at(&segments, i as f64 * step).0),
                slope: 0.0,
            })
            .collect()
    }

    /// This terrain's own cross-slope at each profile sample of `edge`.
    fn natural_slopes(&self, edge: &RegionEdge) -> Vec<f32> {
        let segments = segments(&edge.polyline);
        let length = edge.length();
        let count = sample_count(length);
        let step = length / (count - 1) as f64;
        (0..count)
            .map(|i| {
                let (point, normal) = point_at(&segments, i as f64 * step);
                self.relief.own_slope(point).dot(normal) as f32
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
                        .own_height(point + normal * (side.sign() * inner_depth(k)))
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
    /// pieces aren't carved either, so a distant view can skip routing
    /// them.
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

            paired.push((index, agreement.clone()));
        }

        let mut terrain = Terrain {
            generator: self,
            region,
            agreements: paired,
            hydrology: Arc::default(),
            roads: None,
            outline: None,
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
    /// This terrain with every feature drawn at `tier` planned as that
    /// tier draws it, and carved in (see [`FeatureKind::tiers`]). At
    /// [`Fine`](FeatureTier::Fine) it is the terrain
    /// [`TerrainGenerator::build`] gives, whichever tiers it went through.
    /// Asking for an earlier tier than the terrain has changes nothing.
    pub fn with_tier(mut self, tier: FeatureTier) -> Self {
        // Rivers and ponds are planned when the terrain is built. Roads are
        // outlined at the mid tier, and planned in full along the outline
        // at the fine tier.
        if tier >= FeatureTier::Mid && self.tier < FeatureTier::Mid {
            let before = self.clone();
            let outline = plan::road_outline(
                &self.generator.options,
                &self.generator.relief,
                &self.region,
                &self.paired(),
                &self.hydrology,
                &|p| self.base_height(p),
            );
            self.roads = Some(Arc::new(outline.roads.clone()));
            self.outline = Some(Arc::new(outline));
            self.features = self.gather();
            let lines = self.roads().to_vec();
            self.record_difference(FeatureTier::Mid, &before, &lines);
        }
        if tier >= FeatureTier::Fine && self.tier < FeatureTier::Fine {
            let before = self.clone();
            let outline = self.outline.clone().expect("the outline is planned first");
            let roads = plan::roads(
                &self.generator.options,
                &self.generator.relief,
                &self.region,
                &self.paired(),
                &self.hydrology,
                &|p| self.base_height(p),
                &outline,
            );
            self.roads = Some(Arc::new(roads));
            self.features = self.gather();

            // The ground moves along the full roads, and back where the
            // outline was.
            let lines: Vec<FeatureLine> = self
                .roads()
                .iter()
                .chain(&outline.roads.roads)
                .cloned()
                .collect();
            self.record_difference(FeatureTier::Fine, &before, &lines);
        }
        self.tier = self.tier.max(tier);
        self
    }

    /// Records how far `tier` moved the ground from `before` to this
    /// terrain, measured along `lines`.
    fn record_difference(&mut self, tier: FeatureTier, before: &Terrain, lines: &[FeatureLine]) {
        let index = tier.position();
        self.differences[index] = self.differences[index].max(moved(before, self, lines));
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
            roads.sections,
            roads.open,
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

    /// The terrain's roads as carved: one line per segment of the
    /// [`road_network`](Self::road_network), including the short straight
    /// pieces across borders. A road stops at each river bank, so where it
    /// crosses a river it is two lines, one ending on each bank.
    pub fn roads(&self) -> &[FeatureLine] {
        &self.features.roads
    }

    /// The terrain's roads as a network: where they start, meet and end,
    /// their centre lines and levels, and the river crossings they leave
    /// for the caller to span. The outline at
    /// [`Mid`](FeatureTier::Mid), the full roads at
    /// [`Fine`](FeatureTier::Fine); empty at [`Rough`](FeatureTier::Rough).
    pub fn road_network(&self) -> &RoadNetwork {
        static EMPTY: std::sync::OnceLock<RoadNetwork> = std::sync::OnceLock::new();
        self.roads
            .as_deref()
            .map_or_else(|| EMPTY.get_or_init(RoadNetwork::default), |r| &r.network)
    }

    /// The earthworks of the roads in [`road_network`](Self::road_network):
    /// each station's cross-section and where its cuttings and embankments
    /// meet the natural ground, cut and fill volumes, junction footprints,
    /// and the breaklines a ground mesh should keep. Worked out the first
    /// time it is asked for, which takes a few hundred heights per
    /// station; empty where the terrain has no roads.
    pub fn road_earthworks(&self) -> &RoadEarthworks {
        static EMPTY: std::sync::OnceLock<RoadEarthworks> = std::sync::OnceLock::new();
        let Some(roads) = self.roads.as_deref() else {
            return EMPTY.get_or_init(RoadEarthworks::default);
        };
        roads.earthworks.get_or_init(|| {
            let sections = self.features.road_sections();
            let lines: Vec<CarvedLine<'_>> = self
                .features
                .roads
                .iter()
                .zip(sections)
                .zip(&roads.segments)
                .map(|((line, sections), &segment)| CarvedLine {
                    line,
                    sections,
                    segment,
                })
                .collect();
            earthworks(&roads.network, &lines, &|p| {
                f64::from(self.height_before_roads(p))
            })
        })
    }

    /// The ground's height at `p` (metres) with rivers and ponds cut in, but
    /// not roads: the natural ground the roads' cuttings and embankments
    /// meet. Where no road reaches, the same as [`height`](Self::height).
    pub fn height_before_roads(&self, p: DVec2) -> f32 {
        self.features.before_roads(p, self.base_height(p))
    }

    /// Diagnostics from planning this terrain's roads at `tier`: the
    /// outline at [`Mid`](FeatureTier::Mid), the full roads at
    /// [`Fine`](FeatureTier::Fine). `None` at [`Rough`](FeatureTier::Rough),
    /// which has no roads, and at a tier the terrain hasn't reached.
    pub fn road_stats(&self, tier: FeatureTier) -> Option<RouteStats> {
        if tier > self.tier {
            return None;
        }
        match tier {
            FeatureTier::Rough => None,
            FeatureTier::Mid => self.outline.as_ref().map(|o| o.roads.stats),
            FeatureTier::Fine => self.roads.as_ref().map(|r| r.stats),
        }
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

    /// The height before rivers and roads are cut in, meeting the agreed
    /// border profiles, and with `slope` its gradient (else zero), analytic
    /// throughout.
    fn base(&self, p: DVec2, slope: bool) -> (f32, DVec2) {
        self.generator.relief.base(p, slope)
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
        let (nearby, water_kind) = self.nearby(p, height);
        let around = [DVec2::X, DVec2::NEG_X, DVec2::Y, DVec2::NEG_Y]
            .map(|axis| self.height(p + axis * CURVATURE_STEP));
        let measures = Measures {
            slope: slope_of(gradient),
            curvature: curvature_of(height, around),
            height_above_water: nearby.height_above_water,
            water_distance: nearby.water_distance,
            road_distance: nearby.road_distance,
        };
        (measures, water_kind)
    }

    /// The nearby measures at `p`, and what the nearest water within their
    /// reach is.
    fn nearby(&self, p: DVec2, height: f32) -> (NearbyMeasures, Option<FeatureKind>) {
        let nearby = self.features.nearby(p, f64::from(MEASURE_REACH));
        let measures = NearbyMeasures {
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

/// The most the ground of `after` differs from that of `before` along
/// `lines`, with a margin for the ground between the points measured: on
/// each point, at both edges and a few metres beyond them.
fn moved(before: &Terrain, after: &Terrain, lines: &[FeatureLine]) -> f32 {
    // Measured points miss the ground between them; this much more covers
    // that.
    const MARGIN: f32 = 1.0;
    const SCALE: f32 = 1.1;

    // Each point of each line, measured on its own: across the line at its
    // centre, edges and beyond.
    let points = lines
        .iter()
        .flat_map(|line| (0..line.points.len()).map(move |i| (line, i)));
    let moves = points.map(|(line, i): (&FeatureLine, usize)| {
        let p = line.points[i];
        let next = line
            .points
            .get(i + 1)
            .or(i.checked_sub(1).map(|j| &line.points[j]));
        let across = next.map_or(DVec2::X, |&q| (q - p).perp().normalize_or(DVec2::X));
        let half = 0.5 * f64::from(line.widths[i]);
        [0.0, half, -half, half + 4.0, -half - 4.0]
            .into_iter()
            .map(|offset| {
                let at = p + across * offset;
                (after.height(at) - before.height(at)).abs()
            })
            .fold(0.0f32, f32::max)
    });
    let most = moves.into_iter().fold(0.0f32, f32::max);
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
            gradient: self.relief.own_slope(p).as_vec2(),
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
        (0..size.y).flat_map(row).collect()
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
    use crate::{Convergence, Elevation, RegionSpec, StitchedWorld, TerrainPreset, fixtures};

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
                    options: options(fixtures::plains_medium(), 102),
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
                    options: options(fixtures::rolling_hills_medium(), 102),
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
