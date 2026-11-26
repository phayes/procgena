//! Measures what rivers cost: a cold build of the opening view split by
//! region stage, and, per height read in the region at the origin, what
//! carving adds to the bent ground, on points inside a river's valley reach
//! and on points outside it.
//!
//! ```bash
//! RAYON_NUM_THREADS=1 cargo run --release -p procgena_terrain --example river_cost
//! ```
//!
//! Options: `--preset <key>` (default: every shipped preset), `--seed <n>`
//! world seed (default 1), `--spacing <m>` the probe grid's spacing in
//! metres (default 8), `--repeat <n>` timing passes, the fastest kept
//! (default 3), `--no-build` skips the 3 × 3 cold build and builds only
//! the region at the origin.
//!
//! The per-point times are the fastest pass over the probe grid's points,
//! each class timed on its own: the bent ground before carving
//! (`uncarved_height`), with rivers and ponds cut in
//! (`height_before_roads`), the full height (`height`), and the full
//! height with its gradient (`sample`).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use glam::DVec2;
use procgena_terrain::{
    Boundary, ChunkSource, Elevation, FeatureKind, FeatureTier, RegionId, RegionProgress,
    RegionStage, Terrain, TerrainPreset, WorldTerrain, WorldTerrainOptions,
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
    let spacing: f64 = option("--spacing").map_or(8.0, |s| s.parse().expect("--spacing <m>"));
    let repeat: usize = option("--repeat").map_or(3, |s| s.parse().expect("--repeat <n>"));
    let build = !args.iter().any(|a| a == "--no-build");
    let probe = !args.iter().any(|a| a == "--no-probe");
    let chunks: i64 = option("--chunks").map_or(0, |s| s.parse().expect("--chunks <n>"));

    for preset in TerrainPreset::all() {
        if only.as_deref().is_some_and(|key| key != preset.key) {
            continue;
        }
        println!("== {} (seed {seed})", preset.key);
        let options = preset.terrain.clone();
        let source = move |_: RegionId, _: DVec2, _: &Elevation| options.clone();

        // Stage times, summed over regions.
        let stages: Arc<Mutex<BTreeMap<String, (Duration, usize)>>> = Arc::default();
        let hook_stages = stages.clone();
        let world = WorldTerrain::new(
            WorldTerrainOptions {
                seed,
                ..WorldTerrainOptions::default()
            },
            Arc::new(source),
        )
        .expect("the default world options are valid")
        .with_progress(Arc::new(move |event: &RegionProgress| {
            if let RegionProgress::Finished { stage, elapsed, .. } = *event {
                let name = match stage {
                    RegionStage::Plan => "plan".to_string(),
                    RegionStage::Tier(tier) => format!("{tier:?}"),
                };
                let mut stages = hook_stages.lock().unwrap();
                let entry = stages.entry(name).or_default();
                entry.0 += elapsed;
                entry.1 += 1;
            }
        }));

        // The cold build: the region at the origin and the regions round it.
        let centre = world.region_at(DVec2::ZERO);
        let started = Instant::now();
        let mut ids = vec![centre];
        if build {
            let region = world.region(centre);
            for edge in &region.edges {
                if let Some(n) = edge.neighbor
                    && !ids.contains(&n)
                {
                    ids.push(n);
                }
            }
        }
        for &id in &ids {
            world.terrain(id).expect("the region builds");
        }
        println!(
            "cold build of {} regions at Fine: {:.2} s",
            ids.len(),
            started.elapsed().as_secs_f64()
        );
        for (name, (time, count)) in stages.lock().unwrap().iter() {
            println!(
                "  {name:<6} {count:>3} × {:>8.1} ms = {:>7.2} s",
                time.as_secs_f64() * 1e3 / *count as f64,
                time.as_secs_f64()
            );
        }

        // Every built region's rivers and ponds, to compare builds bit for bit.
        let mut hash = Fnv::default();
        for &id in &ids {
            let terrain = world.terrain(id).expect("the region builds");
            for river in terrain.rivers() {
                for (p, level) in river.points.iter().zip(&river.levels) {
                    hash.add(p.x.to_bits());
                    hash.add(p.y.to_bits());
                    hash.add(level.to_bits().into());
                }
            }
            for pond in terrain.ponds() {
                hash.add(pond.centre.x.to_bits());
                hash.add(pond.centre.y.to_bits());
                hash.add(pond.level.to_bits().into());
            }
        }
        println!("  rivers and ponds fingerprint {:016x}", hash.0);

        let terrain = world.terrain(centre).expect("the region builds");
        if chunks > 0 {
            river_chunks(&world, &terrain, chunks);
        }
        if probe {
            per_point(&terrain, spacing, repeat);
        }
    }
}

