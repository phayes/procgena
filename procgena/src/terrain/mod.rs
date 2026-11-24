//! Controls for choosing a terrain preset and customising its land,
//! rivers, roads and borders, for the Terrain tab, which shows an endless
//! world with those settings in every region.

pub mod editor;
pub mod params;

use procgena_terrain::TerrainPreset;

use crate::params::{Group, Section, toggle};
use crate::scene::{TerrainSettings, TerrainShow};

/// The key of the terrain preset the editor shows first.
pub const START_PRESET: &str = "rolling_hills";

/// The terrain preset the editor shows first.
pub fn start_preset() -> &'static TerrainPreset {
    TerrainPreset::named(START_PRESET).expect("the start preset is built in")
}

/// What the scene shows on the Terrain tab at first: [`START_PRESET`], with
/// no debug drawing.
pub fn start_settings() -> TerrainSettings {
    TerrainSettings {
        terrain: start_preset().terrain.clone(),
        show: TerrainShow::default(),
    }
}

/// The debug drawing controls.
pub fn show_sections() -> Vec<Section<TerrainShow>> {
    vec![Section {
        title: "Show",
        groups: vec![Group::plain(vec![
            toggle!(TerrainShow, "levels", "Chunk Levels", |s| s.levels),
            toggle!(TerrainShow, "borders", "Chunk Borders", |s| s.borders),
        ])],
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::checks;

    #[test]
    fn show_controls_cover_every_choice() {
        let params: Vec<_> = show_sections()
            .into_iter()
            .flat_map(|s| s.groups)
            .flat_map(|g| g.params)
            .collect();
        let cases = vec![("start".to_owned(), TerrainShow::default())];
        checks::keys_are_unique(&params);
        checks::cases_fit_the_controls(&params, &cases);
        checks::settings_follow_what_shows_them(&params, &cases);
        checks::no_setting_hides_itself(&params, &cases);
    }

    #[test]
    fn the_start_preset_is_valid() {
        assert!(start_settings().terrain.validate().is_ok());
    }
}
