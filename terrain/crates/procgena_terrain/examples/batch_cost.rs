//! Compares warm pointwise and batched world sampling, checking equal output.
//!
//! `cargo run --release -p procgena_terrain --example batch_cost`
//! Region construction is warmed up before timing. Times include ownership
//! and sampling, but exclude mesh construction.

use std::sync::Arc;
use std::time::Instant;

use glam::DVec2;
use procgena_terrain::{
    ChunkSource, Elevation, FeatureLevel, RegionId, SurfacePoint, TerrainPreset, WorldTerrain,
    WorldTerrainOptions,
};

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

fn main() {
    let world = WorldTerrain::new(
        WorldTerrainOptions::default(),
        Arc::new(|_: RegionId, _: DVec2, _: &Elevation| {
            TerrainPreset::rolling_hills().terrain.clone()
        }),
    )
    .unwrap();
    let pointwise = Pointwise(&world);
    let region = world.region(world.region_at(DVec2::ZERO));
    let edge = &region.edges[0].polyline;
    for (name, centre) in [("origin", DVec2::ZERO), ("border", edge[edge.len() / 2])] {
        let points: Vec<_> = (0..129)
            .flat_map(|z| {
                (0..129)
                    .map(move |x| centre + DVec2::new(f64::from(x - 64), f64::from(z - 64)) * 0.5)
            })
            .collect();
        for level in [FeatureLevel::COARSEST, FeatureLevel::FINEST] {
            world.surfaces(&points, level);
            compare(
                &format!("{name} level {} surfaces", level.0),
                || pointwise.surfaces(&points, level),
                || world.surfaces(&points, level),
            );
        }
    }
}

fn compare<T: PartialEq>(name: &str, scalar: impl Fn() -> T, batch: impl Fn() -> T) {
    let mut before = Vec::new();
    let mut after = Vec::new();
    for round in 0..3 {
        let time = |f: &dyn Fn() -> T| {
            let start = Instant::now();
            let value = f();
            (value, start.elapsed())
        };
        let (a, b) = if round % 2 == 0 {
            (time(&scalar), time(&batch))
        } else {
            let b = time(&batch);
            (time(&scalar), b)
        };
        assert!(a.0 == b.0, "{name}: batch changed the samples");
        before.push(a.1);
        after.push(b.1);
    }
    before.sort();
    after.sort();
    println!(
        "{name}: pointwise {:.1} ms, batch {:.1} ms, {:.2}x",
        before[1].as_secs_f64() * 1000.0,
        after[1].as_secs_f64() * 1000.0,
        before[1].as_secs_f64() / after[1].as_secs_f64()
    );
}
