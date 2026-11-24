//! Save and load tree settings as `.tree.yml` preset files, and export the
//! tree as a GLB or OBJ model for other 3D tools.
//!
//! Each function does the work after the user has chosen a file. Pass copies
//! of the settings taken when the user asked, so later edits do not change a
//! save or export already under way.

use std::path::Path;

use bevy::feathers::controls::FeathersButton;
use bevy::feathers::display::caption;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use procgena_tree::{TextureSize, TreeOptions, TreePreset};

use super::images::{ImageStore, fetch_images};
use super::tree::{self as model, ModelSettings};
use super::{
    Done, ExportImages, FileOps, chosen_stem, read_preset, save_preset, shown_name, write_file,
    write_files,
};
use crate::params::{ParamPanel, ParamPanelPlugin, Section, choice, spawn_param_panel, toggle};
use crate::tree::editor::{TreeEditor, load_tree};

/// Makes the tree's save, load and export controls work.
pub struct TreeExportPlugin;

impl Plugin for TreeExportPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ParamPanelPlugin::<ModelSettings>::default());
    }
}

/// Spawns the export settings and the save, load and export buttons for the
/// tree editor, as the last child of `parent`.
pub fn spawn_tree_export(commands: &mut Commands, parent: Entity) {
    let root = commands
        .spawn((
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0,
                row_gap: px(8),
                ..default()
            },
            ChildOf(parent),
        ))
        .id();
    spawn_param_panel(commands, root, ModelSettings::default(), sections(), &[]);
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
                    on(save_tree_preset)
                    --
                    @FeathersButton { @caption: bsn! { @caption("Load Preset…") } }
                    on(load_tree_preset)
                ]
                --
                Node { display: Display::Flex, column_gap: px(6), flex_shrink: 0.0 }
                Children [
                    @FeathersButton { @caption: bsn! { @caption("Export GLB…") } }
                    on(export_tree_glb)
                    --
                    @FeathersButton { @caption: bsn! { @caption("Export OBJ…") } }
                    on(export_tree_obj)
                ]
            ]
        })
        .insert(ChildOf(root));
}

/// The tree settings, their suggested file name, and the export settings,
/// copied so later edits do not change a save already under way.
fn snapshot(
    editor: &TreeEditor,
    panels: &Query<&ParamPanel<TreeOptions>>,
    settings: &Query<&ParamPanel<ModelSettings>>,
) -> Option<(TreeOptions, String, ModelSettings)> {
    let options = panels.get(editor.panel()).ok()?.value().clone();
    let settings = *settings.single().ok()?.value();
    Some((options, editor.file_stem(), settings))
}

/// Saves the tree settings as a `.tree.yml` preset file based on the chosen
/// preset.
fn save_tree_preset(
    _: On<Activate>,
    editor: Single<&TreeEditor>,
    panels: Query<&ParamPanel<TreeOptions>>,
    settings: Query<&ParamPanel<ModelSettings>>,
    mut files: FileOps,
) {
    if let Some((options, stem, _)) = snapshot(&editor, &panels, &settings) {
        let base = editor.preset();
        files.save(&format!("{stem}.tree.yml"), move |path| {
            save_preset(options, base, path)
        });
    }
}

/// Loads tree settings from a `.tree.yml` preset file into the editor.
fn load_tree_preset(_: On<Activate>, mut files: FileOps) {
    files.open(|text, world| {
        let options = read_options(text)?;
        world
            .run_system_cached_with(load_tree, options)
            .map_err(|error| format!("Cannot show the tree settings: {error}"))?;
        Ok(Done::new("Loaded the tree settings"))
    });
}

/// Exports the tree as a GLB model.
fn export_tree_glb(
    _: On<Activate>,
    editor: Single<&TreeEditor>,
    panels: Query<&ParamPanel<TreeOptions>>,
    settings: Query<&ParamPanel<ModelSettings>>,
    images: Res<ExportImages>,
    mut files: FileOps,
) {
    if let Some((options, stem, settings)) = snapshot(&editor, &panels, &settings) {
        let images = images.tree.clone();
        files.save(&format!("{stem}.glb"), move |path| {
            export_glb(&options, &settings, images.as_deref(), path)
        });
    }
}

/// Exports the tree as an OBJ model with its material and textures.
fn export_tree_obj(
    _: On<Activate>,
    editor: Single<&TreeEditor>,
    panels: Query<&ParamPanel<TreeOptions>>,
    settings: Query<&ParamPanel<ModelSettings>>,
    images: Res<ExportImages>,
    mut files: FileOps,
) {
    if let Some((options, stem, settings)) = snapshot(&editor, &panels, &settings) {
        let images = images.tree.clone();
        files.save(&format!("{stem}.obj"), move |path| {
            export_obj(&options, &settings, images.as_deref(), &stem, path)
        });
    }
}

/// Reads the tree settings of a `.tree.yml` preset file. Returns a readable
/// error for text that isn't a tree preset file or holds invalid settings.
pub fn read_options(text: &str) -> Result<TreeOptions, String> {
    read_preset::<TreePreset>(text, "tree")
}

/// Builds the tree and writes it to `path` as a GLB model.
///
/// Texture images come from `images`, downloading any not cached yet; images
/// that cannot be had are left out and listed in the result's warnings.
/// This blocks for the whole build, so call it off the UI thread.
pub fn export_glb(
    options: &TreeOptions,
    settings: &ModelSettings,
    images: Option<&ImageStore>,
    path: &Path,
) -> Result<Done, String> {
    let paths = model::image_paths(options, settings);
    let (found, warnings) = fetch_images(images, &paths);
    let bytes = model::glb(options, settings, &found).map_err(|error| error.to_string())?;
    write_file(path, &bytes)?;
    Ok(Done {
        message: format!(
            "Saved {} ({:.1} MB)",
            shown_name(path),
            bytes.len() as f64 / 1e6
        ),
        warnings,
    })
}

/// Builds the tree and writes it to `path` as an OBJ model, with its material
/// file and texture folder beside it.
///
/// The companion files are named after the chosen file, falling back to
/// `suggested_stem`. Images are fetched as for [`export_glb`], and this also
/// blocks for the whole build.
pub fn export_obj(
    options: &TreeOptions,
    settings: &ModelSettings,
    images: Option<&ImageStore>,
    suggested_stem: &str,
    path: &Path,
) -> Result<Done, String> {
    let paths = model::image_paths(options, settings);
    let (found, warnings) = fetch_images(images, &paths);
    let stem = chosen_stem(path, suggested_stem);
    let export = model::obj(options, settings, &found, &stem).map_err(|error| error.to_string())?;
    write_files(path, &export.files)?;
    Ok(Done {
        message: format!(
            "Saved {} with {} other files",
            shown_name(path),
            export.files.len() - 1
        ),
        warnings,
    })
}

/// Defines the texture resolution, detail level and animation rig controls.
pub fn sections() -> Vec<Section<ModelSettings>> {
    vec![Section {
        title: "Export",
        groups: vec![crate::params::Group::plain(vec![
            choice!(
                ModelSettings,
                "textures",
                "Textures",
                [
                    "Colours only" => None,
                    "512" => Some(TextureSize::Size512),
                    "1k" => Some(TextureSize::Size1k),
                    "2k" => Some(TextureSize::Size2k),
                ],
                |s| s.textures
            ),
            toggle!(ModelSettings, "detailLevels", "Detail Levels (GLB)", |s| s
                .detail_levels),
            toggle!(ModelSettings, "rig", "Rig (GLB)", |s| s.rig),
        ])],
    }]
}
