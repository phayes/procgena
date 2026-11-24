//! Writes shaded relief images of joined terrain regions, to check by eye
//! that borders are seamless.
//!
//! ```bash
//! cargo run --release -p procgena_terrain --example seam_spike -- target/terrain_spike
//! ```
//!
//! Each world is written twice as a binary PPM image: joined, with rivers,
//! ponds and roads; and with each region's own unbent terrain, for
//! comparison. An optional second argument sets the relief blend (0 to 1,
//! default 0.5; see `Convergence::blend`). A third argument, `warped`,
//! warps the borders with the default `BorderWarp`.

#[path = "../tests/common/mod.rs"]
mod common;

use std::fs;
use std::path::{Path, PathBuf};

use glam::{DVec2, I64Vec2, UVec2, Vec3};
use procgena_terrain::{HeightField, HeightTile, StitchedWorld};

/// Image size in pixels, and metres per pixel.
const PIXELS: u32 = 1000;
const STEP: f64 = 4.0;

/// Region tints, by position in the world's region list.
const TINTS: [Vec3; 3] = [
    Vec3::new(0.75, 0.9, 0.6),
    Vec3::new(0.85, 0.8, 0.95),
    Vec3::new(0.95, 0.85, 0.6),
];

/// The world's terrains before bending: each region's own height.
struct Unbent<'a>(&'a StitchedWorld);

impl HeightField for Unbent<'_> {
    fn height(&self, p: DVec2) -> f32 {
        self.0
            .owner(p)
            .map_or(f32::NAN, |t| t.generator().natural_height(p))
    }
}

fn main() -> std::io::Result<()> {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/terrain_spike".into()),
    );
    fs::create_dir_all(&dir)?;
    let blend = std::env::args().nth(2).map_or(common::DEFAULT.blend, |b| {
        b.parse().expect("a blend from 0 to 1")
    });
    let warp = match std::env::args().nth(3).as_deref() {
        None => None,
        Some("warped") => Some(procgena_terrain::BorderWarp::default()),
        Some(other) => panic!("the third argument may only be `warped`, not {other}"),
    };
    let config = common::Config {
        blend,
        warp,
        ..common::DEFAULT
    };
    for (name, world) in [
        ("two_terrains", common::two_terrains(DVec2::ZERO, config)),
        ("triple_junction", common::triple_junction(config)),
        (
            "hills_into_plains",
            common::hills_into_plains(common::RIVER_SEEDS[0], config),
        ),
    ] {
        write_relief(&dir.join(format!("{name}.ppm")), &world, &world, true)?;
        write_relief(
            &dir.join(format!("{name}_unbent.ppm")),
            &world,
            &Unbent(&world),
            false,
        )?;
    }
    println!("wrote images to {}", dir.display());
    Ok(())
}

/// Writes `field` as a shaded, region-tinted relief image. With `features`,
/// water is drawn blue and roads dark grey.
fn write_relief(
    path: &Path,
    world: &StitchedWorld,
    field: &impl HeightField,
    features: bool,
) -> std::io::Result<()> {
    let half = i64::from(PIXELS / 2);
    let origin = I64Vec2::splat(-half);
    let tile = HeightTile::sample(field, origin, UVec2::splat(PIXELS), STEP);
    let light = Vec3::new(-0.5, 0.8, -0.4).normalize();
    let position = |i: u32, j: u32| {
        DVec2::new(
            (origin.x + i64::from(i)) as f64 * STEP,
            (origin.y + i64::from(j)) as f64 * STEP,
        )
    };

    // Roads, drawn into a mask by walking each line.
    let mut road = vec![false; (PIXELS * PIXELS) as usize];
    if features {
        for line in world.terrains().iter().flat_map(|t| t.roads()) {
            for w in line.points.windows(2) {
                let steps = (w[0].distance(w[1]) / (STEP / 2.0)).ceil() as usize;
                for k in 0..=steps {
                    let q = (w[0].lerp(w[1], k as f64 / steps.max(1) as f64) / STEP).round();
                    let (i, j) = (q.x as i64 - origin.x, q.y as i64 - origin.y);
                    if (0..i64::from(PIXELS)).contains(&i) && (0..i64::from(PIXELS)).contains(&j) {
                        road[(j * i64::from(PIXELS) + i) as usize] = true;
                    }
                }
            }
        }
    }

    let mut bytes = format!("P6\n{PIXELS} {PIXELS}\n255\n").into_bytes();
    for j in 0..PIXELS {
        for i in 0..PIXELS {
            // Normal from neighbouring samples.
            let h = |di: i32, dj: i32| {
                let x = (i as i32 + di).clamp(0, PIXELS as i32 - 1) as u32;
                let z = (j as i32 + dj).clamp(0, PIXELS as i32 - 1) as u32;
                tile.get(x, z)
            };
            let dx = (h(1, 0) - h(-1, 0)) / (2.0 * STEP as f32);
            let dz = (h(0, 1) - h(0, -1)) / (2.0 * STEP as f32);
            let normal = Vec3::new(-dx, 1.0, -dz).normalize();
            let shade = 0.25 + 0.75 * normal.dot(light).max(0.0);

            // Tint by owning region, then water and roads.
            let p = position(i, j);
            let owner = world.owner(p);
            let tint = owner
                .and_then(|owner| world.terrains().iter().position(|t| std::ptr::eq(t, owner)))
                .map_or(Vec3::splat(0.3), |k| TINTS[k % TINTS.len()]);
            let mut colour = tint * shade;
            if features && owner.and_then(|t| t.water_level(p)).is_some() {
                colour = Vec3::new(0.15, 0.35, 0.8) * (0.6 + 0.4 * shade);
            }
            if road[(j * PIXELS + i) as usize] {
                colour = Vec3::splat(0.25);
            }
            bytes.extend([colour.x, colour.y, colour.z].map(|c| (c.clamp(0.0, 1.0) * 255.0) as u8));
        }
    }
    fs::write(path, bytes)
}
