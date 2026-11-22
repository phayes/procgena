//! Measurements of the test worlds, run by hand:
//! `cargo test -p procgena_terrain --release --test probes -- --ignored --nocapture`.

mod common;

use common::{CONFIGS, Config, TWO_REGION_BORDER, hills_into_plains, two_biomes};
use glam::DVec2;
use procgena_terrain::{PortKind, StitchedWorld};

/// Points along the two-region border, at most `spacing` metres apart.
fn border_points(spacing: f64) -> Vec<DVec2> {
    let mut out = Vec::new();
    for w in TWO_REGION_BORDER.windows(2) {
        let (a, b) = (DVec2::from(w[0]), DVec2::from(w[1]));
        let count = (a.distance(b) / spacing).ceil() as usize;
        out.extend((0..count).map(|k| a.lerp(b, k as f64 / count as f64)));
    }
    out
}

/// A configuration as a short label.
fn label(config: Config) -> String {
    format!("{:?} seeds, meeting {}", config.seeds, config.meeting)
}

/// How far the mountains' own ground lies above the plains' own ground
/// along the two-biome border (mean and largest, with the part of the mean
/// that comes from the presets' mean elevations), and how far the
/// negotiated border moves each side's own ground (mean).
#[test]
#[ignore]
fn border_gap() {
    for config in CONFIGS {
        let world = two_biomes(DVec2::ZERO, config);
        let [plains, mountains] = [0, 1].map(|i| &world.terrains()[i]);
        let points = border_points(5.0);
        let gaps: Vec<f32> = points
            .iter()
            .map(|&p| {
                mountains.generator().natural_height(p) - plains.generator().natural_height(p)
            })
            .collect();
        let mean = gaps.iter().sum::<f32>() / gaps.len() as f32;
        let largest = gaps.iter().fold(0.0f32, |m, g| m.max(g.abs()));
        let elevation = mountains.generator().options().base.elevation
            - plains.generator().options().base.elevation;
        let shift = |t: &procgena_terrain::Terrain| {
            points
                .iter()
                .map(|&p| (t.uncarved_height(p) - t.generator().natural_height(p)).abs())
                .sum::<f32>()
                / points.len() as f32
        };
        eprintln!(
            "{}: gap mean {mean:.0} m, largest {largest:.0} m, elevation {elevation:.0} m; \
             shift plains {:.0} m, mountains {:.0} m",
            label(config),
            shift(plains),
            shift(mountains),
        );
    }
}

/// The number of river crossings in `world`.
fn river_crossings(world: &StitchedWorld) -> usize {
    world
        .agreements()
        .iter()
        .flat_map(|a| &a.ports)
        .filter(|p| matches!(p.kind, PortKind::River { .. }))
        .count()
}

/// Which hill seeds from 0 to 299 give a river crossing in
/// `hills_into_plains`.
#[test]
#[ignore]
fn river_crossing_seeds() {
    for config in CONFIGS {
        let seeds: Vec<u32> = (0..300)
            .filter(|&s| river_crossings(&hills_into_plains(s, config)) > 0)
            .collect();
        eprintln!(
            "{}: {} of 300 give a river crossing: {seeds:?}",
            label(config),
            seeds.len()
        );
    }
}

/// Every road steeper than its grade and every river cut deeper than the
/// feature suite allows, in every test world.
#[test]
#[ignore]
fn feature_limits() {
    for config in CONFIGS {
        let mut worlds = vec![
            ("two biomes".to_string(), two_biomes(DVec2::ZERO, config)),
            (
                "triple junction".to_string(),
                common::triple_junction(config),
            ),
        ];
        for s in common::RIVER_SEEDS {
            worlds.push((
                format!("hills into plains {s}"),
                hills_into_plains(s, config),
            ));
        }
        for (name, world) in &worlds {
            for t in world.terrains() {
                let o = t.generator().options();
                let max_grade = f64::from(o.features.roads.max_grade);
                for road in t.roads() {
                    let steepest = road
                        .points
                        .windows(2)
                        .zip(road.levels.windows(2))
                        .map(|(p, l)| f64::from((l[1] - l[0]).abs()) / p[0].distance(p[1]))
                        .fold(0.0, f64::max);
                    if steepest > max_grade * 1.001 {
                        eprintln!(
                            "{}, {name}, region {}: road grade {steepest:.4} over {max_grade}",
                            label(config),
                            t.region().id.0
                        );
                    }
                }
                let allowed = 4.0 + 0.03 * (o.base.hills.height + o.base.mountains.height) + 21.0;
                for river in t.rivers() {
                    let cut = river
                        .points
                        .iter()
                        .zip(&river.levels)
                        .map(|(&p, &l)| world.owner(p).unwrap().uncarved_height(p) - l)
                        .fold(0.0f32, f32::max);
                    if cut > allowed {
                        eprintln!(
                            "{}, {name}, region {}: river cut {cut:.1} m over {allowed:.1}",
                            label(config),
                            t.region().id.0
                        );
                    }
                }
            }
        }
    }
}
