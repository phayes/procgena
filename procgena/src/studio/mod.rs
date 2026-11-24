//! The main window: the 3D view on the left and the settings panel on the right.

mod camera;
mod editing;
mod layout;
mod quit;
mod status;
mod tabs;
mod textures;

use std::sync::Arc;

use crate::scene::{EnvironmentSettings, FieldSettings, SceneMode, SceneStart, ViewerCommand};
use bevy::prelude::*;
use procgena_plant::prelude::FieldPreset;

use crate::params::ParamPanel;
use layout::{PlantLodRow, TabPage};
use status::{BuildState, BuildStatus};
pub use tabs::StudioTab;

/// The Level of Detail choices: switching by camera distance, then each of
/// the scene's tree detail levels, most detailed first.
const LOD_CHOICES: [&str; 5] = [
    "Auto (by distance)",
    "0 · Full",
    "1 · Medium",
    "2 · Low",
    "3 · Lowest",
];

/// The mushroom Level of Detail choices: switching by camera distance, then
/// each of the mushroom detail levels, most detailed first.
const MUSHROOM_LOD_CHOICES: [&str; 6] = [
    "Auto (by distance)",
    "0 · Full",
    "1 · High",
    "2 · Medium",
    "3 · Flat",
    "4 · Billboard",
];

/// The dead wood Level of Detail choices: switching by camera distance,
/// then each of the dead wood detail levels, most detailed first.
const DEADWOOD_LOD_CHOICES: [&str; 5] = [
    "Auto (by distance)",
    "0 · Full",
    "1 · High",
    "2 · Low",
    "3 · Strips",
];

/// The plant Level of Detail choices: switching by camera distance, then each of
/// the grass and plant detail levels, most detailed first. They apply to every
/// blade and plant in the scene.
const PLANT_LOD_CHOICES: [&str; 4] = ["Auto (by distance)", "0 · Near", "1 · Mid", "2 · Far"];

/// The tab whose settings are shown.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ActiveTab(pub StudioTab);

/// The studio's window contents and behaviour. Add it with the scene plugin,
/// started from [`scene_start`].
pub struct StudioPlugin;

impl Plugin for StudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveTab>()
            .init_resource::<BuildState>()
            .add_plugins((
                quit::QuitPlugin,
                editing::EditingPlugin,
                camera::CameraOverlayPlugin,
            ))
            .add_systems(
                Startup,
                (
                    layout::spawn_layout,
                    layout::spawn_environment,
                    editing::spawn_editors,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    apply_tab,
                    status::record_events,
                    status::record_plant_commands,
                    status::record_downloads,
                    status::update_overlay,
                )
                    .chain(),
            )
            .add_systems(Update, textures::report_texture_failures)
            .add_observer(layout::scroll_settings);
    }
}

/// What the scene shows when the studio opens: the Trees tab, with each
/// editor's starting preset.
pub fn scene_start() -> SceneStart {
    let field = FieldPreset::named(crate::field::START_PRESET)
        .map(|preset| preset.field.clone())
        .unwrap_or_default();
    SceneStart {
        mode: StudioTab::default().mode(),
        tree: Arc::new(crate::tree::START_PRESET.options()),
        field: Arc::new(FieldSettings { field }),
        stone: Arc::new(crate::stones::START_PRESET.options()),
        clump: Arc::new(crate::clump::start_settings()),
        mushroom: Arc::new(crate::mushrooms::START_PRESET.options()),
        grove: Arc::new(crate::grove::editor::start_grove()),
        deadwood: Arc::new(crate::deadwood::start_settings()),
        environment: EnvironmentSettings::default(),
        lod: None,
        plant_lod: None,
    }
}

/// Shows the active tab's settings, and switches the scene to its subject.
///
/// Field Size shows only on the plant tabs, which it affects. Switching to a
/// tab with another subject rebuilds that subject, so its status starts over.
/// The plant tabs each send their plants, since they show different ones.
#[allow(clippy::too_many_arguments)]
fn apply_tab(
    tab: Res<ActiveTab>,
    mut shown_mode: Local<Option<SceneMode>>,
    mut pages: Query<(&TabPage, &mut Node), Without<PlantLodRow>>,
    mut plant_lod: Query<&mut Node, With<PlantLodRow>>,
    mut environment: Query<&mut ParamPanel<EnvironmentSettings>>,
    mut state: ResMut<BuildState>,
    shown: editing::ShownField,
    mut viewer: MessageWriter<ViewerCommand>,
) {
    if !tab.is_changed() {
        return;
    }
    let mode = tab.0.mode();

    // Show only the active tab's settings.
    for (page, mut node) in &mut pages {
        node.display = display(page.0 == tab.0);
    }
    for mut node in &mut plant_lod {
        node.display = display(mode == SceneMode::Plants);
    }
    for mut panel in &mut environment {
        panel.set_hidden_keys(crate::environment::hidden_keys(mode).iter().copied());
    }

    // Switch the scene's subject when the tab shows a different one.
    if shown_mode.is_some_and(|shown| shown != mode) {
        viewer.write(ViewerCommand::SetMode(mode));
        state.tree = BuildStatus::Building;
        state.stone = BuildStatus::Building;
        state.clump = BuildStatus::Building;
        state.mushroom = BuildStatus::Building;
        state.grove = BuildStatus::Building;
        state.deadwood = BuildStatus::Building;
    }
    *shown_mode = Some(mode);

    // The plant tabs show different plants: one alone, or the whole field.
    editing::send_field_on_plant_tab(&shown, &mut viewer);
}

/// [`Display::Flex`] when `shown`, otherwise [`Display::None`].
fn display(shown: bool) -> Display {
    if shown { Display::Flex } else { Display::None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_detail_level_can_be_chosen() {
        assert_eq!(LOD_CHOICES.len(), crate::scene::lod_levels().len() + 1);
        // The scene uses the mushroom plugin's default levels.
        let mushroom_levels = procgena_mushroom::MushroomLod::default().levels.len();
        assert_eq!(MUSHROOM_LOD_CHOICES.len(), mushroom_levels + 1);
        // The scene uses the dead wood plugin's default levels.
        let deadwood_levels = procgena_deadwood::default_lod().levels.len();
        assert_eq!(DEADWOOD_LOD_CHOICES.len(), deadwood_levels + 1);
    }

    #[test]
    fn the_studio_starts_from_each_editors_preset() {
        let start = scene_start();
        assert_eq!(start.mode, SceneMode::Tree);
        assert_eq!(*start.tree, crate::tree::START_PRESET.options());
        assert_eq!(*start.stone, crate::stones::START_PRESET.options());
        assert_eq!(*start.clump, crate::clump::start_settings());
        assert_eq!(*start.mushroom, crate::mushrooms::START_PRESET.options());
        assert_eq!(*start.grove, crate::grove::editor::start_grove());
        assert_eq!(*start.deadwood, crate::deadwood::start_settings());
        assert!(FieldPreset::named(crate::field::START_PRESET).is_some());
    }
}
