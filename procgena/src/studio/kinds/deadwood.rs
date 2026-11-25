//! Dead Wood: one log, branch or stump, possibly covered, or a scatter of
//! sticks, from a `.deadwood.yml` preset.

use bevy::prelude::*;
use procgena_deadwood::DeadwoodPreset;

use super::{Kind, RegisterKind, entries};
use crate::deadwood::DeadwoodShow;
use crate::deadwood::editor::{
    DeadwoodEdited, DeadwoodEditor, DeadwoodEditorPlugin, DeadwoodPanel, spawn_deadwood_editor,
};
use crate::export::deadwood::spawn_deadwood_export;
use crate::params::ParamPanel;
use crate::scene::{DeadwoodSubject, Show};
use crate::studio::documents::DocumentEdited;
use crate::studio::documents::panels::settings_panel;
use crate::studio::status;

/// The steps of a dead wood piece's drawing chain, most detailed first.
pub const LEVELS: &[&str] = &["0 · Game", "1 · Lower", "2 · Impostor"];

/// Registers dead wood.
pub fn register(app: &mut App) {
    app.add_plugins(DeadwoodEditorPlugin)
        .add_observer(|edited: On<DeadwoodEdited>, mut commands: Commands| {
            commands.trigger(DocumentEdited {
                reframe: edited.reframe,
            });
        })
        .register_kind(Kind {
            id: "deadwood",
            label: "Dead Wood",
            presets: || entries(DeadwoodPreset::all(), |p| &p.key, |p| &p.name, |p| &p.group),
            start: crate::deadwood::START_PRESET,
            spawn_editor: |commands, page, key| {
                let editor = spawn_deadwood_editor(commands, page, key);
                spawn_deadwood_export(commands, page);
                editor
            },
            show,
            settings: Some(settings_panel::<DeadwoodPreset>()),
            detail_levels: LEVELS,
            hidden_environment: crate::environment::SINGLE_OBJECT,
            status_line: |state| status::leveled_line(state, "dead wood"),
        });
}

/// What the editor `editor` shows: its piece, view and cover.
fn show(world: &mut World, editor: Entity, document: u64) -> Option<Show> {
    let editor = world.get::<DeadwoodEditor>(editor)?;
    let (panel, show_panel) = (editor.panel(), editor.show_panel());
    let piece = world.get::<DeadwoodPanel>(panel)?.value().options().clone();
    let shown = *world.get::<ParamPanel<DeadwoodShow>>(show_panel)?.value();
    Some(Show::new::<DeadwoodSubject>(
        document,
        shown.settings(piece),
    ))
}
