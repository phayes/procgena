//! What a ground rule reads at one point, and where it comes from.

use glam::{DVec2, Vec2};
use procgena_core::{Ground, GroundSample};

use crate::measure::slope_of;
use crate::{
    FeatureTier, Labels, MEASURE_REACH, RoadTags, SampleDetail, Terrain, TerrainSample,
    WorldTerrain,
};

/// Whether water covers a point, where that is known.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Water {
    /// No river or pond covers the point.
    Dry,
    /// A river or pond covers the point, with its surface at this height
    /// in metres.
    Level(f32),
}

/// Terrain data at one point, used to decide which ground materials appear.
///
/// Optional data is generally unknown when `None`, rather than known to
/// be absent. A layer ignores conditions whose data is unknown. The
/// exception is `nearby_water_height`, which is also `None` when no water
/// is nearby; see [`height_above_water`](Self::height_above_water) for its fallback.
///
/// A plain [`Ground`] is treated as having no water or roads. A [`Terrain`]
/// supplies those features, adds distances and curvature at
/// [`SampleDetail::Measures`], and adds landform categories and flags at
/// [`SampleDetail::Labels`]. Neither source supplies relief height.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct GroundSite {
    /// Ground height in metres. NaN where there is no ground.
    pub height: f32,
    /// The slope, as the sine of the ground's angle from flat: 0 flat, 1 a
    /// wall.
    pub slope: f32,
    /// The height within the region's relief, from 0 at the valley floor
    /// to 1 at the highest peak.
    pub relief_height: Option<f32>,
    /// Whether water covers the point.
    pub water: Option<Water>,
    /// What the roads made of the ground there.
    pub road_tags: Option<RoadTags>,
    /// How far the ground lies above the nearest water within
    /// [`WATER_HEIGHT_REACH`](crate::WATER_HEIGHT_REACH), in metres (see
    /// [`Measures::height_above_water`](crate::Measures::height_above_water)):
    /// `None` where no water is that near, or where it wasn't measured.
    pub nearby_water_height: Option<f32>,
    /// How far it is to the nearest water's edge, in metres, at most
    /// [`MEASURE_REACH`] (see
    /// [`Measures::water_distance`](crate::Measures::water_distance)).
    pub water_distance: Option<f32>,
    /// How far it is to the nearest road surface's edge, in metres, at most
    /// [`MEASURE_REACH`] (see
    /// [`Measures::road_distance`](crate::Measures::road_distance)).
    pub road_distance: Option<f32>,
    /// How the ground bends, per metre: positive convex, negative concave
    /// (see [`Measures::curvature`](crate::Measures::curvature)).
    pub curvature: Option<f32>,
    /// The landform category (such as cliff or riverbank) and flags (such
    /// as wet or road). `None` means unclassified; see [`Terrain::annotate`].
    pub labels: Option<Labels>,
}

impl GroundSite {
    /// A site on plain ground, with no water and no roads, from its height
    /// and the gradient of the ground there (how much it rises per metre
    /// along x and z): water and roads are [`MEASURE_REACH`] or further
    /// away. Its relief height and curvature are unknown. A gradient that
    /// isn't a number gives a slope of 0.
    pub fn new(height: f32, gradient: Vec2) -> Self {
        Self {
            height,
            slope: slope_of(gradient),
            relief_height: None,
            water: Some(Water::Dry),
            road_tags: Some(RoadTags::NONE),
            nearby_water_height: None,
            water_distance: Some(MEASURE_REACH),
            road_distance: Some(MEASURE_REACH),
            curvature: None,
            labels: None,
        }
    }

    /// A site from a [`Ground`]'s sample, as [`new`](Self::new): plain
    /// ground with no water and no roads.
    pub fn from_ground(sample: GroundSample) -> Self {
        Self::new(sample.height, sample.gradient)
    }

    /// Converts a [`Terrain::query`] result into a site without sampling again.
    /// Copies height, water and road tags, plus any measures and labels
    /// included in the sample. Measures contain distances, height above
    /// nearby water and curvature; labels contain the landform and flags.
    ///
    /// A missing gradient gives a slope of 0. Missing measures or labels
    /// remain unknown, and relief height is always unknown.
    pub fn from_query(sample: &TerrainSample) -> Self {
        let measures = sample.measures;
        Self {
            water: Some(sample.water.map_or(Water::Dry, Water::Level)),
            road_tags: Some(sample.tags),
            nearby_water_height: measures.and_then(|m| m.height_above_water),
            water_distance: measures.map(|m| m.water_distance),
            road_distance: measures.map(|m| m.road_distance),
            curvature: measures.map(|m| m.curvature),
            labels: sample.labels,
            ..Self::new(sample.height, sample.gradient.unwrap_or(Vec2::ZERO))
        }
    }

    /// Returns this site with the supplied landform and flags, replacing
    /// any existing labels. Other site data is unchanged.
    #[must_use]
    pub fn with_labels(mut self, labels: Labels) -> Self {
        self.labels = Some(labels);
        self
    }

    /// The same site with its height within the region's relief (0 valley
    /// floor, 1 highest peak).
    #[must_use]
    pub fn with_relief_height(mut self, relief_height: f32) -> Self {
        self.relief_height = Some(relief_height);
        self
    }

