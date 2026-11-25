//! Plant Scatters: a field of mixed plants, from a `.plant_scatter.yml`
//! preset.

use bevy::prelude::*;
use procgena_core::preset::{from_value, to_value};
use procgena_plant_bevy::prelude::{PlantScatterOptions, PlantScatterPreset};

use super::{Kind, RegisterKind, entries};
use crate::field::editor::{
    FieldEdited, FieldEditorPlugin, FieldReader, load_field, spawn_field_editor,
};
use crate::scene::{FieldSettings, PlantsSubject, Show};
use crate::studio::documents::DocumentEdited;
use crate::studio::documents::history::EditorSettings;
use crate::studio::documents::panels::resolved;
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
            settings: Some(EditorSettings {
                value: |world, editor| {
                    let field = world
                        .run_system_cached_with(
                            |In(editor): In<Entity>, reader: FieldReader| reader.field_of(editor),
                            editor,
                        )
                        .ok()??;
                    Some(to_value(&field))
                },
                resolve: resolved::<PlantScatterPreset>,
                load: |world, _, value| {
                    let field: PlantScatterOptions =
                        from_value(value.clone(), "").map_err(|error| error.to_string())?;
                    world
                        .run_system_cached_with(load_field, field)
                        .map_err(|error| error.to_string())
                },
            }),
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
