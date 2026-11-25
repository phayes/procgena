//! Fruit and flowers: one fruit cluster, from a `.fruit.yml` preset, or one
//! flower cluster, from a `.flower.yml` preset.

use bevy::prelude::*;
use procgena_fruit::{FlowerPreset, FruitPreset};

use super::{Kind, RegisterKind, entries};
use crate::fruit::editor::{
    FruitEdited, FruitEditor, FruitEditorPlugin, FruitPanel, spawn_fruit_editor,
};
use crate::scene::{FruitSubject, Show};
use crate::studio::documents::DocumentEdited;
use crate::studio::status;

/// Registers fruit, with the editor shared by fruit and flowers.
pub fn register_fruit(app: &mut App) {
    app.add_plugins(FruitEditorPlugin)
        .add_observer(|edited: On<FruitEdited>, mut commands: Commands| {
            commands.trigger(DocumentEdited {
                reframe: edited.reframe,
            });
        })
        .register_kind(Kind {
            id: "fruit",
            label: "Fruit",
            presets: || entries(FruitPreset::all(), |p| &p.key, |p| &p.name, |p| &p.group),
            start: crate::fruit::START_FRUIT,
            spawn_editor: spawn_fruit_editor,
            show,
            detail_levels: &[],
            hidden_environment: crate::environment::STILL_AIR,
            status_line: |state| status::object_line(state, "fruit"),
        });
}

/// Registers flowers. Call [`register_fruit`] too, which adds their editor.
pub fn register_flower(app: &mut App) {
    app.register_kind(Kind {
        id: "flower",
        label: "Flowers",
        presets: || entries(FlowerPreset::all(), |p| &p.key, |p| &p.name, |p| &p.group),
        start: crate::fruit::START_FLOWER,
        spawn_editor: spawn_fruit_editor,
        show,
        detail_levels: &[],
        hidden_environment: crate::environment::STILL_AIR,
        status_line: |state| status::object_line(state, "flower"),
    });
}

/// The cluster the editor `editor` holds.
fn show(world: &mut World, editor: Entity, document: u64) -> Option<Show> {
    let panel = world.get::<FruitEditor>(editor)?.panel();
    let options = world.get::<FruitPanel>(panel)?.value().options().clone();
    Some(Show::new::<FruitSubject>(document, options))
}
