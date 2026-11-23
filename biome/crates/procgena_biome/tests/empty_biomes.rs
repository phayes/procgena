//! Empty Biomes: every Terrain preset has one, and building it gives the
//! same land as building the Terrain on its own.

use glam::DVec2;
use procgena_biome::{Biome, BiomePreset, BiomeTerrain, terrain_seed};
use procgena_terrain::{
    Boundary, Convergence, Region, RegionId, RegionSpec, StitchedWorld, Terrain, TerrainGenerator,
    TerrainOptions, TerrainPreset,
};

/// World seeds to build with: the presets' default and one other.
const SEEDS: [u32; 2] = [0, 7919];

/// Side of the square test region, in metres. Large enough that every
/// preset plans some rivers or roads.
const SIDE: f64 = 3000.0;

/// Samples per side of the height and water grid.
const GRID: usize = 61;

/// The square test region: a polygon (so rivers and roads are planned) with
/// no neighbours.
fn region() -> Region {
    Region {
        id: RegionId(1),
        boundary: Boundary::Polygon(square(DVec2::ZERO)),
        edges: Vec::new(),
        corners: Vec::new(),
    }
}

/// The corners of a `SIDE`-wide square with its lower corner at `origin`.
fn square(origin: DVec2) -> Vec<DVec2> {
    vec![
        origin,
        origin + DVec2::new(SIDE, 0.0),
        origin + DVec2::new(SIDE, SIDE),
        origin + DVec2::new(0.0, SIDE),
    ]
}

/// The Terrain built straight from the terrain crate, with `options` and the
/// Terrain seed for `world_seed`.
fn reference(options: TerrainOptions, world_seed: u32) -> Terrain {
    let options = TerrainOptions {
        seed: terrain_seed(world_seed),
        ..options
    };
    TerrainGenerator::new(options)
        .expect("valid options")
        .build(region(), &[])
        .expect("region with no neighbours builds")
}

/// What one comparison covered, so tests can check it wasn't empty.
#[derive(Debug, Default)]
struct Covered {
    /// Rivers, ponds, roads and points of interest compared.
    features: usize,
    /// Grid points that had water.
    wet_points: usize,
}

/// Asserts that `a` and `b` are the same land, bit for bit: heights, water
/// levels and road tags over a grid inside `origin`'s square, and every
/// feature.
fn assert_same_land(a: &Terrain, b: &Terrain, origin: DVec2, label: &str) -> Covered {
    let mut covered = Covered::default();

    // Heights, water and road tags at every grid point.
    for i in 0..GRID {
        for j in 0..GRID {
            let step = SIDE / (GRID - 1) as f64;
            let p = origin + DVec2::new(i as f64 * step, j as f64 * step);
            assert_eq!(
                a.height(p).to_bits(),
                b.height(p).to_bits(),
                "{label}: height at {p}"
            );
            let (wa, wb) = (a.water_level(p), b.water_level(p));
            assert_eq!(
                wa.map(f32::to_bits),
                wb.map(f32::to_bits),
                "{label}: water level at {p}"
            );
            covered.wet_points += usize::from(wa.is_some());
            assert_eq!(a.road_tags(p), b.road_tags(p), "{label}: road tags at {p}");
        }
    }

    // Every feature.
    assert_eq!(a.rivers(), b.rivers(), "{label}: rivers");
    assert_eq!(a.ponds(), b.ponds(), "{label}: ponds");
    assert_eq!(a.roads(), b.roads(), "{label}: roads");
    assert_eq!(
        a.points_of_interest(),
        b.points_of_interest(),
        "{label}: points of interest"
    );
    covered.features =
        a.rivers().len() + a.ponds().len() + a.roads().len() + a.points_of_interest().len();
    covered
}

#[test]
fn every_terrain_preset_has_an_empty_biome() {
    for terrain in TerrainPreset::ALL {
        let preset = BiomePreset::bare(terrain)
            .unwrap_or_else(|| panic!("{} has no empty Biome", terrain.name()));
        assert_eq!(preset.biome.terrain, BiomeTerrain::Preset(terrain));
        assert!(preset.biome.is_empty(), "{}", preset.key);
        assert_eq!(preset.default_seed, 0, "{}", preset.key);
    }
}

