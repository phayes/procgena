//! A World of empty Biomes: Biomes follow the fields and pins, seeds are
//! stable, and the land is the same whatever order it is built in.

use std::borrow::Cow;

use glam::{DVec2, IVec2};
use procgena_biome::{Biome, BiomePreset, BiomeTerrain};
use procgena_terrain::{
    ElevationOptions, FeatureTier, HeightField, NoiseLayer, RegionId, RegionKey, TerrainPreset,
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
        pins: vec![BiomePin {
            region: RegionKey(IVec2::new(1, -1)).id().0,
            biome: "mountains".to_owned(),
        }],
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
        pins: vec![BiomePin {
            region: region.0,
            biome: "mountains".to_owned(),
        }],
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
    let cold = height_bits(a.terrain(), p, q, 41);
    b.terrain().terrain(second).expect("region builds");
    std::thread::scope(|s| {
        for region in [second, first] {
            let b = &b;
            s.spawn(move || b.terrain().terrain(region).expect("region builds"));
        }
    });
    let warm = height_bits(b.terrain(), p, q, 41);
    assert_eq!(cold, warm);
    assert!(cold.iter().all(|&h| f32::from_bits(h).is_finite()));
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
                pins: vec![BiomePin {
                    region: region.0,
                    biome: pin.to_owned(),
                }],
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
    // A tilt and a bump besides the layers, with a slope limit steep
    // enough for every scale below.
    let elevation = ElevationOptions {
        max_slope: 0.3,
        tilt: DVec2::new(0.004, -0.002),
        bumps: vec![procgena_terrain::HeightBump {
            centre: DVec2::new(3000.0, -2000.0),
            radius: 5000.0,
            height: 60.0,
        }],
        ..ElevationOptions::default()
    };
    let at = |horizontal, vertical| {
        World::new(WorldOptions {
            elevation: elevation.clone(),
            scale: WorldScale {
                horizontal,
                vertical,
            },
            ..WorldOptions::default()
        })
        .expect("valid World")
    };
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
    assert_eq!(WorldPreset::medium().world, WorldOptions::default());
}

#[test]
fn world_files_round_trip() {
    // A pin on a region west of the origin has an id above `i64::MAX`.
    let region = RegionKey(IVec2::new(-3, 2)).id();
    assert!(region.0 > i64::MAX as u64);
    let mut preset = WorldPreset::large().clone();
    preset.key = Cow::Borrowed("mine");
    preset.world.seed = 77;
    preset.world.pins.push(BiomePin {
        region: region.0,
        biome: "mountains".to_owned(),
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
        "base: medium\nworld:\n  scale:\n    horizontal: 2.0\n",
    )
    .expect("valid file");
    assert_eq!(read.world.scale.horizontal, 2.0);
    assert_eq!(read.world.scale.vertical, 1.0);
    assert_eq!(read.world.regions, WorldOptions::default().regions);

    // A file that makes E too steep is refused.
    let steep = WorldPreset::from_yaml(
        "taller",
        "base: medium\nworld:\n  scale:\n    vertical: 2.0\n",
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
    let pin = BiomePin {
        region: 4,
        biome: "plains".to_owned(),
    };
    assert_eq!(
        refused(WorldOptions {
            pins: vec![pin.clone(), pin],
            ..WorldOptions::default()
        }),
        WorldError::Options(WorldOptionsError::DuplicatePin(4))
    );
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
