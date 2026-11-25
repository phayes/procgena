//! Cold and warm benchmarks: how long each planning stage of a region
//! takes, how long sampling a built terrain takes, per point and per tile,
//! and how long a chunk mesh takes to read and mesh, warm and cold.
//! Ignored; run in release and read the printed tables:
//!
//! `cargo test -p procgena_terrain --release --test bench -- --ignored --nocapture --test-threads 1`

mod common;

use std::time::{Duration, Instant};

use common::{DEFAULT, RIVER_SEEDS, WARPED, hills_into_plains, triple_junction, two_terrains};
use glam::{DVec2, I64Vec2, UVec2};
use procgena_terrain::{EdgeAgreement, HeightTile, SampleDetail, StitchedWorld, Terrain};

/// The 50th, 95th and 99th percentiles and the largest of `times`, in
/// milliseconds.
fn quantiles(times: &mut [f64]) -> String {
    times.sort_by(f64::total_cmp);
    let q = |f: f64| times[((times.len() - 1) as f64 * f).round() as usize];
    format!(
        "p50 {:8.2}  p95 {:8.2}  p99 {:8.2}  max {:8.2}  (n {})",
        q(0.5),
        q(0.95),
        q(0.99),
        times[times.len() - 1],
        times.len()
    )
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// The machine's load averages, as `uptime` prints them, to read beside
/// the timings; empty where `uptime` can't be run.
fn load() -> String {
    std::process::Command::new("uptime")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| {
            s.split("load average")
                .nth(1)
                .map(|l| l.trim_start_matches(['s', ':', ' ']).trim().to_owned())
        })
        .unwrap_or_default()
}

/// The median of `times`.
fn median(times: &[f64]) -> f64 {
    let mut sorted = times.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[sorted.len() / 2]
}

/// The test worlds, with labels: straight and warped borders, two and
/// three regions, plains, hills and mountains.
fn worlds() -> Vec<(&'static str, StitchedWorld)> {
    (0..WORLDS).map(world).collect()
}

/// How many test worlds there are.
const WORLDS: usize = 5;

/// Test world `i`, built from nothing, with its label.
fn world(i: usize) -> (&'static str, StitchedWorld) {
    match i {
        0 => ("two terrains", two_terrains(DVec2::ZERO, DEFAULT)),
        1 => ("two terrains, warped", two_terrains(DVec2::ZERO, WARPED[0])),
        2 => ("triple junction", triple_junction(DEFAULT)),
        3 => ("triple junction, warped", triple_junction(WARPED[0])),
        _ => (
            "hills into plains",
            hills_into_plains(RIVER_SEEDS[0], DEFAULT),
        ),
    }
}

/// The agreements of `terrain`'s edges in `world`.
fn agreements_of(world: &StitchedWorld, terrain: &Terrain) -> Vec<EdgeAgreement> {
    world
        .agreements()
        .iter()
        .filter(|a| terrain.region().edges.iter().any(|e| e.id == a.edge))
        .cloned()
        .collect()
}

/// Repeatable points inside `terrain`'s region.
fn points_in(terrain: &Terrain, count: usize) -> Vec<DVec2> {
    let mut out = Vec::with_capacity(count);
    let mut k: u64 = 1;
    while out.len() < count {
        k = k
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let x = ((k >> 11) as f64 / (1u64 << 53) as f64) * 4000.0 - 2000.0;
        let z = ((k.rotate_left(29) >> 11) as f64 / (1u64 << 53) as f64) * 3000.0 - 1500.0;
        let p = DVec2::new(x, z);
        if terrain.region().boundary.contains(p) {
            out.push(p);
        }
    }
    out
}

