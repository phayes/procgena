//! Draws a large World of bare land as images: where each Biome falls, the
//! fields that chose it, and the land in the middle.
//!
//! ```text
//! cargo run -p procgena_world --example large_world -- <dir> [--seed N] [--preset KEY]
//!     [--span KM] [--land KM] [--centre X Z] [--size PX]
//! ```
//!
//! Writes binary PPM images into `<dir>`:
//!
//! - `biomes.ppm`: each region filled with its Biome's colour, shaded by
//!   the world elevation E, with region borders dark and the pinned region
//!   outlined in red;
//! - `elevation.ppm`, `ruggedness.ppm` and `moisture.ppm`: the fields the
//!   Biome rules read, with region borders;
//! - `land.ppm`: the land in a square `--land` km wide (default 12) around
//!   `--centre` (x and z in km, default the origin), shaded, tinted by
//!   Biome, with rivers and ponds (roads are left out to keep it quick).
//!
//! The wide images cover `--span` km (default 96) around the origin; the
//! region in the middle is pinned to mountains. Prints how many regions
//! each Biome got and how often neighbours share a Biome.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use glam::{DVec2, Vec3};
use procgena_terrain::{FeatureTier, RegionId};
use procgena_world::{BiomePin, World, WorldPreset};

/// The settings read from the command line.
struct Args {
    dir: PathBuf,
    seed: u32,
    preset: String,
    span_km: f64,
    land_km: f64,
    /// The middle of the land image, in km.
    centre_km: DVec2,
    size: usize,
}

fn args() -> Args {
    let mut args = Args {
        dir: PathBuf::new(),
        seed: 0,
        preset: "medium".to_owned(),
        span_km: 96.0,
        land_km: 12.0,
        centre_km: DVec2::ZERO,
        size: 512,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || it.next().expect("a value after the flag");
        match arg.as_str() {
            "--seed" => args.seed = value().parse().expect("a whole number"),
            "--preset" => args.preset = value(),
            "--span" => args.span_km = value().parse().expect("a number"),
            "--land" => args.land_km = value().parse().expect("a number"),
            "--centre" => {
                let x = value().parse().expect("a number");
                let z = value().parse().expect("a number");
                args.centre_km = DVec2::new(x, z);
            }
            "--size" => args.size = value().parse().expect("a whole number"),
            _ => args.dir = PathBuf::from(arg),
        }
    }
    assert!(
        !args.dir.as_os_str().is_empty(),
        "usage: large_world <dir> [--seed N] [--preset KEY] [--span KM] [--land KM] \
         [--centre X Z] [--size PX]"
    );
    args
}

/// A Biome's colour on the maps.
fn biome_colour(key: &str) -> Vec3 {
    match key {
        "plains" => Vec3::new(0.66, 0.78, 0.42),
        "rolling_hills" => Vec3::new(0.36, 0.56, 0.27),
        "mountains" => Vec3::new(0.56, 0.48, 0.42),
        _ => Vec3::new(0.8, 0.3, 0.8),
    }
}

/// An image being drawn: `size` × `size` pixels over the square of side
/// `span` metres centred on `centre`.
struct Image {
    size: usize,
    span: f64,
    centre: DVec2,
    pixels: Vec<Vec3>,
}

impl Image {
    fn new(size: usize, span: f64, centre: DVec2) -> Self {
        Self {
            size,
            span,
            centre,
            pixels: vec![Vec3::ZERO; size * size],
        }
    }

    /// The world position at the middle of pixel (`i`, `j`); `j` runs
    /// north (towards −z) to south.
    fn position(&self, i: usize, j: usize) -> DVec2 {
        let step = self.span / self.size as f64;
        self.centre
            + DVec2::new(
                -0.5 * self.span + (i as f64 + 0.5) * step,
                -0.5 * self.span + (j as f64 + 0.5) * step,
            )
    }

