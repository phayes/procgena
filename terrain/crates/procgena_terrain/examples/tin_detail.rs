//! Calibrates coarse-to-fine sampling for canonical chunk meshes. On the
//! full 1 m lattice, for each cell of the chunk's dyadic cells, it
//! compares what the cell's samples show with what they hide:
//!
//! - the cell's *bend*: how far its middle and side midpoints stray from
//!   the flat (bilinear) surface through its corners;
//! - its *estimate* of the ground it hides: a quarter of the larger of its
//!   own bend and a quarter of its parent's (the cell twice as wide
//!   holding it), which is what smooth ground would hide between the
//!   cell's nine samples;
//! - what it really hides: the worst any lattice point inside strays from
//!   the bilinear surfaces through those nine samples.
//!
//! ```bash
//! cargo run --release -p procgena_terrain --example tin_detail
//! ```
//!
//! Options: `--fixtures` (the medium-scale fixtures on their world,
//! rivers on), `--seeds <n>` (default 3), `--chunks <n>` (default 4).

use std::sync::Arc;

use glam::DVec2;
use procgena_terrain::tin::{Cm, ProbeAddr, sample_heights};
use procgena_terrain::{
    ChunkKey, Elevation, RegionId, TerrainPreset, WorldTerrain, WorldTerrainOptions, fixtures,
};

/// Lattice points along a chunk's side.
const SIDE: usize = 65;

/// The upper ends of the estimate's buckets, in millimetres.
const BUCKETS: [f64; 5] = [12.5, 25.0, 50.0, 100.0, 200.0];

fn main() {
    // Options.
    let args: Vec<String> = std::env::args().collect();
    let option = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let use_fixtures = args.iter().any(|a| a == "--fixtures");
    let seeds: u32 = option("--seeds").map_or(3, |s| s.parse().expect("--seeds <n>"));
    let n: i64 = option("--chunks").map_or(4, |s| s.parse().expect("--chunks <n>"));
    println!(
        "{seeds} seeds, {n} × {n} chunks; per estimate bucket (mm): cells, then the worst hidden"
    );
    println!(
        "{:<22} {:>5} {}",
        "preset",
        "size",
        BUCKETS.map(|b| format!("{:>16}", format!("≤{b}"))).join("")
    );

    let presets: Vec<&TerrainPreset> = if use_fixtures {
        fixtures::all().collect()
    } else {
        TerrainPreset::all().collect()
    };
    for preset in presets {
        // Per cell size, per bucket: how many cells, and the worst hidden.
        let sizes = [4usize, 8, 16, 32];
        let mut found = [[(0usize, 0.0f64); BUCKETS.len()]; 4];
        for seed in 1..=seeds {
            let options = preset.terrain.clone();
            let source = move |_: RegionId, _: DVec2, _: &Elevation| options.clone();
            let world_options = if use_fixtures {
                fixtures::world_medium()
            } else {
                WorldTerrainOptions::default()
            };
            let world = WorldTerrain::new(
                WorldTerrainOptions {
                    seed,
                    ..world_options
                },
                Arc::new(source),
            )
            .expect("the world options are valid");
            for z in -n / 2..n - n / 2 {
                for x in -n / 2..n - n / 2 {
                    let key = ChunkKey::new(x, z, 0);
                    let probes: Vec<ProbeAddr> = (0..SIDE * SIDE)
                        .map(|i| {
                            ProbeAddr::new(
                                Cm(key.x * 6_400 + (i % SIDE) as i64 * 100),
                                Cm(key.z * 6_400 + (i / SIDE) as i64 * 100),
                            )
                        })
                        .collect();
                    let Ok(heights) = sample_heights(&world, &probes) else {
                        continue;
                    };
                    // In millimetres (to the centimetre).
                    let h: Vec<f64> = heights.iter().map(|h| f64::from(h.0) * 10.0).collect();
                    for (k, &size) in sizes.iter().enumerate() {
                        measure(&h, size, &mut found[k]);
                    }
                }
            }
        }
        for (k, size) in sizes.iter().enumerate() {
            println!(
                "{:<22} {:>5} {}",
                preset.key,
                size,
                found[k]
                    .map(|(count, worst)| format!("{:>16}", format!("{count} / {worst:.0}")))
                    .join("")
            );
        }
    }
}

/// Every cell `size` steps wide of the chunk's lattice heights `h`: adds
/// it to the first bucket of `found` its estimate fits in, keeping the
/// worst it hides.
fn measure(h: &[f64], size: usize, found: &mut [(usize, f64); BUCKETS.len()]) {
    let at = |c: usize, r: usize| h[r * SIDE + c];
    let parent = 2 * size;
    for r in (0..(SIDE - 1) / size).map(|k| k * size) {
        for c in (0..(SIDE - 1) / size).map(|k| k * size) {
            let (bend, hidden) = square(&at, c, r, size);
            let (pc, pr) = (c / parent * parent, r / parent * parent);
            let parent_bend = square(&at, pc, pr, parent).0;
            let estimate = bend.max(parent_bend / 4.0) / 4.0;
            if let Some(b) = BUCKETS.iter().position(|&b| estimate <= b) {
                found[b].0 += 1;
                found[b].1 = found[b].1.max(hidden);
            }
        }
    }
}

/// The bend of the cell `s` wide at column `c`, row `r`, and the worst
/// any lattice point inside strays from the bilinear surfaces through its
/// nine samples (its four quarters).
fn square(at: &dyn Fn(usize, usize) -> f64, c: usize, r: usize, s: usize) -> (f64, f64) {
    let bilinear = |c: usize, r: usize, s: usize, i: usize, j: usize| {
        let [a, b, d, e] = [at(c, r), at(c + s, r), at(c, r + s), at(c + s, r + s)];
        let (u, v) = (i as f64 / s as f64, j as f64 / s as f64);
        a * (1.0 - u) * (1.0 - v) + b * u * (1.0 - v) + d * (1.0 - u) * v + e * u * v
    };
    let half = s / 2;
    let bend = [(half, 0), (0, half), (half, half), (s, half), (half, s)]
        .map(|(i, j)| (at(c + i, r + j) - bilinear(c, r, s, i, j)).abs())
        .into_iter()
        .fold(0.0, f64::max);
    let mut hidden: f64 = 0.0;
    for (qi, qj) in [(0, 0), (half, 0), (0, half), (half, half)] {
        for j in 0..=half {
            for i in 0..=half {
                let off = at(c + qi + i, r + qj + j) - bilinear(c + qi, r + qj, half, i, j);
                hidden = hidden.max(off.abs());
            }
        }
    }
    (bend, hidden)
}
