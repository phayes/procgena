//! A World of empty Biomes: Biomes follow the fields and pins, seeds are
//! stable, and the land is the same whatever order it is built in.

use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use glam::{DVec2, IVec2};
use procgena_biome::{Biome, BiomePreset, BiomeTerrain};
use procgena_terrain::{
    ElevationOptions, FeatureTier, HeightField, NoiseLayer, RegionId, RegionKey, RegionProgress,
    TerrainPreset, fixtures,
};
use procgena_world::{
    BiomePin, BiomeRule, BiomeRules, FieldRange, World, WorldError, WorldOptions,
    WorldOptionsError, WorldPreset, WorldScale,
};

/// The regions whose sites are in the `n` × `n` lattice cells around the
/// origin.
fn regions(n: i32) -> Vec<RegionId> {
    let half = n / 2;
    (-half..n - half)
        .flat_map(|x| (-half..n - half).map(move |z| RegionKey(IVec2::new(x, z)).id()))
        .collect()
}

/// A noise layer that adds nothing.
fn off(layer: &NoiseLayer) -> NoiseLayer {
    NoiseLayer {
        height: 0.0,
        ..layer.clone()
    }
}

/// E as a plain tilt: `slope` metres of rise per metre along x.
fn tilted(slope: f64) -> ElevationOptions {
    let e = ElevationOptions::default();
    ElevationOptions {
        continents: off(&e.continents),
        uplift: off(&e.uplift),
        tilt: DVec2::new(slope, 0.0),
        ..e
    }
}

/// A rule giving `biome` wherever all three bounds hold.
fn rule(
    biome: &str,
    elevation: FieldRange,
    ruggedness: FieldRange,
    moisture: FieldRange,
) -> BiomeRule {
    BiomeRule {
        biome: biome.to_owned(),
        elevation,
        ruggedness,
        moisture,
    }
}

/// Plains everywhere, unless pinned.
fn all_plains() -> BiomeRules {
    BiomeRules {
        rules: Vec::new(),
        fallback: "plains".to_owned(),
    }
}

/// A neighbour of `region`: the region across its first edge.
fn neighbour(world: &World, region: RegionId) -> RegionId {
    world
        .terrain()
        .region(region)
        .edges
        .iter()
        .find_map(|edge| edge.neighbor)
        .expect("lattice regions have neighbours")
}

/// Heights on an `n` × `n` grid over the square from `a` to `b`, as bits.
fn height_bits(field: &impl HeightField, a: DVec2, b: DVec2, n: usize) -> Vec<u32> {
    let mut bits = Vec::with_capacity(n * n);
    for i in 0..n {
        for j in 0..n {
            let t = DVec2::new(i as f64, j as f64) / (n - 1) as f64;
            bits.push(field.height(a + (b - a) * t).to_bits());
        }
    }
    bits
}

#[test]
fn biomes_follow_e() {
    // E rises to the east; mountains wherever it is above sea level.
    let options = WorldOptions {
        elevation: tilted(0.01),
        biomes: BiomeRules {
            rules: vec![rule(
                "mountains",
                FieldRange::at_least(0.0),
                FieldRange::ANY,
                FieldRange::ANY,
            )],
            fallback: "plains".to_owned(),
        },
        ..WorldOptions::default()
    };
    let world = World::new(options).expect("valid World");
    let (mut east, mut west) = (0, 0);
    for region in regions(12) {
        let site = world.terrain().site(region);
        let biome = world.biome(region);
        let expected = if site.x >= 0.0 { "mountains" } else { "plains" };
        assert_eq!(biome.preset.key, expected, "region at {site}");
        assert!(!biome.pinned);
        assert_eq!(biome.fields, world.fields(site));
        if site.x >= 0.0 {
            east += 1;
        } else {
            west += 1;
        }
    }
    assert!(east > 20 && west > 20, "{east} east, {west} west");
}

