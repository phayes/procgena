//! Test worlds shared by the seam tests and the `seam_spike` example.

#![allow(dead_code)]

use glam::DVec2;
use procgena_terrain::{
    Convergence, Elevation, ElevationOptions, HeightBump, HeightField, NoiseLayer, RegionId,
    RegionSpec, StitchedWorld, TerrainOptions, TerrainPreset,
};

/// The largest height change between neighbouring samples along `path`
/// (a function from 0..=1 to a position), using `count` steps.
fn largest_jump(world: &StitchedWorld, path: &impl Fn(f64) -> DVec2, count: usize) -> f32 {
    let mut last = world.height(path(0.0));
    let mut largest = 0.0f32;
    for k in 1..=count {
        let h = world.height(path(k as f64 / count as f64));
        largest = largest.max((h - last).abs());
        last = h;
    }
    largest
}

/// Checks that `world` has no cracks along `path`, which is `length` metres
/// long. On smooth ground the largest jump shrinks about tenfold when the
/// steps do; across a crack it stays the same size however fine the steps.
/// A narrow steep feature (such as a river channel's edge) can shrink less
/// at 50 cm and 5 cm steps; then the path is measured again at 5 mm steps,
/// and it passes only if the jump is still shrinking.
pub fn assert_no_crack(
    world: &StitchedWorld,
    path: impl Fn(f64) -> DVec2,
    length: f64,
    label: &str,
) {
    let coarse_count = (length / 0.5).ceil() as usize;
    let coarse = largest_jump(world, &path, coarse_count);
    let fine = largest_jump(world, &path, coarse_count * 10);
    if fine < 0.2 * coarse + 1e-3 {
        return;
    }
    let finest = largest_jump(world, &path, coarse_count * 100);
    assert!(
        finest < 0.5 * fine + 1e-4,
        "{label}: largest jump {finest} at 5 mm steps, {fine} at 5 cm, {coarse} at 50 cm"
    );
}

/// How the regions of a test world are seeded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seeds {
    /// Every region has the same world seed, so they share their noise.
    Shared,
    /// Every region has its own seed: unrelated ground on each side, the
    /// hardest case for the border.
    Unrelated,
}

/// The world elevation a test world stands on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Land {
    /// Flat land at sea level.
    Flat,
    /// A steady slope, falling towards the west (and a little to the
    /// south): [`TILT`].
    Tilted,
    /// The default continents and uplands, plus [`BUMP`] standing across
    /// the two-region border.
    Curved,
}

/// The slope of [`Land::Tilted`], rise over run along x and z.
pub const TILT: DVec2 = DVec2::new(0.02, 0.005);

/// The bump of [`Land::Curved`]: 40 m high, reaching 1.5 km, centred
/// beside the two-region border so it rises on both sides of it.
pub const BUMP: HeightBump = HeightBump {
    centre: DVec2::new(-100.0, 0.0),
    radius: 1500.0,
    height: 40.0,
};

/// How a test world is built.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    /// How the regions are seeded.
    pub seeds: Seeds,
    /// The relief meeting exponent passed to `StitchedWorld::build`.
    pub meeting: f32,
    /// The relief blend passed to `StitchedWorld::build`.
    pub blend: f32,
    /// The world elevation the regions stand on.
    pub land: Land,
}

impl Config {
    /// The world elevation to build with, from the world seed 1.
    pub fn elevation(self) -> Elevation {
        let flat = NoiseLayer {
            height: 0.0,
            ..ElevationOptions::default().uplift
        };
        let options = match self.land {
            Land::Flat => return Elevation::flat(),
            Land::Tilted => ElevationOptions {
                continents: flat.clone(),
                uplift: flat,
                tilt: TILT,
                max_slope: 0.05,
                bumps: Vec::new(),
            },
            Land::Curved => ElevationOptions {
                max_slope: 0.1,
                bumps: vec![BUMP],
                ..ElevationOptions::default()
            },
        };
        Elevation::new(options, 1).expect("test elevations are valid")
    }

    /// The convergence settings to build with.
    pub fn convergence(self) -> Convergence {
        Convergence {
            meeting: self.meeting,
            blend: self.blend,
        }
    }

    /// The seed for a region whose own seed is `own`, in a world seeded
    /// with `world`.
    pub fn seed(self, world: u32, own: u32) -> u32 {
        match self.seeds {
            Seeds::Shared => world,
            Seeds::Unrelated => own,
        }
    }
}

/// The usual world: one world seed and the default convergence.
pub const DEFAULT: Config = Config {
    seeds: Seeds::Shared,
    meeting: procgena_terrain::DEFAULT_RELIEF_MEETING,
    blend: procgena_terrain::DEFAULT_RELIEF_BLEND,
    land: Land::Flat,
};

/// The usual world on a steady slope.
pub const TILTED: Config = Config {
    land: Land::Tilted,
    ..DEFAULT
};

/// The usual world on curved land, with a bump across the border.
pub const CURVED: Config = Config {
    land: Land::Curved,
    ..DEFAULT
};

/// Every configuration the suites run on: both kinds of seeding, each at
/// the relief meeting exponents −∞, −1, 0 and 1 with the default blend,
/// and at the default exponent with blends 0 and 1.
pub const CONFIGS: [Config; 12] = {
    let meetings = [f32::NEG_INFINITY, -1.0, 0.0, 1.0];
    let mut out = [DEFAULT; 12];
    let mut i = 0;
    while i < 12 {
        let seeds = if i % 2 == 0 {
            Seeds::Shared
        } else {
            Seeds::Unrelated
        };
        out[i] = if i < 8 {
            Config {
                seeds,
                meeting: meetings[i / 2],
                ..DEFAULT
            }
        } else {
            Config {
                seeds,
                blend: if i < 10 { 0.0 } else { 1.0 },
                ..DEFAULT
            }
        };
        i += 1;
    }
    out
};

