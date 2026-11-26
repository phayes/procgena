//! Weighted Biome selection in a World: it agrees with the land, checks its
//! settings, reads and writes `.world.yml`, and leaves rule-based Worlds
//! alone.

use std::borrow::Cow;
use std::collections::BTreeMap;

use glam::IVec2;
use procgena_biome::{Biome, BiomePreset, BiomeTerrain};
use procgena_terrain::{FeatureTier, RegionId, RegionKey};
use procgena_world::{
    BiomePin, BiomeSelectionOptions, BiomeSuitability, CompatibilityPair, FieldPreference,
    SelectionError, World, WorldError, WorldOptions, WorldOptionsError, WorldPreset,
};

fn key(x: i32, z: i32) -> RegionId {
    RegionKey(IVec2::new(x, z)).id()
}

/// The default World with the default weighted selection.
fn weighted(seed: u32) -> WorldOptions {
    WorldOptions {
        seed,
        biome_selection: Some(BiomeSelectionOptions::default()),
        ..WorldOptions::default()
    }
}

/// The regions of an `n` × `n` patch of lattice cells from the origin.
fn patch(n: i32) -> Vec<RegionId> {
    (0..n)
        .flat_map(|z| (0..n).map(move |x| key(x, z)))
        .collect()
}

#[test]
fn every_query_gives_the_same_biome() {
    let world = World::new(weighted(4)).expect("valid World");
    let ids = patch(8);
    let many = world.biomes(&ids);
    let mut counts = BTreeMap::new();
    for (&id, batch) in ids.iter().zip(&many) {
        let one = world.biome(id);
        assert_eq!(one.preset.key, batch.preset.key);
        assert_eq!(one.fields, batch.fields);
        assert_eq!(one.seed, batch.seed);
        assert!(!one.pinned);
        let at = world.biome_at(world.terrain().site(id));
        assert_eq!(at.region, id);
        assert_eq!(at.preset.key, one.preset.key);
        *counts.entry(one.preset.key.to_string()).or_insert(0) += 1;
    }
    // The default settings use more than one Biome over 64 regions.
    assert!(counts.len() > 1, "{counts:?}");
}

#[test]
fn the_land_takes_the_selected_biomes_terrain_and_joins_at_borders() {
    let seed = 12;
    let world = World::new(weighted(seed)).expect("valid World");

    // Two neighbouring regions with different Biomes.
    let ids = patch(10);
    let biomes = world.biomes(&ids);
    let (a, b) = ids
        .iter()
        .zip(&biomes)
        .find_map(|(&id, biome)| {
            world.terrain().region(id).edges.iter().find_map(|edge| {
                let n = edge.neighbor?;
                (world.biome(n).preset.key != biome.preset.key).then_some((id, n))
            })
        })
        .expect("some neighbours differ");

    // Each region's land is built from its Biome's Terrain.
    for region in [a, b] {
        let terrain = world
            .terrain()
            .terrain_tier(region, FeatureTier::Rough)
            .expect("region builds");
        let preset = world.biome(region).preset;
        let mut expected = preset.biome.terrain_options(seed).expect("resolves");
        expected.default_seed = seed;
        assert_eq!(terrain.generator().options(), &expected, "{}", preset.key);
    }

    // The land is continuous across their shared border.
    let edge = world
        .terrain()
        .region(a)
        .edges
        .iter()
        .find(|e| e.neighbor == Some(b))
        .expect("a and b share an edge")
        .clone();
    for pair in edge.polyline.windows(2) {
        let p = (pair[0] + pair[1]) / 2.0;
        let across = (pair[1] - pair[0]).perp().normalize() * 0.05;
        let (h0, h1) = (
            world.terrain().height(p - across),
            world.terrain().height(p + across),
        );
        assert!((h0 - h1).abs() < 0.5, "{h0} vs {h1} at {p}");
    }
}

#[test]
fn the_catalogue_order_doesnt_matter() {
    let ids = patch(6);
    let forward = World::new(weighted(2)).expect("valid World");
    let mut reversed: Vec<BiomePreset> = BiomePreset::all().cloned().collect();
    reversed.reverse();
    let backward = World::with_biomes(weighted(2), reversed).expect("valid World");
    for (x, y) in forward.biomes(&ids).iter().zip(backward.biomes(&ids)) {
        assert_eq!(x.preset.key, y.preset.key);
    }
}

