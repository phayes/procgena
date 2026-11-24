//! Edit one plant independently of any Plant Scatter's plant mix.
//!
//! Open a preset or load a file, then adjust the plant's settings. The same
//! controls also customise individual plants in a Plant Scatter's editor.

pub mod editor;
pub mod params;

use std::borrow::Cow;
use std::path::Path;

use procgena_core::preset::FILE_GROUP;
use procgena_plant::prelude::{PlantParams, PlantPreset};

use crate::export::{Done, file_key, shown_name, write_file};

/// The plant preset shown when the app starts, by its key.
pub const START_PRESET: &str = "meadow_grass";

/// Reads a plant from the text of a `.plant.yml` preset file.
///
/// Returns a readable error if the text is not a plant file or a setting is
/// out of range.
pub fn read_plant(text: &str) -> Result<PlantParams, String> {
    PlantParams::from_yaml(text).map_err(|error| format!("Not a plant: {error}"))
}

/// Writes `plant` to `path` as a `.plant.yml` preset file that names `base`
/// as its base and holds only the settings that differ from it. The file's
/// name is its key, as [`file_key`] reads it.
pub fn save_plant_preset(
    plant: PlantParams,
    base: &PlantPreset,
    path: &Path,
) -> Result<Done, String> {
    let key = file_key(path, "plant");
    let preset = PlantPreset {
        key: Cow::Owned(key.clone()),
        name: Cow::Owned(key),
        description: Cow::Borrowed(""),
        group: Cow::Borrowed(FILE_GROUP),
        plant,
    };
    write_file(path, preset.to_yaml(Some(base)).as_bytes())?;
    Ok(Done::new(format!("Saved {}", shown_name(path))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_names_are_unique_and_the_start_preset_exists() {
        let mut names: Vec<_> = PlantPreset::all().map(|p| p.name.to_string()).collect();
        let count = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), count);
        assert!(PlantPreset::named(START_PRESET).is_some());
    }

    #[test]
    fn plants_are_read_from_preset_files() {
        let plant = PlantParams::CLOVER;
        assert_eq!(read_plant(&plant.to_yaml()), Ok(plant));
        let based = PlantPreset::CLOVER.to_yaml(Some(&PlantPreset::FERN));
        assert_eq!(read_plant(&based), Ok(plant));
        assert!(read_plant("not a plant").is_err());
    }
}