#[test]
fn biomes_follow_the_selector_fields() {
    let options = WorldOptions {
        biomes: BiomeRules {
            rules: vec![
                rule(
                    "mountains",
                    FieldRange::ANY,
                    FieldRange::at_least(0.3),
                    FieldRange::ANY,
                ),
                rule(
                    "rolling_hills",
                    FieldRange::ANY,
                    FieldRange::ANY,
                    FieldRange::at_least(0.3),
                ),
            ],
            fallback: "plains".to_owned(),
        },
        ..WorldOptions::default()
    };
    let world = World::new(options).expect("valid World");
    let mut counts = [0; 3];
    for region in regions(20) {
        let fields = world.fields(world.terrain().site(region));
        let (expected, slot) = if fields.ruggedness >= 0.3 {
            ("mountains", 0)
        } else if fields.moisture >= 0.3 {
            ("rolling_hills", 1)
        } else {
            ("plains", 2)
        };
        assert_eq!(world.biome(region).preset.key, expected, "{fields:?}");
        counts[slot] += 1;
    }
    // Each field decides some regions.
    assert!(counts.iter().all(|&c| c >= 10), "{counts:?}");
}

#[test]
fn neighbours_may_share_a_biome_type_but_not_a_seed() {
    let world = World::new(WorldOptions {
        biomes: all_plains(),
        ..WorldOptions::default()
    })
    .expect("valid World");
    let a = RegionKey(IVec2::ZERO).id();
    let b = neighbour(&world, a);
    let (biome_a, biome_b) = (world.biome(a), world.biome(b));
    assert_eq!(biome_a.preset.key, biome_b.preset.key);
    assert_ne!(biome_a.seed, biome_b.seed);
}

#[test]
fn pinned_regions_keep_their_biome() {
    let options = |seed, rules| WorldOptions {
        seed,
        biomes: rules,
        pins: vec![BiomePin::region(
            RegionKey(IVec2::new(1, -1)).id(),
            "mountains",
        )],
        ..WorldOptions::default()
    };
    let pinned = RegionKey(IVec2::new(1, -1)).id();
    for world in [
        World::new(options(0, all_plains())),
        World::new(options(9, all_plains())),
        World::new(options(0, BiomeRules::default())),
    ] {
        let world = world.expect("valid World");
        let biome = world.biome(pinned);
        assert_eq!(biome.preset.key, "mountains");
        assert!(biome.pinned);
    }

    // In an all-plains World, only the pinned region has mountains, and
    // its land is built from the mountains' Terrain.
    let world = World::new(options(3, all_plains())).expect("valid World");
    for region in regions(6).into_iter().filter(|&r| r != pinned) {
        let biome = world.biome(region);
        assert_eq!(biome.preset.key, "plains");
        assert!(!biome.pinned);
    }
    let terrain = world
        .terrain()
        .terrain_tier(pinned, FeatureTier::Rough)
        .expect("pinned region builds");
    let mut expected = TerrainPreset::mountains().terrain.clone();
    expected.default_seed = 3;
    assert_eq!(terrain.generator().options(), &expected);
}

#[test]
fn a_pin_at_a_point_stays_at_its_place() {
    // Whatever the region size and seed, the region holding the point is
    // the pinned one, and only that region.
    let p = DVec2::new(5100.0, -3300.0);
    for (size, seed) in [(4000.0, 0), (6000.0, 0), (4000.0, 8)] {
        let world = World::new(WorldOptions {
            seed,
            regions: procgena_terrain::RegionGraphOptions {
                size,
                ..WorldOptions::default().regions
            },
            biomes: all_plains(),
            pins: vec![BiomePin::at(p, "mountains")],
            ..WorldOptions::default()
        })
        .expect("valid World");
        let pinned = world.region_at(p);
        let biome = world.biome(pinned);
        assert_eq!(biome.preset.key, "mountains", "size {size}, seed {seed}");
        assert!(biome.pinned);
        let other = neighbour(&world, pinned);
        assert_eq!(world.biome(other).preset.key, "plains");
    }
}