#[test]
#[ignore]
fn cold_region_planning() {
    eprintln!(
        "Cold planning per region, in ms (each stage alone, from a built world's inputs); \
         load {}:",
        load()
    );
    let (mut whole, mut trace, mut offers, mut hydrology, mut roads) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    // The warm reference, under the same load: 64 × 64 heights at 1 m,
    // on one thread, on each built terrain.
    let mut warm = Vec::new();
    for i in 0..WORLDS {
        // The whole world, built from nothing.
        let started = Instant::now();
        let (_, world) = world(i);
        let regions = world.terrains().len();
        whole.push(ms(started.elapsed()) / regions as f64);

        for terrain in world.terrains() {
            let generator = terrain.generator().clone();
            let region = terrain.region().clone();
            let mine = agreements_of(&world, terrain);

            let started = Instant::now();
            let traced = generator.trace_rivers(&region);
            trace.push(ms(started.elapsed()));

            let started = Instant::now();
            let _ = generator.offers_traced(&region, &traced);
            offers.push(ms(started.elapsed()));

            let started = Instant::now();
            let bare = generator
                .build_traced(region, &mine, &traced)
                .expect("builds");
            hydrology.push(ms(started.elapsed()));

            let started = Instant::now();
            let full = bare.with_tier(procgena_terrain::FeatureTier::Fine);
            roads.push(ms(started.elapsed()));

            let corner = points_in(&full, 1)[0];
            let started = Instant::now();
            for j in 0..64 {
                for i in 0..64 {
                    let q = corner + DVec2::new(f64::from(i), f64::from(j));
                    std::hint::black_box(full.height(q));
                }
            }
            warm.push(ms(started.elapsed()));
        }
    }
    let ratio = |stage: &[f64]| median(stage) / median(&warm);
    let plan: Vec<f64> = (0..trace.len())
        .map(|k| trace[k] + offers[k] + hydrology[k] + roads[k])
        .collect();
    let far: Vec<f64> = (0..trace.len())
        .map(|k| trace[k] + offers[k] + hydrology[k])
        .collect();
    eprintln!(
        "  cold ÷ warm (median stage ÷ median 64 × 64 heights tile, 1 thread, {:.2} ms):",
        median(&warm)
    );
    eprintln!(
        "    trace {:.1}, offers {:.1}, hydrology {:.1}, roads {:.1}; \
         whole plan {:.1}, without roads {:.1}",
        ratio(&trace),
        ratio(&offers),
        ratio(&hydrology),
        ratio(&roads),
        ratio(&plan),
        ratio(&far)
    );
    eprintln!("  river trace       {}", quantiles(&mut trace));
    eprintln!("  offers            {}", quantiles(&mut offers));
    eprintln!("  bend + hydrology  {}", quantiles(&mut hydrology));
    eprintln!("  roads             {}", quantiles(&mut roads));
    eprintln!("  whole plan        {}", quantiles(&mut plan.clone()));
    eprintln!("  without roads     {}", quantiles(&mut far.clone()));
    eprintln!(
        "  whole world ÷ regions (with negotiation and threads) {}",
        quantiles(&mut whole)
    );
    eprintln!("  load after: {}", load());
}

#[test]
#[ignore]
fn warm_sampling() {
    eprintln!("Warm sampling, per call, in µs; load {}:", load());
    let (mut height, mut query, mut sample) = (Vec::new(), Vec::new(), Vec::new());
    let (mut measures, mut labels) = (Vec::new(), Vec::new());
    let mut border = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (_, world) in worlds() {
        for terrain in world.terrains() {
            for p in points_in(terrain, 4000) {
                let near_border = terrain.region().boundary.distance_to_border(p) < 200.0;
                let started = Instant::now();
                std::hint::black_box(terrain.height(p));
                let h = started.elapsed().as_secs_f64() * 1e6;
                let started = Instant::now();
                std::hint::black_box(terrain.query(p, SampleDetail::Height));
                query.push(started.elapsed().as_secs_f64() * 1e6);
                let started = Instant::now();
                std::hint::black_box(terrain.sample(p));
                let s = started.elapsed().as_secs_f64() * 1e6;
                let started = Instant::now();
                std::hint::black_box(terrain.query(p, SampleDetail::Measures));
                let m = started.elapsed().as_secs_f64() * 1e6;
                let started = Instant::now();
                std::hint::black_box(terrain.query(p, SampleDetail::Labels));
                let l = started.elapsed().as_secs_f64() * 1e6;
                height.push(h);
                sample.push(s);
                measures.push(m);
                labels.push(l);
                if near_border {
                    border.0.push(h);
                    border.1.push(s);
                    border.2.push(m);
                    border.3.push(l);
                }
            }
        }
    }
    eprintln!("  height                     {}", quantiles(&mut height));
    eprintln!("  query, no gradient         {}", quantiles(&mut query));
    eprintln!("  sample (with gradient)     {}", quantiles(&mut sample));
    eprintln!("  query, with measures       {}", quantiles(&mut measures));
    eprintln!("  query, with labels         {}", quantiles(&mut labels));
    eprintln!(
        "  measures, within 200 m of a border  {}",
        quantiles(&mut border.2)
    );
    eprintln!(
        "  labels, within 200 m of a border  {}",
        quantiles(&mut border.3)
    );
    eprintln!(
        "  height, within 200 m of a border  {}",
        quantiles(&mut border.0)
    );
    eprintln!(
        "  sample, within 200 m of a border  {}",
        quantiles(&mut border.1)
    );
}

