//! The Plant tab's controls: preset and every plant setting.

use bevy::feathers::controls::{
    FeathersButton, FeathersSelect, OptionIndex, list_rows_from_strings,
};
use bevy::feathers::display::caption;
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui_widgets::{Activate, ValueChange};
use procgena_plant::prelude::{PlantParams, PlantPreset};

use super::{START_PRESET, params, read_plant};
use crate::export::{Done, FileOps, shown_name, write_file};
use crate::params::{ParamPanel, ParamPanelPlugin, ParamsChanged, spawn_param_panel};
use crate::ui::labelled_row;

/// Triggered after the plant settings change. Read them from the panel named
/// by [`PlantEditor::panel`].
#[derive(Event, Debug)]
pub struct PlantEdited;

/// The plant editor's state. There is one, on the Plant tab.
#[derive(Component)]
pub struct PlantEditor {
    /// The panel holding the edited plant.
    panel: Entity,
}

impl PlantEditor {
    /// The panel entity holding the edited [`PlantParams`].
    pub fn panel(&self) -> Entity {
        self.panel
    }
}

/// Makes the plant editor work, and every `ParamPanel<PlantParams>`. Needs the
/// param panel plugins.
pub struct PlantEditorPlugin;

impl Plugin for PlantEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ParamPanelPlugin::<PlantParams>::default());
    }
}

/// The plant shown when the studio opens.
///
/// # Panics
///
/// Panics if [`START_PRESET`] names no plant preset.
pub fn start_plant() -> PlantParams {
    PlantPreset::named(START_PRESET)
        .expect("START_PRESET names a plant preset")
        .plant
}

/// Spawns the plant editor, showing [`START_PRESET`], as the last child of `parent`.
pub fn spawn_plant_editor(commands: &mut Commands, parent: Entity) {
    let names: Vec<String> = PlantPreset::all().map(|p| p.name.to_string()).collect();
    let selected = PlantPreset::all().position(|p| p.key == START_PRESET);
    let preset: Box<dyn Scene> = Box::new(bsn! {
        @FeathersSelect { @options: list_rows_from_strings(names, selected) }
        Node { flex_grow: 1.0 }
        on(choose_preset)
    });
    let editor = commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0,
                row_gap: px(8),
            }
            Children [ @labelled_row("Preset", preset) ]
        })
        .insert(ChildOf(parent))
        .id();

    // Every other setting; the first section starts open.
    let panel = spawn_param_panel(commands, editor, start_plant(), params::sections(), &[0]);
    commands
        .entity(panel)
        .observe(|_: On<ParamsChanged>, mut commands: Commands| {
            commands.trigger(PlantEdited);
        });
    commands.entity(editor).insert(PlantEditor { panel });

    // Saving and loading, below the settings.
    commands
        .spawn_scene(bsn! {
            Node { display: Display::Flex, column_gap: px(6), flex_shrink: 0.0 }
            Children [
                @FeathersButton { @caption: bsn! { @caption("Save Plant…") } }
                on(save_plant)
                --
                @FeathersButton { @caption: bsn! { @caption("Load Plant…") } }
                on(load_plant_file)
            ]
        })
        .insert(ChildOf(editor));
}

/// Shows a loaded plant in the editor. Run it with `World::run_system_cached_with`.
pub fn load_plant(
    In(plant): In<PlantParams>,
    editor: Single<&PlantEditor>,
    mut panels: Query<&mut ParamPanel<PlantParams>>,
    mut commands: Commands,
) {
    if let Ok(mut panel) = panels.get_mut(editor.panel) {
        panel.set_value(plant);
        commands.trigger(PlantEdited);
    }
}

/// Saves the plant as a `.plant.ron` file, or as JSON when the chosen name
/// ends in `.json`.
fn save_plant(
    _: On<Activate>,
    editor: Single<&PlantEditor>,
    panels: Query<&ParamPanel<PlantParams>>,
    mut files: FileOps,
) {
    let Ok(plant) = panels.get(editor.panel).map(|panel| *panel.value()) else {
        return;
    };
    files.save("plant.plant.ron", move |path| {
        let json = path.extension().is_some_and(|ext| ext == "json");
        let text = if json {
            plant.to_json()
        } else {
            plant.to_ron()
        };
        write_file(path, text.as_bytes())?;
        Ok(Done::new(format!("Saved {}", shown_name(path))))
    });
}

/// Loads a plant file into the editor.
fn load_plant_file(_: On<Activate>, mut files: FileOps) {
    files.open(|text, world| {
        let plant = read_plant(text)?;
        world
            .run_system_cached_with(load_plant, plant)
            .map_err(|error| format!("Cannot show the plant: {error}"))?;
        Ok(Done::new("Loaded the plant"))
    });
}

/// Starts over from the chosen preset.
fn choose_preset(
    change: On<ValueChange<Entity>>,
    rows: Query<&OptionIndex>,
    editor: Single<&PlantEditor>,
    mut panels: Query<&mut ParamPanel<PlantParams>>,
    mut commands: Commands,
) {
    let Some(preset) = rows
        .get(change.value)
        .ok()
        .and_then(|index| PlantPreset::all().nth(index.0))
    else {
        return;
    };
    if let Ok(mut panel) = panels.get_mut(editor.panel) {
        panel.set_value(preset.plant);
        commands.trigger(PlantEdited);
    }
}