#[test]
fn seeds_are_stable() {
    // Reference values from `procgena_core::region_seed`'s own test, which
    // were worked out outside the crate.
    let world = World::new(WorldOptions {
        seed: 42,
        ..WorldOptions::default()
    })
    .expect("valid World");
    let region = RegionKey(IVec2::new(3, -2)).id();
    assert_eq!(region, RegionId(0x3_ffff_fffe));
    assert_eq!(world.biome_seed(region), 0x2e49_3036);
    assert_eq!(world.terrain_local_seed(region), 0x2d9a_d2c9);
    assert_eq!(world.biome(region).seed, 0x2e49_3036);

    // Only the World seed and the region move them: not the rules, the
    // pins or the scale.
    let other = World::new(WorldOptions {
        seed: 42,
        scale: WorldScale {
            horizontal: 2.0,
            vertical: 1.5,
        },
        biomes: all_plains(),
        pins: vec![BiomePin::region(region, "mountains")],
        ..WorldOptions::default()
    })
    .expect("valid World");
    for r in regions(4) {
        assert_eq!(world.biome_seed(r), other.biome_seed(r));
        assert_eq!(world.terrain_local_seed(r), other.terrain_local_seed(r));
        assert_ne!(world.biome_seed(r), world.terrain_local_seed(r));
    }
}

#[test]
fn the_same_land_in_any_region_build_order() {
    let options = WorldOptions {
        seed: 11,
        ..WorldOptions::default()
    };
    let a = World::new(options.clone()).expect("valid World");
    let b = World::new(options).expect("valid World");
    let first = RegionKey(IVec2::ZERO).id();
    let second = neighbour(&a, first);
    let (p, q) = (a.terrain().site(first), a.terrain().site(second));

    // `a` builds as it samples, from the first region; `b` builds the second
    // region first, then both at once on two threads.
    let cold = height_bits(a.terrain().as_ref(), p, q, 41);
    b.terrain().terrain(second).expect("region builds");
    std::thread::scope(|s| {
        for region in [second, first] {
            let b = &b;
            s.spawn(move || b.terrain().terrain(region).expect("region builds"));
        }
    });
    let warm = height_bits(b.terrain().as_ref(), p, q, 41);
    assert_eq!(cold, warm);
    assert!(cold.iter().all(|&h| f32::from_bits(h).is_finite()));
}

#[test]
fn the_land_is_shared_and_its_builds_are_told() {
    // A World that counts the region stages it starts and finishes.
    let told = Arc::new(Mutex::new(Vec::new()));
    let hook = {
        let told = Arc::clone(&told);
        Arc::new(move |progress: &RegionProgress| told.lock().unwrap().push(*progress))
    };
    let world = World::new(WorldOptions::default())
        .expect("valid World")
        .with_progress(hook);

    // A clone of its land builds the same regions it does.
    let shared = Arc::clone(world.terrain());
    let region = RegionKey(IVec2::ZERO).id();
    shared.terrain(region).expect("region builds");
    assert!(Arc::ptr_eq(&shared, world.terrain()));
    let p = world.terrain().site(region);
    assert_eq!(
        world.terrain().height(p).to_bits(),
        shared.height(p).to_bits()
    );

    // Every stage of that region was told starting and then finishing, and
    // building it again tells nothing more.
    let stages = |told: &[RegionProgress]| {
        let mut started = Vec::new();
        let mut finished = Vec::new();
        for progress in told {
            match *progress {
                RegionProgress::Started { region: r, stage } if r == region => started.push(stage),
                RegionProgress::Finished {
                    region: r,
                    stage,
                    built,
                    ..
                } if r == region => {
                    assert!(built, "{stage:?} built");
                    finished.push(stage);
                }
                _ => {}
            }
        }
        (started, finished)
    };
    let (started, finished) = stages(&told.lock().unwrap());
    assert!(!started.is_empty());
    let mut sorted_started = started.clone();
    sorted_started.sort();
    let mut sorted_finished = finished.clone();
    sorted_finished.sort();
    assert_eq!(sorted_started, sorted_finished);
    let count = told.lock().unwrap().len();
    world.terrain().terrain(region).expect("region builds");
    assert_eq!(told.lock().unwrap().len(), count);
}

#[test]
#[should_panic(expected = "before the land is shared")]
fn progress_is_set_before_the_land_is_shared() {
    let world = World::new(WorldOptions::default()).expect("valid World");
    let _shared = Arc::clone(world.terrain());
    let _ = world.with_progress(Arc::new(|_: &RegionProgress| {}));
}

