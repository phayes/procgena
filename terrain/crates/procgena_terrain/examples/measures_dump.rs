//! Writes top-down images of each of the terrain's measures, to check them
//! by eye: slope, curvature, height above water, and distance to water and
//! to roads.
//!
//! ```bash
//! cargo run --release -p procgena_terrain --example measures_dump -- target/measures_dump
//! ```
//!
//! The world is the test world of plains and hills with a river crossing
//! their warped border. Each measure is written twice as a binary PPM
//! image: `<measure>_world.ppm`, the whole 4 km world at 4 m a pixel, and
//! `<measure>_crossing.ppm`, 256 m round the river's border crossing at
//! 0.5 m a pixel. Borders between regions are drawn in red.
//!
//! - **slope**: black flat to white for a wall.
//! - **curvature**: blue concave, red convex, white flat.
//! - **height_above_water**: deep blue under water, green just above it,
//!   to yellow 10 m above; grey where no water is within reach.
//! - **water_distance** and **road_distance**: white at the water or road,
//!   fading to black at the measures' reach.

#[path = "../tests/common/mod.rs"]
mod common;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use glam::{DVec2, Vec3};
use procgena_terrain::{MEASURE_REACH, Measures, SampleDetail, StitchedWorld};

/// A measure's colour at a point.
type Colour = fn(&Measures) -> Vec3;

/// The measures drawn, by file name, each with its colour at a point.
const DRAWN: [(&str, Colour); 5] = [
    ("slope", |m| Vec3::splat(m.slope)),
    ("curvature", |m| {
        let c = (m.curvature / 0.05).tanh();
        if c < 0.0 {
            Vec3::new(1.0 + c, 1.0 + c, 1.0)
        } else {
            Vec3::new(1.0, 1.0 - c, 1.0 - c)
        }
    }),
    ("height_above_water", |m| match m.height_above_water {
        None => Vec3::splat(0.35),
        Some(h) if h < 0.0 => Vec3::new(0.0, 0.1, 0.4 + 0.6 * (1.0 + h / 3.0).clamp(0.0, 1.0)),
        Some(h) => Vec3::new(0.1, 0.7, 0.2).lerp(Vec3::new(1.0, 0.9, 0.2), (h / 10.0).min(1.0)),
    }),
    ("water_distance", |m| {
        Vec3::splat(1.0 - m.water_distance / MEASURE_REACH)
    }),
    ("road_distance", |m| {
        Vec3::splat(1.0 - m.road_distance / MEASURE_REACH)
    }),
];

fn main() -> std::io::Result<()> {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/measures_dump".into()),
    );
    fs::create_dir_all(&dir)?;
    let world = common::hills_into_plains(common::RIVER_SEEDS[0], common::WARPED[0]);

    // The whole world, then the river's crossing close up.
    draw(&world, &dir, "world", DVec2::ZERO, 4.0, 1000)?;
    let plains = &world.terrains()[0];
    let crossing = plains
        .rivers()
        .iter()
        .flat_map(|river| river.points.iter().copied())
        .min_by(|&a, &b| {
            let border = |p| plains.region().boundary.distance_to_border(p);
            border(a).total_cmp(&border(b))
        })
        .expect("the test world's plains have a river");
    draw(&world, &dir, "crossing", crossing, 0.5, 512)
}

/// Writes one image per measure of `world`, `pixels` wide and `step` metres
/// a pixel, centred on `centre`, named `<measure>_<name>.ppm` in `dir`.
fn draw(
    world: &StitchedWorld,
    dir: &Path,
    name: &str,
    centre: DVec2,
    step: f64,
    pixels: usize,
) -> std::io::Result<()> {
    // Each pixel's measures and region, rows from +z at the top.
    let origin = centre - DVec2::splat(step * pixels as f64 / 2.0);
    let mut cells = Vec::with_capacity(pixels * pixels);
    for y in 0..pixels {
        for x in 0..pixels {
            let p = origin + DVec2::new(x as f64, (pixels - 1 - y) as f64) * step;
            cells.push(world.owner(p).map(|terrain| {
                let measures = terrain
                    .query(p, SampleDetail::Measures)
                    .measures
                    .expect("measures were asked for");
                (terrain.region().id, measures)
            }));
        }
    }

    // One image per measure, borders in red, outside the world black.
    for (measure, colour) in DRAWN {
        let mut bytes = format!("P6\n{pixels} {pixels}\n255\n").into_bytes();
        for y in 0..pixels {
            for x in 0..pixels {
                let cell = cells[y * pixels + x];
                let region = |i: usize, j: usize| cells[j * pixels + i].map(|(id, _)| id);
                let border = (x > 0 && region(x - 1, y) != region(x, y))
                    || (y > 0 && region(x, y - 1) != region(x, y));
                let c = match cell {
                    _ if border => Vec3::new(1.0, 0.0, 0.0),
                    None => Vec3::ZERO,
                    Some((_, measures)) => colour(&measures),
                };
                for v in c.to_array() {
                    bytes.push((v.clamp(0.0, 1.0) * 255.0).round() as u8);
                }
            }
        }
        let path = dir.join(format!("{measure}_{name}.ppm"));
        fs::File::create(&path)?.write_all(&bytes)?;
        println!("wrote {}", path.display());
    }
    Ok(())
}
