//! Ecotopes: Scatters of trees, stones, dead wood and mushrooms on a patch
//! of ground, from a `.ecotope.yml` preset.

use bevy::prelude::*;
use procgena_biome::EcotopePreset;

use super::{Kind, RegisterKind, entries};
use crate::ecotope::editor::{
    EcotopeEdited, EcotopeEditor, EcotopeEditorPlugin, spawn_ecotope_editor,
};
use crate::scene::{EcotopeSubject, Show};
use crate::studio::documents::DocumentEdited;
use crate::studio::status;

/// Registers Ecotopes.
pub fn register(app: &mut App) {
    app.add_plugins(EcotopeEditorPlugin)
        .add_observer(|edited: On<EcotopeEdited>, mut commands: Commands| {
            commands.trigger(DocumentEdited {
                reframe: edited.reframe,
            });
        })
        .register_kind(Kind {
            id: "ecotope",
            label: "Ecotope",
            presets: || entries(EcotopePreset::all(), |p| &p.key, |p| &p.name, |p| &p.group),
            start: crate::ecotope::START_PRESET,
            spawn_editor: spawn_ecotope_editor,
            show,
            detail_levels: &[],
            hidden_environment: crate::environment::SINGLE_OBJECT,
            status_line: status::ecotope_line,
        });
}

/// The Ecotope the editor `editor` holds, on its ground.
fn show(world: &mut World, editor: Entity, document: u64) -> Option<Show> {
    let editor = world.get::<EcotopeEditor>(editor)?;
    let settings = crate::ecotope::settings(editor.ecotope(), editor.view());
    Some(Show::new::<EcotopeSubject>(document, settings))
}
