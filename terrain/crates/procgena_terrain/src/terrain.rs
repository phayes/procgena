//! Terrain generators, and terrains bound to a region.

use std::fmt;

use glam::DVec2;

use crate::border::{Pull, SegmentIndex, surely_beyond};
use crate::carve::{FeatureLine, Features, PointOfInterest, Pond, RoadTags};
use crate::converge::{EdgeTarget, Fade, FadeOwn, total_spread};
use crate::elevation::Elevation;
use crate::hash::{SEAM_DETAIL, salt};
use crate::noise::{Octaves, fbm};
use crate::options::{OptionsError, RoadGrades, TerrainOptions};
use crate::plan;
use crate::ports::{INNER_DEPTHS, Side, inner_depth, slope_reach};
use crate::region::{
    EdgeId, Region, RegionCorner, RegionEdge, RegionId, Segment, point_at, segments,
};
use crate::relief::Relief;
use crate::seam::{
    CornerOffer, EDGE_STEP, EdgeAgreement, EdgeOffer, EdgeSample, Profile, elevation_samples,
    relative_to, sample_count, smooth, smootherstep,
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
    features: Features,
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

    /// Makes this terrain's offers for every shared edge of `region`, in the
    /// order of `region.edges`.
    ///
    /// The offers depend only on this generator and the region, never on
    /// other offers or agreements. They include the region's own rivers
    /// that reach each edge, so this traces the region's rivers once.
    pub fn offers(&self, region: &Region) -> Vec<EdgeOffer> {
        let own = plan::own_rivers(&self.options, &self.relief, region);
        let edges = &self.options.edges;
        region
            .edges
            .iter()
            .enumerate()
            .filter(|(_, e)| e.neighbor.is_some())
            .map(|(i, edge)| {
                let natural = self.natural_samples(edge);
                let step = edge.length() / (natural.len() - 1) as f64;
                let (rivers, roads) = plan::wishes(
                    &self.options,
                    &self.relief,
                    region,
                    own.as_ref(),
                    i,
                    &natural,
                    step,
                );
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
                let slope = self.relief.gradient(point).dot(normal.as_vec2());
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
            gradient: self.relief.gradient(corner.position),
            stiffness: self.options.edges.stiffness,
        }
    }

    /// Binds the generator to `region`, bending it to meet `agreements`.
    ///
    /// There must be exactly one agreement for every edge of the region that
    /// has a neighbour. Edges without a neighbour are left as they are.
    pub fn build(
        self,
        region: Region,
        agreements: &[EdgeAgreement],
    ) -> Result<Terrain, TerrainError> {
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
            paired.push((index, agreement));
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
            features: Features::default(),
        };

        // Rivers and roads, planned on the bent terrain.
        let features = plan::features(
            &terrain.generator.options,
            &terrain.generator.relief,
            &terrain.region,
            &paired,
            &|p| terrain.base_height(p),
        );
        terrain.features = features;
        Ok(terrain)
    }
}

impl Terrain {
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
        let base = self.base_height(p);
        self.features.apply(p, base).height
    }

    /// The water surface height at `p` (metres) if a river or pond covers
    /// it, otherwise `None`.
    pub fn water_level(&self, p: DVec2) -> Option<f32> {
        let base = self.base_height(p);
        self.features.apply(p, base).water
    }

    /// What the roads made of the ground at `p`: the road surface, a
    /// cutting or an embankment, and how steep the bank is (see
    /// [`RoadTags`]). Empty where no road changed the ground.
    pub fn road_tags(&self, p: DVec2) -> RoadTags {
        let base = self.base_height(p);
        self.features.apply(p, base).tags
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
        let own = f64::from(self.generator.relief.height(p));
        if self.seams.is_empty() {
            return own as f32;
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
            return own as f32;
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
            (Some(targets), _) => return on_e(targets).seam(p) as f32,
            (None, None) => return own as f32,
            (None, Some(_)) if distance >= self.margin => return own as f32,
            (None, Some([agreed, own, detail])) => on_e(Targets {
                agreed,
                own,
                detail,
            }),
        };

        // Across the margin: shift by how far the agreement is from this
        // terrain's own profile, which keeps the terrain's detail.
        let broad =
            own + (border.agreed - border.own) * (1.0 - smootherstep(distance / self.margin));

        // Next to the border: give way to the agreed profile and shared detail.
        let band = DETAIL_BAND.min(self.margin);
        let seam = border.seam(p);
        (seam + (broad - seam) * smootherstep(distance / band)) as f32
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

/// Anything with a ground height at every world position.
pub trait HeightField: Sync {
    /// Ground height at world position `p` (x and z, metres). NaN where
    /// there is no ground.
    fn height(&self, p: DVec2) -> f32;
}

impl HeightField for Terrain {
    fn height(&self, p: DVec2) -> f32 {
        Terrain::height(self, p)
    }
}

impl HeightField for TerrainGenerator {
    fn height(&self, p: DVec2) -> f32 {
        self.natural_height(p)
    }
}

/// A grid of heights on the world lattice.
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
}

impl HeightTile {
    /// Samples `field` on a `size` grid of lattice points starting at
    /// `origin`, `step` metres apart. `step` must be positive.
    pub fn sample(
        field: &impl HeightField,
        origin: glam::I64Vec2,
        size: glam::UVec2,
        step: f64,
    ) -> Self {
        let row = |j: u32| -> Vec<f32> {
            let z = (origin.y + i64::from(j)) as f64 * step;
            (0..size.x)
                .map(|i| field.height(DVec2::new((origin.x + i64::from(i)) as f64 * step, z)))
                .collect()
        };
        #[cfg(feature = "rayon")]
        let rows: Vec<Vec<f32>> = {
            use rayon::prelude::*;
            (0..size.y).into_par_iter().map(row).collect()
        };
        #[cfg(not(feature = "rayon"))]
        let rows: Vec<Vec<f32>> = (0..size.y).map(row).collect();
        Self {
            origin,
            step,
            size,
            heights: rows.concat(),
        }
    }

    /// The height at sample `(i, j)`.
    pub fn get(&self, i: u32, j: u32) -> f32 {
        self.heights[(j * self.size.x + i) as usize]
    }
}
