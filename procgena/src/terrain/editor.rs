//! The Terrain tab's controls: preset, seed, debug drawing, every terrain
//! setting, and saving and loading `.terrain.yml` preset files.

use std::sync::Arc;
use std::time::Duration;

use bevy::feathers::controls::{
    FeathersButton, FeathersNumberInput, FeathersSelect, HardLimit, NumberInputValue, OptionIndex,
    list_rows_from_strings,
};
use bevy::feathers::display::caption;
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui_widgets::{Activate, ValueChange};
use procgena_core::Rng;
use procgena_terrain::{TerrainOptions, TerrainPreset};

use super::{START_PRESET, params, show_sections, start_preset, start_settings};
use crate::export::{Done, FileOps, read_preset, save_preset};
use crate::params::{ParamPanel, ParamPanelPlugin, ParamsChanged, spawn_param_panel};
use crate::scene::{TerrainSettings, TerrainShow, ViewerCommand};
use crate::ui::{LABEL_WIDTH, labelled_row};

/// How long the settings must stay unchanged before the terrain is built
/// again, so dragging a slider doesn't start a build at every step.
const SETTLE: Duration = Duration::from_millis(400);

/// The terrain editor's state. There is one, on the Terrain tab.
#[derive(Component)]
pub struct TerrainEditor {
    /// The preset last chosen, which saved files name as their base.
    preset: &'static TerrainPreset,
    /// The panel holding the edited options.
    panel: Entity,
    /// The panel holding the debug drawing settings.
    show_panel: Entity,
    /// Picks seeds for Random Seed.
    rng: Rng,
    /// When the settings last changed, while that change is unsent.
    changed: Option<Duration>,
    /// Whether the camera should be fitted to the next settings sent.
    reframe: bool,
}

/// Marks the seed field.
#[derive(Component, Default, Clone)]
struct TerrainSeed;

/// Makes the terrain editor work. Needs the param panel plugins.
pub struct TerrainEditorPlugin;

impl Plugin for TerrainEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ParamPanelPlugin::<TerrainOptions>::default(),
            ParamPanelPlugin::<TerrainShow>::default(),
        ))
        .add_systems(Update, send_terrain)
        .add_systems(PostUpdate, show_seed);
    }
}

/// Spawns the terrain editor, showing [`START_PRESET`], as the last child
/// of `parent`, with the save and load buttons below it.
pub fn spawn_terrain_editor(commands: &mut Commands, parent: Entity) {
    let names: Vec<String> = TerrainPreset::all().map(|p| p.name.to_string()).collect();
    let selected = TerrainPreset::all().position(|p| p.key == START_PRESET);
    let start = start_preset();
    let options = start.terrain.clone();
    let seed = i64::from(options.default_seed);

    // Preset, seed, and the seed button, above the panels.
    let preset: Box<dyn Scene> = Box::new(bsn! {
        @FeathersSelect { @options: list_rows_from_strings(names, selected) }
        Node { flex_grow: 1.0 }
        on(choose_preset)
    });
    let seed_field: Box<dyn Scene> = Box::new(bsn! {
        @FeathersNumberInput
        Node { flex_grow: 1.0 }
        NumberInputValue::I64({seed})
        HardLimit::i64(0..=i64::from(u32::MAX))
        TerrainSeed
        on(type_seed)
    });
    let editor = commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0,
                row_gap: px(8),
            }
            Children [
                @labelled_row("Preset", preset)
                --
                @labelled_row("Seed", seed_field)
                --
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    flex_shrink: 0.0,
                    padding: UiRect::left(px(LABEL_WIDTH + 8.0)),
                }
                Children [
                    @FeathersButton { @caption: bsn! { @caption("Random Seed") } }
                    on(random_seed)
                ]
            ]
        })
        .insert(ChildOf(parent))
        .id();

    // Debug drawing, open; then every terrain setting, with Land open.
    let show_panel = spawn_param_panel(
        commands,
        editor,
        TerrainShow::default(),
        show_sections(),
        &[0],
    );
    let panel = spawn_param_panel(commands, editor, options, params::sections(), &[0]);
    for edited in [show_panel, panel] {
        commands.entity(edited).observe(
            |_: On<ParamsChanged>, mut editor: Single<&mut TerrainEditor>, time: Res<Time>| {
                editor.changed = Some(time.elapsed());
            },
        );
    }
    commands.entity(editor).insert(TerrainEditor {
        preset: start,
        panel,
        show_panel,
        rng: Rng::new(std::process::id()),
        changed: None,
        reframe: false,
    });

    // Save and load.
    commands
        .spawn_scene(bsn! {
            Node { display: Display::Flex, column_gap: px(6), flex_shrink: 0.0 }
            Children [
                @FeathersButton { @caption: bsn! { @caption("Save Preset…") } }
                on(save_terrain_preset)
                --
                @FeathersButton { @caption: bsn! { @caption("Load Preset…") } }
                on(load_terrain_preset)
            ]
        })
        .insert(ChildOf(parent));
}

/// Sends the settings to the scene once they have stopped changing for
/// [`SETTLE`], if they differ from the ones sent last.
fn send_terrain(
    time: Res<Time>,
    editor: Option<Single<&mut TerrainEditor>>,
    panels: Query<&ParamPanel<TerrainOptions>>,
    shows: Query<&ParamPanel<TerrainShow>>,
    mut sent: Local<Option<TerrainSettings>>,
    mut viewer: MessageWriter<ViewerCommand>,
) {
    let Some(mut editor) = editor else { return };
    let Some(changed) = editor.changed else {
        return;
    };
    let (Ok(panel), Ok(show)) = (panels.get(editor.panel), shows.get(editor.show_panel)) else {
        return;
    };
    let wanted = TerrainSettings {
        terrain: panel.value().clone(),
        show: *show.value(),
    };
    // Debug drawing is cheap: send it at once. A new terrain waits until
    // the settings settle.
    let sent = sent.get_or_insert_with(start_settings);
    let settled = time.elapsed().saturating_sub(changed) >= SETTLE;
    if wanted.terrain == sent.terrain || settled {
        editor.changed = None;
        if wanted != *sent {
            *sent = wanted;
            viewer.write(ViewerCommand::SetTerrain(Arc::new(sent.clone())));
        }
        if std::mem::take(&mut editor.reframe) {
            viewer.write(ViewerCommand::ResetCamera);
        }
    }
}