/// [`CONFIGS`] on flat land, then the usual world on tilted and on curved
/// land.
pub const WITH_LAND: [Config; 14] = {
    let mut out = [TILTED; 14];
    let mut i = 0;
    while i < 12 {
        out[i] = CONFIGS[i];
        i += 1;
    }
    out[13] = CURVED;
    out
};

/// Settings for `preset` with `seed`.
pub fn preset(preset: TerrainPreset, seed: u32) -> TerrainOptions {
    let mut options = preset.options();
    options.seed = seed;
    options
}

/// Moves every point by `offset`.
fn shifted(points: &[(f64, f64)], offset: DVec2) -> Vec<DVec2> {
    points
        .iter()
        .map(|&(x, z)| DVec2::new(x, z) + offset)
        .collect()
}

/// Plains in the west and mountains in the east, 4 km × 3 km, split by a
/// bent border, all moved by `offset`.
pub fn two_terrains(offset: DVec2, config: Config) -> StitchedWorld {
    two_regions(
        offset,
        preset(TerrainPreset::Plains, config.seed(1, 1)),
        preset(TerrainPreset::Mountains, config.seed(1, 2)),
        config.convergence(),
        &config.elevation(),
    )
}

/// Two regions with the given settings, split by the two-terrain bent border
/// (non-convex on both sides), joined with `convergence`, standing on
/// `elevation`.
pub fn two_regions(
    offset: DVec2,
    west: TerrainOptions,
    east: TerrainOptions,
    convergence: Convergence,
    elevation: &Elevation,
) -> StitchedWorld {
    let west_points = [
        (-2000.0, -1500.0),
        (0.0, -1500.0),
        (150.0, -700.0),
        (-200.0, 0.0),
        (100.0, 800.0),
        (0.0, 1500.0),
        (-2000.0, 1500.0),
    ];
    let east_points = [
        (0.0, -1500.0),
        (2000.0, -1500.0),
        (2000.0, 1500.0),
        (0.0, 1500.0),
        (100.0, 800.0),
        (-200.0, 0.0),
        (150.0, -700.0),
    ];
    StitchedWorld::build(
        vec![
            RegionSpec {
                id: RegionId(1),
                polygon: shifted(&west_points, offset),
                options: west,
            },
            RegionSpec {
                id: RegionId(2),
                polygon: shifted(&east_points, offset),
                options: east,
            },
        ],
        &convergence,
        elevation,
    )
    .expect("two-region world builds")
}

/// Hill seeds for [`hills_into_plains`] that give a river crossing with
/// every one of [`CONFIGS`]: the only two found by searching seeds 0 to 299
/// with the `river_crossing_seeds` probe (8 to 33 of the 300 give one with
/// each configuration).
pub const RIVER_SEEDS: [u32; 2] = [81, 293];

/// Hill seeds for [`hills_into_plains`] whose plains have a river joining
/// another river on a bend, where the smoothed rivers are easy to get
/// crossed: 64 with shared seeds, 136 with unrelated seeds.
pub const JOIN_SEEDS: [u32; 2] = [64, 136];

/// Plains in the west and rolling hills in the east, split by the
/// two-region border. With a seed from [`RIVER_SEEDS`] a river from the
/// hills crosses into the plains.
pub fn hills_into_plains(seed: u32, config: Config) -> StitchedWorld {
    two_regions(
        DVec2::ZERO,
        preset(TerrainPreset::Plains, config.seed(seed, seed + 100)),
        preset(TerrainPreset::RollingHills, seed),
        config.convergence(),
        &config.elevation(),
    )
}

/// The shared border of [`two_regions`], before any offset.
pub const TWO_REGION_BORDER: [(f64, f64); 5] = [
    (0.0, -1500.0),
    (150.0, -700.0),
    (-200.0, 0.0),
    (100.0, 800.0),
    (0.0, 1500.0),
];

/// Where the three regions of [`triple_junction`] meet.
pub const JUNCTION: (f64, f64) = (100.0, -50.0);

/// Plains (north-west), mountains (north-east) and rolling hills (south)
/// meeting at [`JUNCTION`], in a 4 km square.
pub fn triple_junction(config: Config) -> StitchedWorld {
    let c = JUNCTION;
    let north_west = [
        c,
        (-800.0, -500.0),
        (-2000.0, -900.0),
        (-2000.0, 2000.0),
        (0.0, 2000.0),
        (-50.0, 900.0),
    ];
    let north_east = [
        c,
        (-50.0, 900.0),
        (0.0, 2000.0),
        (2000.0, 2000.0),
        (2000.0, -1200.0),
        (700.0, -600.0),
    ];
    let south = [
        c,
        (700.0, -600.0),
        (2000.0, -1200.0),
        (2000.0, -2000.0),
        (-2000.0, -2000.0),
        (-2000.0, -900.0),
        (-800.0, -500.0),
    ];
    StitchedWorld::build(
        vec![
            RegionSpec {
                id: RegionId(10),
                polygon: shifted(&north_west, DVec2::ZERO),
                options: preset(TerrainPreset::Plains, config.seed(3, 3)),
            },
            RegionSpec {
                id: RegionId(20),
                polygon: shifted(&north_east, DVec2::ZERO),
                options: preset(TerrainPreset::Mountains, config.seed(3, 4)),
            },
            RegionSpec {
                id: RegionId(30),
                polygon: shifted(&south, DVec2::ZERO),
                options: preset(TerrainPreset::RollingHills, config.seed(3, 5)),
            },
        ],
        &config.convergence(),
        &config.elevation(),
    )
    .expect("triple-junction world builds")
}
