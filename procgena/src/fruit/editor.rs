//! The Fruit and Flower tabs' controls: preset, seed, every cluster
//! setting, and saving, loading and exporting.
//!
//! Both tabs use the same editor, told apart by whether it edits fruit or
//! flowers. Systems that act on one of them take that as a `const FRUIT:
//! bool` parameter.

use std::path::Path;

use bevy::feathers::controls::{
    FeathersButton, FeathersNumberInput, FeathersSelect, HardLimit, NumberInputValue, OptionIndex,
    list_rows_from_strings,
};
use bevy::feathers::display::caption;
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui_widgets::{Activate, ValueChange};
use procgena_core::Rng;
use procgena_fruit::export::{gltf, obj};
use procgena_fruit::{Fruit, FruitMesh, FruitOptions, FruitPreset, default_lod};

use super::{params, presets, start_options};
use crate::export::{
    Done, FileOps, chosen_stem, read_preset, save_preset, shown_name, write_file, write_files,
};
use crate::params::{ParamPanel, ParamPanelPlugin, ParamsChanged, spawn_param_panel};
use crate::ui::{LABEL_WIDTH, labelled_row};

/// Triggered after the Fruit or Flower tab's settings change. Read them
/// from the panel named by that tab's [`FruitEditor::panel`].
#[derive(Event, Debug)]
pub struct FruitEdited {
    /// Whether the Fruit tab (true) or the Flower tab changed.
    pub fruit: bool,
    /// Whether the camera should reset to fit the new cluster, as after
    /// choosing a preset or loading settings.
    pub reframe: bool,
}

/// One of the two cluster editors' state, on the Fruit or the Flower tab.
#[derive(Component)]
pub struct FruitEditor {
    /// Whether this is the Fruit tab's editor (true) or the Flower tab's.
    fruit: bool,
    /// The preset last chosen.
    preset: &'static FruitPreset,
    /// The panel holding the edited options.
    panel: Entity,
    /// Picks seeds for Random Seed.
    rng: Rng,
}

/// Marks a tab's seed field; true on the Fruit tab.
#[derive(Component, Default, Clone)]
struct FruitSeed(bool);

/// Makes the Fruit and Flower editors work. Needs the param panel plugins.
pub struct FruitEditorPlugin;

impl Plugin for FruitEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ParamPanelPlugin::<FruitOptions>::default())
            .add_systems(PostUpdate, show_seed);
    }
}

/// The settings `editors` holds for the Fruit tab (`fruit` true) or the
/// Flower tab, or `None` before the editors are spawned.
pub fn edited_options(
    fruit: bool,
    editors: &Query<&FruitEditor>,
    panels: &Query<&ParamPanel<FruitOptions>>,
) -> Option<FruitOptions> {
    let editor = editors.iter().find(|e| e.fruit == fruit)?;
    Some(panels.get(editor.panel).ok()?.value().clone())
}

/// Spawns the Fruit tab's editor (`FRUIT` true) or the Flower tab's, with
/// its save, load and export buttons, as the last child of `parent`.
pub fn spawn_fruit_editor<const FRUIT: bool>(commands: &mut Commands, parent: Entity) {
    let listed = presets(FRUIT);
    let names: Vec<String> = listed.iter().map(|p| p.name.to_string()).collect();
    let options = start_options(FRUIT);
    let start = *listed
        .iter()
        .find(|p| p.fruit == options)
        .expect("the start preset is listed");
    let selected = listed.iter().position(|p| std::ptr::eq(*p, start));
    let seed = i64::from(options.default_seed);

    // Preset, seed, and the seed button, above the panel.
    let preset: Box<dyn Scene> = Box::new(bsn! {
        @FeathersSelect { @options: list_rows_from_strings(names, selected) }
        Node { flex_grow: 1.0 }
        on(choose_preset::<FRUIT>)
    });
    let seed_field: Box<dyn Scene> = Box::new(bsn! {
        @FeathersNumberInput
        Node { flex_grow: 1.0 }
        NumberInputValue::I64({seed})
        HardLimit::i64(0..=i64::from(u32::MAX))
        FruitSeed({FRUIT})
        on(type_seed::<FRUIT>)
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
                    on(random_seed::<FRUIT>)
                ]
            ]
        })
        .insert(ChildOf(parent))
        .id();

    // Every other setting; the cluster section starts open.
    let panel = spawn_param_panel(commands, editor, options, params::sections(FRUIT), &[0]);
    commands
        .entity(panel)
        .observe(|_: On<ParamsChanged>, mut commands: Commands| {
            commands.trigger(FruitEdited {
                fruit: FRUIT,
                reframe: false,
            });
        });
    commands.entity(editor).insert(FruitEditor {
        fruit: FRUIT,
        preset: start,
        panel,
        rng: Rng::new(std::process::id() ^ u32::from(FRUIT)),
    });

    // Save, load and export.
    commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0,
                row_gap: px(6),
            }
            Children [
                Node { display: Display::Flex, column_gap: px(6), flex_shrink: 0.0 }
                Children [
                    @FeathersButton { @caption: bsn! { @caption("Save Preset…") } }
                    on(save_fruit_preset::<FRUIT>)
                    --
                    @FeathersButton { @caption: bsn! { @caption("Load Preset…") } }
                    on(load_fruit_preset::<FRUIT>)
                ]
                --
                Node { display: Display::Flex, column_gap: px(6), flex_shrink: 0.0 }
                Children [
                    @FeathersButton { @caption: bsn! { @caption("Export GLB…") } }
                    on(export_fruit_glb::<FRUIT>)
                    --
                    @FeathersButton { @caption: bsn! { @caption("Export OBJ…") } }
                    on(export_fruit_obj::<FRUIT>)
                ]
            ]
        })
        .insert(ChildOf(parent));
}