#[test]
fn one_terrain_preset_gives_each_region_its_own_land() {
    // Plains everywhere, and a second Biome on the plains' Terrain.
    let steppe = BiomePreset {
        key: Cow::Borrowed("steppe"),
        name: Cow::Borrowed("Steppe"),
        description: Cow::Borrowed("Plains under another name."),
        group: Cow::Borrowed("Test"),
        biome: Biome::empty(BiomeTerrain::Preset(Cow::Borrowed("plains"))),
        default_seed: 0,
    };
    let mut biomes: Vec<BiomePreset> = BiomePreset::all().cloned().collect();
    biomes.push(steppe);
    let region = RegionKey(IVec2::ZERO).id();
    let world = |pin: &str| {
        World::with_biomes(
            WorldOptions {
                seed: 5,
                biomes: all_plains(),
                pins: vec![BiomePin::region(region, pin)],
                ..WorldOptions::default()
            },
            biomes.clone(),
        )
        .expect("valid World")
    };
    let plains = world("plains");
    let steppe = world("steppe");
    assert_eq!(steppe.biome(region).preset.key, "steppe");

    // Two regions on one preset have the same settings, World seed
    // included, but different land.
    let other = neighbour(&plains, region);
    let here = plains
        .terrain()
        .terrain_tier(region, FeatureTier::Rough)
        .expect("builds");
    let there = plains
        .terrain()
        .terrain_tier(other, FeatureTier::Rough)
        .expect("builds");
    assert_eq!(here.generator().options(), there.generator().options());
    assert_eq!(here.generator().options().default_seed, 5);
    let site = plains.terrain().site(region);
    let offset = plains.terrain().site(other) - site;
    let (a, b) = (site - DVec2::splat(300.0), site + DVec2::splat(300.0));
    assert_ne!(
        height_bits(here.as_ref(), a, b, 11),
        height_bits(there.as_ref(), a + offset, b + offset, 11)
    );

    // Switching the region to another Biome on the same Terrain keeps its
    // land exactly.
    let switched = steppe
        .terrain()
        .terrain_tier(region, FeatureTier::Rough)
        .expect("builds");
    assert_eq!(
        height_bits(here.as_ref(), a, b, 11),
        height_bits(switched.as_ref(), a, b, 11)
    );
}

#[test]
fn scale_stretches_e_and_the_fields() {
    // A tilt besides the layers, with a slope limit steep enough for every
    // scale below.
    let elevation = ElevationOptions {
        max_slope: 0.3,
        tilt: DVec2::new(0.004, -0.002),
        ..ElevationOptions::default()
    };
    let bump = procgena_terrain::HeightBump {
        centre: DVec2::new(3000.0, -2000.0),
        radius: 5000.0,
        height: 60.0,
    };
    let with_bumps = |horizontal, vertical, bumps: Vec<procgena_terrain::HeightBump>| {
        World::new(WorldOptions {
            elevation: ElevationOptions {
                bumps,
                ..elevation.clone()
            },
            scale: WorldScale {
                horizontal,
                vertical,
            },
            ..WorldOptions::default()
        })
        .expect("valid World")
    };
    let at = |horizontal, vertical| with_bumps(horizontal, vertical, Vec::new());
    let one = at(1.0, 1.0);
    let scaled = at(2.0, 3.0);
    for i in 0..50 {
        let p = DVec2::new(
            f64::from(i) * 731.0 - 9000.0,
            f64::from(i * i) * 13.0 - 4000.0,
        );
        let (a, b) = (one.fields(p), scaled.fields(p * 2.0));
        assert_eq!(a.ruggedness, b.ruggedness, "at {p}");
        assert_eq!(a.moisture, b.moisture, "at {p}");
        let e = f64::from(a.elevation) * 3.0;
        assert!(
            (f64::from(b.elevation) - e).abs() <= 1e-3 * e.abs().max(1.0),
            "at {p}: {} against {e}",
            b.elevation
        );
    }

    // A bump is placed by hand in real metres: at any scale it adds its
    // full height at its centre, and nothing beyond its radius.
    let bumped = with_bumps(2.0, 3.0, vec![bump]);
    let rise = |p: DVec2| bumped.fields(p).elevation - scaled.fields(p).elevation;
    assert!((rise(bump.centre) - bump.height).abs() < 1e-3);
    let outside = bump.centre + DVec2::new(bump.radius + 1.0, 0.0);
    assert_eq!(rise(outside), 0.0);

    // The rules' elevation bounds follow the vertical scale, so a World
    // made only taller picks the same Biomes.
    let taller = at(1.0, 2.0);
    for region in regions(10) {
        assert_eq!(
            one.biome(region).preset.key,
            taller.biome(region).preset.key
        );
    }
}