#[test]
#[ignore]
fn warm_tiles() {
    eprintln!(
        "Warm tiles of 64 × 64 samples, per tile, in ms (rayon threads); load {}:",
        load()
    );
    for step in [1.0, 8.0] {
        let (mut with, mut heights) = (Vec::new(), Vec::new());
        for (_, world) in worlds() {
            for terrain in world.terrains() {
                for p in points_in(terrain, 6) {
                    let origin = (p / step).floor().as_i64vec2() - I64Vec2::splat(32);
                    let size = UVec2::splat(64);
                    let started = Instant::now();
                    std::hint::black_box(HeightTile::sample_with_gradients(
                        terrain, origin, size, step,
                    ));
                    with.push(ms(started.elapsed()));
                    let started = Instant::now();
                    std::hint::black_box(HeightTile::sample(terrain, origin, size, step));
                    heights.push(ms(started.elapsed()));
                }
            }
        }
        eprintln!(
            "  step {step} m, heights and gradients  {}",
            quantiles(&mut with)
        );
        eprintln!(
            "  step {step} m, heights only            {}",
            quantiles(&mut heights)
        );
    }
}

#[test]
#[ignore]
fn chunks() {
    use procgena_terrain::{
        ChunkKey, ChunkSamples, Elevation, FeatureTier, RegionId, TerrainPreset, Transitions,
        WorldTerrain, WorldTerrainOptions,
    };
    use std::sync::Arc;

    eprintln!(
        "Chunks of 64 × 64 cells, per chunk, in ms (rayon threads); load {}:",
        load()
    );

    // Warm: reading and meshing chunks of built terrains.
    for level in [0, 3, 6] {
        let (mut read, mut plain, mut split) = (Vec::new(), Vec::new(), Vec::new());
        for (_, world) in worlds() {
            for terrain in world.terrains() {
                for p in points_in(terrain, 3) {
                    let key = ChunkKey::containing(p, level);
                    let started = Instant::now();
                    let samples = ChunkSamples::sample(terrain, key, FeatureTier::Fine)
                        .expect("ground everywhere");
                    read.push(ms(started.elapsed()));
                    let started = Instant::now();
                    std::hint::black_box(samples.mesh(Transitions::NONE, 8.0));
                    plain.push(ms(started.elapsed()));
                    let started = Instant::now();
                    std::hint::black_box(samples.mesh(Transitions::ALL, 8.0));
                    split.push(ms(started.elapsed()));
                }
            }
        }
        eprintln!(
            "  warm, level {level}, read          {}",
            quantiles(&mut read)
        );
        eprintln!(
            "  warm, level {level}, mesh          {}",
            quantiles(&mut plain)
        );
        eprintln!(
            "  warm, level {level}, mesh, split   {}",
            quantiles(&mut split)
        );
    }

    // Cold: the first chunk of a fresh world, which builds the regions it
    // touches and their neighbours' plans.
    for (stage, level) in [
        (FeatureTier::Fine, 0),
        (FeatureTier::Rough, 0),
        (FeatureTier::Rough, 5),
    ] {
        let mut cold = Vec::new();
        for seed in 1..=4 {
            let source = |_: RegionId, _: DVec2, _: &Elevation| {
                TerrainPreset::rolling_hills().terrain.clone()
            };
            let options = WorldTerrainOptions {
                seed,
                ..WorldTerrainOptions::default()
            };
            let world = WorldTerrain::new(options, Arc::new(source)).unwrap();
            let key = ChunkKey::containing(DVec2::new(300.0, -200.0), level);
            let started = Instant::now();
            std::hint::black_box(ChunkSamples::sample(&world, key, stage).unwrap());
            cold.push(ms(started.elapsed()));
        }
        eprintln!(
            "  cold, {stage:?}, level {level}   {}",
            quantiles(&mut cold)
        );
    }
}
