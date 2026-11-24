use std::collections::HashSet;

use super::*;
use crate::look::MAX_PLANTS;

#[test]
fn every_field_preset_grows_something_and_fits() {
    for preset in PlantScatterPreset::all() {
        let key = &preset.key;
        let plants = &preset.plant_scatter.plants;
        assert!(!plants.is_empty(), "{key}");
        assert!(
            plants.len() <= MAX_PLANTS,
            "{key}: at most {MAX_PLANTS} plants"
        );
        for entry in plants.iter() {
            assert!(entry.density() > 0.0, "{key}: every plant grows");
            assert!((0.0..=1.0).contains(&entry.clustering), "{key}");
        }
        assert!(
            !preset.name.is_empty() && !preset.description.is_empty(),
            "{key}"
        );
    }
}

#[test]
fn keys_and_names_are_unique_and_named_finds_every_preset() {
    let mut keys = HashSet::new();
    let mut names = HashSet::new();
    for preset in PlantScatterPreset::all() {
        assert!(keys.insert(&preset.key), "field key {} twice", preset.key);
        assert!(
            names.insert(&preset.name),
            "field name {} twice",
            preset.name
        );
        assert_eq!(PlantScatterPreset::named(&preset.key), Some(preset));
    }
    let (mut keys, mut names) = (HashSet::new(), HashSet::new());
    for preset in PlantPreset::all() {
        assert!(keys.insert(&preset.key), "plant key {} twice", preset.key);
        assert!(
            names.insert(&preset.name),
            "plant name {} twice",
            preset.name
        );
        assert_eq!(PlantPreset::named(&preset.key), Some(preset));
        assert!(!preset.description.is_empty(), "{}", preset.key);
    }
    assert!(PlantScatterPreset::named("not_a_preset").is_none());
    assert!(PlantPreset::named("not_a_preset").is_none());
    // Keys are lower case; the constant's name is no other spelling of one.
    assert!(PlantScatterPreset::named("MEADOW").is_none());
    assert!(PlantPreset::named("FERN").is_none());
}

#[test]
fn a_plant_preset_is_its_plant_constant() {
    assert_eq!(PlantPreset::FERN.plant, PlantParams::FERN);
    assert_eq!(PlantPreset::MEADOW_GRASS.plant, PlantParams::MEADOW_GRASS);
    assert_eq!(PlantPreset::BROOM.plant, PlantParams::BROOM);
    assert_eq!(PlantPreset::FERN.key, "fern");
    assert_eq!(PlantPreset::FERN.group, "Ground cover");
    assert_eq!(PlantScatterPreset::MEADOW.key, "meadow");
    assert_eq!(
        PlantScatterPreset::RYE_AND_CORNFLOWERS.key,
        "rye_and_cornflowers"
    );
    assert_eq!(PlantScatterPreset::MEADOW.group, "General");
    // The group arrays list each preset once, in order.
    assert_eq!(PlantScatterPreset::GENERAL[0], PlantScatterPreset::MEADOW);
    assert_eq!(PlantPreset::GRASSES[0], PlantPreset::MEADOW_GRASS);
    let listed: usize = PlantScatterPreset::GROUPS
        .iter()
        .map(|group| group.len())
        .sum();
    assert_eq!(listed, PlantScatterPreset::all().count());
}

#[test]
fn every_plant_preset_grows_at_a_density_of_its_own() {
    // The Plant tab of an editor shows a plant at its own density, so every
    // plant preset needs one, and none is denser than a lawn.
    for preset in PlantPreset::all() {
        let density = preset.plant.density;
        assert!(
            density > 0.0 && density <= PlantParams::LAWN_GRASS.density,
            "{}: {density} plants per m²",
            preset.key
        );
    }
    // A field preset grows no denser than its densest plant could on its own
    // within twice its scale, so preset scales stay near 1 or below.
    for preset in PlantScatterPreset::all() {
        for entry in preset.plant_scatter.plants.iter() {
            assert!(
                entry.density_scale <= 2.0 && preset.plant_scatter.density_scale <= 2.0,
                "{}: scales stay near the plants' own densities",
                preset.key
            );
        }
    }
    let broom = &PlantScatterPreset::BROOM.plant_scatter.plants;
    assert_eq!(
        broom.last().map(|entry| entry.plant),
        Some(PlantParams::BROOM)
    );
    assert_eq!(broom[0].plant, PlantParams::MEADOW_GRASS);
}
