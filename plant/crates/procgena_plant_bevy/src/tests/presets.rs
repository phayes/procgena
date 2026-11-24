//! The preset tables in `docs/presets.md`, checked against the constants.
//! They live here because a branched plant's height comes from the CPU
//! mirror of the shaders.

use std::fmt::Write as _;

use crate::{PlantParams, PlantPreset, PlantScatterPreset};

/// The lines around the generated tables in `docs/presets.md`.
const TABLE_START: &str =
    "<!-- preset numbers: checked against the constants by a test, do not edit by hand -->";
const TABLE_END: &str = "<!-- end of preset numbers -->";

/// How high `plant` reaches: a branched plant's highest point at rest, or an
/// unbranched plant's length, which also leaves room for its head.
fn top_of(plant: &PlantParams) -> f32 {
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
        "| Scatter preset | Name | Plants | Tallest plant (m) | Plants per m² | Spacing (cm) |\n\
         | --- | --- | --- | --- | --- | --- |\n",
    );
    for preset in PlantScatterPreset::all() {
        let field = &preset.plant_scatter;
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
    // When a preset changes, copy the table this test prints into the page.
    let path = format!("{}/../../docs/presets.md", env!("CARGO_MANIFEST_DIR"));
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
    assert_eq!(
        written,
        table.trim(),
        "docs/presets.md is out of date: replace its table with the one above"
    );
}