/// Shows loaded terrain settings in the editor and fits the camera to them.
/// The chosen preset, and so the suggested file name, stays as it was. Run
/// it with `World::run_system_cached_with`.
pub fn load_terrain(
    In(options): In<TerrainOptions>,
    editor: Single<&mut TerrainEditor>,
    mut panels: Query<&mut ParamPanel<TerrainOptions>>,
    time: Res<Time>,
) {
    replace(options, true, &mut editor.into_inner(), &mut panels, &time);
}

/// Shows `options` in the editor's controls, to be sent once they settle.
fn replace(
    options: TerrainOptions,
    reframe: bool,
    editor: &mut TerrainEditor,
    panels: &mut Query<&mut ParamPanel<TerrainOptions>>,
    time: &Time,
) {
    if let Ok(mut panel) = panels.get_mut(editor.panel) {
        panel.set_value(options);
        editor.changed = Some(time.elapsed().saturating_sub(SETTLE));
        editor.reframe |= reframe;
    }
}

/// Starts over from the chosen preset, and fits the camera to it.
fn choose_preset(
    change: On<ValueChange<Entity>>,
    rows: Query<&OptionIndex>,
    mut editor: Single<&mut TerrainEditor>,
    mut panels: Query<&mut ParamPanel<TerrainOptions>>,
    time: Res<Time>,
) {
    let Some(preset) = rows
        .get(change.value)
        .ok()
        .and_then(|index| TerrainPreset::all().nth(index.0))
    else {
        return;
    };
    editor.preset = preset;
    replace(
        preset.terrain.clone(),
        true,
        &mut editor,
        &mut panels,
        &time,
    );
}

/// Uses a typed seed.
fn type_seed(
    change: On<ValueChange<i64>>,
    mut editor: Single<&mut TerrainEditor>,
    mut panels: Query<&mut ParamPanel<TerrainOptions>>,
    time: Res<Time>,
) {
    let (Ok(panel), Ok(seed)) = (panels.get(editor.panel), u32::try_from(change.value)) else {
        return;
    };
    // Ignore echoes of the seed already in use.
    if panel.value().default_seed == seed {
        return;
    }
    let mut options = panel.value().clone();
    options.default_seed = seed;
    replace(options, false, &mut editor, &mut panels, &time);
}

/// Picks a random seed.
fn random_seed(
    _: On<Activate>,
    mut editor: Single<&mut TerrainEditor>,
    mut panels: Query<&mut ParamPanel<TerrainOptions>>,
    time: Res<Time>,
) {
    let Ok(panel) = panels.get(editor.panel) else {
        return;
    };
    let mut options = panel.value().clone();
    options.default_seed = editor.rng.next_u32() % 65536;
    replace(options, false, &mut editor, &mut panels, &time);
}

/// Shows the options' seed in the seed field whenever it changes.
fn show_seed(
    editor: Option<Single<&TerrainEditor>>,
    panels: Query<&ParamPanel<TerrainOptions>, Changed<ParamPanel<TerrainOptions>>>,
    fields: Query<(Entity, &NumberInputValue), With<TerrainSeed>>,
    mut commands: Commands,
) {
    let Some(editor) = editor else { return };
    let Ok(panel) = panels.get(editor.panel) else {
        return;
    };
    let seed = NumberInputValue::I64(i64::from(panel.value().default_seed));
    for (field, shown) in &fields {
        if *shown != seed {
            commands.entity(field).insert(seed);
        }
    }
}

/// Saves the terrain settings as a `.terrain.yml` preset file based on the
/// chosen preset.
fn save_terrain_preset(
    _: On<Activate>,
    editor: Single<&TerrainEditor>,
    panels: Query<&ParamPanel<TerrainOptions>>,
    mut files: FileOps,
) {
    let Ok(panel) = panels.get(editor.panel) else {
        return;
    };
    let options = panel.value().clone();
    let base = editor.preset;
    files.save(&format!("{}.terrain.yml", base.key), move |path| {
        save_preset(options, base, path)
    });
}

/// Loads terrain settings from a `.terrain.yml` preset file into the
/// editor.
fn load_terrain_preset(_: On<Activate>, mut files: FileOps) {
    files.open(|text, world| {
        let options = read_options(text)?;
        world
            .run_system_cached_with(load_terrain, options)
            .map_err(|error| format!("Cannot show the terrain settings: {error}"))?;
        Ok(Done::new("Loaded the terrain settings"))
    });
}

/// Reads the terrain settings of a `.terrain.yml` preset file and checks
/// them.
pub fn read_options(text: &str) -> Result<TerrainOptions, String> {
    read_preset::<TerrainPreset>(text, "terrain")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_preset_loads_back() {
        let mut options = start_preset().terrain.clone();
        options.base.hills.height = 17.0;
        let dir = std::env::temp_dir().join(format!("procgena-terrain-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hills.terrain.yml");
        save_preset(options.clone(), start_preset(), &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(read_options(&text).unwrap(), options);
        assert!(read_options("not: terrain").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
