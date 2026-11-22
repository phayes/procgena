//! The Camera and Environment settings, shared by every tab.

use crate::scene::{EnvironmentSettings, SceneMode};

use crate::params::{Group, Section, number, toggle};

/// Key for Field Size, which controls the preview area on both plant tabs.
const FIELD_SIZE: &str = "fieldSize";

/// Lists shared environment controls that do not apply to the selected tab.
/// Field Size is hidden on Trees and Stones because it affects only plant previews.
pub fn hidden_keys(mode: SceneMode) -> &'static [&'static str] {
    match mode {
        SceneMode::Plants => &[],
        SceneMode::Tree | SceneMode::Stone => &[FIELD_SIZE],
    }
}

/// The Camera and Environment sections.
pub fn sections() -> Vec<Section<EnvironmentSettings>> {
    vec![
        Section {
            title: "Camera",
            groups: vec![Group::plain(vec![
                toggle!(EnvironmentSettings, "autoRotate", "Auto Rotate", |e| e
                    .auto_rotate),
                number!(
                    EnvironmentSettings,
                    "rotateSpeed",
                    "Rotate Speed",
                    (0.0, 90.0, 1.0),
                    |e| e.rotate_speed
                )
                .shown_if(|e| e.auto_rotate),
            ])],
        },
        Section {
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
                ),
            ])],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_keys_name_settings_and_field_size_shows_only_for_plants() {
        let keys: Vec<String> = sections()
            .into_iter()
            .flat_map(|section| section.groups)
            .flat_map(|group| group.params)
            .map(|param| param.key)
            .collect();
        for mode in [SceneMode::Tree, SceneMode::Plants, SceneMode::Stone] {
            for hidden in hidden_keys(mode) {
                assert!(
                    keys.iter().any(|key| key == hidden),
                    "{hidden} is not a setting"
                );
            }
            let field_size_hidden = hidden_keys(mode).contains(&FIELD_SIZE);
            assert_eq!(field_size_hidden, mode != SceneMode::Plants, "{mode:?}");
        }
    }
}
