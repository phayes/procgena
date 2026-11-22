//! Edit one plant independently of the Plant Field tab's plant mix.
//!
//! Start from a preset or load a file, then adjust the plant's settings.
//! The same controls also customise individual plants in the Plant Field tab.

pub mod editor;
pub mod params;

use procgena_plant::prelude::PlantParams;

/// The plant preset shown when the app starts, by its key.
pub const START_PRESET: &str = "MEADOW_GRASS";

/// Reads a plant from a saved file's text.
///
/// Text beginning with `{` is read as JSON; everything else is read as RON.
/// Returns a readable error if parsing or validation fails.
pub fn read_plant(text: &str) -> Result<PlantParams, String> {
    let read = if text.trim_start().starts_with('{') {
        PlantParams::from_json(text)
    } else {
        PlantParams::from_ron(text)
    };
    read.map_err(|error| format!("Not a plant: {error}"))
}

#[cfg(test)]
mod tests {
    use procgena_plant::prelude::PlantPreset;

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
    fn plants_are_read_from_ron_or_json() {
        let plant = PlantParams::CLOVER;
        assert_eq!(read_plant(&plant.to_ron()), Ok(plant));
        assert_eq!(read_plant(&plant.to_json()), Ok(plant));
        assert!(read_plant("not a plant").is_err());
    }
}