#[test]
fn a_pin_may_name_a_biome_that_is_never_drawn() {
    // A fourth Biome, on the plains' Terrain, that only a pin uses.
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
    let pinned = key(2, 3);
    let world = World::with_biomes(
        WorldOptions {
            pins: vec![BiomePin::region(pinned, "steppe")],
            ..weighted(6)
        },
        biomes,
    )
    .expect("valid World");
    let pin = world.biome(pinned);
    assert!(pin.pinned);
    assert_eq!(pin.preset.key, "steppe");
    for biome in world.biomes(&patch(8)) {
        if biome.region != pinned {
            assert_ne!(biome.preset.key, "steppe");
        }
    }
}

#[test]
fn pins_hold_in_weighted_worlds() {
    let pinned = key(-1, 4);
    let world = World::new(WorldOptions {
        pins: vec![BiomePin::region(pinned, "plains")],
        biome_selection: Some(BiomeSelectionOptions {
            suitability: BTreeMap::from([("mountains".to_owned(), BiomeSuitability::default())]),
            ..BiomeSelectionOptions::default()
        }),
        ..WorldOptions::default()
    })
    .expect("valid World");
    assert_eq!(world.biome(pinned).preset.key, "plains");
    assert!(world.biome(pinned).pinned);
    assert_eq!(world.biome(key(0, 4)).preset.key, "mountains");
}

#[test]
fn every_scale_class_builds_a_weighted_world() {
    for preset in WorldPreset::all() {
        let world = World::new(WorldOptions {
            biome_selection: Some(BiomeSelectionOptions::default()),
            ..preset.world.clone()
        })
        .expect("valid World");
        assert_eq!(world.biomes(&patch(3)).len(), 9);
    }
}

#[test]
fn bad_selection_settings_are_refused() {
    let refused = |change: &dyn Fn(&mut BiomeSelectionOptions)| {
        let mut selection = BiomeSelectionOptions::default();
        change(&mut selection);
        World::new(WorldOptions {
            biome_selection: Some(selection),
            ..WorldOptions::default()
        })
        .map(|_| ())
        .unwrap_err()
    };
    let selection = |e| WorldError::Options(WorldOptionsError::Selection(e));
    assert_eq!(
        refused(&|s| s.version = 2),
        selection(SelectionError::Version(2))
    );
    assert_eq!(
        refused(&|s| s.iterations = 9),
        selection(SelectionError::Iterations(9))
    );
    assert_eq!(
        refused(&|s| s.temperature = 0.0),
        selection(SelectionError::Temperature(0.0))
    );
    assert!(matches!(
        refused(&|s| s.neighbour_strength = f64::NAN),
        WorldError::Options(WorldOptionsError::Selection(
            SelectionError::NeighbourStrength(_)
        ))
    ));
    assert_eq!(
        refused(&|s| s.neighbour_strength = -1.0),
        selection(SelectionError::NeighbourStrength(-1.0))
    );
    assert_eq!(
        refused(&|s| s.suitability.clear()),
        selection(SelectionError::NoCandidates)
    );
    assert_eq!(
        refused(&|s| s.suitability.get_mut("plains").unwrap().prevalence = 0.0),
        selection(SelectionError::Prevalence("plains".to_owned()))
    );
    assert_eq!(
        refused(&|s| {
            s.suitability.get_mut("mountains").unwrap().moisture = Some(FieldPreference {
                centre: 0.0,
                spread: 0.0,
            })
        }),
        selection(SelectionError::Preference(
            "mountains".to_owned(),
            "moisture"
        ))
    );
    assert_eq!(
        refused(&|s| s.compatibility.default = f64::INFINITY),
        selection(SelectionError::DefaultWeight(f64::INFINITY))
    );
    assert_eq!(
        refused(&|s| s.compatibility.pairs[2].weight = 0.0),
        selection(SelectionError::PairWeight(2))
    );
    // A pair repeated the other way round, even with the same weight.
    assert_eq!(
        refused(&|s| s.compatibility.pairs.push(CompatibilityPair {
            a: "rolling_hills".to_owned(),
            b: "plains".to_owned(),
            weight: 0.9,
        })),
        selection(SelectionError::DuplicatePair(6))
    );
    // Unknown keys, as candidates or in pairs.
    assert_eq!(
        refused(&|s| {
            s.suitability
                .insert("tundra".to_owned(), BiomeSuitability::default());
        }),
        WorldError::UnknownBiome("tundra".to_owned())
    );
    assert_eq!(
        refused(&|s| s.compatibility.pairs[0].b = "tundra".to_owned()),
        WorldError::UnknownBiome("tundra".to_owned())
    );
}