#[test]
fn every_scale_class_builds_a_world() {
    for preset in WorldPreset::all() {
        let world =
            World::new(preset.world.clone()).unwrap_or_else(|e| panic!("{}: {e}", preset.key));
        let biome = world.biome_at(DVec2::new(100.0, 200.0));
        assert!(!biome.pinned, "{}", preset.key);
    }
    assert_eq!(WorldPreset::medium_small().world, WorldOptions::default());
}

#[test]
fn the_default_is_valheim_sized_and_medium_keeps_the_old_land() {
    // The default World builds E at Valheim scale, once: 200 m continents
    // 75 km apart and 60 m uplands 20 km apart, on 2 km regions.
    let default = WorldOptions::default();
    let e = default.scaled_elevation();
    assert_eq!(e, ElevationOptions::default());
    assert_eq!(
        (e.continents.height, e.continents.wavelength),
        (200.0, 75_000.0)
    );
    assert_eq!((e.uplift.height, e.uplift.wavelength), (60.0, 20_000.0));
    assert_eq!(default.regions.size, 2000.0);

    // The medium class at scale 2 gives exactly the E, selector fields and
    // rules of the earlier medium-scale default.
    let medium = &WorldPreset::medium().world;
    assert_eq!(medium.scaled_elevation(), fixtures::elevation_medium());
    assert_eq!(medium.regions, fixtures::regions_medium());
    let horizontal = medium.scale.horizontal;
    let fields = &medium.fields;
    assert_eq!(
        f64::from(fields.ruggedness.wavelength) * horizontal,
        12_000.0
    );
    assert_eq!(f64::from(fields.moisture.wavelength) * horizontal, 20_000.0);
    let lowest = medium.biomes.rules[0]
        .elevation
        .min
        .expect("mountains need high land");
    assert_eq!(f64::from(lowest) * medium.scale.vertical, 150.0);
}

#[test]
fn world_files_round_trip() {
    // A pin on a region west of the origin has an id above `i64::MAX`.
    let region = RegionKey(IVec2::new(-3, 2)).id();
    assert!(region.0 > i64::MAX as u64);
    let mut preset = WorldPreset::large().clone();
    preset.key = Cow::Borrowed("mine");
    preset.world.seed = 77;
    preset
        .world
        .pins
        .push(BiomePin::region(region, "mountains"));
    preset
        .world
        .pins
        .push(BiomePin::at(DVec2::new(-1234.5, 678.0), "rolling_hills"));
    // A bump and a tilt, which are written as `[x, z]`.
    preset.world.elevation.tilt = DVec2::new(0.001, -0.002);
    preset
        .world
        .elevation
        .bumps
        .push(procgena_terrain::HeightBump {
            centre: DVec2::new(-500.0, 2500.0),
            radius: 4000.0,
            height: 40.0,
        });
    preset.world.biomes.rules[0].moisture = FieldRange {
        min: Some(-0.5),
        max: Some(0.75),
    };
    let text = preset.to_yaml(Some(WorldPreset::large()));
    let read = WorldPreset::from_yaml("mine", &text).expect("written file reads back");
    assert_eq!(read.world, preset.world, "{text}");

    // A file changes one value and keeps the rest of its base.
    let read = WorldPreset::from_yaml(
        "wider",
        "base: medium_small\nworld:\n  scale:\n    horizontal: 2.0\n",
    )
    .expect("valid file");
    assert_eq!(read.world.scale.horizontal, 2.0);
    assert_eq!(read.world.scale.vertical, 1.0);
    assert_eq!(read.world.regions, WorldOptions::default().regions);

    // A file that makes E too steep is refused.
    let steep = WorldPreset::from_yaml(
        "taller",
        "base: medium_small\nworld:\n  scale:\n    vertical: 2.0\n",
    );
    assert!(steep.is_err());
}

