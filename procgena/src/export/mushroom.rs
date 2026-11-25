//! Save and reload mushroom settings as `.mushroom.yml` preset files, or
//! export the current mushroom as a GLB or OBJ model for use in another 3D
//! tool.
//!
//! Each function does the work after the user has chosen a file. Pass copies
//! of the settings taken when the user asked, so later edits do not change a
//! save or export already under way. Models carry vertex colours; textures
//! are not exported.

use std::path::Path;

use bevy::feathers::controls::FeathersButton;
use bevy::feathers::display::caption;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use procgena_mushroom::export::{gltf, obj};
use procgena_mushroom::{Mushroom, MushroomMesh, MushroomOptions, MushroomPreset};

use super::{
    Done, FileOps, chosen_stem, read_preset, save_preset, shown_name, write_file, write_files,
};
use crate::mushrooms::editor::{MushroomEditor, MushroomPanel, load_mushroom};
use crate::params::{ParamPanel, ParamPanelPlugin, Section, spawn_param_panel, toggle};
use crate::studio::Dormant;

/// Selects whether an exported mushroom includes its simpler versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MushroomModelSettings {
    /// For GLB, includes every detail level after the most detailed one, as
    /// separate objects. All of them are visible on import until the
    /// receiving tool hides or switches them. OBJ exports ignore this.
    pub detail_levels: bool,
}

impl Default for MushroomModelSettings {
    /// Includes the simpler meshes in GLB exports.
    fn default() -> Self {
        Self {
            detail_levels: true,
        }
    }
}

/// Makes the mushroom's save, load and export controls work.
pub struct MushroomExportPlugin;

impl Plugin for MushroomExportPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ParamPanelPlugin::<MushroomModelSettings>::default());
    }
}

/// Spawns the export settings and the save, load and export buttons for the
/// mushroom editor, as the last child of `parent`.
pub fn spawn_mushroom_export(commands: &mut Commands, parent: Entity) {
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
        MushroomModelSettings::default(),
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
                    on(save_mushroom_preset)
                    --
                    @FeathersButton { @caption: bsn! { @caption("Load Preset…") } }
                    on(load_mushroom_preset)
                ]
                --
                Node { display: Display::Flex, column_gap: px(6), flex_shrink: 0.0 }
                Children [
                    @FeathersButton { @caption: bsn! { @caption("Export GLB…") } }
                    on(export_mushroom_glb)
                    --
                    @FeathersButton { @caption: bsn! { @caption("Export OBJ…") } }
                    on(export_mushroom_obj)
                ]
            ]
        })
        .insert(ChildOf(root));
}

/// The mushroom settings, their suggested file name, and the export settings,
/// copied so later edits do not change a save already under way.
fn snapshot(
    editor: &MushroomEditor,
    panels: &Query<&MushroomPanel>,
    settings: &ModelPanels,
) -> Option<(MushroomOptions, String, MushroomModelSettings)> {
    let options = panels.get(editor.panel()).ok()?.value().options().clone();
    let settings = *settings
        .iter()
        .find(|(_, visible)| visible.get())?
        .0
        .value();
    Some((options, editor.file_stem(), settings))
}

/// The export settings panels, one per open mushroom, with whether each is
/// on screen: only the selected mushroom's is.
type ModelPanels<'w, 's> = Query<
    'w,
    's,
    (
        &'static ParamPanel<MushroomModelSettings>,
        &'static InheritedVisibility,
    ),
>;

/// Saves the mushroom settings as a `.mushroom.yml` preset file based on
/// the chosen preset.
fn save_mushroom_preset(
    _: On<Activate>,
    editor: Single<&MushroomEditor, Without<Dormant>>,
    panels: Query<&MushroomPanel>,
    settings: ModelPanels,
    mut files: FileOps,
) {
    if let Some((options, stem, _)) = snapshot(&editor, &panels, &settings) {
        let base = editor.preset();
        files.save(&format!("{stem}.mushroom.yml"), move |path| {
            save_preset(options, base, path)
        });
    }
}

