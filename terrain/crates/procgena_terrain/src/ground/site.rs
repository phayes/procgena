//! What a ground rule reads at one point, and where it comes from.

use glam::{DVec2, Vec2};
use procgena_core::{Ground, GroundSample};

use crate::{ChunkStage, RoadTags, Terrain, TerrainSample, WorldTerrain};

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
/// it counts as 1. A plain [`Ground`] has no water and no roads; a
/// [`Terrain`] knows where its water and roads are. Neither knows the
/// relief height yet.
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
}

impl GroundSite {
    /// A site on plain ground, with no water and no roads, from its height
    /// and the gradient of the ground there (how much it rises per metre
    /// along x and z). Its relief height is unknown. A gradient that isn't
    /// a number gives a slope of 0.
    pub fn new(height: f32, gradient: Vec2) -> Self {
        let g = gradient.length();
        let slope = if g.is_nan() {
            0.0
        } else if g.is_infinite() {
            1.0
        } else {
            g / g.hypot(1.0)
        };
        Self {
            height,
            slope,
            relief_height: None,
            water: Some(Water::Dry),
            road_tags: Some(RoadTags::NONE),
        }
    }

    /// A site from a [`Ground`]'s sample, as [`new`](Self::new): plain
    /// ground with no water and no roads.
    pub fn from_ground(sample: GroundSample) -> Self {
        Self::new(sample.height, sample.gradient)
    }

    /// A site from a terrain sample taken with its gradient (see
    /// [`Terrain::query`]): height, slope, water and road tags. Without a
    /// gradient the slope is 0.
    pub fn from_query(sample: &TerrainSample) -> Self {
        Self {
            water: Some(sample.water.map_or(Water::Dry, Water::Level)),
            road_tags: Some(sample.tags),
            ..Self::new(sample.height, sample.gradient.unwrap_or(Vec2::ZERO))
        }
    }

    /// The same site with its height within the region's relief (0 valley
    /// floor, 1 highest peak).
    #[must_use]
    pub fn with_relief_height(mut self, relief_height: f32) -> Self {
        self.relief_height = Some(relief_height);
        self
    }

    /// How far the ground lies above the water covering it, in metres:
    /// negative under water, infinite where no water covers it, and `None`
    /// where that isn't known.
    pub fn height_above_water(&self) -> Option<f32> {
        self.water.map(|water| match water {
            Water::Dry => f32::INFINITY,
            Water::Level(level) => self.height - level,
        })
    }
}

/// Something ground sites can be read from: a terrain, a world of them, or
/// any [`Ground`] wrapped in [`OnGround`].
pub trait GroundSource: Sync {
    /// The site at world position `p` (x and z, metres), from the ground at
    /// `stage` where the source has stages. Its height is NaN where there
    /// is no ground.
    fn site(&self, p: DVec2, stage: ChunkStage) -> GroundSite;
}

impl GroundSource for Terrain {
    /// The terrain's own ground, whatever stage is asked for.
    fn site(&self, p: DVec2, _: ChunkStage) -> GroundSite {
        GroundSite::from_query(&self.query(p, true))
    }
}

impl GroundSource for WorldTerrain {
    /// The ground of the region holding `p`, at `stage` (see
    /// [`ChunkSource`](crate::ChunkSource) for [`WorldTerrain`]). NaN height
    /// where the region can't be built.
    fn site(&self, p: DVec2, stage: ChunkStage) -> GroundSite {
        let region = self.region_at(p);
        let terrain = match stage {
            ChunkStage::Canonical => self.terrain(region),
            ChunkStage::Provisional => self.provisional_terrain(region),
        };
        match terrain {
            Ok(terrain) => terrain.site(p, stage),
            Err(_) => GroundSite::new(f32::NAN, Vec2::ZERO),
        }
    }
}

/// Any [`Ground`] as a [`GroundSource`]: plain ground with no water and no
/// roads (see [`GroundSite::from_ground`]), the same at every stage.
#[derive(Clone, Copy)]
pub struct OnGround<'a>(pub &'a dyn Ground);

impl GroundSource for OnGround<'_> {
    fn site(&self, p: DVec2, _: ChunkStage) -> GroundSite {
        GroundSite::from_ground(self.0.sample(p))
    }
}