/// A running FNV-1a hash of 64-bit words.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn add(&mut self, word: u64) {
        for byte in word.to_le_bytes() {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
}

/// Reads an `n` × `n` block of 64 m chunks round the middle of the
/// longest river of `terrain`, every metre at the fine tier, as chunks read
/// their ground, and prints the time.
fn river_chunks(world: &WorldTerrain, terrain: &Terrain, n: i64) {
    let Some(mid) = terrain
        .rivers()
        .iter()
        .max_by_key(|r| r.points.len())
        .map(|r| r.points[r.points.len() / 2])
    else {
        println!("  no river in the region at the origin");
        return;
    };
    let corner = ((mid / 64.0).floor() - DVec2::splat((n / 2) as f64)) * 64.0;
    let points: Vec<DVec2> = (0..=64 * n)
        .flat_map(|z| (0..=64 * n).map(move |x| corner + DVec2::new(x as f64, z as f64)))
        .collect();
    let started = Instant::now();
    let read = world.surfaces(&points, FeatureTier::Fine);
    let elapsed = started.elapsed().as_secs_f64();
    let mut hash = Fnv::default();
    for s in &read {
        hash.add(s.height.to_bits().into());
        hash.add(s.water.map_or(u64::MAX, |w| w.to_bits().into()));
    }
    println!(
        "  {n} × {n} chunks round a river ({} points): {elapsed:.2} s, fingerprint {:016x}",
        points.len(),
        hash.0
    );
}

/// Times height reads on a grid `spacing` metres apart over `terrain`'s
/// region, split by whether a river or pond reaches each point.
fn per_point(terrain: &Terrain, spacing: f64, repeat: usize) {
    let region = terrain.region();
    let Boundary::Polygon(polygon) = &region.boundary else {
        println!("  (unbounded region: no per-point probe)");
        return;
    };
    let (lo, hi) = polygon
        .iter()
        .fold((DVec2::INFINITY, DVec2::NEG_INFINITY), |(lo, hi), &p| {
            (lo.min(p), hi.max(p))
        });

    // The probe points, by whether water carves them.
    let (mut wet, mut dry) = (Vec::new(), Vec::new());
    let (nx, nz) = (
        ((hi.x - lo.x) / spacing) as i64,
        ((hi.y - lo.y) / spacing) as i64,
    );
    for k in 0..=nz {
        for i in 0..=nx {
            let p = lo + DVec2::new(i as f64, k as f64) * spacing;
            if !region.boundary.contains(p) {
                continue;
            }
            let carved = terrain
                .features_within(p, p)
                .iter()
                .any(|id| matches!(id.kind, FeatureKind::River | FeatureKind::Pond));
            if carved { &mut wet } else { &mut dry }.push(p);
        }
    }
    let total = wet.len() + dry.len();
    println!(
        "  probe: {total} points every {spacing} m; {:.1}% within a river or pond's reach; {} rivers, {} ponds, {} roads",
        100.0 * wet.len() as f64 / total as f64,
        terrain.rivers().len(),
        terrain.ponds().len(),
        terrain.roads().len(),
    );

    // The fastest pass of `read` over `points`, in µs per point.
    let time = |points: &[DVec2], read: &dyn Fn(DVec2) -> f32| -> f64 {
        let mut best = f64::INFINITY;
        for _ in 0..repeat {
            let started = Instant::now();
            let mut sum = 0.0_f32;
            for &p in points {
                sum += read(p);
            }
            std::hint::black_box(sum);
            best = best.min(started.elapsed().as_secs_f64() * 1e6 / points.len().max(1) as f64);
        }
        best
    };
    println!(
        "  {:<6} {:>10} {:>12} {:>10} {:>12} {:>10}",
        "class", "uncarved", "-roads", "height", "carve share", "gradient"
    );
    for (name, points) in [("wet", &wet), ("dry", &dry)] {
        let base = time(points, &|p| terrain.uncarved_height(p));
        let water = time(points, &|p| terrain.height_before_roads(p));
        let full = time(points, &|p| terrain.height(p));
        let gradient = time(points, &|p| terrain.sample(p).height);
        println!(
            "  {name:<6} {base:>8.2}µs {water:>10.2}µs {full:>8.2}µs {:>11.0}% {gradient:>8.2}µs",
            100.0 * (full - base).max(0.0) / full,
        );
    }
}