    fn write(&self, path: &Path) -> std::io::Result<()> {
        let mut out = BufWriter::new(File::create(path)?);
        write!(out, "P6\n{} {}\n255\n", self.size, self.size)?;
        for p in &self.pixels {
            let c = (p.clamp(Vec3::ZERO, Vec3::ONE) * 255.0).round();
            out.write_all(&[c.x as u8, c.y as u8, c.z as u8])?;
        }
        out.flush()
    }
}

/// A colour for E: blue below sea level, then green, brown and white.
fn elevation_colour(e: f32) -> Vec3 {
    if e < 0.0 {
        let t = (-e / 400.0).min(1.0);
        Vec3::new(0.45, 0.65, 0.9).lerp(Vec3::new(0.1, 0.2, 0.5), t)
    } else {
        let stops = [
            (0.0, Vec3::new(0.35, 0.6, 0.35)),
            (150.0, Vec3::new(0.75, 0.75, 0.45)),
            (350.0, Vec3::new(0.55, 0.4, 0.3)),
            (600.0, Vec3::new(0.95, 0.95, 0.95)),
        ];
        for w in stops.windows(2) {
            let ((a, ca), (b, cb)) = (w[0], w[1]);
            if e < b {
                return ca.lerp(cb, (e - a) / (b - a));
            }
        }
        stops[stops.len() - 1].1
    }
}

/// A colour for a selector field value: brown below 0, blue-green above.
fn field_colour(v: f32) -> Vec3 {
    let t = (v.clamp(-1.5, 1.5) / 1.5 + 1.0) * 0.5;
    Vec3::new(0.6, 0.4, 0.2).lerp(Vec3::new(0.15, 0.5, 0.6), t)
}

