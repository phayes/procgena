//! A fruit or flower document's editor: seed, every cluster setting, and
//! saving, loading and exporting.
//!
//! Fruit and flower documents use the same editor, told apart by whether
//! its preset is a fruit or a flower.

use std::path::Path;

use bevy::feathers::controls::{FeathersButton, FeathersNumberInput, HardLimit, NumberInputValue};
use bevy::feathers::display::caption;
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui_widgets::{Activate, ValueChange};
use procgena_core::Rng;
use procgena_fruit::export::{gltf, obj};
use procgena_fruit::{FlowerPreset, Fruit, FruitMesh, FruitOptions, FruitPreset, default_lod};

use crate::export::{
    Done, FileOps, chosen_stem, read_preset, save_preset, shown_name, write_file, write_files,
};
use crate::params::{ParamPanel, ParamsChanged};
use crate::studio::Dormant;
use crate::studio::inspector::{InspectorPlugin, Settings, edit_setting, spawn_meta_panel};
use crate::ui::{LABEL_WIDTH, labelled_row};

/// The panel holding a fruit or flower editor's settings.
pub type FruitPanel = ParamPanel<Settings<FruitOptions>>;

/// Triggered after a cluster's settings change. Read them from the panel
/// named by the selected editor's [`FruitEditor::panel`].
#[derive(Event, Debug)]
pub struct FruitEdited {
    /// Whether the camera should reset to fit the new cluster, as after
    /// loading settings.
    pub reframe: bool,
}

/// The built-in preset a cluster editor edits: a fruit preset, saved as a
/// `.fruit.yml` file, or a flower preset, saved as a `.flower.yml` file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClusterPreset {
    /// A fruit preset.
    Fruit(&'static FruitPreset),
    /// A flower preset.
    Flower(&'static FlowerPreset),
}

impl ClusterPreset {
    /// The built-in fruit or flower preset `key`, if there is one.
    pub fn named(key: &str) -> Option<Self> {
        FruitPreset::named(key)
            .map(Self::Fruit)
            .or_else(|| FlowerPreset::named(key).map(Self::Flower))
    }

    /// The preset's key, such as `apple`.
    pub fn key(self) -> &'static str {
        match self {
            Self::Fruit(preset) => &preset.key,
            Self::Flower(preset) => &preset.key,
        }
    }

    /// The preset's settings.
    pub fn options(self) -> &'static FruitOptions {
        match self {
            Self::Fruit(preset) => &preset.fruit,
            Self::Flower(preset) => &preset.flower,
        }
    }

    /// The preset files' kind: `fruit` or `flower`, as in `.fruit.yml`.
    pub fn file_kind(self) -> &'static str {
        match self {
            Self::Fruit(_) => "fruit",
            Self::Flower(_) => "flower",
        }
    }

    /// Writes `options` to `path` as a preset file of this kind over this
    /// preset.
    fn save(self, options: FruitOptions, path: &Path) -> Result<Done, String> {
        match self {
            Self::Fruit(base) => save_preset(options, base, path),
            Self::Flower(base) => save_preset(options, base, path),
        }
    }

    /// Reads the settings of a preset file of this kind and checks their
    /// values. Returns a readable error for text that isn't a file of this
    /// kind or for invalid settings.
    pub fn read_options(self, text: &str) -> Result<FruitOptions, String> {
        match self {
            Self::Fruit(_) => read_preset::<FruitPreset>(text, "fruit"),
            Self::Flower(_) => read_preset::<FlowerPreset>(text, "flower"),
        }
    }
}

/// A cluster editor's state, one per open fruit or flower.
#[derive(Component)]
pub struct FruitEditor {
    /// The preset edited.
    preset: ClusterPreset,
    /// The panel holding the edited options.
    panel: Entity,
    /// Picks seeds for Random Seed.
    rng: Rng,
}

impl FruitEditor {
    /// The panel holding the edited [`FruitOptions`].
    pub fn panel(&self) -> Entity {
        self.panel
    }
}

/// Marks an editor's seed field.
#[derive(Component, Default, Clone)]
struct FruitSeed;

/// Makes the fruit and flower editors work. Needs the param panel plugins.
pub struct FruitEditorPlugin;

impl Plugin for FruitEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(InspectorPlugin::<FruitOptions>::default())
            .add_systems(PostUpdate, show_seed);
    }
}

/// Spawns a cluster editor for the built-in fruit or flower preset `key`,
/// with its save, load and export buttons, as the last children of
/// `parent`, and returns the editor's entity.
///
/// # Panics
///
/// Panics if `key` names no built-in fruit or flower preset.
pub fn spawn_fruit_editor(commands: &mut Commands, parent: Entity, key: &str) -> Entity {
    let preset = ClusterPreset::named(key).expect("the fruit or flower preset is built in");
    let options = preset.options().clone();
    let fruit = options.item.is_fruit();
    let seed = i64::from(options.default_seed);

    // Seed, and the seed button, above the panel.
    let seed_field: Box<dyn Scene> = Box::new(bsn! {
        @FeathersNumberInput
        Node { flex_grow: 1.0 }
        NumberInputValue::I64({seed})
        HardLimit::i64(0..=i64::from(u32::MAX))
        FruitSeed
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

    // Every other setting; the cluster section starts open.
    let kind = if fruit {
        &super::FRUIT_META
    } else {
        &super::FLOWER_META
    };
    let panel = spawn_meta_panel(
        commands,
        editor,
        kind,
        Settings::new(options),
        &[0],
        &|_, _| Vec::new(),
    );
    commands
        .entity(panel)
        .observe(|_: On<ParamsChanged>, mut commands: Commands| {
            commands.trigger(FruitEdited { reframe: false });
        });
    commands.entity(editor).insert(FruitEditor {
        preset,
        panel,
        rng: Rng::new(std::process::id() ^ u32::from(fruit)),
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
                    on(save_fruit_preset)
                    --
                    @FeathersButton { @caption: bsn! { @caption("Load Preset…") } }
                    on(load_fruit_preset)
                ]
                --
                Node { display: Display::Flex, column_gap: px(6), flex_shrink: 0.0 }
                Children [
                    @FeathersButton { @caption: bsn! { @caption("Export GLB…") } }
                    on(export_fruit_glb)
                    --
                    @FeathersButton { @caption: bsn! { @caption("Export OBJ…") } }
                    on(export_fruit_obj)
                ]
            ]
        })
        .insert(ChildOf(parent));
    editor
}

