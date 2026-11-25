//! Save and reload dead wood settings as `.deadwood.yml` preset files, or
//! export the current piece as a GLB or OBJ model for use in another 3D
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
use procgena_deadwood::export::{gltf, obj};
use procgena_deadwood::{DeadwoodMesh, DeadwoodOptions, DeadwoodPreset, Piece};

use super::{
    Done, FileOps, chosen_stem, read_preset, save_preset, shown_name, write_file, write_files,
};
use crate::deadwood::editor::{DeadwoodEditor, DeadwoodPanel, load_deadwood};
use crate::params::{ParamPanel, ParamPanelPlugin, Section, spawn_param_panel, toggle};
use crate::studio::Dormant;

/// Selects whether an exported piece includes its simpler versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeadwoodModelSettings {
    /// For GLB, includes the lower mesh after the game-level one, as a
    /// separate object. Both are visible on import until the receiving
    /// tool hides or switches them. OBJ exports ignore this.
    pub detail_levels: bool,
}

impl Default for DeadwoodModelSettings {
    /// Includes the lower mesh in GLB exports.
    fn default() -> Self {
        Self {
            detail_levels: true,
        }
    }
}

/// Makes the dead wood save, load and export controls work.
pub struct DeadwoodExportPlugin;

impl Plugin for DeadwoodExportPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ParamPanelPlugin::<DeadwoodModelSettings>::default());
    }
}

/// Spawns the export settings and the save, load and export buttons for the
/// dead wood editor, as the last child of `parent`.
pub fn spawn_deadwood_export(commands: &mut Commands, parent: Entity) {
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
        DeadwoodModelSettings::default(),
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
                    on(save_deadwood_preset)
                    --
                    @FeathersButton { @caption: bsn! { @caption("Load Preset…") } }
                    on(load_deadwood_preset)
                ]
                --
                Node { display: Display::Flex, column_gap: px(6), flex_shrink: 0.0 }
                Children [
                    @FeathersButton { @caption: bsn! { @caption("Export GLB…") } }
                    on(export_deadwood_glb)
                    --
                    @FeathersButton { @caption: bsn! { @caption("Export OBJ…") } }
                    on(export_deadwood_obj)
                ]
            ]
        })
        .insert(ChildOf(root));
}

/// The dead wood settings, their suggested file name, and the export
/// settings, copied so later edits do not change a save already under way.
fn snapshot(
    editor: &DeadwoodEditor,
    panels: &Query<&DeadwoodPanel>,
    settings: &ModelPanels,
) -> Option<(DeadwoodOptions, String, DeadwoodModelSettings)> {
    let options = panels.get(editor.panel()).ok()?.value().options().clone();
    let settings = *settings
        .iter()
        .find(|(_, visible)| visible.get())?
        .0
        .value();
    Some((options, editor.file_stem(), settings))
}

/// The export settings panels, one per open piece of dead wood, with
/// whether each is on screen: only the selected piece's is.
type ModelPanels<'w, 's> = Query<
    'w,
    's,
    (
        &'static ParamPanel<DeadwoodModelSettings>,
        &'static InheritedVisibility,
    ),
>;

/// Saves the dead wood settings as a `.deadwood.yml` preset file based on
/// the chosen preset.
fn save_deadwood_preset(
    _: On<Activate>,
    editor: Single<&DeadwoodEditor, Without<Dormant>>,
    panels: Query<&DeadwoodPanel>,
    settings: ModelPanels,
    mut files: FileOps,
) {
    if let Some((options, stem, _)) = snapshot(&editor, &panels, &settings) {
        let base = editor.preset();
        files.save(&format!("{stem}.deadwood.yml"), move |path| {
            save_preset(options, base, path)
        });
    }
}

/// Loads dead wood settings from a `.deadwood.yml` preset file into the
/// editor.
fn load_deadwood_preset(_: On<Activate>, mut files: FileOps) {
    files.open(|text, world| {
        let options = read_options(text)?;
        world
            .run_system_cached_with(load_deadwood, options)
            .map_err(|error| format!("Cannot show the dead wood settings: {error}"))?;
        Ok(Done::new("Loaded the dead wood settings"))
    });
}

/// Exports the piece as a GLB model.
fn export_deadwood_glb(
    _: On<Activate>,
    editor: Single<&DeadwoodEditor, Without<Dormant>>,
    panels: Query<&DeadwoodPanel>,
    settings: ModelPanels,
    mut files: FileOps,
) {
    if let Some((options, stem, settings)) = snapshot(&editor, &panels, &settings) {
        files.save(&format!("{stem}.glb"), move |path| {
            export_glb(&options, &settings, path)
        });
    }
}

/// Exports the piece as an OBJ model with its material file.
fn export_deadwood_obj(
    _: On<Activate>,
    editor: Single<&DeadwoodEditor, Without<Dormant>>,
    panels: Query<&DeadwoodPanel>,
    settings: ModelPanels,
    mut files: FileOps,
) {
    if let Some((options, stem, _)) = snapshot(&editor, &panels, &settings) {
        files.save(&format!("{stem}.obj"), move |path| {
            export_obj(&options, &stem, path)
        });
    }
}

/// Reads the dead wood settings of a `.deadwood.yml` preset file and checks
/// their values. Returns a readable error for text that isn't a dead wood
/// preset file or for invalid settings.
pub fn read_options(text: &str) -> Result<DeadwoodOptions, String> {
    read_preset::<DeadwoodPreset>(text, "dead wood")
}

/// Builds the meshes to export: the piece's game-level mesh, then, with
/// `detail_levels`, its lower mesh.
fn meshes(options: &DeadwoodOptions, detail_levels: bool) -> Result<Vec<DeadwoodMesh>, String> {
    let piece =
        Piece::new(options.clone()).map_err(|error| format!("Cannot build this piece: {error}"))?;
    if !detail_levels {
        return Ok(vec![piece.mesh()]);
    }
    Ok(piece
        .lod_meshes()
        .into_iter()
        .map(|(_, mesh)| mesh)
        .collect())
}

/// Builds the piece and writes it to `path` as a GLB model. This blocks for
/// the whole build, so call it off the UI thread.
pub fn export_glb(
    options: &DeadwoodOptions,
    settings: &DeadwoodModelSettings,
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

/// Builds the piece's game-level mesh and writes it to `path` as an OBJ model,
/// with its material file beside it. The material file is named after the
/// chosen file, falling back to `suggested_stem`.
pub fn export_obj(
    options: &DeadwoodOptions,
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
pub fn sections() -> Vec<Section<DeadwoodModelSettings>> {
    vec![Section {
        title: "Export",
        groups: vec![crate::params::Group::plain(vec![toggle!(
            DeadwoodModelSettings,
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
        assert!(read_options("base: windthrow_log\n").is_ok());
        assert!(
            read_options("deadwood: [not settings]\n")
                .unwrap_err()
                .starts_with("Not dead wood settings")
        );
        let error = read_options("deadwood:\n  length: -1\n").unwrap_err();
        assert!(error.contains("length"), "{error}");
    }

    #[test]
    fn exports_have_every_level_when_asked() {
        let options = DeadwoodOptions::default();
        assert_eq!(meshes(&options, true).unwrap().len(), 2);
        assert_eq!(meshes(&options, false).unwrap().len(), 1);
        let path = std::env::temp_dir().join("procgena_deadwood_export_test.glb");
        export_glb(&options, &DeadwoodModelSettings::default(), &path).unwrap();
        let _ = std::fs::remove_file(path);
    }
}