/// Loads mushroom settings from a `.mushroom.yml` preset file into the
/// editor.
fn load_mushroom_preset(_: On<Activate>, mut files: FileOps) {
    files.open(|text, world| {
        let options = read_options(text)?;
        world
            .run_system_cached_with(load_mushroom, options)
            .map_err(|error| format!("Cannot show the mushroom settings: {error}"))?;
        Ok(Done::new("Loaded the mushroom settings"))
    });
}

/// Exports the mushroom as a GLB model.
fn export_mushroom_glb(
    _: On<Activate>,
    editor: Single<&MushroomEditor, Without<Dormant>>,
    panels: Query<&MushroomPanel>,
    settings: ModelPanels,
    mut files: FileOps,
) {
    if let Some((options, stem, settings)) = snapshot(&editor, &panels, &settings) {
        files.save(&format!("{stem}.glb"), move |path| {
            export_glb(&options, &settings, path)
        });
    }
}

/// Exports the mushroom as an OBJ model with its material file.
fn export_mushroom_obj(
    _: On<Activate>,
    editor: Single<&MushroomEditor, Without<Dormant>>,
    panels: Query<&MushroomPanel>,
    settings: ModelPanels,
    mut files: FileOps,
) {
    if let Some((options, stem, _)) = snapshot(&editor, &panels, &settings) {
        files.save(&format!("{stem}.obj"), move |path| {
            export_obj(&options, &stem, path)
        });
    }
}

/// Reads the mushroom settings of a `.mushroom.yml` preset file and checks
/// their values. Returns a readable error for text that isn't a mushroom
/// preset file or for invalid settings.
pub fn read_options(text: &str) -> Result<MushroomOptions, String> {
    read_preset::<MushroomPreset>(text, "mushroom")
}

/// Builds the meshes to export: the game-level mesh, then, with
/// `detail_levels`, the cheaper lower mesh derived from it.
fn meshes(options: &MushroomOptions, detail_levels: bool) -> Result<Vec<MushroomMesh>, String> {
    let mushroom = Mushroom::new(options.clone())
        .map_err(|error| format!("Cannot build this mushroom: {error}"))?;
    if !detail_levels {
        return Ok(vec![mushroom.mesh()]);
    }
    Ok(mushroom
        .lod_meshes()
        .into_iter()
        .map(|(_, mesh)| mesh)
        .collect())
}

/// Builds the mushroom and writes it to `path` as a GLB model. This blocks
/// for the whole build, so call it off the UI thread.
pub fn export_glb(
    options: &MushroomOptions,
    settings: &MushroomModelSettings,
    path: &Path,
) -> Result<Done, String> {
    let meshes = meshes(options, settings.detail_levels)?;
    let bytes = gltf::to_glb(&meshes, options).map_err(|error| error.to_string())?;
    write_file(path, &bytes)?;
    Ok(Done::new(format!(
        "Saved {} ({:.1} MB)",
        shown_name(path),
        bytes.len() as f64 / 1e6
    )))
}

/// Builds the mushroom's full mesh and writes it to `path` as an OBJ model,
/// with its material file beside it. The material file is named after the
/// chosen file, falling back to `suggested_stem`.
pub fn export_obj(
    options: &MushroomOptions,
    suggested_stem: &str,
    path: &Path,
) -> Result<Done, String> {
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

/// Builds the export controls.
pub fn sections() -> Vec<Section<MushroomModelSettings>> {
    vec![Section {
        title: "Export",
        groups: vec![crate::params::Group::plain(vec![toggle!(
            MushroomModelSettings,
            "detailLevels",
            "Detail Levels (GLB)",
            |s| s.detail_levels
        )])],
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_checks_the_settings() {
        assert!(read_options("base: fly_agaric\n").is_ok());
        assert!(
            read_options("mushroom: [not settings]\n")
                .unwrap_err()
                .starts_with("Not mushroom settings")
        );
        let error = read_options("mushroom:\n  size: -1\n").unwrap_err();
        assert!(error.contains("size"), "{error}");
    }

    #[test]
    fn exports_have_every_level_when_asked() {
        let options = MushroomOptions::default();
        // The game-level and lower meshes.
        assert_eq!(meshes(&options, true).unwrap().len(), 2);
        assert_eq!(meshes(&options, false).unwrap().len(), 1);
    }
}
