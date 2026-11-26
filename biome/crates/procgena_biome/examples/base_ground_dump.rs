//! Writes top-down images of empty Biomes' base ground beside bare
//! terrain's look, to check by eye that mud follows river banks and gravel
//! lines roads.
//!
//! ```bash
//! cargo run --release -p procgena_biome --example base_ground_dump -- target/base_ground_dump
//! ```
//!
//! It builds the plains Biome west of the rolling hills Biome, two 3 km
//! square regions, so roads cross their border. For the middle of the first
//! river and the first road it finds, it writes `river_base.ppm` and
//! `road_base.ppm` (the Biome's base ground) and `river_bare.ppm` and
//! `road_bare.ppm` (bare terrain's look): binary PPM images of 3 × 3
//! level 0 chunks (192 m square), lightly shaded by slope.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use glam::DVec2;
use procgena_biome::{BiomePreset, biome_seed};
use procgena_terrain::ground::{GROUND_TEXELS, GroundStack, GroundWeights, MaterialCatalogue};
use procgena_terrain::{
    ChunkKey, Convergence, Elevation, FeatureTier, RegionId, RegionSpec, StitchedWorld, Terrain,
};

/// Chunks along each side of an image.
const CHUNKS: i64 = 3;

/// The chunk level drawn.
const LEVEL: u8 = 0;

/// Side of each square region, in metres.
const SIDE: f64 = 3000.0;

/// The World seed.
const SEED: u32 = 0;

fn main() -> std::io::Result<()> {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/base_ground_dump".into()),
    );
    fs::create_dir_all(&dir)?;

    // Plains west of rolling hills.
    let pairs = [
        (RegionId(1), DVec2::new(-SIDE, 0.0), BiomePreset::PLAINS),
        (RegionId(2), DVec2::ZERO, BiomePreset::ROLLING_HILLS),
    ];
    let specs = pairs.clone().map(|(id, origin, preset)| RegionSpec {
        id,
        polygon: [(0.0, 0.0), (SIDE, 0.0), (SIDE, SIDE), (0.0, SIDE)]
            .map(|(x, z)| origin + DVec2::new(x, z))
            .to_vec(),
        options: preset
            .biome
            .terrain_options(SEED)
            .expect("a built-in Terrain"),
    });
    let world = StitchedWorld::build(specs.to_vec(), &Convergence::default(), &Elevation::flat())
        .expect("the world builds");
    let catalogue = Arc::new(MaterialCatalogue::standard());
    let bare = GroundStack::bare(SEED, 1.0);

    // The middle of the first river and the first road, with their region.
    let mut middles: [Option<(DVec2, &Terrain)>; 2] = [None, None];
    for terrain in world.terrains() {
        let middle = |points: &[DVec2]| (points[points.len() / 2], terrain);
        middles[0] = middles[0].or(terrain.rivers().first().map(|r| middle(&r.points)));
        middles[1] = middles[1].or(terrain.roads().first().map(|r| middle(&r.points)));
    }

    // Each painted with its region's base ground and with the bare look.
    for (name, middle) in ["river", "road"].into_iter().zip(middles) {
        let Some((middle, terrain)) = middle else {
            println!("no {name} in the world");
            continue;
        };
        let id = terrain.region().id;
        let (_, _, preset) = pairs.iter().find(|(i, ..)| *i == id).expect("a region");
        let base = preset
            .biome
            .ground(catalogue.clone(), biome_seed(SEED, id), 1.0)
            .expect("the base ground is valid");
        for (look, stack) in [("base", &base), ("bare", &bare)] {
            let path = dir.join(format!("{name}_{look}.ppm"));
            write_image(&path, terrain, stack, ChunkKey::containing(middle, LEVEL))?;
            println!("wrote {} ({})", path.display(), preset.key);
        }
    }
    Ok(())
}

/// Writes the chunks around `centre`, painted with `stack`, to `path`.
fn write_image(
    path: &Path,
    terrain: &Terrain,
    stack: &GroundStack,
    centre: ChunkKey,
) -> std::io::Result<()> {
    // Each chunk's colours, its last row and column left to its neighbour.
    let side = (GROUND_TEXELS - 1) as usize;
    let width = side * CHUNKS as usize;
    let mut image = vec![[0.0_f32; 3]; width * width];
    for cz in 0..CHUNKS {
        for cx in 0..CHUNKS {
            let key = centre.offset(cx - CHUNKS / 2, cz - CHUNKS / 2);
            let weights = GroundWeights::chunk(stack, terrain, key, FeatureTier::Fine)
                .expect("the region has ground there");
            let colours = weights.colours(stack.catalogue());
            for j in 0..side {
                for i in 0..side {
                    let x = cx as usize * side + i;
                    // Rows run from +z at the top to −z at the bottom.
                    let y = width - 1 - (cz as usize * side + j);
                    image[y * width + x] = colours[j * GROUND_TEXELS as usize + i];
                }
            }
        }
    }

    // Light it as if from the north-west, from the terrain's slope.
    let step = ChunkKey::size_at(LEVEL) / f64::from(GROUND_TEXELS - 1);
    let origin = centre.offset(-CHUNKS / 2, -CHUNKS / 2).min_corner();
    let mut bytes = format!("P6\n{width} {width}\n255\n").into_bytes();
    for y in 0..width {
        for x in 0..width {
            let p = origin + DVec2::new(x as f64, (width - 1 - y) as f64) * step;
            let g = terrain.gradient(p);
            let light = (1.0 + 0.8 * (g.x - g.y)).clamp(0.4, 1.4);
            for c in image[y * width + x] {
                let srgb = (c * light).clamp(0.0, 1.0).powf(1.0 / 2.2);
                bytes.push((srgb * 255.0).round() as u8);
            }
        }
    }
    fs::File::create(path)?.write_all(&bytes)
}
