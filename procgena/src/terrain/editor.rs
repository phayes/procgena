//! A terrain document's editor: seed, debug drawing, every terrain setting,
//! and saving and loading `.terrain.yml` preset files.

use std::time::Duration;

use bevy::feathers::controls::{FeathersButton, FeathersNumberInput, HardLimit, NumberInputValue};
use bevy::feathers::display::caption;
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui_widgets::{Activate, ValueChange};
use procgena_core::Rng;
use procgena_terrain::{TerrainOptions, TerrainPreset};

use super::show_sections;
use crate::export::{Done, FileOps, read_preset, save_preset};
use crate::params::{ParamPanel, ParamPanelPlugin, ParamsChanged, spawn_param_panel};
use crate::scene::{TerrainSettings, TerrainShow};
use crate::studio::Dormant;
use crate::studio::inspector::{InspectorPlugin, Settings, edit_setting, spawn_meta_panel};
use crate::ui::{LABEL_WIDTH, labelled_row};

/// The panel holding a terrain editor's settings.
type TerrainPanel = ParamPanel<Settings<TerrainOptions>>;

/// How long the settings must stay unchanged before the terrain is built
/// again, so dragging a slider doesn't start a build at every step.
const SETTLE: Duration = Duration::from_millis(400);

/// Triggered after the selected terrain's settled settings change, or when
/// the camera should be fitted to them. Read them with
/// [`TerrainEditor::settled`].
#[derive(Event, Debug)]
pub struct TerrainEdited {
    /// Whether the camera should reset to fit the terrain, as after
    /// loading settings.
    pub reframe: bool,
}

/// A terrain editor's state, one per open terrain.
#[derive(Component)]
pub struct TerrainEditor {
    /// The preset edited, which saved files name as their base.
    preset: &'static TerrainPreset,
    /// The panel holding the edited options.
    panel: Entity,
    /// The panel holding the debug drawing settings.
    show_panel: Entity,
    /// Picks seeds for Random Seed.
    rng: Rng,
    /// The settings to show: the edited ones, once they have settled.
    settled: TerrainSettings,
    /// When the settings last changed, while that change hasn't settled.
    changed: Option<Duration>,
    /// Whether the camera should be fitted to the next settled settings.
    reframe: bool,
}

impl TerrainEditor {
    /// The settings to show. Terrain edits appear here once the settings
    /// have stayed unchanged for a moment; debug drawing edits appear at
    /// once.
    pub fn settled(&self) -> &TerrainSettings {
        &self.settled
    }
}

/// Marks an editor's seed field.
#[derive(Component, Default, Clone)]
struct TerrainSeed;

/// Makes terrain editors work. Needs the param panel plugins.
pub struct TerrainEditorPlugin;

impl Plugin for TerrainEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            InspectorPlugin::<TerrainOptions>::default(),
            ParamPanelPlugin::<TerrainShow>::default(),
        ))
        .add_systems(Update, settle_terrain)
        .add_systems(PostUpdate, show_seed);
    }
}

