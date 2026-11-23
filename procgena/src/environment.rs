//! The Environment settings, shared by every tab.

use crate::scene::{EnvironmentSettings, SceneMode};

use crate::params::{Group, Section, number, toggle};

/// Key for Field Size, which controls the preview area on both plant tabs.
const FIELD_SIZE: &str = "fieldSize";

/// Key for Grove Size, which controls the ground the Tree Grove tab's grove
/// grows on.
const GROVE_SIZE: &str = "groveSize";

/// Key for the Wind switch.
const WIND: &str = "wind";

/// Lists shared environment controls that do not apply to the selected tab.
/// Field Size shows only on the plant tabs and Grove Size only on the Tree
/// Grove tab, since each sizes only that tab's preview. Wind is hidden on
/// Clumps, which always shows still air.
pub fn hidden_keys(mode: SceneMode) -> &'static [&'static str] {
    match mode {
        SceneMode::Plants => &[GROVE_SIZE],
        SceneMode::Grove => &[FIELD_SIZE],
        SceneMode::Tree | SceneMode::Stone | SceneMode::Mushroom | SceneMode::Deadwood => {
            &[FIELD_SIZE, GROVE_SIZE]
        }
        // The clump would not sway with its tree, so clump mode has no wind.
        SceneMode::Clump => &[FIELD_SIZE, GROVE_SIZE, WIND],
    }
}

/// The Environment section. The camera's rotation settings are edited over
/// the 3D view instead.
pub fn sections() -> Vec<Section<EnvironmentSettings>> {
    vec![Section {
        title: "Environment",
        groups: vec![Group::plain(vec![
            number!(
                EnvironmentSettings,
                "sunAzimuth",
                "Sun Angle",
                (0.0, 360.0, 1.0),
                |e| e.sun_azimuth
            ),
            number!(
                EnvironmentSettings,
                "sunElevation",
                "Sun Height",
                (5.0, 85.0, 1.0),
                |e| e.sun_elevation
            ),
            toggle!(EnvironmentSettings, "wind", "Wind", |e| e.wind),
            toggle!(EnvironmentSettings, "showGround", "Ground", |e| e
                .show_ground),
            number!(
                EnvironmentSettings,
                FIELD_SIZE,
                "Field Size",
                (1.0, 100.0, 1.0),
                |e| e.field_size
            )
            .exponential(),
            number!(
                EnvironmentSettings,
                GROVE_SIZE,
                "Grove Size",
                (crate::grove::SIZE_RANGE.0, crate::grove::SIZE_RANGE.1, 1.0),
                |e| e.grove_size
            )
            .exponential(),
        ])],
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_keys_name_settings_and_each_size_shows_only_on_its_tab() {
        let keys: Vec<String> = sections()
            .into_iter()
            .flat_map(|section| section.groups)
            .flat_map(|group| group.params)
            .map(|param| param.key)
            .collect();
        let modes = [
            SceneMode::Tree,
            SceneMode::Plants,
            SceneMode::Stone,
            SceneMode::Clump,
            SceneMode::Mushroom,
            SceneMode::Grove,
            SceneMode::Deadwood,
        ];
        for mode in modes {
            for hidden in hidden_keys(mode) {
                assert!(
                    keys.iter().any(|key| key == hidden),
                    "{hidden} is not a setting"
                );
            }
            let field_size_hidden = hidden_keys(mode).contains(&FIELD_SIZE);
            assert_eq!(field_size_hidden, mode != SceneMode::Plants, "{mode:?}");
            let grove_size_hidden = hidden_keys(mode).contains(&GROVE_SIZE);
            assert_eq!(grove_size_hidden, mode != SceneMode::Grove, "{mode:?}");
            let wind_hidden = hidden_keys(mode).contains(&WIND);
            assert_eq!(wind_hidden, mode == SceneMode::Clump, "{mode:?}");
        }
    }
}
