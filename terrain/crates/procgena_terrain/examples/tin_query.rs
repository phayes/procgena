//! Measures ground queries on loaded canonical chunks against reading the
//! terrain's own ground, on the shipped presets and the default world.
//!
//! ```bash
//! cargo run --release -p procgena_terrain --example tin_query
//! ```
//!
//! Options: `--preset <key>` (default: every shipped preset), `--seed <n>`
//! world seed (default 1), `--chunks <n>` an n × n block of loaded chunks
//! round the origin (default 4), `--points <n>` queries per run (default
//! 200,000).
//!
//! Every run asks for the same points over the block. "Scattered" points
//! jump about the block; "along a line" points follow a path a metre
//! apart, as a moving object or a road would ask. The terrain's own
//! ground is read one point at a time (`ChunkSource::surface`) and in one
//! batch (`ChunkSource::surfaces`, on every thread with the `rayon`
//! feature). Times are on a world whose regions and chunks are already
//! built; building them is `tin_cost`'s measure.

use std::sync::Arc;
use std::time::Instant;

use glam::DVec2;
use procgena_terrain::tin::{GroundSurface, ResidentTins, TinIndex, TinTolerance, build_chunk};
use procgena_terrain::{
    ChunkKey, ChunkSource, Elevation, FeatureTier, RegionId, TerrainPreset, WorldTerrain,
    WorldTerrainOptions,
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
    let seed: u32 = option("--seed").map_or(1, |s| s.parse().expect("--seed <n>"));
    let n: i64 = option("--chunks").map_or(4, |s| s.parse().expect("--chunks <n>"));
    let count: usize = option("--points").map_or(200_000, |s| s.parse().expect("--points <n>"));
    println!(
        "seed {seed}, {n} × {n} chunks, {count} points, rayon {}",
        cfg!(feature = "rayon")
    );
    println!(
        "{:<16} {:>10} {:>14} {:>14} {:>14} {:>14} {:>14} {:>14}",
        "preset",
        "index µs",
        "TIN scattered",
        "TIN batch",
        "TIN line",
        "H* one",
        "H* batch",
        "H* line batch"
    );

    for preset in TerrainPreset::all() {
        if only.as_deref().is_some_and(|key| key != preset.key) {
            continue;
        }
        let options = preset.terrain.clone();
        let source = move |_: RegionId, _: DVec2, _: &Elevation| options.clone();
        let world = WorldTerrain::new(
            WorldTerrainOptions {
                seed,
                ..WorldTerrainOptions::default()
            },
            Arc::new(source),
        )
        .expect("the default world options are valid");

        // The loaded chunks, and how long indexing each takes.
        let mut tins = ResidentTins::new();
        let mut index_us = Vec::new();
        for z in -n / 2..n - n / 2 {
            for x in -n / 2..n - n / 2 {
                let chunk = build_chunk(&world, ChunkKey::new(x, z, 0), &TinTolerance::GENERAL)
                    .expect("the default world has ground everywhere");
                let start = Instant::now();
                let index = TinIndex::new(Arc::new(chunk));
                index_us.push(start.elapsed().as_secs_f64() * 1e6);
                tins.insert(Arc::new(index));
            }
        }

        // The points: scattered over the block, and along a winding path.
        let (lo, size) = ((-n / 2) as f64 * 64.0, n as f64 * 64.0);
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        let scattered: Vec<DVec2> = (0..count)
            .map(|_| DVec2::new(lo + next() * size, lo + next() * size))
            .collect();
        let line: Vec<DVec2> = (0..count)
            .map(|i| {
                let t = i as f64;
                let centre = DVec2::splat(lo + size / 2.0);
                let radius = 0.45 * size * (0.6 + 0.4 * (t / 977.0).sin());
                centre + radius * DVec2::new((t / 400.0).cos(), (t / 400.0).sin())
            })
            .collect();

        // Nanoseconds per query of `run` over `points`, best of three.
        let time = |run: &dyn Fn(&[DVec2]) -> usize, points: &[DVec2]| {
            (0..3)
                .map(|_| {
                    let start = Instant::now();
                    let answered = run(points);
                    assert!(answered > 0);
                    start.elapsed().as_secs_f64() * 1e9 / points.len() as f64
                })
                .fold(f64::INFINITY, f64::min)
        };
        let tin_one = |points: &[DVec2]| points.iter().filter_map(|&p| tins.sample(p)).count();
        let tin_batch = |points: &[DVec2]| tins.sample_batch(points).iter().flatten().count();
        let h_one = |points: &[DVec2]| {
            points
                .iter()
                .filter(|&&p| !world.surface(p, FeatureTier::Fine).height.is_nan())
                .count()
        };
        let h_batch = |points: &[DVec2]| {
            world
                .surfaces(points, FeatureTier::Fine)
                .iter()
                .filter(|s| !s.height.is_nan())
                .count()
        };
        index_us.sort_by(f64::total_cmp);
        println!(
            "{:<16} {:>10.1} {:>11.0} ns {:>11.0} ns {:>11.0} ns {:>11.0} ns {:>11.0} ns {:>11.0} ns",
            preset.key,
            index_us[index_us.len() / 2],
            time(&tin_one, &scattered),
            time(&tin_batch, &scattered),
            time(&tin_batch, &line),
            time(&h_one, &scattered),
            time(&h_batch, &scattered),
            time(&h_batch, &line),
        );
    }
}
