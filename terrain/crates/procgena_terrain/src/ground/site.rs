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

/// What ground rules read at one point.
///
/// A measure that is `None` is unknown, not absent: a rule's condition on
/// it counts as 1. (`nearby_water_height` is the exception: it is also
/// `None` where no water is near.) A plain [`Ground`] has no water and no
/// roads; a [`Terrain`] knows where its water and roads are, and with
/// measures how far they are and how the ground bends. Neither knows the
/// relief height yet. A terrain gives labels at [`SampleDetail::Labels`].
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
    /// What kind of land the point is, and its flags (see
    /// [`Terrain::annotate`]).
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

    /// A site from a terrain sample (see [`Terrain::query`]): height,
    /// slope, water and road tags; with [`SampleDetail::Measures`] the
    /// distances to water and roads, the height above nearby water and the
    /// curvature; with [`SampleDetail::Labels`] the labels. Without a
    /// gradient the slope is 0; without measures the distances, nearby
    /// water height and curvature are unknown, and without labels the
    /// labels.
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

    /// The same site with its labels.
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

/// Something ground sites can be read from: a terrain, a world of them, or
/// any [`Ground`] wrapped in [`OnGround`].
///
/// A source may work out more about a point when asked for more detail:
/// [`site_at`](Self::site_at) and [`sites_at`](Self::sites_at) take the
/// [`SampleDetail`] a painter needs (see [`GroundPainter::detail`](super::GroundPainter::detail)).
/// Sources that know nothing beyond height, slope, water and roads need
/// only [`site`](Self::site).
pub trait GroundSource: Sync {
    /// The site at world position `p` (x and z, metres), from the ground
    /// with its features planned to `tier` where the source has tiers, at
    /// [`SampleDetail::Gradient`]. Its height is NaN where there is no
    /// ground.
    fn site(&self, p: DVec2, tier: FeatureTier) -> GroundSite;

    /// Sites in the order of `points`, with exactly the same values as
    /// [`site`](Self::site). The default samples points independently;
    /// worlds reuse their region boundaries and terrains for the batch.
    fn sites(&self, points: &[DVec2], tier: FeatureTier) -> Vec<GroundSite> {
        crate::batch::map(points, |&p| self.site(p, tier))
    }

    /// The site at `p` as [`site`](Self::site) gives it, with as much more
    /// as the source knows up to `detail`: measures from
    /// [`SampleDetail::Measures`], labels from [`SampleDetail::Labels`].
    /// The default is [`site`](Self::site), for sources that know nothing
    /// more.
    fn site_at(&self, p: DVec2, tier: FeatureTier, detail: SampleDetail) -> GroundSite {
        let _ = detail;
        self.site(p, tier)
    }

    /// Sites in the order of `points`, with exactly the same values as
    /// [`site_at`](Self::site_at). The default samples points
    /// independently with [`site_at`](Self::site_at).
    fn sites_at(
        &self,
        points: &[DVec2],
        tier: FeatureTier,
        detail: SampleDetail,
    ) -> Vec<GroundSite> {
        crate::batch::map(points, |&p| self.site_at(p, tier, detail))
    }
}

/// `detail`, but never less than the gradient, which every site needs.
fn at_least_gradient(detail: SampleDetail) -> SampleDetail {
    detail.max(SampleDetail::Gradient)
}

impl GroundSource for Terrain {
    /// The terrain's own ground, whatever tier is asked for.
    fn site(&self, p: DVec2, tier: FeatureTier) -> GroundSite {
        self.site_at(p, tier, SampleDetail::Gradient)
    }

    /// The terrain's own ground, whatever tier is asked for.
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

    /// The ground of the region holding `p`, at `tier` and `detail`. NaN
    /// height where the region can't be built.
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
