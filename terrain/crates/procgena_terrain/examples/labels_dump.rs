//! Writes top-down maps of the terrain's labels, to check them by eye:
//! each point coloured by its landform, shaded by the light on the ground.
//!
//! ```bash
//! cargo run --release -p procgena_terrain --example labels_dump -- target/labels_dump
//! ```
//!
//! Two test worlds are drawn: `hills`, plains and rolling hills with a river
//! crossing their warped border, and `junction`, plains, mountains and
//! rolling hills meeting at one point. Both use the medium terrain settings,
//! so their labels read shapes at medium scale (2 × 2). Each is written as
//! binary PPM images:
//! `landforms_<world>.ppm`, the whole 4 km world at 4 m a pixel, and
//! `landforms_<world>_close.ppm`, 256 m round a river's border crossing
//! (`hills`) or the junction (`junction`) at 0.5 m a pixel. Borders between
//! regions are drawn in black. The colours are [`Landform::colour`]; the
//! legend, and how much of each world each landform covers, are printed.

#[path = "../tests/common/mod.rs"]
mod common;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use glam::{DVec2, I64Vec2, UVec2, Vec3};
use procgena_terrain::{HeightTile, Landform, StitchedWorld, WorldScale};

/// The scale of the test worlds' terrain settings: medium, twice as long
/// and as high as scale 1.
const MEDIUM: WorldScale = WorldScale {
    horizontal: 2.0,
    vertical: 2.0,
};

fn main() -> std::io::Result<()> {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/labels_dump".into()),
    );
    fs::create_dir_all(&dir)?;
    for landform in Landform::ALL {
        let [r, g, b] = landform.colour();
        println!("{:>10}: rgb({r}, {g}, {b})", landform.name());
    }

    // Plains into hills, with the river's crossing close up.
    let hills =
        common::hills_into_plains(common::RIVER_SEEDS[0], common::WARPED[0]).with_scale(MEDIUM);
    draw(&hills, &dir, "hills", DVec2::ZERO, 4.0, 1000)?;
    let plains = &hills.terrains()[0];
    let crossing = plains
        .rivers()
        .iter()
        .flat_map(|river| river.points.iter().copied())
        .min_by(|&a, &b| {
            let border = |p| plains.region().boundary.distance_to_border(p);
            border(a).total_cmp(&border(b))
        })
        .expect("the test world's plains have a river");
    draw(&hills, &dir, "hills_close", crossing, 0.5, 512)?;

    // Plains, mountains and hills, with their junction close up.
    let junction = common::triple_junction(common::WARPED[0]).with_scale(MEDIUM);
    draw(&junction, &dir, "junction", DVec2::ZERO, 4.0, 1000)?;
    let (x, z) = common::JUNCTION;
    draw(
        &junction,
        &dir,
        "junction_close",
        DVec2::new(x, z),
        0.5,
        512,
    )
}

/// Writes `landforms_<name>.ppm` in `dir`: the landforms of `world`,
/// `pixels` wide and `step` metres a pixel, centred on `centre`, and prints
/// how much of the image each landform covers.
fn draw(
    world: &StitchedWorld,
    dir: &Path,
    name: &str,
    centre: DVec2,
    step: f64,
    pixels: u32,
) -> std::io::Result<()> {
    // The labels on the lattice, rows from -z; the image's rows run from +z.
    let origin = ((centre / step).round().as_i64vec2()) - I64Vec2::splat(i64::from(pixels) / 2);
    let tile = HeightTile::sample_labelled(world, origin, UVec2::splat(pixels), step);
    let region = |i: u32, j: u32| {
        let p = (origin + I64Vec2::new(i64::from(i), i64::from(j))).as_dvec2() * step;
        world.owner(p).map(|t| t.region().id)
    };

    // Each pixel: its landform's colour, lit from the north-west, with
    // borders in black.
    let light = Vec3::new(-1.0, 1.5, 1.0).normalize();
    let mut counts = [0usize; Landform::ALL.len()];
    let mut bytes = format!("P6\n{pixels} {pixels}\n255\n").into_bytes();
    for y in 0..pixels {
        let j = pixels - 1 - y;
        for i in 0..pixels {
            let border = (i > 0 && region(i - 1, j) != region(i, j))
                || (j > 0 && region(i, j - 1) != region(i, j));
            let c = match tile.labels(i, j) {
                _ if border => Vec3::ZERO,
                None => Vec3::splat(0.2),
                Some(labels) => {
                    counts[labels.landform as usize] += 1;
                    let [r, g, b] = labels.landform.colour();
                    let (i0, j0) = (i.saturating_sub(1), j.saturating_sub(1));
                    let (i1, j1) = ((i + 1).min(pixels - 1), (j + 1).min(pixels - 1));
                    let dx = (tile.get(i1, j) - tile.get(i0, j)) / ((i1 - i0) as f64 * step) as f32;
                    let dz = (tile.get(i, j1) - tile.get(i, j0)) / ((j1 - j0) as f64 * step) as f32;
                    let normal = Vec3::new(-dx, 1.0, -dz).normalize();
                    let shade = 0.55 + 0.45 * normal.dot(light).max(0.0);
                    Vec3::new(r.into(), g.into(), b.into()) / 255.0 * shade
                }
            };
            for v in c.to_array() {
                bytes.push((v.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
        }
    }
    let path = dir.join(format!("landforms_{name}.ppm"));
    fs::File::create(&path)?.write_all(&bytes)?;

    // How much of the image each landform covers.
    let total: usize = counts.iter().sum();
    let shares: Vec<String> = Landform::ALL
        .iter()
        .zip(counts)
        .filter(|&(_, n)| n > 0)
        .map(|(l, n)| format!("{} {:.1}%", l.name(), 100.0 * n as f64 / total as f64))
        .collect();
    println!("wrote {}: {}", path.display(), shares.join(", "));
    Ok(())
}
