//! Controls for customising a terrain preset's land, rivers, roads and
//! borders, for terrain documents, which show an endless world with those
//! settings in every region.

pub mod editor;
pub mod params;

#[cfg(test)]
use procgena_terrain::TerrainPreset;

use crate::params::{Group, Section, toggle};
#[cfg(test)]
use crate::scene::TerrainSettings;
use crate::scene::TerrainShow;

/// The key of the terrain preset opened when terrain is the kind opened
/// first.
pub const START_PRESET: &str = "rolling_hills";

/// The terrain preset named by [`START_PRESET`].
#[cfg(test)]
pub fn start_preset() -> &'static TerrainPreset {
    TerrainPreset::named(START_PRESET).expect("the start preset is built in")
}

/// What the scene shows for [`START_PRESET`] when it is first opened: its
/// terrain, with no debug drawing.
#[cfg(test)]
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
