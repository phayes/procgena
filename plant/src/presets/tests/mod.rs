use std::collections::HashSet;
use std::fmt::Write as _;

use super::*;
use crate::look::MAX_PLANTS;

/// The lines around the generated tables in `docs/presets.md`.
const TABLE_START: &str =
    "<!-- preset numbers: written by `BLESS_PRESETS=1 cargo test`, do not edit -->";
const TABLE_END: &str = "<!-- end of preset numbers -->";

/// How high `plant` reaches: a branched plant's highest point at rest, or an
/// unbranched plant's length, which also leaves room for its head.
fn top_of(plant: &GrassPlant) -> f32 {
    if plant.levels[0].is_on() {
        crate::mirror::branched_top(plant)
    } else {
        plant.height.y
    }
}

/// Generates preset size tables from the constants for `docs/presets.md`.
/// This keeps the dimensions readers use to choose plants in sync with the code.
fn preset_tables() -> String {
    let mut table = String::from(
        "| Field preset | Name | Plants | Tallest plant (m) | Plants per m² | Spacing (cm) |\n\
         | --- | --- | --- | --- | --- | --- |\n",
    );
    for preset in FieldPreset::all() {
        let field = &preset.field;
        let tallest = field
            .plants
            .iter()
            .map(|entry| top_of(&entry.plant))
            .fold(0.0, f32::max);
        writeln!(
            table,
            "| `{}` | {} | {} | {tallest:.2} | {:.0} | {:.1} |",
            preset.key,
            preset.name,
            field.plants.len(),
            field.density(),
            field.spacing() * 100.0,
        )
        .expect("writing to a String cannot fail");
    }
    table.push_str(
        "\n| Plant preset | Name | Plants per m² | Height (m) | Top (m) |\n\
         | --- | --- | --- | --- | --- |\n",
    );
    for preset in PlantPreset::all() {
        let plant = &preset.plant;
        writeln!(
            table,
            "| `{}` | {} | {} | {:.2}–{:.2} | {:.2} |",
            preset.key,
            preset.name,
            plant.density,
            plant.height.x,
            plant.height.y,
            top_of(plant),
        )
        .expect("writing to a String cannot fail");
    }
    table
}

#[test]
fn the_preset_numbers_in_the_docs_match_the_constants() {
    // `BLESS_PRESETS=1 cargo test` rewrites the tables from the constants.
    let path = format!("{}/docs/presets.md", env!("CARGO_MANIFEST_DIR"));
    // The crates.io package leaves `docs/` out; there is nothing to check.
    let Ok(page) = std::fs::read_to_string(&path) else {
        return;
    };
    let start = page
        .find(TABLE_START)
        .expect("docs/presets.md has the table's start line")
        + TABLE_START.len();
    let end = page
        .find(TABLE_END)
        .expect("docs/presets.md has the table's end line");
    let written = page[start..end].trim();
    let table = preset_tables();
    if std::env::var_os("BLESS_PRESETS").is_some() {
        let blessed = format!("{}\n{}\n{}", &page[..start], table, &page[end..]);
        std::fs::write(&path, blessed).unwrap();
        return;
    }
    assert_eq!(
        written,
        table.trim(),
        "docs/presets.md is out of date: run `BLESS_PRESETS=1 cargo test`"
    );
}

#[test]
fn every_field_preset_grows_something_and_fits() {
    for preset in FieldPreset::all() {
        let key = &preset.key;
        let plants = &preset.field.plants;
        assert!(!plants.is_empty(), "{key}");
        assert!(
            plants.len() <= MAX_PLANTS,
            "{key}: at most {MAX_PLANTS} plants"
        );
        for entry in plants.iter() {
            assert!(entry.density() > 0.0, "{key}: every plant grows");
            assert!((0.0..=1.0).contains(&entry.clustering), "{key}");
        }
        assert_eq!(preset.field.size, Vec2::ZERO, "{key}: a preset has no size");
        assert_eq!(preset.field.ground, GrassGround::Flat, "{key}");
        assert!(preset.field.density_map.is_none(), "{key}");
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
    for preset in FieldPreset::all() {
        assert!(keys.insert(&preset.key), "field key {} twice", preset.key);
        assert!(
            names.insert(&preset.name),
            "field name {} twice",
            preset.name
        );
        assert_eq!(FieldPreset::named(&preset.key), Some(preset));
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
    assert!(FieldPreset::named("NOT_A_PRESET").is_none());
    assert!(PlantPreset::named("NOT_A_PRESET").is_none());
}

#[test]
fn a_plant_preset_is_its_plant_constant() {
    assert_eq!(PlantPreset::FERN.plant, GrassPlant::FERN);
    assert_eq!(PlantPreset::MEADOW_GRASS.plant, GrassPlant::MEADOW_GRASS);
    assert_eq!(PlantPreset::BROOM.plant, GrassPlant::BROOM);
    assert_eq!(PlantPreset::FERN.key, "FERN");
    assert_eq!(PlantPreset::FERN.group, "Ground cover");
    assert_eq!(FieldPreset::MEADOW.key, "MEADOW");
    assert_eq!(FieldPreset::MEADOW.group, "General");
    // The group arrays list each preset once, in order.
    assert_eq!(FieldPreset::GENERAL[0], FieldPreset::MEADOW);
    assert_eq!(PlantPreset::GRASSES[0], PlantPreset::MEADOW_GRASS);
    let listed: usize = FieldPreset::GROUPS.iter().map(|group| group.len()).sum();
    assert_eq!(listed, FieldPreset::all().count());
}

#[test]
fn every_plant_preset_grows_at_a_density_of_its_own() {
    // The Plant tab of an editor shows a plant at its own density, so every
    // plant preset needs one, and none is denser than a lawn.
    for preset in PlantPreset::all() {
        let density = preset.plant.density;
        assert!(
            density > 0.0 && density <= GrassPlant::LAWN_GRASS.density,
            "{}: {density} plants per m²",
            preset.key
        );
    }
    // A field preset grows no denser than its densest plant could on its own
    // within twice its scale, so preset scales stay near 1 or below.
    for preset in FieldPreset::all() {
        for entry in preset.field.plants.iter() {
            assert!(
                entry.density_scale <= 2.0 && preset.field.density_scale <= 2.0,
                "{}: scales stay near the plants' own densities",
                preset.key
            );
        }
    }
    let broom = &FieldPreset::BROOM.field.plants;
    assert_eq!(
        broom.last().map(|entry| entry.plant),
        Some(GrassPlant::BROOM)
    );
    assert_eq!(broom[0].plant, GrassPlant::MEADOW_GRASS);
}
