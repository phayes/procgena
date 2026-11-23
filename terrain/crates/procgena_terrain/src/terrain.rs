//! Terrain generators, and terrains bound to a region.

use std::fmt;

use glam::DVec2;

use crate::carve::{FeatureLine, Features, Pond};
use crate::converge::{EdgeTarget, Fade, total_spread};
use crate::noise::{Octaves, fbm};
use crate::options::{OptionsError, RoadGrades, TerrainOptions};
use crate::plan;
use crate::ports::{FEATURE_CLEARANCE, Side, slope_reach};
use crate::region::{EdgeId, Region, RegionCorner, RegionEdge, Segment, point_at, segments};
use crate::relief::Relief;
use crate::seam::{
    CornerOffer, EdgeAgreement, EdgeOffer, EdgeSample, Profile, sample_count, smooth, smootherstep,
};

/// Power of the inverse-distance weights that mix border targets. Above 1,
/// so each target's cross-slope survives right up to its edge.
pub(crate) const SHEPARD_POWER: i32 = 3;

/// Power of the soft minimum used for the distance to the border. The plain
/// minimum has creases where the nearest segment changes (along the
/// bisectors of bends and corners); the soft minimum is smooth, and slightly
/// shorter where several segments are about equally near.
pub(crate) const SOFT_DISTANCE_POWER: i32 = 8;

/// Distances below this (metres) count as on the edge.
pub(crate) const ON_EDGE: f64 = 1e-9;

/// Width of the strip next to a border where a terrain's own fine detail
/// gives way to the shared border detail, in metres (at most the margin).
const DETAIL_BAND: f64 = 48.0;

/// Wavelength of the shared border detail, in metres.
const DETAIL_WAVELENGTH: f64 = 40.0;

/// Seed of the shared border detail. Every terrain uses the same one, so
/// both sides of a border add the same detail.
const DETAIL_SEED: u32 = 0x5ea3_de7a;

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
    margin: f64,
    features: Features,
}

/// One shared edge: the agreed profile and this terrain's own smoothed
/// profile along it.
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
    /// A relief target has a negative or non-finite amplitude.
    InvalidTarget(EdgeId),
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
            Self::InvalidTarget(id) => {
                write!(f, "relief target for edge {} isn't a valid amplitude", id.0)
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
    /// `targets` near the matching edges of `region`, so that each edge's
    /// relief amplitude and road grades are the target's there. Deep inside
    /// the region both are unchanged. Edges without a target are left as
    /// they are. Replaces any earlier targets.
    ///
    /// Call it before making offers, and use the same region for the
    /// offers and for [`build`](Self::build). Both terrains on an edge
    /// must be given the same target, or their borders have more to bend.
    ///
    /// # Errors
    ///
    /// If a target is for an edge that isn't a shared edge of `region`, if
    /// two targets are for one edge, or if a target has a negative or
    /// non-finite amplitude or road grades out of range.
    pub fn with_edge_targets(
        mut self,
        region: &Region,
        targets: &[EdgeTarget],
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
        let min_width = f64::from(self.options.edges.margin);
        let roads = self.options.features.roads.grades();
        let fade = Fade::new(region, targets, self.relief.spreads(), roads, min_width);
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

    /// This terrain's own height [`FEATURE_CLEARANCE`] metres in from each
    /// profile sample of `edge`, towards `side`.
    fn inner_samples(&self, edge: &RegionEdge, side: Side) -> Vec<f32> {
        let segments = segments(&edge.polyline);
        let length = edge.length();
        let count = sample_count(length);
        let step = length / (count - 1) as f64;
        (0..count)
            .map(|i| {
                let (point, normal) = point_at(&segments, i as f64 * step);
                self.relief
                    .height(point + normal * (side.sign() * FEATURE_CLEARANCE))
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

            // This terrain's own profile, smoothed the same way as in negotiation.
            let natural = self.natural_samples(edge);
            let step = edge.length() / (expected - 1) as f64;
            let own = EdgeAgreement {
                edge: edge.id,
                profile: smooth(&natural, step),
                ports: Vec::new(),
            };
            seams.push(Seam {
                agreed: Profile::new(edge, agreement),
                own: Profile::new(edge, &own),
            });
            paired.push((index, agreement));
        }
        let margin = f64::from(self.options.edges.margin);
        let mut terrain = Terrain {
            generator: self,
            region,
            seams,
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

        // Mix every border segment's targets, weighted by inverse distance,
        // and find the (soft) distance to the border.
        let mut sum = Targets::default();
        let mut total = 0.0f64;
        let mut closeness = 0.0f64;
        let mut on_edge = None;
        'seams: for seam in &self.seams {
            for segment in seam.agreed.segments() {
                let (targets, distance) = Targets::at(seam, segment, p);
                if distance < ON_EDGE {
                    on_edge = Some(targets);
                    break 'seams;
                }
                let w = distance.powi(-SHEPARD_POWER);
                sum = sum.add(targets, w);
                total += w;
                closeness += (distance / self.margin).powi(-SOFT_DISTANCE_POWER);
            }
        }
        // In margin units, so the powers stay in range at any scale.
        let distance = self.margin * closeness.powf(-1.0 / f64::from(SOFT_DISTANCE_POWER));
        let border = match on_edge {
            Some(targets) => return targets.seam(p) as f32,
            None if distance >= self.margin => return own as f32,
            None => sum.scaled(1.0 / total),
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

/// What the border asks for at one point, mixed from nearby segments.
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

    fn add(self, other: Self, weight: f64) -> Self {
        Self {
            agreed: self.agreed + other.agreed * weight,
            own: self.own + other.own * weight,
            detail: self.detail + other.detail * weight,
        }
    }

    fn scaled(self, factor: f64) -> Self {
        Self {
            agreed: self.agreed * factor,
            own: self.own * factor,
            detail: self.detail * factor,
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
