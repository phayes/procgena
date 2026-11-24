//! Compose a Plant Scatter from plants and adjust its shared appearance.
//!
//! The Plant Scatter tab starts from a Plant Scatter preset. Each row
//! controls one plant's appearance, density multiplier and grouping into
//! patches. Each plant has a natural density in plants per square metre;
//! the row's multiplier and the Scatter's multiplier both scale that density.

pub mod editor;
pub mod params;

use std::borrow::Cow;
use std::path::Path;

use procgena_core::preset::FILE_GROUP;
use procgena_plant::prelude::{
    PlantParams, PlantPreset, PlantScatter, PlantScatterPreset, ScatterPlant,
};

use crate::export::{Done, file_key, shown_name, write_file};

/// The Plant Scatter preset shown when the app starts, by its key.
pub const START_PRESET: &str = "meadow";

/// Clover added by `+ Add Plant`, at 10% of its natural density.
///
/// The Scatter's density scale also applies. Its percentage of the mix
/// depends on the densities of the other entries.
pub const ADDED_PLANT: ScatterPlant = ScatterPlant::new(PlantParams::CLOVER).scaled(0.1);

/// Picker label for a plant whose settings match no built-in preset.
pub const CUSTOM: &str = "Custom";

/// What a plant row shows about its place in the whole field.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RowStatus {
    /// Plants per square metre for this entry, including both density scales.
    pub per_m2: f32,
    /// The plant's share of the field, from 0 to 1.
    pub share: f32,
    /// Whether this entry fills the remaining gaps and cannot use clustering.
    /// This is the last entry with a positive density.
    pub takes_rest: bool,
    /// Whether the row may be removed; the last remaining row may not.
    pub removable: bool,
}

/// Works out each row's status from the entries' densities, in row order,
/// and the field's density multiplier (negative values count as zero).
pub fn row_statuses(densities: &[f32], field_scale: f32) -> Vec<RowStatus> {
    let field_scale = field_scale.max(0.0);
    let shares = shares(densities);
    let rest = densities.iter().rposition(|density| *density > 0.0);
    let removable = densities.len() > 1;
    densities
        .iter()
        .zip(shares)
        .enumerate()
        .map(|(index, (density, share))| RowStatus {
            per_m2: density * field_scale,
            share,
            takes_rest: rest == Some(index),
            removable,
        })
        .collect()
}

/// Reads Plant Scatter settings from the text of a `.plant_scatter.yml`
/// preset file. The result has size 0, flat ground and no density map.
/// Returns a readable error if the text is not a Plant Scatter file or a
/// setting is out of range.
pub fn read_field(text: &str) -> Result<PlantScatter, String> {
    PlantScatter::from_yaml(text).map_err(|error| format!("Not a Plant Scatter: {error}"))
}

/// Writes `field`'s plants and look to `path` as a `.plant_scatter.yml`
/// preset file that names `base` as its base and holds only the settings
/// that differ from it. Size, ground and density map are not written. The
/// file's name is its key, as [`file_key`] reads it.
pub fn save_field_preset(
    field: PlantScatter,
    base: &PlantScatterPreset,
    path: &Path,
) -> Result<Done, String> {
    let key = file_key(path, "plant_scatter");
    let preset = PlantScatterPreset {
        key: Cow::Owned(key.clone()),
        name: Cow::Owned(key),
        description: Cow::Borrowed(""),
        group: Cow::Borrowed(FILE_GROUP),
        plant_scatter: field,
    };
    write_file(path, preset.to_yaml(Some(base)).as_bytes())?;
    Ok(Done::new(format!("Saved {}", shown_name(path))))
}

/// Plants per square metre as a row shows them, with fewer decimals the
/// more there are: `120/m²`, `1.4/m²`, `0.12/m²`.
pub fn per_m2_text(per_m2: f32) -> String {
    let decimals = if per_m2 >= 10.0 {
        0
    } else if per_m2 >= 1.0 {
        1
    } else {
        2
    };
    format!("{per_m2:.decimals$}/m²")
}

/// Returns each entry's fraction of the total density for the row percentages.
///
/// Pass non-negative densities from `ScatterPlant::density`. Returns all zeros
/// when the total is zero.
fn shares(densities: &[f32]) -> Vec<f32> {
    let total: f32 = densities.iter().sum();
    densities
        .iter()
        .map(|density| if total > 0.0 { density / total } else { 0.0 })
        .collect()
}

/// Finds the picker option whose preset exactly matches the plant settings.
/// Returns the trailing [`CUSTOM`] option when no preset matches.
pub fn choice_of(plant: &PlantParams) -> usize {
    let presets = PlantPreset::all();
    let count = PlantPreset::all().count();
    presets
        .enumerate()
        .find(|(_, preset)| preset.plant == *plant)
        .map_or(count, |(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_names_are_unique_and_the_start_preset_exists() {
        let mut names: Vec<_> = PlantScatterPreset::all()
            .map(|p| p.name.to_string())
            .collect();
        let count = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), count);
        assert!(PlantScatterPreset::named(START_PRESET).is_some());
        assert!(PlantPreset::all().all(|p| p.name != CUSTOM));
    }

    #[test]
    fn shares_are_each_density_of_the_total() {
        assert_eq!(shares(&[1.0, 3.0]), vec![0.25, 0.75]);
        assert_eq!(shares(&[0.0, 0.0]), vec![0.0, 0.0]);
    }

    #[test]
    fn the_last_planted_row_takes_the_rest_and_lone_rows_stay() {
        let rows = row_statuses(&[1.0, 3.0, 0.0], 2.0);
        assert_eq!(rows[0].per_m2, 2.0);
        assert_eq!(rows[1].share, 0.75);
        assert!(!rows[0].takes_rest && rows[1].takes_rest && !rows[2].takes_rest);
        assert!(rows.iter().all(|row| row.removable));
        assert!(!row_statuses(&[1.0], 1.0)[0].removable);
    }

    #[test]
    fn plants_per_square_metre_read_at_a_glance() {
        assert_eq!(per_m2_text(120.0), "120/m²");
        assert_eq!(per_m2_text(1.44), "1.4/m²");
        assert_eq!(per_m2_text(0.12), "0.12/m²");
    }

    #[test]
    fn a_plant_is_shown_as_its_preset_or_as_custom() {
        let count = PlantPreset::all().count();
        let clover = PlantPreset::all()
            .position(|p| p.plant == PlantParams::CLOVER)
            .unwrap();
        assert_eq!(choice_of(&PlantParams::CLOVER), clover);
        let mut custom = PlantParams::CLOVER;
        custom.width *= 2.0;
        assert_eq!(choice_of(&custom), count);
    }

    #[test]
    fn fields_are_read_from_preset_files() {
        let preset = PlantScatterPreset::SUMMER_WILD;
        let field = preset.plant_scatter.clone();
        assert_eq!(read_field(&field.to_yaml()), Ok(field.clone()));
        let based = preset.to_yaml(Some(&PlantScatterPreset::MEADOW));
        assert_eq!(read_field(&based), Ok(field));
        assert!(read_field("not a field").is_err());
    }
}