    /// How far the ground lies above the water, in metres: negative under
    /// water. Where water is within
    /// [`WATER_HEIGHT_REACH`](crate::WATER_HEIGHT_REACH) and was measured,
    /// the height above it
    /// ([`nearby_water_height`](Self::nearby_water_height)),
    /// so banks read how high they stand; otherwise the height above the
    /// water covering the point, infinite where none covers it. `None`
    /// where neither is known.
    pub fn height_above_water(&self) -> Option<f32> {
        if let Some(height) = self.nearby_water_height {
            return Some(height);
        }
        self.water.map(|water| match water {
            Water::Dry => f32::INFINITY,
            Water::Level(level) => self.height - level,
        })
    }
}

/// Supplies ground data for painting, from a terrain, a world of terrains,
/// or a [`Ground`] wrapped in [`OnGround`].
///
/// Pass the painter's [`detail`](super::GroundPainter::detail) to
/// [`site_at`](Self::site_at) or [`sites_at`](Self::sites_at). Higher detail
/// requests extra measurements or classifications, which cost more to
/// compute. A source may leave unsupported data unknown.
///
/// Implementors that cannot supply extra detail need only implement
/// [`site`](Self::site); the default methods delegate to it.
pub trait GroundSource: Sync {
    /// Samples world position `p` (x and z, metres) at
    /// [`SampleDetail::Gradient`]. `tier` selects the planned terrain
    /// features for sources that support tiers. Returns a site with NaN
    /// height where ground is unavailable.
    fn site(&self, p: DVec2, tier: FeatureTier) -> GroundSite;

    /// Sites in the order of `points`, with exactly the same values as
    /// [`site`](Self::site). The default samples points independently;
    /// worlds reuse their region boundaries and terrains for the batch.
    fn sites(&self, points: &[DVec2], tier: FeatureTier) -> Vec<GroundSite> {
        crate::batch::map(points, |&p| self.site(p, tier))
    }

    /// Samples world position `p` (x and z, metres) at `tier`, requesting
    /// the data needed by `detail`. Measures add distances and curvature;
    /// labels add landform categories and flags. Unsupported data may
    /// remain unknown. Returns NaN height where ground is unavailable.
    ///
    /// The default calls [`site`](Self::site) and ignores `detail`.
    /// Terrain sources always request at least gradient detail for slope.
    fn site_at(&self, p: DVec2, tier: FeatureTier, detail: SampleDetail) -> GroundSite {
        let _ = detail;
        self.site(p, tier)
    }

    /// Samples all `points` at `tier` and `detail`, returning one site per
    /// point in the same order. Values must match [`site_at`](Self::site_at).
    /// The default calls that method independently for each point.
    fn sites_at(
        &self,
        points: &[DVec2],
        tier: FeatureTier,
        detail: SampleDetail,
    ) -> Vec<GroundSite> {
        crate::batch::map(points, |&p| self.site_at(p, tier, detail))
    }
}

/// Raises lower detail requests to gradient detail so sites include slope.
fn at_least_gradient(detail: SampleDetail) -> SampleDetail {
    detail.max(SampleDetail::Gradient)
}

impl GroundSource for Terrain {
    /// The terrain's own ground, whatever tier is asked for.
    fn site(&self, p: DVec2, tier: FeatureTier) -> GroundSite {
        self.site_at(p, tier, SampleDetail::Gradient)
    }

    /// Samples this terrain at the requested `detail`, with a minimum of
    /// gradient detail so the site includes slope.
    /// Ignores `tier`: the terrain's features are already built.
    fn site_at(&self, p: DVec2, _: FeatureTier, detail: SampleDetail) -> GroundSite {
        GroundSite::from_query(&self.query(p, at_least_gradient(detail)))
    }
}

impl GroundSource for WorldTerrain {
    fn sites(&self, points: &[DVec2], tier: FeatureTier) -> Vec<GroundSite> {
        self.sites_at(points, tier, SampleDetail::Gradient)
    }

    /// The ground of the region holding `p`, at `tier` (see
    /// [`WorldTerrain::terrain_tier`]). NaN height where the region can't
    /// be built.
    fn site(&self, p: DVec2, tier: FeatureTier) -> GroundSite {
        self.site_at(p, tier, SampleDetail::Gradient)
    }

    /// Samples the region containing `p` at `tier` and the requested
    /// `detail`, with a minimum of gradient detail. Returns a site with
    /// NaN height if the region cannot be built.
    fn site_at(&self, p: DVec2, tier: FeatureTier, detail: SampleDetail) -> GroundSite {
        match self.terrain_tier(self.region_at(p), tier) {
            Ok(terrain) => terrain.site_at(p, tier, detail),
            Err(_) => GroundSite::new(f32::NAN, Vec2::ZERO),
        }
    }

    fn sites_at(
        &self,
        points: &[DVec2],
        tier: FeatureTier,
        detail: SampleDetail,
    ) -> Vec<GroundSite> {
        self.sample_batch(points, tier, |terrain, p| {
            terrain.map_or_else(
                || GroundSite::new(f32::NAN, Vec2::ZERO),
                |terrain| terrain.site_at(p, tier, detail),
            )
        })
    }
}

/// Any [`Ground`] as a [`GroundSource`]: plain ground with no water and no
/// roads (see [`GroundSite::from_ground`]), the same at every tier.
#[derive(Clone, Copy)]
pub struct OnGround<'a>(pub &'a dyn Ground);

impl GroundSource for OnGround<'_> {
    fn site(&self, p: DVec2, _: FeatureTier) -> GroundSite {
        GroundSite::from_ground(self.0.sample(p))
    }
}
