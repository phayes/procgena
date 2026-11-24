//! Trees: one tree, from a `.tree.yml` preset.

use bevy::prelude::*;
use procgena_tree::{TreeOptions, TreePreset};

use super::{Kind, RegisterKind, entries};
use crate::export::tree_panel::spawn_tree_export;
use crate::params::ParamPanel;
use crate::scene::{Show, TreeSubject};
use crate::studio::documents::DocumentEdited;
use crate::studio::status;
use crate::tree::editor::{TreeEdited, TreeEditor, TreeEditorPlugin, spawn_tree_editor};

/// The tree detail levels, most detailed first.
pub const LEVELS: &[&str] = &["0 · Full", "1 · Medium", "2 · Low", "3 · Lowest"];

/// Registers trees.
pub fn register(app: &mut App) {
    app.add_plugins(TreeEditorPlugin)
        .add_observer(|_: On<TreeEdited>, mut commands: Commands| {
            commands.trigger(DocumentEdited::default());
        })
        .register_kind(Kind {
            id: "tree",
            label: "Trees",
            presets: || entries(TreePreset::all(), |p| &p.key, |p| &p.name, |p| &p.group),
            start: crate::tree::START_PRESET,
            spawn_editor: |commands, page, key| {
                let editor = spawn_tree_editor(commands, page, key);
                spawn_tree_export(commands, page);
                editor
            },
            show,
            detail_levels: LEVELS,
            hidden_environment: crate::environment::SINGLE_OBJECT,
            status_line: |state| status::leveled_line(state, "tree"),
        });
}

/// The tree the editor `editor` holds.
fn show(world: &mut World, editor: Entity, document: u64) -> Option<Show> {
    let panel = world.get::<TreeEditor>(editor)?.panel();
    let options = world.get::<ParamPanel<TreeOptions>>(panel)?.value().clone();
    Some(Show::new::<TreeSubject>(document, options))
}
