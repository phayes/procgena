//! Save and reload dead wood settings as JSON, or export the current piece
//! as a GLB or OBJ model for use in another 3D tool.
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
use procgena_deadwood::{DeadwoodDetail, DeadwoodLod, DeadwoodMesh, DeadwoodOptions, Piece};

use super::{Done, FileOps, chosen_stem, shown_name, write_file, write_files};
use crate::deadwood::editor::{DeadwoodEditor, load_deadwood};
use crate::params::{ParamPanel, ParamPanelPlugin, Section, spawn_param_panel, toggle};

/// Selects whether an exported piece includes its simpler versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeadwoodModelSettings {
    /// For GLB, includes every detail level after the most detailed one, as
    /// separate objects. All of them are visible on import until the
    /// receiving tool hides or switches them. OBJ exports ignore this.
    pub detail_levels: bool,
}

impl Default for DeadwoodModelSettings {
    /// Includes the simpler meshes in GLB exports.
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
                    @FeathersButton { @caption: bsn! { @caption("Save JSON…") } }
                    on(save_deadwood_json)
                    --
                    @FeathersButton { @caption: bsn! { @caption("Load JSON…") } }
                    on(load_deadwood_json)
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
    panels: &Query<&ParamPanel<DeadwoodOptions>>,
    settings: &Query<&ParamPanel<DeadwoodModelSettings>>,
) -> Option<(DeadwoodOptions, String, DeadwoodModelSettings)> {
    let options = panels.get(editor.panel()).ok()?.value().clone();
    let settings = *settings.single().ok()?.value();
    Some((options, editor.file_stem(), settings))
}

/// Saves the dead wood settings as JSON.
fn save_deadwood_json(
    _: On<Activate>,
    editor: Single<&DeadwoodEditor>,
    panels: Query<&ParamPanel<DeadwoodOptions>>,
    settings: Query<&ParamPanel<DeadwoodModelSettings>>,
    mut files: FileOps,
) {
    if let Some((options, stem, _)) = snapshot(&editor, &panels, &settings) {
        files.save(&format!("{stem}.json"), move |path| {
            save_json(&options, path)
        });
    }
}

/// Loads dead wood settings from JSON into the editor.
fn load_deadwood_json(_: On<Activate>, mut files: FileOps) {
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
    editor: Single<&DeadwoodEditor>,
    panels: Query<&ParamPanel<DeadwoodOptions>>,
    settings: Query<&ParamPanel<DeadwoodModelSettings>>,
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
    editor: Single<&DeadwoodEditor>,
    panels: Query<&ParamPanel<DeadwoodOptions>>,
    settings: Query<&ParamPanel<DeadwoodModelSettings>>,
    mut files: FileOps,
) {
    if let Some((options, stem, _)) = snapshot(&editor, &panels, &settings) {
        files.save(&format!("{stem}.obj"), move |path| {
            export_obj(&options, &stem, path)
        });
    }
}

/// Writes `options` to `path` as JSON.
pub fn save_json(options: &DeadwoodOptions, path: &Path) -> Result<Done, String> {
    write_file(path, options.to_json().as_bytes())?;
    Ok(Done::new(format!("Saved {}", shown_name(path))))
}

/// Parses JSON dead wood settings and checks their values. Returns a
/// readable error for malformed JSON or invalid settings.
pub fn read_options(text: &str) -> Result<DeadwoodOptions, String> {
    let options = DeadwoodOptions::from_json(text)
        .map_err(|error| format!("Not dead wood settings: {error}"))?;
    options
        .validate()
        .map_err(|error| format!("These dead wood settings cannot be used: {error}"))?;
    Ok(options)
}

/// Builds the meshes to export: the full mesh, then, with `detail_levels`,
/// each simpler default level, sized for the piece.
fn meshes(options: &DeadwoodOptions, detail_levels: bool) -> Result<Vec<DeadwoodMesh>, String> {
    let piece =
        Piece::new(options.clone()).map_err(|error| format!("Cannot build this piece: {error}"))?;
    if !detail_levels {
        return Ok(vec![piece.mesh_with_detail(&DeadwoodDetail::FULL)]);
    }
    let lod = DeadwoodLod::for_options(options);
    Ok(piece
        .lod_meshes(&lod)
        .map_err(|error| error.to_string())?
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

/// Builds the piece's full mesh and writes it to `path` as an OBJ model,
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
        assert!(read_options(&DeadwoodOptions::default().to_json()).is_ok());
        assert!(
            read_options("not json")
                .unwrap_err()
                .starts_with("Not dead wood settings")
        );
        let error = read_options(r#"{"length": -1}"#).unwrap_err();
        assert!(error.contains("cannot be used"), "{error}");
    }

    #[test]
    fn exports_have_every_level_when_asked() {
        let options = DeadwoodOptions::default();
        let levels = DeadwoodLod::default().levels.len();
        assert_eq!(meshes(&options, true).unwrap().len(), levels);
        assert_eq!(meshes(&options, false).unwrap().len(), 1);
        let path = std::env::temp_dir().join("procgena_deadwood_export_test.glb");
        export_glb(&options, &DeadwoodModelSettings::default(), &path).unwrap();
        let _ = std::fs::remove_file(path);
    }
}
