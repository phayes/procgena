//! Measures what bare terrain's ground weights cost per chunk, beside the
//! chunk's own samples, and how much the colours step where a chunk meets
//! a chunk one level coarser.
//!
//! ```bash
//! cargo run -p procgena_terrain --example ground_cost
//! ```
//!
//! Options: `--seed <n>` (default 1), `--preset <key>` (default
//! `rolling_hills`), `--chunks <n>` chunks timed per level (default 8).
//!
//! It builds a world of one terrain preset, reads every timed chunk once
//! first so region builds don't count, then prints for each chunk level
//! the median time to sample a chunk ([`ChunkSamples::sample`]) and to
//! paint its weights ([`GroundWeights::chunk`]), both at the fine tier.
//!
//! For the seam, it paints pairs of side-by-side chunks one level apart.
//! Every second texel along the finer chunk's edge lies on a texel of the
//! coarser one; the texels between are drawn by the coarser chunk as the
//! mean of its two texels on either side. It prints the largest and the
//! mean colour difference there, as linear RGB distance.

use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::{DVec2, Vec3};
use procgena_terrain::ground::{GROUND_TEXELS, GroundStack, GroundWeights, MaterialCatalogue};
use procgena_terrain::{
    ChunkKey, ChunkSamples, Elevation, FeatureTier, RegionId, TerrainPreset, WorldTerrain,
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
    let seed: u32 = option("--seed").map_or(1, |s| s.parse().expect("--seed <n>"));
    let preset = option("--preset").unwrap_or_else(|| "rolling_hills".into());
    let chunks: i64 = option("--chunks").map_or(8, |s| s.parse().expect("--chunks <n>"));
    let options = TerrainPreset::all()
        .find(|p| p.key == preset)
        .unwrap_or_else(|| panic!("no terrain preset {preset}"))
        .terrain
        .clone();

    // The world, and bare terrain's look over it.
    let source = move |_: RegionId, _: DVec2, _: &Elevation| options.clone();
    let world = WorldTerrain::new(
        WorldTerrainOptions {
            seed,
            ..WorldTerrainOptions::default()
        },
        Arc::new(source),
    )
    .expect("the default world options are valid");
    let bare = GroundStack::bare(seed, 1.0);
    let tier = FeatureTier::Fine;
    println!(
        "preset {preset}, seed {seed}, {chunks} chunks per level, rayon {}",
        cfg!(feature = "rayon")
    );

    // Time each level: a row of chunks along x from the origin.
    println!("level  chunk m  texel m  samples ms  weights ms  ratio");
    for level in 0..6u8 {
        let keys: Vec<ChunkKey> = (0..chunks).map(|x| ChunkKey::new(x, 0, level)).collect();
        for &key in &keys {
            ChunkSamples::sample(&world, key, tier).expect("the world has ground");
        }
        let mut samples = Vec::new();
        let mut weights = Vec::new();
        for &key in &keys {
            let start = Instant::now();
            ChunkSamples::sample(&world, key, tier).expect("the world has ground");
            samples.push(start.elapsed());
            let start = Instant::now();
            GroundWeights::chunk(&bare, &world, key, tier).expect("the world has ground");
            weights.push(start.elapsed());
        }
        let (s, w) = (median(&mut samples), median(&mut weights));
        let size = ChunkKey::size_at(level);
        println!(
            "{level:>5}  {size:>7}  {:>7}  {:>10.1}  {:>10.1}  {:>5.1}",
            size as f64 / f64::from(GROUND_TEXELS - 1),
            ms(s),
            ms(w),
            w.as_secs_f64() / s.as_secs_f64()
        );
    }

    // The seam: pairs of a fine chunk and the coarse chunk east of it,
    // which it meets along the coarse chunk's south half.
    println!("fine level  pairs  largest step  mean step  (linear RGB)");
    for fine in 0..4u8 {
        let mut largest = 0.0f32;
        let mut total = 0.0f32;
        let mut count = 0usize;
        let pairs = chunks.min(6);
        for z in 0..pairs {
            // A coarse chunk and the fine chunk west of its south half.
            let coarse = ChunkKey::new(1, z, fine + 1);
            let west = ChunkKey::new(1, 2 * z, fine);
            let c = GroundWeights::chunk(&bare, &world, coarse, tier).expect("ground");
            let f = GroundWeights::chunk(&bare, &world, west, tier).expect("ground");
            let fc = colours(&f, bare.catalogue());
            let cc = colours(&c, bare.catalogue());
            let n = GROUND_TEXELS as usize;
            // The fine chunk's east edge, against the coarse chunk's west
            // edge from its first texel.
            for j in (1..n - 1).step_by(2) {
                let fine_colour = fc[j * n + (n - 1)];
                let (a, b) = (cc[(j / 2) * n], cc[(j / 2 + 1) * n]);
                let step = (fine_colour - (a + b) * 0.5).length();
                largest = largest.max(step);
                total += step;
                count += 1;
            }
            // Coinciding texels must agree.
            for j in (0..n).step_by(2) {
                let step = (fc[j * n + (n - 1)] - cc[(j / 2) * n]).length();
                assert!(step < 0.02, "texels on the same point differ by {step}");
            }
        }
        println!(
            "{fine:>10}  {pairs:>5}  {largest:>12.3}  {:>9.4}",
            total / count as f32
        );
    }
}

/// Each texel's colour, row by row along x.
fn colours(weights: &GroundWeights, catalogue: &MaterialCatalogue) -> Vec<Vec3> {
    weights
        .colours(catalogue)
        .into_iter()
        .map(Vec3::from)
        .collect()
}

/// The median of `times`.
fn median(times: &mut [Duration]) -> Duration {
    times.sort();
    times[times.len() / 2]
}

/// `d` in milliseconds.
fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}