/// Shows loaded settings in the Fruit tab's editor (`FRUIT` true) or the
/// Flower tab's, rebuilds the cluster and fits the camera to it. Run it
/// with `World::run_system_cached_with`.
pub fn load_fruit<const FRUIT: bool>(
    In(options): In<FruitOptions>,
    editors: Query<&FruitEditor>,
    mut panels: Query<&mut ParamPanel<FruitOptions>>,
    mut commands: Commands,
) {
    if let Some(editor) = editors.iter().find(|e| e.fruit == FRUIT) {
        replace(options, true, editor, &mut panels, &mut commands);
    }
}

/// Shows `options` in `editor`'s controls and triggers [`FruitEdited`].
fn replace(
    options: FruitOptions,
    reframe: bool,
    editor: &FruitEditor,
    panels: &mut Query<&mut ParamPanel<FruitOptions>>,
    commands: &mut Commands,
) {
    if let Ok(mut panel) = panels.get_mut(editor.panel) {
        panel.set_value(options);
        commands.trigger(FruitEdited {
            fruit: editor.fruit,
            reframe,
        });
    }
}

/// Starts over from the chosen preset, and fits the camera to it.
fn choose_preset<const FRUIT: bool>(
    change: On<ValueChange<Entity>>,
    rows: Query<&OptionIndex>,
    mut editors: Query<&mut FruitEditor>,
    mut panels: Query<&mut ParamPanel<FruitOptions>>,
    mut commands: Commands,
) {
    let Some(preset) = rows
        .get(change.value)
        .ok()
        .and_then(|index| presets(FRUIT).get(index.0).copied())
    else {
        return;
    };
    let Some(mut editor) = editors.iter_mut().find(|e| e.fruit == FRUIT) else {
        return;
    };
    editor.preset = preset;
    replace(
        preset.fruit.clone(),
        true,
        &editor,
        &mut panels,
        &mut commands,
    );
}

/// Uses a typed seed.
fn type_seed<const FRUIT: bool>(
    change: On<ValueChange<i64>>,
    editors: Query<&FruitEditor>,
    mut panels: Query<&mut ParamPanel<FruitOptions>>,
    mut commands: Commands,
) {
    let Some(editor) = editors.iter().find(|e| e.fruit == FRUIT) else {
        return;
    };
    let (Ok(panel), Ok(seed)) = (panels.get(editor.panel), u32::try_from(change.value)) else {
        return;
    };
    // Ignore echoes of the seed already in use.
    if panel.value().default_seed == seed {
        return;
    }
    let mut options = panel.value().clone();
    options.default_seed = seed;
    replace(options, false, editor, &mut panels, &mut commands);
}

/// Picks a random seed.
fn random_seed<const FRUIT: bool>(
    _: On<Activate>,
    mut editors: Query<&mut FruitEditor>,
    mut panels: Query<&mut ParamPanel<FruitOptions>>,
    mut commands: Commands,
) {
    let Some(mut editor) = editors.iter_mut().find(|e| e.fruit == FRUIT) else {
        return;
    };
    let Ok(panel) = panels.get(editor.panel) else {
        return;
    };
    let mut options = panel.value().clone();
    options.default_seed = editor.rng.next_u32() % 65536;
    replace(options, false, &editor, &mut panels, &mut commands);
}

/// Shows each editor's seed in its seed field whenever it changes.
fn show_seed(
    editors: Query<&FruitEditor>,
    panels: Query<&ParamPanel<FruitOptions>, Changed<ParamPanel<FruitOptions>>>,
    fields: Query<(Entity, &FruitSeed, &NumberInputValue)>,
    mut commands: Commands,
) {
    for editor in &editors {
        let Ok(panel) = panels.get(editor.panel) else {
            continue;
        };
        let seed = NumberInputValue::I64(i64::from(panel.value().default_seed));
        for (field, tab, shown) in &fields {
            if tab.0 == editor.fruit && *shown != seed {
                commands.entity(field).insert(seed);
            }
        }
    }
}