#[test]
fn empty_biome_builds_the_same_land_as_its_terrain() {
    let mut features = 0;
    let mut wet_points = 0;
    for terrain in TerrainPreset::ALL {
        let preset = BiomePreset::bare(terrain).expect("every preset has an empty Biome");
        for seed in SEEDS {
            let label = format!("{} seed {seed}", preset.key);
            let built = preset
                .biome
                .build(seed, region(), &[])
                .expect("Biome builds");
            let alone = reference(terrain.options(), seed);
            let covered = assert_same_land(&built, &alone, DVec2::ZERO, &label);
            features += covered.features;
            wet_points += covered.wet_points;
        }
    }

    // The comparison must have covered features and water, not only heights.
    assert!(features > 0, "no preset planned any rivers or roads");
    assert!(wet_points > 0, "no grid point had water");
}

#[test]
fn inline_terrain_builds_the_same_land_and_takes_the_world_seed() {
    for terrain in TerrainPreset::ALL {
        // An inline seed is replaced by the World's.
        let inline = Biome::empty(BiomeTerrain::Inline(TerrainOptions {
            seed: 12345,
            ..terrain.options()
        }));
        for seed in SEEDS {
            let label = format!("inline {} seed {seed}", terrain.name());
            assert_eq!(inline.terrain_options(seed).seed, terrain_seed(seed));
            let built = inline.build(seed, region(), &[]).expect("Biome builds");
            let alone = reference(terrain.options(), seed);
            assert_same_land(&built, &alone, DVec2::ZERO, &label);
        }
    }
}

#[test]
fn invalid_inline_terrain_is_an_error() {
    let mut options = TerrainPreset::Plains.options();
    options.base.erosion = f32::NAN;
    let biome = Biome::empty(BiomeTerrain::Inline(options));
    assert!(matches!(
        biome.build(0, region(), &[]),
        Err(procgena_biome::BiomeError::Options(_))
    ));
}

#[test]
fn stitched_biomes_build_the_same_world_as_stitched_terrains() {
    // Plains to the west of mountains, as a World of bare land will build them.
    let west = DVec2::new(-SIDE, 0.0);
    let pairs = [
        (RegionId(1), west, TerrainPreset::Plains),
        (RegionId(2), DVec2::ZERO, TerrainPreset::Mountains),
    ];
    for seed in SEEDS {
        let from_biomes = pairs.map(|(id, origin, terrain)| RegionSpec {
            id,
            polygon: square(origin),
            options: BiomePreset::bare(terrain)
                .expect("every preset has an empty Biome")
                .biome
                .terrain_options(seed),
        });
        let from_terrains = pairs.map(|(id, origin, terrain)| RegionSpec {
            id,
            polygon: square(origin),
            options: TerrainOptions {
                seed: terrain_seed(seed),
                ..terrain.options()
            },
        });
        let convergence = Convergence::default();
        let a = StitchedWorld::build(from_biomes.to_vec(), &convergence).expect("world builds");
        let b = StitchedWorld::build(from_terrains.to_vec(), &convergence).expect("world builds");

        assert_eq!(a.agreements(), b.agreements(), "seed {seed}: agreements");
        for (ta, tb) in a.terrains().iter().zip(b.terrains()) {
            let origin = pairs
                .iter()
                .find(|(id, ..)| *id == ta.region().id)
                .expect("region exists")
                .1;
            let label = format!("stitched region {} seed {seed}", ta.region().id.0);
            assert_same_land(ta, tb, origin, &label);
        }
    }
}

#[test]
fn catalogue_keys_are_unique_and_named_finds_them() {
    let presets: Vec<_> = BiomePreset::all().collect();
    assert_eq!(presets.len(), TerrainPreset::ALL.len());
    for (i, preset) in presets.iter().enumerate() {
        assert!(
            presets[..i].iter().all(|other| other.key != preset.key),
            "duplicate key {}",
            preset.key
        );
        assert_eq!(BiomePreset::named(&preset.key), Some(*preset));
        assert!(!preset.name.is_empty() && !preset.description.is_empty());
    }
    assert_eq!(BiomePreset::named("plains"), None);
}