#[test]
fn weighted_settings_round_trip_through_world_files() {
    let mut preset = WorldPreset::medium().clone();
    preset.key = Cow::Borrowed("weighted");
    preset.world.biome_selection = Some(BiomeSelectionOptions::default());
    let text = preset.to_yaml(Some(WorldPreset::medium()));
    assert!(text.contains("biome_selection"), "{text}");
    let read = WorldPreset::from_yaml("weighted", &text).expect("written file reads back");
    assert_eq!(read.world, preset.world, "{text}");

    // Rule-based Worlds write no selection at all.
    let text = WorldPreset::medium().to_yaml(None);
    assert!(!text.contains("biome_selection"), "{text}");

    // A hand-written file, as the docs show it.
    let read = WorldPreset::from_yaml(
        "hand",
        "base: medium_small
world:
  biome_selection:
    version: 1
    iterations: 2
    temperature: 0.5
    neighbour_strength: 1.0
    suitability:
      plains: { prevalence: 1.0 }
      mountains: { prevalence: 0.5, elevation: { centre: 150.0, spread: 80.0 } }
    compatibility:
      default: 0.5
",
    )
    .expect("valid file");
    let selection = read.world.biome_selection.expect("selection read");
    assert_eq!(selection.iterations, 2);
    assert_eq!(selection.suitability.len(), 2);
    assert!(selection.compatibility.pairs.is_empty());

    // Unknown fields and an unsupported version are refused.
    for bad in [
        "base: medium_small\nworld:\n  biome_selection:\n    version: 1\n    iterations: 1\n    \
         temperature: 1.0\n    neighbour_strength: 1.0\n    suitability: { plains: { prevalence: 1.0 } }\n    \
         compatibility: { default: 1.0 }\n    passes: 3\n",
        "base: medium_small\nworld:\n  biome_selection:\n    version: 7\n    iterations: 1\n    \
         temperature: 1.0\n    neighbour_strength: 1.0\n    suitability: { plains: { prevalence: 1.0 } }\n    \
         compatibility: { default: 1.0 }\n",
    ] {
        assert!(WorldPreset::from_yaml("bad", bad).is_err(), "{bad}");
    }

    // A Biome listed twice under `suitability` is refused, not merged.
    let twice = "base: medium_small\nworld:\n  biome_selection:\n    version: 1\n    iterations: 1\n    \
         temperature: 1.0\n    neighbour_strength: 1.0\n    suitability:\n      plains: { prevalence: 1.0 }\n      \
         plains: { prevalence: 2.0 }\n    compatibility: { default: 1.0 }\n";
    assert!(WorldPreset::from_yaml("twice", twice).is_err());
}

#[test]
fn rule_based_worlds_ignore_the_selection_code() {
    // Without `biome_selection`, a World's Biomes come from its rules,
    // exactly as before: mountains pinned in an all-plains World.
    let pinned = key(0, 0);
    let world = World::new(WorldOptions {
        biomes: procgena_world::BiomeRules {
            rules: Vec::new(),
            fallback: "plains".to_owned(),
        },
        pins: vec![BiomePin::region(pinned, "mountains")],
        ..WorldOptions::default()
    })
    .expect("valid World");
    assert_eq!(world.biome(pinned).preset.key, "mountains");
    for biome in world.biomes(&patch(5)) {
        if biome.region != pinned {
            assert_eq!(biome.preset.key, "plains");
        }
        // The rules read the fields at the region's own site.
        assert_eq!(
            biome.fields,
            world.fields(world.terrain().site(biome.region))
        );
    }
}
