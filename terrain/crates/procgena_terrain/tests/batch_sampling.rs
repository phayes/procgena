//! Batched world reads preserve point ownership, sample order and meshes.

use std::sync::Arc;

use glam::DVec2;
use procgena_terrain::{
    ChunkError, ChunkKey, ChunkSamples, ChunkSide, ChunkSource, Elevation, FeatureLevel, RegionId,
    SurfacePoint, TierLevels, Transitions, WorldTerrain, fixtures,
};

/// Uses the trait defaults, deliberately doing a world lookup per point.
struct Pointwise<'a>(&'a WorldTerrain);

impl ChunkSource for Pointwise<'_> {
    fn surface(&self, p: DVec2, level: FeatureLevel) -> SurfacePoint {
        self.0.surface(p, level)
    }
    fn level(&self, level: FeatureLevel) -> FeatureLevel {
        self.0.level(level)
    }
    fn level_difference(&self, min: DVec2, max: DVec2, from: FeatureLevel) -> f32 {
        self.0.level_difference(min, max, from)
    }
}

/// A world that reads each tier at one level (see [`TierLevels::ONE_EACH`]).
fn world() -> WorldTerrain {
    WorldTerrain::new(
        fixtures::world_medium(),
        Arc::new(|id: RegionId, _: DVec2, _: &Elevation| {
            if id.0.is_multiple_of(2) {
                fixtures::plains_medium().terrain.clone()
            } else {
                fixtures::rolling_hills_medium().terrain.clone()
            }
        }),
    )
    .unwrap()
    .with_tier_levels(TierLevels::ONE_EACH)
}

/// The levels that read each tier in [`world`], roughest first.
const EVERY_TIER: [FeatureLevel; 3] = [
    FeatureLevel::COARSEST,
    FeatureLevel(1),
    FeatureLevel::FINEST,
];

#[test]
fn batches_match_individual_reads_on_warped_borders_and_corners() {
    let world = world();
    let region = world.region(world.region_at(DVec2::ZERO));
    let mut points = vec![DVec2::ZERO, DVec2::new(-31.5, 12.0)];
    for edge in &region.edges {
        for &p in &edge.polyline {
            points.extend([p, p + DVec2::splat(0.001), p - DVec2::splat(0.001)]);
        }
    }
    let pointwise = Pointwise(&world);
    for level in EVERY_TIER {
        assert_eq!(
            world.surfaces(&points, level),
            pointwise.surfaces(&points, level)
        );
        points.reverse();
    }
    assert!(world.surfaces(&[], FeatureLevel::FINEST).is_empty());
}

#[test]
fn batch_meshes_match_pointwise_across_a_region_border() {
    let world = world();
    let region = world.region(world.region_at(DVec2::ZERO));
    let edge = &region.edges[0];
    let key = ChunkKey::containing(edge.polyline[edge.polyline.len() / 2], 1);
    let pointwise = Pointwise(&world);
    for level in [FeatureLevel::COARSEST, FeatureLevel::FINEST] {
        let batch = ChunkSamples::sample(&world, key, level).unwrap();
        let single = ChunkSamples::sample(&pointwise, key, level).unwrap();
        for transitions in [
            Transitions::NONE,
            Transitions::NONE.with_side(ChunkSide::MinX),
        ] {
            assert_eq!(batch.mesh(transitions, 8.0), single.mesh(transitions, 8.0));
        }
    }
}

#[test]
fn failed_regions_and_missing_ground_keep_their_error_behavior() {
    let world = WorldTerrain::new(
        fixtures::world_medium(),
        Arc::new(|_: RegionId, _: DVec2, _: &Elevation| {
            let mut options = fixtures::plains_medium().terrain.clone();
            options.base.elevation = f32::NAN;
            options
        }),
    )
    .unwrap();
    let points = [DVec2::ZERO, DVec2::new(-2.0, 3.0), DVec2::ZERO];
    assert!(
        world
            .surfaces(&points, FeatureLevel::FINEST)
            .iter()
            .all(|s| s.height.is_nan() && s.water.is_none())
    );
    let key = ChunkKey::new(0, 0, 0);
    assert!(matches!(
        ChunkSamples::sample(&world, key, FeatureLevel::FINEST),
        Err(ChunkError::NoGround(_))
    ));
}
