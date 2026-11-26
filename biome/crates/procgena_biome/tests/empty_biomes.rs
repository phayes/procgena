//! Empty Biomes: every Terrain preset has one, and building it gives the
//! same land as building the Terrain on its own.

use glam::DVec2;
use procgena_biome::{Biome, BiomePreset, BiomeTerrain};
use procgena_terrain::{
    Boundary, Convergence, Elevation, Region, RegionId, RegionSpec, StitchedWorld, Terrain,
    TerrainGenerator, TerrainOptions, TerrainPreset,
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

/// The river sources per km² each shipped Terrain preset was tuned for.
/// The presets ship with rivers and roads switched off.
const RIVER_DENSITIES: [(&str, f32); 3] =
    [("plains", 0.6), ("rolling_hills", 1.0), ("mountains", 1.2)];

/// `terrain`'s settings with its rivers and roads switched back on.
fn with_features(terrain: &TerrainPreset) -> TerrainOptions {
    let (_, density) = RIVER_DENSITIES
        .iter()
        .find(|(key, _)| *key == terrain.key)
        .expect("every shipped preset has a river density");
    let mut options = terrain.terrain.clone();
    options.features.rivers.density = *density;
    options.features.roads.enabled = true;
    options
}

/// The Terrain built straight from the terrain crate, with `options` and
/// the World seed `world_seed`, which every Terrain in a World takes.
fn reference(options: TerrainOptions, world_seed: u32) -> Terrain {
    let options = TerrainOptions {
        default_seed: world_seed,
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
    for terrain in TerrainPreset::all() {
        let preset = BiomePreset::bare(&terrain.key)
            .unwrap_or_else(|| panic!("{} has no empty Biome", terrain.name));
        assert_eq!(
            preset.biome.terrain,
            BiomeTerrain::Preset(terrain.key.clone())
        );
        assert!(preset.biome.is_empty(), "{}", preset.key);
        assert_eq!(preset.default_seed, 0, "{}", preset.key);
    }
}

#[test]
fn empty_biome_builds_the_same_land_as_its_terrain() {
    let mut features = 0;
    let mut wet_points = 0;
    for terrain in TerrainPreset::all() {
        let preset = BiomePreset::bare(&terrain.key).expect("every preset has an empty Biome");
        for seed in SEEDS {
            let label = format!("{} seed {seed}", preset.key);
            let built = preset
                .biome
                .build(seed, region(), &[])
                .expect("Biome builds");
            let alone = reference(terrain.terrain.clone(), seed);
            assert_same_land(&built, &alone, DVec2::ZERO, &label);

            // The same with the preset's rivers and roads switched on.
            let options = with_features(terrain);
            let built = Biome::empty(BiomeTerrain::Inline(options.clone()))
                .build(seed, region(), &[])
                .expect("Biome builds");
            let alone = reference(options, seed);
            let label = format!("{label} with rivers and roads");
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
    for terrain in TerrainPreset::all() {
        // An inline seed is replaced by the World's.
        let inline = Biome::empty(BiomeTerrain::Inline(TerrainOptions {
            default_seed: 12345,
            ..terrain.terrain.clone()
        }));
        for seed in SEEDS {
            let label = format!("inline {} seed {seed}", terrain.name);
            let resolved = inline
                .terrain_options(seed)
                .expect("inline settings resolve");
            assert_eq!(resolved.default_seed, seed);
            let built = inline.build(seed, region(), &[]).expect("Biome builds");
            let alone = reference(terrain.terrain.clone(), seed);
            assert_same_land(&built, &alone, DVec2::ZERO, &label);
        }
    }
}

#[test]
fn invalid_inline_terrain_is_an_error() {
    let mut options = TerrainPreset::plains().terrain.clone();
    options.base.erosion = f32::NAN;
    let biome = Biome::empty(BiomeTerrain::Inline(options));
    assert!(matches!(
        biome.build(0, region(), &[]),
        Err(procgena_biome::BiomeError::Options(_))
    ));
}

#[test]
fn unknown_terrain_preset_is_an_error() {
    let biome = Biome::empty(BiomeTerrain::Preset("PLAINS".into()));
    let unknown = |result| matches!(result, Err(procgena_biome::BiomeError::UnknownTerrainPreset(key)) if key == "PLAINS");
    assert!(unknown(biome.terrain_options(0).map(|_| ())));
    assert!(unknown(biome.build(0, region(), &[]).map(|_| ())));
}

#[test]
fn stitched_biomes_build_the_same_world_as_stitched_terrains() {
    // Plains to the west of mountains, as a World of bare land will build them.
    let west = DVec2::new(-SIDE, 0.0);
    let pairs = [
        (RegionId(1), west, TerrainPreset::plains()),
        (RegionId(2), DVec2::ZERO, TerrainPreset::mountains()),
    ];
    for seed in SEEDS {
        let from_biomes = pairs.map(|(id, origin, terrain)| RegionSpec {
            id,
            polygon: square(origin),
            options: BiomePreset::bare(&terrain.key)
                .expect("every preset has an empty Biome")
                .biome
                .terrain_options(seed)
                .expect("built-in Terrain preset"),
        });
        let from_terrains = pairs.map(|(id, origin, terrain)| RegionSpec {
            id,
            polygon: square(origin),
            options: TerrainOptions {
                default_seed: seed,
                ..terrain.terrain.clone()
            },
        });
        let convergence = Convergence::default();
        let elevation = Elevation::flat();
        let a = StitchedWorld::build(from_biomes.to_vec(), &convergence, &elevation)
            .expect("world builds");
        let b = StitchedWorld::build(from_terrains.to_vec(), &convergence, &elevation)
            .expect("world builds");

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
    assert_eq!(presets.len(), TerrainPreset::all().count());
    for (i, preset) in presets.iter().enumerate() {
        assert!(
            presets[..i].iter().all(|other| other.key != preset.key),
            "duplicate key {}",
            preset.key
        );
        assert_eq!(BiomePreset::named(&preset.key), Some(*preset));
        assert!(!preset.name.is_empty() && !preset.description.is_empty());
    }
    assert_eq!(BiomePreset::named("PLAINS"), None);
}

/// Paints chunk `key`, requesting the sampling detail required by `painter`.
fn paint(
    painter: &procgena_terrain::ground::GroundStack,
    terrain: &Terrain,
    key: procgena_terrain::ChunkKey,
) -> procgena_terrain::ground::GroundWeights {
    use procgena_terrain::FeatureTier;
    use procgena_terrain::ground::GroundWeights;
    GroundWeights::chunk(painter, terrain, key, FeatureTier::Fine).expect("ground in the region")
}

#[test]
fn an_empty_biomes_base_ground_replaces_the_bare_look() {
    use std::sync::Arc;

    use procgena_biome::{BiomeGround, biome_seed, standard_base_layers};
    use procgena_terrain::ChunkKey;
    use procgena_terrain::ground::{GroundStack, MaterialCatalogue, SiteDetail, bare_layers};

    let catalogue = Arc::new(MaterialCatalogue::standard());
    let bare = GroundStack::bare(0, 1.0);
    let mud = catalogue.id("mud").unwrap();
    let gravel = catalogue.id("gravel_road").unwrap();
    // Empty Biome presets still provide standard base materials, including
    // mud near water, gravel shoulders and labelled cliff rock.
    for preset in BiomePreset::BARE_LAND {
        assert_eq!(preset.biome.ground, BiomeGround::Standard, "{}", preset.key);
        let seed = biome_seed(preset.default_seed, RegionId(1));
        let base = preset.biome.ground(catalogue.clone(), seed, 1.0).unwrap();
        assert_eq!(base.detail(), SiteDetail::Labels);
        let keys: Vec<_> = base.layers().map(|(k, _)| k.clone()).collect();
        assert!(bare_layers().keys().all(|k| keys.contains(k)), "{keys:?}");
        assert_eq!(keys.len(), standard_base_layers().len());
    }

    // Adjacent plains and rolling hills, with their rivers and roads switched
    // on, provide rivers and connecting roads. Compared with bare terrain,
    // the Biome base should add mud near rivers and increase gravel
    // coverage around roads.
    let pairs = [
        (RegionId(1), DVec2::new(-SIDE, 0.0), BiomePreset::PLAINS),
        (RegionId(2), DVec2::ZERO, BiomePreset::ROLLING_HILLS),
    ];
    let specs = pairs.clone().map(|(id, origin, preset)| {
        let BiomeTerrain::Preset(key) = &preset.biome.terrain else {
            panic!("{} names a Terrain preset", preset.key);
        };
        let terrain = TerrainPreset::named(key).expect("a shipped Terrain preset");
        RegionSpec {
            id,
            polygon: square(origin),
            options: with_features(terrain),
        }
    });
    let world = StitchedWorld::build(specs.to_vec(), &Convergence::default(), &Elevation::flat())
        .expect("world builds");
    let (mut rivers, mut roads) = (0, 0);
    for terrain in world.terrains() {
        let id = terrain.region().id;
        let (_, _, preset) = pairs.iter().find(|(i, ..)| *i == id).unwrap();
        let base = preset
            .biome
            .ground(catalogue.clone(), biome_seed(0, id), 1.0)
            .unwrap();
        for river in terrain.rivers() {
            let key = ChunkKey::containing(river.points[river.points.len() / 2], 0);
            let (on_base, on_bare) = (paint(&base, terrain, key), paint(&bare, terrain, key));
            assert!(
                on_base.palette().contains(&mud),
                "region {id:?}: mud by the river"
            );
            assert!(!on_bare.palette().contains(&mud), "region {id:?}");
            rivers += 1;
        }
        for road in terrain.roads() {
            let key = ChunkKey::containing(road.points[road.points.len() / 2], 0);
            let (on_base, on_bare) = (paint(&base, terrain, key), paint(&bare, terrain, key));
            let total = |w: &procgena_terrain::ground::GroundWeights| -> u64 {
                (0..w.size().y)
                    .flat_map(|j| (0..w.size().x).map(move |i| (i, j)))
                    .map(|(i, j)| u64::from(w.weight(i, j, gravel)))
                    .sum()
            };
            assert!(
                total(&on_base) > total(&on_bare),
                "region {id:?}: gravel shoulders"
            );
            roads += 1;
        }
    }
    assert!(rivers > 0 && roads > 0, "{rivers} rivers and {roads} roads");

    // Custom layers replace the standard set. Supplying only bare layers
    // should reproduce the bare terrain's materials.
    let plain = Biome {
        ground: BiomeGround::Layers(bare_layers()),
        ..BiomePreset::PLAINS.biome
    };
    let base = plain.ground(catalogue.clone(), 5, 1.0).unwrap();
    let terrain = plain.build(0, region(), &[]).unwrap();
    let key = ChunkKey::containing(DVec2::splat(SIDE / 2.0), 1);
    assert_eq!(paint(&base, &terrain, key), paint(&bare, &terrain, key));
}
