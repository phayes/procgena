//! Writes a top-down image of bare terrain's ground materials, to check
//! the look by eye: grass, rock on steep ground, gravel roads, dirt road
//! banks and riverbeds.
//!
//! ```bash
//! cargo run --release -p procgena_terrain --example ground_dump -- target/ground_dump
//! ```
//!
//! It writes `bare.ppm` (a binary PPM image): a 3 × 3 block of level 1
//! chunks (384 m square) around the middle of a road on the plains of the
//! test world, lightly shaded by slope, built with `ChunkWeights`.

#[path = "../tests/common/mod.rs"]
mod common;

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use procgena_terrain::ground::{ChunkWeights, GROUND_TEXELS, GroundStack};
use procgena_terrain::{ChunkKey, ChunkStage};

/// Chunks along each side of the image.
const CHUNKS: i64 = 3;

fn main() -> std::io::Result<()> {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/ground_dump".into()),
    );
    fs::create_dir_all(&dir)?;

    // The plains of the test world, with rivers and roads.
    let world = common::hills_into_plains(common::RIVER_SEEDS[0], common::DEFAULT);
    let plains = &world.terrains()[0];
    let road = &plains.roads()[0].points;
    let centre = ChunkKey::containing(road[road.len() / 2], 1);
    let bare = GroundStack::bare(1, 1.0);

    // Each chunk's colours, its last row and column left to its neighbour.
    let side = (GROUND_TEXELS - 1) as usize;
    let width = side * CHUNKS as usize;
    let mut image = vec![[0.0_f32; 3]; width * width];
    for cz in 0..CHUNKS {
        for cx in 0..CHUNKS {
            let key = centre.offset(cx - CHUNKS / 2, cz - CHUNKS / 2);
            let weights = ChunkWeights::build(&bare, plains, key, ChunkStage::Canonical)
                .expect("the plains have ground there");
            let colours = weights.colours(bare.catalogue());
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
    let step = ChunkKey::size_at(1) / f64::from(GROUND_TEXELS - 1);
    let origin = centre.offset(-CHUNKS / 2, -CHUNKS / 2).min_corner();
    let mut bytes = format!("P6\n{width} {width}\n255\n").into_bytes();
    for y in 0..width {
        for x in 0..width {
            let p = origin + glam::DVec2::new(x as f64, (width - 1 - y) as f64) * step;
            let g = plains.gradient(p);
            let light = (1.0 + 0.8 * (g.x - g.y)).clamp(0.4, 1.4);
            for c in image[y * width + x] {
                let srgb = (c * light).clamp(0.0, 1.0).powf(1.0 / 2.2);
                bytes.push((srgb * 255.0).round() as u8);
            }
        }
    }
    let path = dir.join("bare.ppm");
    fs::File::create(&path)?.write_all(&bytes)?;
    println!("wrote {}", path.display());
    Ok(())
}
