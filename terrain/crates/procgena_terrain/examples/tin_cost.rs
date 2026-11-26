//! Measures canonical chunk meshes against the regular 1 m chunk mesh:
//! triangles per chunk, how far each misses the ground between samples,
//! and what building one costs, on the shipped presets and the default
//! world.
//!
//! ```bash
//! cargo run --release -p procgena_terrain --example tin_cost
//! ```
//!
//! Options: `--preset <key>` (default: every shipped preset), `--seed <n>`
//! world seed (default 1), `--chunks <n>` an n × n block of chunks round
//! the origin (default 4).
//!
//! The error is measured at points between the 1 m samples (every half
//! metre, a quarter metre off the samples), where neither mesh was fitted.
//! Times are per chunk, on a world whose regions are already built:
//! sampling is the 65 × 65 samples of one chunk, building is the whole
//! canonical mesh, sampling included.

use std::sync::Arc;
use std::time::Instant;

use glam::DVec2;
use procgena_terrain::tin::{
    Mm, ProbeAddr, TinChunk, TinTolerance, TinTriangle, build_chunk, sample_heights,
};
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
    let tolerance = TinTolerance::GENERAL;
    println!(
        "seed {seed}, {n} × {n} chunks, tolerance {tolerance:?}, rayon {}",
        cfg!(feature = "rayon")
    );
    println!(
        "{:<16} {:>9} {:>9} {:>7} {:>27} {:>27} {:>9} {:>9}",
        "preset",
        "triangles",
        "vertices",
        "unmet",
        "TIN error p50/p95/max mm",
        "grid error p50/p95/max mm",
        "sample ms",
        "build ms"
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

        let (mut triangles, mut vertices, mut unmet) = (Vec::new(), Vec::new(), 0);
        let (mut tin_errors, mut grid_errors) = (Vec::new(), Vec::new());
        let (mut sample_ms, mut build_ms) = (Vec::new(), Vec::new());
        for z in -n / 2..n - n / 2 {
            for x in -n / 2..n - n / 2 {
                let key = ChunkKey::new(x, z, 0);
                let lattice = lattice(key);
                // Build the regions first, so only the chunk is timed.
                if sample_heights(&world, &lattice).is_err() {
                    println!("{:<16} chunk ({x}, {z}) has no ground", preset.key);
                    continue;
                }
                let started = Instant::now();
                let heights = sample_heights(&world, &lattice).expect("sampled once already");
                sample_ms.push(started.elapsed().as_secs_f64() * 1000.0);
                let started = Instant::now();
                let mesh = build_chunk(&world, key, &tolerance).expect("the ground is there");
                build_ms.push(started.elapsed().as_secs_f64() * 1000.0);
                triangles.push(mesh.triangles.len() as f64);
                vertices.push(mesh.vertices.len() as f64);
                unmet += mesh.unmet.len();

                // Errors between the samples, for both meshes.
                let (points, truth) = between(&world, key);
                let grid: Vec<f64> = heights.iter().map(|h| f64::from(h.0)).collect();
                for (&p, &t) in points.iter().zip(&truth) {
                    tin_errors.push((tin_height(&mesh, p) - t).abs());
                    grid_errors.push((grid_height(&grid, key, p) - t).abs());
                }
            }
        }
        if triangles.is_empty() {
            continue;
        }
        let spread = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            let at = |q: f64| v[((v.len() - 1) as f64 * q) as usize];
            format!("{:.0} / {:.0} / {:.0}", at(0.5), at(0.95), at(1.0))
        };
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        println!(
            "{:<16} {:>9.0} {:>9.0} {:>7} {:>27} {:>27} {:>9.2} {:>9.2}",
            preset.key,
            mean(&triangles),
            mean(&vertices),
            unmet,
            spread(&mut tin_errors),
            spread(&mut grid_errors),
            mean(&sample_ms),
            mean(&build_ms),
        );
    }
    println!("(the regular 1 m chunk mesh has 8192 triangles and 4225 vertices)");
}

/// The 65 × 65 samples of chunk `key`, a metre apart.
fn lattice(key: ChunkKey) -> Vec<ProbeAddr> {
    (0..65 * 65)
        .map(|i| {
            let (col, row) = (i % 65, i / 65);
            ProbeAddr::new(
                Mm(key.x * 64_000 + col * 1000),
                Mm(key.z * 64_000 + row * 1000),
            )
        })
        .collect()
}

/// Points between the samples of chunk `key` (millimetres) and the ground
/// there (millimetres, unrounded).
fn between(world: &WorldTerrain, key: ChunkKey) -> (Vec<(i64, i64)>, Vec<f64>) {
    let points: Vec<(i64, i64)> = (0..128 * 128)
        .map(|i| {
            let (col, row) = (i % 128, i / 128);
            (
                key.x * 64_000 + 250 + col * 500,
                key.z * 64_000 + 250 + row * 500,
            )
        })
        .collect();
    let at: Vec<DVec2> = points
        .iter()
        .map(|&(x, z)| DVec2::new(x as f64, z as f64) / 1000.0)
        .collect();
    let truth = world
        .surfaces(&at, FeatureTier::Fine)
        .iter()
        .map(|s| f64::from(s.height) * 1000.0)
        .collect();
    (points, truth)
}

/// The height of `mesh` at `p` (millimetres).
fn tin_height(mesh: &TinChunk, p: (i64, i64)) -> f64 {
    mesh.triangles
        .iter()
        .find_map(|&TinTriangle([a, b, c])| {
            let [a, b, c] = [a, b, c].map(|i| mesh.vertices[i as usize]);
            let orient = |q: (i64, i64), r: (i64, i64), s: (i64, i64)| {
                -(((r.0 - q.0) * (s.1 - q.1) - (r.1 - q.1) * (s.0 - q.0)) as f64)
            };
            let (pa, pb, pc) = ((a.x.0, a.z.0), (b.x.0, b.z.0), (c.x.0, c.z.0));
            let (wa, wb, wc) = (orient(pb, pc, p), orient(pc, pa, p), orient(pa, pb, p));
            (wa >= 0.0 && wb >= 0.0 && wc >= 0.0).then(|| {
                (wa * f64::from(a.h.0) + wb * f64::from(b.h.0) + wc * f64::from(c.h.0))
                    / orient(pa, pb, pc)
            })
        })
        .expect("the mesh covers its chunk")
}

/// The height at `p` (millimetres) of the regular 1 m mesh through
/// `heights` (65 × 65, row by row), each square split corner to corner.
fn grid_height(heights: &[f64], key: ChunkKey, p: (i64, i64)) -> f64 {
    let (fx, fz) = (
        (p.0 - key.x * 64_000) as f64 / 1000.0,
        (p.1 - key.z * 64_000) as f64 / 1000.0,
    );
    let (col, row) = (fx.floor().min(63.0) as usize, fz.floor().min(63.0) as usize);
    let (u, v) = (fx - col as f64, fz - row as f64);
    let h = |c: usize, r: usize| heights[r * 65 + c];
    if u >= v {
        h(col, row)
            + u * (h(col + 1, row) - h(col, row))
            + v * (h(col + 1, row + 1) - h(col + 1, row))
    } else {
        h(col, row)
            + v * (h(col, row + 1) - h(col, row))
            + u * (h(col + 1, row + 1) - h(col, row + 1))
    }
}
