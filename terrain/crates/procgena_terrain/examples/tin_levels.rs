//! What a canonical TIN chunk costs at each chunk level: level `L` covers
//! 64 m × 2^L, samples its 65 × 65 lattice 2^L metres apart, and follows
//! the ground within the world's tolerance scaled to its level.
//!
//! ```bash
//! cargo run --release -p procgena_terrain --example tin_levels
//! ```
//!
//! Options: `--levels <a>-<b>` (default `0-6`), `--preset <key>` (default:
//! every shipped preset), `--seed <n>` (default 1), `--chunks <n>` an n × n
//! block round the origin (default 3), `--level-name <name>` the base
//! tolerance (default `general`).
//!
//! Each chunk is built for real at its level from the shipped preset on the
//! default world, with a fresh contract cache, so its four edge contracts
//! (and their border normals) are built with it. Regions are built before
//! timing, and the fastest of 3 builds is reported, with the heights read
//! from the ground (`H*`) per build. Errors are measured in metres against
//! the ground at 256 × 256 points a quarter of a lattice step off the
//! samples. Runs on one thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use glam::DVec2;
use procgena_terrain::tin::{ContractCache, TinIndex, TinTolerance, build_chunk_with};
use procgena_terrain::{
    ChunkKey, ChunkSource, Elevation, FeatureLevel, RegionId, SurfacePoint, TerrainPreset,
    WorldTerrain, WorldTerrainOptions,
};

/// A world, counting the heights read from it.
struct Counted<'a> {
    world: &'a WorldTerrain,
    reads: AtomicUsize,
}

impl ChunkSource for Counted<'_> {
    fn surface(&self, p: DVec2, level: FeatureLevel) -> SurfacePoint {
        self.surfaces(&[p], level)[0]
    }
    fn surfaces(&self, points: &[DVec2], level: FeatureLevel) -> Vec<SurfacePoint> {
        self.reads.fetch_add(points.len(), Ordering::Relaxed);
        self.world.surfaces(points, level)
    }
    fn level(&self, level: FeatureLevel) -> FeatureLevel {
        self.world.level(level)
    }
    fn level_difference(&self, min: DVec2, max: DVec2, from: FeatureLevel) -> f32 {
        self.world.level_difference(min, max, from)
    }
    fn lines_within(
        &self,
        min: DVec2,
        max: DVec2,
        level: FeatureLevel,
    ) -> Vec<procgena_terrain::ChunkFeature> {
        self.world.lines_within(min, max, level)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let option = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let (first, last): (u8, u8) = option("--levels").map_or((0, 6), |s| {
        let (a, b) = s.split_once('-').expect("--levels <a>-<b>");
        (a.parse().expect("a level"), b.parse().expect("a level"))
    });
    let only = option("--preset");
    let seed: u32 = option("--seed").map_or(1, |s| s.parse().expect("--seed <n>"));
    let n: i64 = option("--chunks").map_or(3, |s| s.parse().expect("--chunks <n>"));
    let tolerance = option("--level-name").map_or(TinTolerance::GENERAL, |name| {
        TinTolerance::named(&name).expect("--level-name dense|general|low_poly|very_low_poly")
    });
    println!("seed {seed}, {n} × {n} chunks round the origin, fastest of 3, one thread");
    println!(
        "{:>5} {:<14} {:>6} {:>7} {:>9} {:>22}",
        "level", "preset", "tris", "reads", "build ms", "error p50/p95/max m"
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
                tin: tolerance,
                ..WorldTerrainOptions::default()
            },
            Arc::new(source),
        )
        .expect("valid world");
        let counted = Counted {
            world: &world,
            reads: AtomicUsize::new(0),
        };
        for level in first..=last {
            let (mut tris, mut reads, mut ms, mut errors) = (0.0, 0.0, 0.0, Vec::new());
            for z in -n / 2..n - n / 2 {
                for x in -n / 2..n - n / 2 {
                    let key = ChunkKey::new(x, z, level);
                    // Build the regions first, then time.
                    let build = || {
                        let cache = ContractCache::new(&tolerance);
                        build_chunk_with(&counted, key, &tolerance, &cache).expect("built")
                    };
                    let _ = build();
                    counted.reads.store(0, Ordering::Relaxed);
                    let mut fastest = f64::INFINITY;
                    let mut mesh = None;
                    for _ in 0..3 {
                        let started = Instant::now();
                        mesh = Some(build());
                        fastest = fastest.min(started.elapsed().as_secs_f64() * 1000.0);
                    }
                    let mesh = mesh.unwrap();
                    reads += counted.reads.swap(0, Ordering::Relaxed) as f64 / 3.0;
                    tris += mesh.triangles.len() as f64;
                    ms += fastest;
                    errors.extend(errors_of(TinIndex::new(Arc::new(mesh)), &world, key));
                }
            }
            let count = (n * n) as f64;
            errors.sort_by(f64::total_cmp);
            let at = |q: f64| errors[((errors.len() - 1) as f64 * q) as usize];
            println!(
                "{:>5} {:<14} {:>6.0} {:>7.0} {:>9.2} {:>22}",
                level,
                preset.key,
                tris / count,
                reads / count,
                ms / count,
                format!("{:.2} / {:.2} / {:.2}", at(0.5), at(0.95), at(1.0))
            );
        }
    }
}

/// How far the mesh of chunk `key` misses the ground of `world`, in metres,
/// at 256 × 256 points a quarter of a lattice step off the samples.
fn errors_of(index: TinIndex, world: &WorldTerrain, key: ChunkKey) -> Vec<f64> {
    let step = key.size() / 64.0;
    let points: Vec<DVec2> = (0..256 * 256)
        .map(|i| {
            key.min_corner() + (DVec2::new((i % 256) as f64, (i / 256) as f64) + 0.5) * step / 4.0
        })
        .collect();
    let truth = world.surfaces(&points, FeatureLevel::FINEST);
    points
        .iter()
        .zip(truth)
        .map(|(&p, t)| {
            let mesh = index.sample(p).expect("the mesh covers its chunk").height;
            (mesh - f64::from(t.height)).abs()
        })
        .collect()
}
