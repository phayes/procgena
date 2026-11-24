//! Tree Scatters: a grove of tree species, from a `.tree_scatter.yml`
//! preset.

use bevy::prelude::*;
use procgena_tree::{TreeScatter, TreeScatterPreset};

use super::{Kind, RegisterKind, entries};
use crate::grove::editor::{GroveEdited, GroveEditorPlugin, GroveReader, spawn_grove_editor};
use crate::scene::{GroveSubject, Show};
use crate::studio::documents::DocumentEdited;
use crate::studio::status;

/// Registers tree Scatters.
pub fn register(app: &mut App) {
    app.add_plugins(GroveEditorPlugin)
        .add_observer(|_: On<GroveEdited>, mut commands: Commands| {
            commands.trigger(DocumentEdited::default());
        })
        .register_kind(Kind {
            id: "tree_scatter",
            label: "Tree Scatter",
            presets: || {
                entries(
                    TreeScatterPreset::all(),
                    |p| &p.key,
                    |p| &p.name,
                    |p| &p.group,
                )
            },
            start: crate::grove::START_PRESET,
            spawn_editor: spawn_grove_editor,
            show,
            detail_levels: &[],
            hidden_environment: crate::environment::TREE_SCATTER,
            status_line: status::grove_line,
        });
}

/// The tree Scatter the editor `editor` holds.
fn show(world: &mut World, editor: Entity, document: u64) -> Option<Show> {
    let grove = world.run_system_cached_with(grove_of, editor).ok()??;
    Some(Show::new::<GroveSubject>(document, grove))
}

/// The tree Scatter of the editor `editor`, if it is a tree Scatter editor.
fn grove_of(In(editor): In<Entity>, reader: GroveReader) -> Option<TreeScatter> {
    reader.grove_of(editor)
}
