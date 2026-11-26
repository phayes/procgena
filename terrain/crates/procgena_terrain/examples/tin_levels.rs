//! What a canonical TIN chunk would cost at a coarser chunk level: level `L`
//! covers 64 m × 2^L, samples its 65 × 65 lattice 1 m × 2^L apart, and
//! follows the ground within the tolerance × 2^L.
//!
//! ```bash
//! cargo run --release -p procgena_terrain --example tin_levels -- --level 3
//! ```
//!
//! Options: `--level <L>` (default 3), `--preset <key>` (default: every
//! shipped preset), `--seed <n>` (default 1), `--chunks <n>` an n × n block
//! (default 3), `--level-name <name>` the base tolerance (default
//! `general`).
//!
//! The level-`L` chunk is built as a level-0 chunk over the world shrunk by
//! 2^L in every direction, so the same builder samples the same points with
//! the scaled tolerance; heights are rounded to the centimetre of the
//! shrunk world, 2^L cm of the real one. Errors are measured back in real
//! metres against the ground at independent points (on a 257 × 257 grid a
//! quarter of a lattice step off the samples). Regions are built before
//! timing, and the fastest of 3 builds is reported.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use glam::DVec2;
use procgena_terrain::tin::{TinChunk, TinTolerance, TinTriangle, build_chunk};
use procgena_terrain::{
    ChunkKey, ChunkSource, Elevation, FeatureTier, RegionId, SurfacePoint, TerrainPreset,
    WorldTerrain, WorldTerrainOptions,
};

/// A world shrunk by `scale` in x, z and height, counting its reads.
struct Shrunk<'a> {
    world: &'a WorldTerrain,
    scale: f64,
    reads: AtomicUsize,
}

impl ChunkSource for Shrunk<'_> {
    fn surface(&self, p: DVec2, tier: FeatureTier) -> SurfacePoint {
        self.surfaces(&[p], tier)[0]
    }
    fn surfaces(&self, points: &[DVec2], tier: FeatureTier) -> Vec<SurfacePoint> {
        self.reads.fetch_add(points.len(), Ordering::Relaxed);
        let real: Vec<DVec2> = points.iter().map(|&p| p * self.scale).collect();
        self.world
            .surfaces(&real, tier)
            .into_iter()
            .map(|s| SurfacePoint {
                height: (f64::from(s.height) / self.scale) as f32,
                water: s.water.map(|w| (f64::from(w) / self.scale) as f32),
            })
            .collect()
    }
    fn tier(&self, tier: FeatureTier) -> FeatureTier {
        self.world.tier(tier)
    }
    fn tier_difference(&self, _: DVec2, _: DVec2, _: FeatureTier) -> f32 {
        0.0
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
    let level: u32 = option("--level").map_or(3, |s| s.parse().expect("--level <L>"));
    let only = option("--preset");
    let seed: u32 = option("--seed").map_or(1, |s| s.parse().expect("--seed <n>"));
    let n: i64 = option("--chunks").map_or(3, |s| s.parse().expect("--chunks <n>"));
    let tolerance = option("--level-name").map_or(TinTolerance::GENERAL, |name| {
        TinTolerance::named(&name).expect("--level-name dense|general|low_poly|very_low_poly")
    });
    let scale = f64::from(1u32 << level);
    println!(
        "level {level}: chunks {} m wide, samples {} m apart, open land within {} m; seed {seed}, {n} × {n} chunks",
        64.0 * scale,
        scale,
        tolerance.general.height * scale
    );
    println!(
        "{:<16} {:>6} {:>7} {:>9} {:>9} {:>20}",
        "preset", "tris", "reads", "build ms", "H* µs/rd", "error p50/p95/max m"
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
        .expect("valid world");
        let shrunk = Shrunk {
            world: &world,
            scale,
            reads: AtomicUsize::new(0),
        };
        let (mut tris, mut reads, mut ms, mut errors) = (0.0, 0.0, 0.0, Vec::new());
        let mut read_us = 0.0;
        for z in -n / 2..n - n / 2 {
            for x in -n / 2..n - n / 2 {
                let key = ChunkKey::new(x, z, 0);
                // Build the regions first, then time.
                let _ = build_chunk(&shrunk, key, &tolerance);
                shrunk.reads.store(0, Ordering::Relaxed);
                let mut fastest = f64::INFINITY;
                let mut mesh = None;
                for _ in 0..3 {
                    let started = Instant::now();
                    mesh = Some(build_chunk(&shrunk, key, &tolerance).expect("built"));
                    fastest = fastest.min(started.elapsed().as_secs_f64() * 1000.0);
                }
                let mesh = mesh.unwrap();
                let chunk_reads = shrunk.reads.swap(0, Ordering::Relaxed) as f64 / 3.0;
                // The cost of a read alone, from a batch of the same size.
                let probe: Vec<DVec2> = (0..chunk_reads as usize)
                    .map(|i| {
                        key.min_corner()
                            + DVec2::new((i % 64) as f64 + 0.5, (i / 64 % 64) as f64 + 0.5)
                    })
                    .collect();
                let started = Instant::now();
                let _ = shrunk.surfaces(&probe, FeatureTier::Fine);
                read_us += started.elapsed().as_secs_f64() * 1e6 / chunk_reads.max(1.0);
                tris += mesh.triangles.len() as f64;
                reads += chunk_reads;
                ms += fastest;
                errors.extend(errors_of(&mesh, &shrunk, key));
            }
        }
        let count = (n * n) as f64;
        errors.sort_by(f64::total_cmp);
        let at = |q: f64| errors[((errors.len() - 1) as f64 * q) as usize];
        println!(
            "{:<16} {:>6.0} {:>7.0} {:>9.2} {:>9.2} {:>20}",
            preset.key,
            tris / count,
            reads / count,
            ms / count,
            read_us / count,
            format!("{:.2} / {:.2} / {:.2}", at(0.5), at(0.95), at(1.0))
        );
    }
}

