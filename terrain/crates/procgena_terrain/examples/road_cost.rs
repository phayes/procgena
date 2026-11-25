//! Measures what planning a region's roads costs, beside the rest of
//! building the region, on the shipped presets and the default world.
//!
//! ```bash
//! cargo run --release -p procgena_terrain --example road_cost
//! ```
//!
//! Options: `--preset <key>` (default: every shipped preset), `--seeds <n>`
//! world seeds 1 to n (default 3), `--regions <n>` regions timed per world,
//! nearest the origin first (default 3), `--repeat <n>` how many times
//! each region's roads are planned again for the median (default 1).
//!
//! For each region it first builds the rough tier (the region's plan, its
//! neighbours' plans and its ground without roads), then times the fine
//! tier on its own, which today is the roads: routing, levels, and how far
//! they move the ground. It prints both times, the number of road lines
//! and their total length.

use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::DVec2;
use procgena_terrain::{
    Elevation, FeatureTier, RegionId, TerrainPreset, WorldTerrain, WorldTerrainOptions,
};

fn main() {
    // Options.
    let args: Vec<String> = std::env::args().collect();
    let option = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let only = option("--preset");
    let seeds: u32 = option("--seeds").map_or(3, |s| s.parse().expect("--seeds <n>"));
    let regions: usize = option("--regions").map_or(3, |s| s.parse().expect("--regions <n>"));
    let repeat: usize = option("--repeat").map_or(1, |s| s.parse().expect("--repeat <n>"));
    println!(
        "seeds 1..={seeds}, {regions} regions per world, repeat {repeat}, rayon {}",
        cfg!(feature = "rayon")
    );
    println!(
        "{:<16} {:>4} {:>8} {:>9} {:>9} {:>6} {:>8}",
        "preset", "seed", "region", "rough ms", "roads ms", "lines", "length m"
    );

    // Each preset over every seed.
    let mut all_roads = Vec::new();
    for preset in TerrainPreset::all() {
        if only.as_deref().is_some_and(|key| key != preset.key) {
            continue;
        }
        let mut preset_roads = Vec::new();
        for seed in 1..=seeds {
            let options = preset.terrain.clone();
            let source = move |_: RegionId, _: DVec2, _: &Elevation| options.clone();
            let world_options = WorldTerrainOptions {
                seed,
                ..WorldTerrainOptions::default()
            };
            let probe = WorldTerrain::new(world_options.clone(), Arc::new(source.clone()))
                .expect("the default world options are valid");

            // The region at the origin, then those around it nearest first.
            let centre = probe.region_at(DVec2::ZERO);
            let mut ids: Vec<RegionId> = probe
                .region(centre)
                .corners
                .iter()
                .flat_map(|c| c.incident.iter().copied())
                .filter(|&r| r != centre)
                .collect();
            ids.sort();
            ids.dedup();
            ids.sort_by(|&a, &b| probe.site(a).length().total_cmp(&probe.site(b).length()));
            ids.insert(0, centre);
            ids.truncate(regions);

            for id in ids {
                // A fresh world for each repeat, so nothing is kept.
                let mut rough = Vec::new();
                let mut roads = Vec::new();
                let mut lines = 0;
                let mut length = 0.0;
                for _ in 0..repeat {
                    let world = WorldTerrain::new(world_options.clone(), Arc::new(source.clone()))
                        .expect("the default world options are valid");
                    let started = Instant::now();
                    if world.terrain_tier(id, FeatureTier::Rough).is_err() {
                        break;
                    }
                    rough.push(started.elapsed());
                    let started = Instant::now();
                    let Ok(fine) = world.terrain(id) else {
                        break;
                    };
                    roads.push(started.elapsed());
                    lines = fine.roads().len();
                    length = fine
                        .roads()
                        .iter()
                        .map(|r| {
                            r.points
                                .windows(2)
                                .map(|w| w[0].distance(w[1]))
                                .sum::<f64>()
                        })
                        .sum();
                }
                if roads.is_empty() {
                    println!(
                        "{:<16} {seed:>4} {:>8} could not be built",
                        preset.key, id.0
                    );
                    continue;
                }
                let roads_ms = median(&mut roads);
                println!(
                    "{:<16} {seed:>4} {:>8} {:>9.0} {:>9.0} {lines:>6} {length:>8.0}",
                    preset.key,
                    id.0,
                    median(&mut rough),
                    roads_ms,
                );
                preset_roads.push(roads_ms);
            }
        }
        if !preset_roads.is_empty() {
            println!(
                "{:<16} roads median {:.0} ms, max {:.0} ms",
                preset.key,
                median_ms(&mut preset_roads.clone()),
                preset_roads.iter().copied().fold(0.0, f64::max)
            );
        }
        all_roads.extend(preset_roads);
    }
    if !all_roads.is_empty() {
        println!(
            "all presets: roads median {:.0} ms, max {:.0} ms over {} regions",
            median_ms(&mut all_roads.clone()),
            all_roads.iter().copied().fold(0.0, f64::max),
            all_roads.len()
        );
    }
}

/// The median of `times`, in milliseconds.
fn median(times: &mut [Duration]) -> f64 {
    times.sort();
    times[times.len() / 2].as_secs_f64() * 1000.0
}

/// The median of `values`.
fn median_ms(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}