fn main() -> std::io::Result<()> {
    let args = args();
    std::fs::create_dir_all(&args.dir)?;

    // The World, with the region in the middle pinned to mountains.
    let mut options = WorldPreset::named(&args.preset)
        .unwrap_or_else(|| panic!("no World preset is named {:?}", args.preset))
        .world
        .clone();
    options.seed = args.seed;
    let middle = World::new(options.clone())
        .expect("valid World")
        .region_at(DVec2::ZERO);
    options.pins.push(BiomePin {
        region: middle.0,
        biome: "mountains".to_owned(),
    });
    let world = World::new(options).expect("valid World");
    let scale = world.options().scale;
    println!(
        "preset {}, seed {}, scale {} × {}, regions {} m",
        args.preset,
        args.seed,
        scale.horizontal,
        scale.vertical,
        world.options().regions.size
    );

    // Every pixel's region over the wide square.
    let started = Instant::now();
    let span = args.span_km * 1000.0;
    let mut wide = Image::new(args.size, span, DVec2::ZERO);
    let owners: Vec<RegionId> = (0..args.size * args.size)
        .map(|k| world.region_at(wide.position(k % args.size, k / args.size)))
        .collect();
    let mut biomes = HashMap::new();
    for &region in &owners {
        biomes.entry(region).or_insert_with(|| world.biome(region));
    }
    println!(
        "{} regions in {span_km} km × {span_km} km, found in {:.1} s",
        biomes.len(),
        started.elapsed().as_secs_f64(),
        span_km = args.span_km,
    );
    let border = |k: usize| {
        let (i, j) = (k % args.size, k / args.size);
        (i + 1 < args.size && owners[k + 1] != owners[k])
            || (j + 1 < args.size && owners[k + args.size] != owners[k])
    };

    // The fields.
    let fields: Vec<_> = (0..args.size * args.size)
        .map(|k| world.fields(wide.position(k % args.size, k / args.size)))
        .collect();
    let (mut low, mut high) = (f32::INFINITY, f32::NEG_INFINITY);
    for f in &fields {
        low = low.min(f.elevation);
        high = high.max(f.elevation);
    }
    println!("E from {low:.0} m to {high:.0} m");
    let names = ["elevation", "ruggedness", "moisture"];
    for (n, name) in names.iter().enumerate() {
        for (k, f) in fields.iter().enumerate() {
            let c = match n {
                0 => elevation_colour(f.elevation / scale.vertical as f32),
                1 => field_colour(f.ruggedness),
                _ => field_colour(f.moisture),
            };
            wide.pixels[k] = if border(k) { c * 0.4 } else { c };
        }
        wide.write(&args.dir.join(format!("{name}.ppm")))?;
    }

    // The Biome map.
    for k in 0..args.size * args.size {
        let biome = &biomes[&owners[k]];
        let e = fields[k].elevation / scale.vertical as f32;
        let shade = 0.8 + 0.2 * (e / 400.0).clamp(-1.0, 1.0);
        let mut c = biome_colour(&biome.preset.key) * shade;
        if border(k) {
            c = if biome.pinned {
                Vec3::new(0.9, 0.1, 0.1)
            } else {
                c * 0.35
            };
        }
        wide.pixels[k] = c;
    }
    wide.write(&args.dir.join("biomes.ppm"))?;

    // How many regions each Biome got, and how often neighbours match.
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for biome in biomes.values() {
        *counts.entry(&biome.preset.key).or_default() += 1;
    }
    let mut counts: Vec<_> = counts.into_iter().collect();
    counts.sort();
    println!("regions per Biome: {counts:?}");
    let (mut same, mut pairs) = (0, 0);
    for (&region, biome) in &biomes {
        for edge in &world.terrain().region(region).edges {
            if let Some(other) = edge.neighbor.and_then(|n| biomes.get(&n)) {
                pairs += 1;
                if other.preset.key == biome.preset.key {
                    same += 1;
                }
            }
        }
    }
    println!(
        "neighbours sharing a Biome: {same} of {pairs} ({:.0} %)",
        100.0 * f64::from(same) / f64::from(pairs.max(1))
    );

    // The land in the middle: build the regions it touches on all cores.
    let started = Instant::now();
    let mut land = Image::new(args.size, args.land_km * 1000.0, args.centre_km * 1000.0);
    let land_owners: Vec<RegionId> = (0..args.size * args.size)
        .map(|k| world.region_at(land.position(k % args.size, k / args.size)))
        .collect();
    let mut regions: Vec<RegionId> = land_owners.clone();
    regions.sort_by_key(|r| r.0);
    regions.dedup();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|s| {
        for chunk in regions.chunks(regions.len().div_ceil(threads)) {
            let world = &world;
            s.spawn(move || {
                for &region in chunk {
                    world
                        .terrain()
                        .terrain_tier(region, FeatureTier::Rough)
                        .expect("region builds");
                }
            });
        }
    });
    println!(
        "{} regions of land built in {:.1} s",
        regions.len(),
        started.elapsed().as_secs_f64()
    );

    // Shade the land from the north-west, tinted by Biome, with water blue.
    let step = land.span / land.size as f64;
    let height = |p: DVec2| {
        let terrain = world
            .terrain()
            .terrain_tier(world.region_at(p), FeatureTier::Rough)
            .expect("region builds");
        (terrain.height(p), terrain.water_level(p))
    };
    let light = Vec3::new(-1.0, 1.5, -1.0).normalize();
    for k in 0..land.size * land.size {
        let p = land.position(k % land.size, k / land.size);
        let (h, water) = height(p);
        let dx = height(p + DVec2::X * step).0 - h;
        let dz = height(p + DVec2::Y * step).0 - h;
        let normal = Vec3::new(-dx, step as f32, -dz).normalize();
        let lit = 0.35 + 0.65 * normal.dot(light).max(0.0);
        let biome = world.biome(land_owners[k]);
        let mut c = biome_colour(&biome.preset.key) * lit;
        if water.is_some_and(|w| w > h) {
            c = Vec3::new(0.2, 0.4, 0.8) * (0.6 + 0.4 * lit);
        }
        let (i, j) = (k % land.size, k / land.size);
        let edge = (i + 1 < land.size && land_owners[k + 1] != land_owners[k])
            || (j + 1 < land.size && land_owners[k + land.size] != land_owners[k]);
        land.pixels[k] = if edge { c * 0.5 } else { c };
    }
    land.write(&args.dir.join("land.ppm"))?;
    println!("wrote {}", args.dir.display());
    Ok(())
}