/// Shows loaded settings in the selected cluster's editor, rebuilds the
/// cluster and fits the camera to it. The preset, and so the suggested
/// file name, stays as it was. Run it with `World::run_system_cached_with`.
pub fn load_fruit(
    In(options): In<FruitOptions>,
    editor: Single<&FruitEditor, Without<Dormant>>,
    mut panels: Query<&mut FruitPanel>,
    mut commands: Commands,
) {
    if let Ok(mut panel) = panels.get_mut(editor.panel) {
        panel.set_value(Settings::new(options));
        commands.trigger(FruitEdited { reframe: true });
    }
}

/// Uses a typed seed.
fn type_seed(
    change: On<ValueChange<i64>>,
    editor: Single<&FruitEditor, Without<Dormant>>,
    mut panels: Query<&mut FruitPanel>,
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
    mut editor: Single<&mut FruitEditor, Without<Dormant>>,
    mut panels: Query<&mut FruitPanel>,
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

/// Shows each cluster's seed in its own seed field whenever it changes.
fn show_seed(
    editors: Query<(Entity, &FruitEditor)>,
    panels: Query<&FruitPanel, Changed<FruitPanel>>,
    children: Query<&Children>,
    fields: Query<&NumberInputValue, With<FruitSeed>>,
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

/// The selected editor's settings, its preset and the preset's key as a
/// suggested file name, copied so later edits do not change a save already
/// under way.
fn snapshot(
    editor: &FruitEditor,
    panels: &Query<&FruitPanel>,
) -> Option<(FruitOptions, ClusterPreset, String)> {
    let options = panels.get(editor.panel).ok()?.value().options().clone();
    Some((options, editor.preset, editor.preset.key().to_owned()))
}

/// Saves the settings as a `.fruit.yml` or `.flower.yml` preset file based
/// on the preset.
fn save_fruit_preset(
    _: On<Activate>,
    editor: Single<&FruitEditor, Without<Dormant>>,
    panels: Query<&FruitPanel>,
    mut files: FileOps,
) {
    if let Some((options, base, stem)) = snapshot(&editor, &panels) {
        files.save(&format!("{stem}.{}.yml", base.file_kind()), move |path| {
            base.save(options, path)
        });
    }
}

/// Loads settings from a preset file of the selected editor's kind,
/// `.fruit.yml` or `.flower.yml`, into the editor.
fn load_fruit_preset(_: On<Activate>, mut files: FileOps) {
    files.open(|text, world| {
        let preset = world
            .query_filtered::<&FruitEditor, Without<Dormant>>()
            .single(world)
            .map_err(|_| "No fruit or flower is selected".to_owned())?
            .preset;
        let options = preset.read_options(text)?;
        world
            .run_system_cached_with(load_fruit, options)
            .map_err(|error| format!("Cannot show the cluster settings: {error}"))?;
        Ok(Done::new("Loaded the cluster settings"))
    });
}

/// Exports the cluster as a GLB model with every detail level.
fn export_fruit_glb(
    _: On<Activate>,
    editor: Single<&FruitEditor, Without<Dormant>>,
    panels: Query<&FruitPanel>,
    mut files: FileOps,
) {
    if let Some((options, _, stem)) = snapshot(&editor, &panels) {
        files.save(&format!("{stem}.glb"), move |path| {
            export_glb(&options, path)
        });
    }
}

/// Exports the cluster's full mesh as an OBJ model with its material file.
fn export_fruit_obj(
    _: On<Activate>,
    editor: Single<&FruitEditor, Without<Dormant>>,
    panels: Query<&FruitPanel>,
    mut files: FileOps,
) {
    if let Some((options, _, stem)) = snapshot(&editor, &panels) {
        files.save(&format!("{stem}.obj"), move |path| {
            export_obj(&options, &stem, path)
        });
    }
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
    use crate::fruit::start_options;

    #[test]
    fn loading_checks_the_settings_and_the_kind_of_file() {
        let fruit = ClusterPreset::named("apple").unwrap();
        assert!(fruit.read_options("base: apple\n").is_ok());
        let error = fruit.read_options("fruit:\n  size: -1\n").unwrap_err();
        assert!(error.contains("size"), "{error}");

        // Each kind reads only its own files.
        let flower = ClusterPreset::named("dogwood_bloom").unwrap();
        assert_eq!(flower.file_kind(), "flower");
        assert!(flower.read_options("base: dogwood_bloom\n").is_ok());
        assert!(flower.read_options("base: apple\n").is_err());
        assert!(fruit.read_options("base: dogwood_bloom\n").is_err());
    }

    #[test]
    fn exports_have_every_level_when_asked() {
        let options = start_options();
        assert_eq!(
            meshes(&options, true).unwrap().len(),
            default_lod().levels.len()
        );
        assert_eq!(meshes(&options, false).unwrap().len(), 1);
    }
}
