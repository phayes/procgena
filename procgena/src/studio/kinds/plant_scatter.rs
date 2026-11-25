//! Plant Scatters: a field of mixed plants, from a `.plant_scatter.yml`
//! preset.

use bevy::prelude::*;
use procgena_plant_bevy::prelude::PlantScatterPreset;

use super::{Kind, RegisterKind, entries};
use crate::field::editor::{FieldEdited, FieldEditorPlugin, FieldReader, spawn_field_editor};
use crate::scene::{FieldSettings, PlantsSubject, Show};
use crate::studio::documents::DocumentEdited;
use crate::studio::status;

/// Registers Plant Scatters. Register plants first: their editor plugin
/// also edits the Plant Scatter's plants.
pub fn register(app: &mut App) {
    app.add_plugins(FieldEditorPlugin)
        .add_observer(|_: On<FieldEdited>, mut commands: Commands| {
            commands.trigger(DocumentEdited::default());
        })
        .register_kind(Kind {
            id: "plant_scatter",
            label: "Plant Scatter",
            presets: || {
                entries(
                    PlantScatterPreset::all(),
                    |p| &p.key,
                    |p| &p.name,
                    |p| &p.group,
                )
            },
            start: crate::field::START_PRESET,
            spawn_editor: spawn_field_editor,
            show,
            settings: None,
            detail_levels: super::plant::LEVELS,
            hidden_environment: crate::environment::PLANTS,
            status_line: status::plants_line,
        });
}

/// The field the editor `editor` holds, as edited.
fn show(world: &mut World, editor: Entity, document: u64) -> Option<Show> {
    let field = world
        .run_system_cached_with(
            |In(editor): In<Entity>, reader: FieldReader| reader.field_of(editor),
            editor,
        )
        .ok()??;
    Some(Show::new::<PlantsSubject>(
        document,
        FieldSettings { field },
    ))
}