/// Spawns a terrain editor for the built-in preset `key` as the last child
/// of `parent`, with the save and load buttons below it, and returns the
/// editor's entity.
///
/// # Panics
///
/// Panics if `key` names no built-in terrain preset.
pub fn spawn_terrain_editor(commands: &mut Commands, parent: Entity, key: &str) -> Entity {
    let preset = TerrainPreset::named(key).expect("the terrain preset is built in");
    let options = preset.terrain.clone();
    let seed = i64::from(options.default_seed);

    // Seed, and the seed button, above the panels.
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
    let panel = spawn_meta_panel(
        commands,
        editor,
        &super::META,
        Settings::new(options.clone()),
        &[0],
        &|_, _| Vec::new(),
    );
    for edited in [show_panel, panel] {
        commands.entity(edited).observe(
            move |_: On<ParamsChanged>, mut editors: Query<&mut TerrainEditor>, time: Res<Time>| {
                if let Ok(mut editor) = editors.get_mut(editor) {
                    editor.changed = Some(time.elapsed());
                }
            },
        );
    }
    commands.entity(editor).insert(TerrainEditor {
        preset,
        panel,
        show_panel,
        rng: Rng::new(std::process::id()),
        settled: TerrainSettings {
            terrain: options,
            show: TerrainShow::default(),
        },
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
    editor
}

/// Takes each editor's settings as settled once they have stopped changing
/// for [`SETTLE`], and triggers [`TerrainEdited`] for the selected editor
/// when its settled settings change or its camera should be fitted.
fn settle_terrain(
    time: Res<Time>,
    mut editors: Query<(&mut TerrainEditor, Has<Dormant>)>,
    panels: Query<&TerrainPanel>,
    shows: Query<&ParamPanel<TerrainShow>>,
    mut commands: Commands,
) {
    for (mut editor, dormant) in &mut editors {
        let Some(changed) = editor.changed else {
            continue;
        };
        let (Ok(panel), Ok(show)) = (panels.get(editor.panel), shows.get(editor.show_panel)) else {
            continue;
        };
        let wanted = TerrainSettings {
            terrain: panel.value().options().clone(),
            show: *show.value(),
        };

        // Debug drawing is cheap: take it at once. A new terrain waits
        // until the settings settle.
        let settled = time.elapsed().saturating_sub(changed) >= SETTLE;
        if wanted.terrain != editor.settled.terrain && !settled {
            continue;
        }
        editor.changed = None;
        let edited = wanted != editor.settled;
        if edited {
            editor.settled = wanted;
        }

        // Only the selected editor's edits reach the scene.
        let reframe = std::mem::take(&mut editor.reframe);
        if !dormant && (edited || reframe) {
            commands.trigger(TerrainEdited { reframe });
        }
    }
}

/// Shows loaded terrain settings in the selected terrain's editor and fits
/// the camera to them. The preset, and so the suggested file name, stays
/// as it was. Run it with `World::run_system_cached_with`.
pub fn load_terrain(
    In(options): In<TerrainOptions>,
    editor: Single<&mut TerrainEditor, Without<Dormant>>,
    mut panels: Query<&mut TerrainPanel>,
    time: Res<Time>,
) {
    let mut editor = editor.into_inner();
    if let Ok(mut panel) = panels.get_mut(editor.panel) {
        panel.set_value(Settings::new(options));
        editor.changed = Some(time.elapsed().saturating_sub(SETTLE));
        editor.reframe = true;
    }
}

/// Uses a typed seed. Like any edit, it is built once the settings settle.
fn type_seed(
    change: On<ValueChange<i64>>,
    editor: Single<&TerrainEditor, Without<Dormant>>,
    mut panels: Query<&mut TerrainPanel>,
    mut commands: Commands,
) {
    if let Ok(seed) = u32::try_from(change.value) {
        edit_setting(
            &mut commands,
            &mut panels,
            editor.panel,
            "default_seed",
            seed.into(),
        );
    }
}

/// Picks a random seed.
fn random_seed(
    _: On<Activate>,
    mut editor: Single<&mut TerrainEditor, Without<Dormant>>,
    mut panels: Query<&mut TerrainPanel>,
    mut commands: Commands,
) {
    let seed = editor.rng.next_u32() % 65536;
    edit_setting(
        &mut commands,
        &mut panels,
        editor.panel,
        "default_seed",
        seed.into(),
    );
}

/// Shows each terrain's seed in its own seed field whenever it changes.
fn show_seed(
    editors: Query<(Entity, &TerrainEditor)>,
    panels: Query<&TerrainPanel, Changed<TerrainPanel>>,
    children: Query<&Children>,
    fields: Query<&NumberInputValue, With<TerrainSeed>>,
    mut commands: Commands,
) {
    for (entity, editor) in &editors {
        let Ok(panel) = panels.get(editor.panel) else {
            continue;
        };
        let seed = NumberInputValue::I64(i64::from(panel.value().options().default_seed));
        for field in children.iter_descendants(entity) {
            if fields.get(field).is_ok_and(|shown| *shown != seed) {
                commands.entity(field).insert(seed);
            }
        }
    }
}

/// Saves the terrain settings as a `.terrain.yml` preset file based on the
/// preset.
fn save_terrain_preset(
    _: On<Activate>,
    editor: Single<&TerrainEditor, Without<Dormant>>,
    panels: Query<&TerrainPanel>,
    mut files: FileOps,
) {
    let Ok(panel) = panels.get(editor.panel) else {
        return;
    };
    let options = panel.value().options().clone();
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
    use crate::terrain::start_preset;

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
