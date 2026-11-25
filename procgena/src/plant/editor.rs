//! A plant document's editor: every plant setting, and saving and loading
//! plant files.

use bevy::feathers::controls::FeathersButton;
use bevy::feathers::display::caption;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use procgena_plant_bevy::prelude::{PlantParams, PlantPreset};

use super::{params, read_plant, save_plant_preset};
use crate::export::{Done, FileOps};
use crate::params::{ParamPanel, ParamPanelPlugin, ParamsChanged, spawn_param_panel};
use crate::studio::Dormant;

/// Triggered after the selected plant's settings change. Read them from the
/// panel named by [`PlantEditor::panel`].
#[derive(Event, Debug)]
pub struct PlantEdited;

/// A plant editor's state, one per open plant.
#[derive(Component)]
pub struct PlantEditor {
    /// The preset edited, which saved files name as their base.
    preset: &'static PlantPreset,
    /// The panel holding the edited plant.
    panel: Entity,
}

impl PlantEditor {
    /// The panel entity holding the edited [`PlantParams`].
    pub fn panel(&self) -> Entity {
        self.panel
    }
}

/// Makes plant editors work, and every `ParamPanel<PlantParams>`. Needs the
/// param panel plugins.
pub struct PlantEditorPlugin;

impl Plugin for PlantEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ParamPanelPlugin::<PlantParams>::default());
    }
}

/// The plant preset shown when the studio opens.
///
/// # Panics
///
/// Panics if [`START_PRESET`] names no plant preset.
#[cfg(test)]
pub fn start_preset() -> &'static PlantPreset {
    PlantPreset::named(super::START_PRESET).expect("START_PRESET names a plant preset")
}

/// The plant shown when the studio opens.
///
/// # Panics
///
/// Panics if [`START_PRESET`] names no plant preset.
#[cfg(test)]
pub fn start_plant() -> PlantParams {
    start_preset().plant
}

/// Spawns a plant editor for the built-in preset `key` as the last child of
/// `parent`, and returns the editor's entity.
///
/// # Panics
///
/// Panics if `key` names no built-in plant preset.
pub fn spawn_plant_editor(commands: &mut Commands, parent: Entity, key: &str) -> Entity {
    let preset = PlantPreset::named(key).expect("the plant preset is built in");
    let editor = commands
        .spawn(Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            row_gap: px(8),
            ..default()
        })
        .insert(ChildOf(parent))
        .id();

    // Every plant setting; the first section starts open.
    let panel = spawn_param_panel(commands, editor, preset.plant, params::sections(), &[0]);
    commands
        .entity(panel)
        .observe(|_: On<ParamsChanged>, mut commands: Commands| {
            commands.trigger(PlantEdited);
        });
    commands
        .entity(editor)
        .insert(PlantEditor { preset, panel });

    // Saving and loading, below the settings.
    commands
        .spawn_scene(bsn! {
            Node { display: Display::Flex, column_gap: px(6), flex_shrink: 0.0 }
            Children [
                @FeathersButton { @caption: bsn! { @caption("Save Preset…") } }
                on(save_plant)
                --
                @FeathersButton { @caption: bsn! { @caption("Load Preset…") } }
                on(load_plant_file)
            ]
        })
        .insert(ChildOf(editor));
    editor
}

/// Shows a loaded plant in the selected plant's editor. Run it with
/// `World::run_system_cached_with`.
pub fn load_plant(
    In(plant): In<PlantParams>,
    editor: Single<&PlantEditor, Without<Dormant>>,
    mut panels: Query<&mut ParamPanel<PlantParams>>,
    mut commands: Commands,
) {
    if let Ok(mut panel) = panels.get_mut(editor.panel) {
        panel.set_value(plant);
        commands.trigger(PlantEdited);
    }
}

/// Saves the selected plant as a `.plant.yml` preset file based on its
/// preset.
fn save_plant(
    _: On<Activate>,
    editor: Single<&PlantEditor, Without<Dormant>>,
    panels: Query<&ParamPanel<PlantParams>>,
    mut files: FileOps,
) {
    let Ok(plant) = panels.get(editor.panel).map(|panel| *panel.value()) else {
        return;
    };
    let base = editor.preset;
    files.save(&format!("{}.plant.yml", base.key), move |path| {
        save_plant_preset(plant, base, path)
    });
}

/// Loads a `.plant.yml` preset file into the selected plant's editor.
fn load_plant_file(_: On<Activate>, mut files: FileOps) {
    files.open(|text, world| {
        let plant = read_plant(text)?;
        world
            .run_system_cached_with(load_plant, plant)
            .map_err(|error| format!("Cannot show the plant: {error}"))?;
        Ok(Done::new("Loaded the plant"))
    });
}
