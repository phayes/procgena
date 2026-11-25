//! Terrain: an endless world of land, from a `.terrain.yml` preset.

use bevy::prelude::*;
use procgena_terrain::{TerrainOptions, TerrainPreset};

use super::{Kind, RegisterKind, entries};
use crate::scene::{Show, TerrainSubject};
use crate::studio::documents::DocumentEdited;
use crate::studio::documents::panels::settings_panel;
use crate::studio::status;
use crate::terrain::editor::{
    TerrainEdited, TerrainEditor, TerrainEditorPlugin, spawn_terrain_editor,
};

/// Registers terrain.
pub fn register(app: &mut App) {
    app.add_plugins(TerrainEditorPlugin)
        .add_observer(|edited: On<TerrainEdited>, mut commands: Commands| {
            commands.trigger(DocumentEdited {
                reframe: edited.reframe,
            });
        })
        .register_kind(Kind {
            id: "terrain",
            label: "Terrain",
            presets: || entries(TerrainPreset::all(), |p| &p.key, |p| &p.name, |p| &p.group),
            start: crate::terrain::START_PRESET,
            spawn_editor: spawn_terrain_editor,
            show,
            settings: Some(settings_panel::<TerrainOptions>()),
            detail_levels: &[],
            hidden_environment: crate::environment::SINGLE_OBJECT,
            status_line: status::terrain_line,
        });
}

/// The terrain the editor `editor` holds, as last settled.
fn show(world: &mut World, editor: Entity, document: u64) -> Option<Show> {
    let settings = world.get::<TerrainEditor>(editor)?.settled().clone();
    Some(Show::new::<TerrainSubject>(document, settings))
}
