//! Save and reload rock settings as `.stone.yml` preset files, or export the
//! current rock as a GLB or OBJ model for use in another 3D tool.
//!
//! Each function does the work after the user has chosen a file. Pass copies
//! of the settings taken when the user asked, so later edits do not change a
//! save or export already under way.

use std::path::Path;

use bevy::feathers::controls::FeathersButton;
use bevy::feathers::display::caption;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use procgena_stone::{StoneOptions, StonePreset, TEXTURE_SETS};

use super::images::{ImageStore, fetch_images};
use super::stone::{self as model, StoneModelSettings};
use super::{
    Done, ExportImages, FileOps, chosen_stem, read_preset, save_preset, shown_name, write_file,
    write_files,
};
use crate::params::{ParamPanel, ParamPanelPlugin, Section, spawn_param_panel, toggle};
use crate::stones::editor::{StoneEditor, load_stone};
use crate::studio::Dormant;

/// Makes the rock's save, load and export controls work.
pub struct StoneExportPlugin;

impl Plugin for StoneExportPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ParamPanelPlugin::<StoneModelSettings>::default());
    }
}

/// Spawns the export settings and the save, load and export buttons for the
/// rock editor, as the last child of `parent`.
pub fn spawn_stone_export(commands: &mut Commands, parent: Entity) {
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
    spawn_param_panel(
        commands,
        root,
        StoneModelSettings::default(),
        sections(),
        &[],
    );
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
                    on(save_stone_preset)
                    --
                    @FeathersButton { @caption: bsn! { @caption("Load Preset…") } }
                    on(load_stone_preset)
                ]
                --
                Node { display: Display::Flex, column_gap: px(6), flex_shrink: 0.0 }
                Children [
                    @FeathersButton { @caption: bsn! { @caption("Export GLB…") } }
                    on(export_stone_glb)
                    --
                    @FeathersButton { @caption: bsn! { @caption("Export OBJ…") } }
                    on(export_stone_obj)
                ]
            ]
        })
        .insert(ChildOf(root));
}

/// The rock settings, their suggested file name, and the export settings,
/// copied so later edits do not change a save already under way.
fn snapshot(
    editor: &StoneEditor,
    panels: &Query<&ParamPanel<StoneOptions>>,
    settings: &ModelPanels,
) -> Option<(StoneOptions, String, StoneModelSettings)> {
    let options = panels.get(editor.panel()).ok()?.value().clone();
    let settings = *settings
        .iter()
        .find(|(_, visible)| visible.get())?
        .0
        .value();
    Some((options, editor.file_stem(), settings))
}

/// The export settings panels, one per open rock, with whether each is on
/// screen: only the selected rock's is.
type ModelPanels<'w, 's> = Query<
    'w,
    's,
    (
        &'static ParamPanel<StoneModelSettings>,
        &'static InheritedVisibility,
    ),
>;

/// Saves the rock settings as a `.stone.yml` preset file based on the chosen
/// preset.
fn save_stone_preset(
    _: On<Activate>,
    editor: Single<&StoneEditor, Without<Dormant>>,
    panels: Query<&ParamPanel<StoneOptions>>,
    settings: ModelPanels,
    mut files: FileOps,
) {
    if let Some((options, stem, _)) = snapshot(&editor, &panels, &settings) {
        let base = editor.preset();
        files.save(&format!("{stem}.stone.yml"), move |path| {
            save_preset(options, base, path)
        });
    }
}

/// Loads rock settings from a `.stone.yml` preset file into the editor.
fn load_stone_preset(_: On<Activate>, mut files: FileOps) {
    files.open(|text, world| {
        let options = read_options(text)?;
        world
            .run_system_cached_with(load_stone, options)
            .map_err(|error| format!("Cannot show the rock settings: {error}"))?;
        Ok(Done::new("Loaded the rock settings"))
    });
}

/// Exports the rock as a GLB model.
fn export_stone_glb(
    _: On<Activate>,
    editor: Single<&StoneEditor, Without<Dormant>>,
    panels: Query<&ParamPanel<StoneOptions>>,
    settings: ModelPanels,
    images: Res<ExportImages>,
    mut files: FileOps,
) {
    if let Some((options, stem, settings)) = snapshot(&editor, &panels, &settings) {
        let images = images.stone.clone();
        files.save(&format!("{stem}.glb"), move |path| {
            export_glb(&options, &settings, images.as_deref(), path)
        });
    }
}

/// Exports the rock as an OBJ model with its material and textures.
fn export_stone_obj(
    _: On<Activate>,
    editor: Single<&StoneEditor, Without<Dormant>>,
    panels: Query<&ParamPanel<StoneOptions>>,
    settings: ModelPanels,
    images: Res<ExportImages>,
    mut files: FileOps,
) {
    if let Some((options, stem, settings)) = snapshot(&editor, &panels, &settings) {
        let images = images.stone.clone();
        files.save(&format!("{stem}.obj"), move |path| {
            export_obj(&options, &settings, images.as_deref(), &stem, path)
        });
    }
}

/// Reads the rock settings of a `.stone.yml` preset file and checks their
/// values and texture name. Returns a readable error for text that isn't a
/// rock preset file, invalid settings or unavailable textures.
pub fn read_options(text: &str) -> Result<StoneOptions, String> {
    let options = read_preset::<StonePreset>(text, "rock")?;
    if !TEXTURE_SETS.contains(&options.material.texture.as_str()) {
        return Err(format!(
            "These rock settings use the texture {:?}, which this app does not have",
            options.material.texture
        ));
    }
    Ok(options)
}

/// Builds the rock and writes it to `path` as a GLB model.
///
/// Texture images come from `images`, downloading any not cached yet; images
/// that cannot be had are left out and listed in the result's warnings.
/// This blocks for the whole build, so call it off the UI thread.
pub fn export_glb(
    options: &StoneOptions,
    settings: &StoneModelSettings,
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

/// Builds the rock and writes it to `path` as an OBJ model, with its material
/// file and textures beside it.
///
/// The companion files are named after the chosen file, falling back to
/// `suggested_stem`. Images are fetched as for [`export_glb`], and this also
/// blocks for the whole build.
pub fn export_obj(
    options: &StoneOptions,
    settings: &StoneModelSettings,
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

/// Builds the export controls for texture images and optional simpler GLB meshes.
/// Image size and mesh resolutions come from the rock's Detail setting.
pub fn sections() -> Vec<Section<StoneModelSettings>> {
    vec![Section {
        title: "Export",
        groups: vec![crate::params::Group::plain(vec![
            toggle!(StoneModelSettings, "textures", "Textures", |s| s.textures),
            toggle!(
                StoneModelSettings,
                "detailLevels",
                "Detail Levels (GLB)",
                |s| s.detail_levels
            ),
        ])],
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_checks_the_settings() {
        assert!(read_options("base: pebble\n").is_ok());
        assert!(
            read_options("stone: [not settings]\n")
                .unwrap_err()
                .starts_with("Not rock settings")
        );
        let error = read_options("stone:\n  scale: -1\n").unwrap_err();
        assert!(error.contains("scale"), "{error}");
        let error = read_options("stone:\n  material:\n    texture: marble\n").unwrap_err();
        assert!(error.contains("marble"), "{error}");
    }
}