#[test]
fn bad_settings_are_refused() {
    let refused = |options: WorldOptions| World::new(options).map(|_| ()).unwrap_err();
    assert!(matches!(
        refused(WorldOptions {
            scale: WorldScale {
                horizontal: f64::NAN,
                vertical: 1.0,
            },
            ..WorldOptions::default()
        }),
        WorldError::Options(WorldOptionsError::Scale(_))
    ));
    assert_eq!(
        refused(WorldOptions {
            biomes: BiomeRules {
                rules: Vec::new(),
                fallback: "Plains".to_owned(),
            },
            ..WorldOptions::default()
        }),
        WorldError::UnknownBiome("Plains".to_owned())
    );
    // Two pins on one region, one by id and one by a point inside it.
    let region = RegionKey(IVec2::ZERO).id();
    let site = World::new(WorldOptions::default())
        .expect("valid World")
        .terrain()
        .site(region);
    assert_eq!(
        refused(WorldOptions {
            pins: vec![
                BiomePin::region(region, "plains"),
                BiomePin::at(site, "mountains"),
            ],
            ..WorldOptions::default()
        }),
        WorldError::DuplicatePin(region)
    );
    // A pin must give a point or an id, not both or neither.
    let both = BiomePin {
        region: Some(4),
        ..BiomePin::at(DVec2::ZERO, "plains")
    };
    let neither = BiomePin {
        at: None,
        region: None,
        biome: "plains".to_owned(),
    };
    for pin in [both, neither] {
        assert_eq!(
            refused(WorldOptions {
                pins: vec![BiomePin::region(region, "plains"), pin],
                ..WorldOptions::default()
            }),
            WorldError::Options(WorldOptionsError::Pin(1))
        );
    }
    let mut rules = BiomeRules::default();
    rules.rules[1].elevation = FieldRange {
        min: Some(10.0),
        max: Some(5.0),
    };
    assert_eq!(
        refused(WorldOptions {
            biomes: rules,
            ..WorldOptions::default()
        }),
        WorldError::Options(WorldOptionsError::Rule(1))
    );
    // E made steeper than its limit by the scale.
    assert!(matches!(
        refused(WorldOptions {
            scale: WorldScale {
                horizontal: 1.0,
                vertical: 3.0,
            },
            ..WorldOptions::default()
        }),
        WorldError::Options(WorldOptionsError::Elevation(_))
    ));
    let twice: Vec<BiomePreset> = vec![BiomePreset::PLAINS, BiomePreset::PLAINS];
    assert_eq!(
        World::with_biomes(WorldOptions::default(), twice)
            .map(|_| ())
            .unwrap_err(),
        WorldError::DuplicateBiome("plains".to_owned())
    );
}

#[test]
fn the_ground_mesh_setting_reaches_the_terrain_and_round_trips() {
    use procgena_terrain::ChunkSource;
    use procgena_terrain::tin::{DomainTolerance, TinTolerance};
    use procgena_world::{GroundMesh, GroundMeshLevel};

    // A World's ground mesh is built within its own level, with its own
    // error for open land.
    let ground_mesh = GroundMesh {
        level: GroundMeshLevel::LowPoly,
        error: Some(1.5),
    };
    let world = World::new(WorldOptions {
        ground_mesh,
        ..WorldOptions::default()
    })
    .unwrap();
    let expected = TinTolerance {
        general: DomainTolerance {
            height: 1.5,
            ..TinTolerance::LOW_POLY.general
        },
        ..TinTolerance::LOW_POLY
    };
    assert_eq!(world.terrain().tin_tolerance(), expected);
    assert_eq!(
        World::new(WorldOptions::default())
            .unwrap()
            .terrain()
            .tin_tolerance(),
        TinTolerance::GENERAL
    );

    // Written to a file and read back.
    let mut preset = WorldPreset::large().clone();
    preset.key = Cow::Borrowed("mine");
    preset.world.ground_mesh = ground_mesh;
    let text = preset.to_yaml(Some(WorldPreset::large()));
    let read = WorldPreset::from_yaml("mine", &text).expect("written file reads back");
    assert_eq!(read.world.ground_mesh, ground_mesh);

    // An error that isn't positive is refused.
    let bad = WorldOptions {
        ground_mesh: GroundMesh {
            error: Some(0.0),
            ..GroundMesh::default()
        },
        ..WorldOptions::default()
    };
    assert_eq!(
        World::new(bad).map(|_| ()).unwrap_err(),
        WorldError::Options(WorldOptionsError::GroundMeshError(0.0))
    );
}