/// The settings of the `FRUIT` tab's editor, its chosen preset and the
/// preset's key as a suggested file name, copied so later edits do not
/// change a save already under way.
fn snapshot<const FRUIT: bool>(
    editors: &Query<&FruitEditor>,
    panels: &Query<&ParamPanel<FruitOptions>>,
) -> Option<(FruitOptions, &'static FruitPreset, String)> {
    let editor = editors.iter().find(|e| e.fruit == FRUIT)?;
    let options = panels.get(editor.panel).ok()?.value().clone();
    Some((options, editor.preset, editor.preset.key.to_string()))
}

/// Saves the settings as a `.fruit.yml` preset file based on the chosen preset.
fn save_fruit_preset<const FRUIT: bool>(
    _: On<Activate>,
    editors: Query<&FruitEditor>,
    panels: Query<&ParamPanel<FruitOptions>>,
    mut files: FileOps,
) {
    if let Some((options, base, stem)) = snapshot::<FRUIT>(&editors, &panels) {
        files.save(&format!("{stem}.fruit.yml"), move |path| {
            save_preset(options, base, path)
        });
    }
}

/// Loads settings from a `.fruit.yml` preset file into the editor.
fn load_fruit_preset<const FRUIT: bool>(_: On<Activate>, mut files: FileOps) {
    files.open(|text, world| {
        let options = read_options(text)?;
        world
            .run_system_cached_with(load_fruit::<FRUIT>, options)
            .map_err(|error| format!("Cannot show the cluster settings: {error}"))?;
        Ok(Done::new("Loaded the cluster settings"))
    });
}

/// Exports the cluster as a GLB model with every detail level.
fn export_fruit_glb<const FRUIT: bool>(
    _: On<Activate>,
    editors: Query<&FruitEditor>,
    panels: Query<&ParamPanel<FruitOptions>>,
    mut files: FileOps,
) {
    if let Some((options, _, stem)) = snapshot::<FRUIT>(&editors, &panels) {
        files.save(&format!("{stem}.glb"), move |path| {
            export_glb(&options, path)
        });
    }
}

/// Exports the cluster's full mesh as an OBJ model with its material file.
fn export_fruit_obj<const FRUIT: bool>(
    _: On<Activate>,
    editors: Query<&FruitEditor>,
    panels: Query<&ParamPanel<FruitOptions>>,
    mut files: FileOps,
) {
    if let Some((options, _, stem)) = snapshot::<FRUIT>(&editors, &panels) {
        files.save(&format!("{stem}.obj"), move |path| {
            export_obj(&options, &stem, path)
        });
    }
}

/// Reads the settings of a `.fruit.yml` preset file and checks their values.
/// Returns a readable error for text that isn't a fruit preset file or for
/// invalid settings.
pub fn read_options(text: &str) -> Result<FruitOptions, String> {
    read_preset::<FruitPreset>(text, "fruit or flower")
}

/// Grows the cluster's first specimen and meshes it: every default level
/// with `detail_levels`, else the full mesh alone.
fn meshes(options: &FruitOptions, detail_levels: bool) -> Result<Vec<FruitMesh>, String> {
    let fruit = Fruit::new(options.clone(), options.default_seed, 0)
        .map_err(|error| format!("Cannot build this cluster: {error}"))?;
    let mut meshes: Vec<FruitMesh> = fruit
        .lod_meshes(&default_lod())
        .into_iter()
        .map(|(_, mesh)| mesh)
        .collect();
    if !detail_levels {
        meshes.truncate(1);
    }
    Ok(meshes)
}

/// Writes the cluster to `path` as a GLB model with every detail level.
fn export_glb(options: &FruitOptions, path: &Path) -> Result<Done, String> {
    let meshes = meshes(options, true)?;
    let bytes = gltf::to_glb(&meshes, options).map_err(|error| error.to_string())?;
    write_file(path, &bytes)?;
    Ok(Done::new(format!(
        "Saved {} ({:.1} kB)",
        shown_name(path),
        bytes.len() as f64 / 1e3
    )))
}

/// Writes the cluster's full mesh to `path` as an OBJ model, with its
/// material file beside it, named after the chosen file or `suggested_stem`.
fn export_obj(options: &FruitOptions, suggested_stem: &str, path: &Path) -> Result<Done, String> {
    let meshes = meshes(options, false)?;
    let stem = chosen_stem(path, suggested_stem);
    let material_library = format!("{stem}.mtl");
    let files = obj::to_obj(&meshes, options, &material_library);
    write_files(
        path,
        &[
            (format!("{stem}.obj"), files.obj.into_bytes()),
            (material_library, files.mtl.into_bytes()),
        ],
    )?;
    Ok(Done::new(format!(
        "Saved {} with its material file",
        shown_name(path)
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_checks_the_settings() {
        assert!(read_options("base: apple\n").is_ok());
        let error = read_options("fruit:\n  size: -1\n").unwrap_err();
        assert!(error.contains("size"), "{error}");
    }

    #[test]
    fn exports_have_every_level_when_asked() {
        let options = start_options(true);
        assert_eq!(
            meshes(&options, true).unwrap().len(),
            default_lod().levels.len()
        );
        assert_eq!(meshes(&options, false).unwrap().len(), 1);
    }
}
