//! The Environment settings on the view frame, shared by every document.

use crate::scene::EnvironmentSettings;

use crate::params::{Group, Section, number, toggle};

/// Key for Field Size, which sizes the preview area of plants.
pub const FIELD_SIZE: &str = "fieldSize";

/// Key for Scatter Size, which sizes the ground a tree Scatter grows on.
pub const GROVE_SIZE: &str = "groveSize";

/// Key for the Wind switch.
pub const WIND: &str = "wind";

/// The settings hidden for a single object or a patch that sizes itself:
/// both preview area sizes.
pub const SINGLE_OBJECT: &[&str] = &[FIELD_SIZE, GROVE_SIZE];

/// The settings hidden for a subject shown in still air, such as a clump,
/// which would not sway with its tree, or a fruit cluster on its own.
pub const STILL_AIR: &[&str] = &[FIELD_SIZE, GROVE_SIZE, WIND];

/// The settings hidden for plants: the tree Scatter's size.
pub const PLANTS: &[&str] = &[GROVE_SIZE];

/// The settings hidden for a tree Scatter: the plant field's size.
pub const TREE_SCATTER: &[&str] = &[FIELD_SIZE];

/// The Environment section. The camera's rotation settings are edited on
/// the view frame's camera controls instead.
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
            toggle!(EnvironmentSettings, WIND, "Wind", |e| e.wind),
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
                "Scatter Size",
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
    fn hidden_keys_name_settings() {
        let keys: Vec<String> = sections()
            .into_iter()
            .flat_map(|section| section.groups)
            .flat_map(|group| group.params)
            .map(|param| param.key)
            .collect();
        for hidden in [SINGLE_OBJECT, STILL_AIR, PLANTS, TREE_SCATTER].concat() {
            assert!(
                keys.iter().any(|key| key == hidden),
                "{hidden} is not a setting"
            );
        }
    }
}
