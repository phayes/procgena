//! Writes a top-down image of the pine forest floor's ground layers, to
//! check by eye that needle litter lies under the pines and bare dirt
//! round the boulders.
//!
//! ```bash
//! cargo run -p procgena_biome --example ground_dump -- target/ground_dump
//! ```
//!
//! It writes `pine_forest_floor.ppm` (a binary PPM image): one 64 m chunk
//! of the Ecotope over gently rolling ground, painted with its ground
//! layers over bare terrain's look, at 8 pixels a metre. Each pine trunk
//! is marked with a dark green dot, and each stone is drawn as a grey disc
//! of its own size.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use glam::{DVec2, Vec2, Vec3Swizzles};
use procgena_biome::{EcotopePreset, ScatterCopies};
use procgena_core::HeightGrid;
use procgena_core::scatter::{ScatterArea, ScatterInput};
use procgena_terrain::ground::{GROUND_TEXELS, GroundStack, GroundWeights, OnGround};
use procgena_terrain::{ChunkKey, FeatureTier};

/// Pixels per texel (a texel is half a metre).
const ZOOM: usize = 4;

fn main() -> std::io::Result<()> {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/ground_dump".into()),
    );
    fs::create_dir_all(&dir)?;

    // Place the pine forest floor over chunk (0, 0).
    let key = ChunkKey::new(0, 0, 0);
    let centre = DVec2::splat(32.0);
    let grid = HeightGrid::from_fn(DVec2::splat(-8.0), DVec2::splat(72.0), 0.5, |p| {
        let (x, z) = (p.x as f32, p.y as f32);
        0.6 * (0.21 * x).sin() + 0.4 * (0.17 * z + 0.1 * x).cos()
    });
    let preset = EcotopePreset::pine_forest_floor();
    let ecotope = &preset.ecotope;
    let area = ScatterArea::new(centre, Vec2::splat(64.0));
    let placement = ecotope
        .place(&ScatterInput::new(&grid, area, ecotope.default_seed))
        .expect("the pine forest floor places");

    // Paint the chunk.
    let bare = GroundStack::bare(1, 1.0);
    let painter = ecotope
        .ground(&bare, &placement)
        .expect("the pine floor's ground fits the standard catalogue");
    let weights = GroundWeights::chunk(&painter, &OnGround(&grid), key, FeatureTier::Fine)
        .expect("the grid covers the chunk");
    let colours = weights.colours(bare.catalogue());

    // Scale the texels up, with +z at the top.
    let n = GROUND_TEXELS as usize;
    let width = n * ZOOM;
    let mut image = vec![[0.0_f32; 3]; width * width];
    for y in 0..width {
        for x in 0..width {
            let (i, j) = (x / ZOOM, n - 1 - y / ZOOM);
            image[y * width + x] = colours[j * n + i];
        }
    }

    // Mark the pines and stones.
    let per_metre = (ZOOM * 2) as f64;
    let mut disc = |at: DVec2, radius: f64, colour: [f32; 3]| {
        let local = (at - key.min_corner()) * per_metre;
        let r = radius * per_metre;
        for y in 0..width {
            for x in 0..width {
                let px = DVec2::new(x as f64 + 0.5, (width - y) as f64 - 0.5);
                if px.distance(local) <= r {
                    image[y * width + x] = colour;
                }
            }
        }
    };
    for layer in placement.layers() {
        match layer.copies() {
            ScatterCopies::Tree(trees) => {
                for tree in trees.instances() {
                    disc(
                        centre + tree.position.xz().as_dvec2(),
                        0.35,
                        [0.0, 0.05, 0.0],
                    );
                }
            }
            ScatterCopies::Stone(stones) => {
                for stone in stones.instances() {
                    disc(
                        centre + stone.position.xz().as_dvec2(),
                        f64::from(stone.radius),
                        [0.45, 0.45, 0.47],
                    );
                }
            }
            _ => {}
        }
    }

    // Write it, in sRGB.
    let mut bytes = format!("P6\n{width} {width}\n255\n").into_bytes();
    for colour in &image {
        for c in colour {
            bytes.push((c.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0).round() as u8);
        }
    }
    let path = dir.join("pine_forest_floor.ppm");
    fs::File::create(&path)?.write_all(&bytes)?;
    println!("wrote {}", path.display());
    Ok(())
}