/// How far `mesh` (of shrunk chunk `key`) misses the real ground, in real
/// metres, at points a quarter of a lattice step off the samples.
fn errors_of(mesh: &TinChunk, shrunk: &Shrunk, key: ChunkKey) -> Vec<f64> {
    let points: Vec<DVec2> = (0..256 * 256)
        .map(|i| key.min_corner() + DVec2::new((i % 256) as f64, (i / 256) as f64) * 0.25 + 0.125)
        .collect();
    let truth = shrunk.surfaces(&points, FeatureTier::Fine);
    points
        .iter()
        .zip(truth)
        .map(|(p, t)| (mesh_height(mesh, *p) - f64::from(t.height)).abs() * shrunk.scale)
        .collect()
}

/// The height of `mesh` at `p` (shrunk metres).
fn mesh_height(mesh: &TinChunk, p: DVec2) -> f64 {
    let q = (p.x * 100.0, p.y * 100.0);
    mesh.triangles
        .iter()
        .find_map(|&TinTriangle([a, b, c])| {
            let [a, b, c] = [a, b, c].map(|i| mesh.vertices[i as usize]);
            let pt = |v: procgena_terrain::tin::TinVertex| (v.x.0 as f64, v.z.0 as f64);
            let (pa, pb, pc) = (pt(a), pt(b), pt(c));
            let orient = |r: (f64, f64), s: (f64, f64), t: (f64, f64)| {
                -((s.0 - r.0) * (t.1 - r.1) - (s.1 - r.1) * (t.0 - r.0))
            };
            let (wa, wb, wc) = (orient(pb, pc, q), orient(pc, pa, q), orient(pa, pb, q));
            (wa >= 0.0 && wb >= 0.0 && wc >= 0.0).then(|| {
                let area = orient(pa, pb, pc);
                (wa * f64::from(a.h.0) + wb * f64::from(b.h.0) + wc * f64::from(c.h.0))
                    / area
                    / 100.0
            })
        })
        .expect("the mesh covers its chunk")
}
