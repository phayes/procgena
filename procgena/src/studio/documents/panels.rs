//! [`EditorSettings`] for editors that keep their preset's settings in one
//! Inspector panel of [`Settings<T>`].

use bevy::prelude::*;
use procgena_core::preset::from_value;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::history::EditorSettings;
use crate::params::{ParamPanel, ParamsChanged};
use crate::studio::inspector::{NestedPanel, Settings};

/// The [`EditorSettings`] of an editor that keeps its preset's options `T`
/// in a panel of [`Settings<T>`] somewhere inside it, not marked
/// [`NestedPanel`]. Loading settings shows them in that panel and tells
/// the editor with [`ParamsChanged`], as a user edit would.
pub fn settings_panel<T>() -> EditorSettings
where
    T: Serialize + DeserializeOwned + Clone + PartialEq + Send + Sync + 'static,
{
    EditorSettings {
        value: panel_value::<T>,
        load: load_panel::<T>,
    }
}

/// The settings in `editor`'s panel of `Settings<T>`.
fn panel_value<T>(world: &World, editor: Entity) -> Option<Value>
where
    T: Send + Sync + 'static,
{
    let panel = find_panel::<T>(world, editor)?;
    Some(
        world
            .get::<ParamPanel<Settings<T>>>(panel)?
            .value()
            .value()
            .clone(),
    )
}

/// Shows `value` in `editor`'s panel of `Settings<T>`.
fn load_panel<T>(world: &mut World, editor: Entity, value: &Value) -> Result<(), String>
where
    T: Serialize + DeserializeOwned + Clone + PartialEq + Send + Sync + 'static,
{
    let options: T = from_value(value.clone(), "").map_err(|error| error.to_string())?;
    let panel = find_panel::<T>(world, editor).ok_or("the editor has no settings panel yet")?;
    let mut shown = world
        .get_mut::<ParamPanel<Settings<T>>>(panel)
        .ok_or("the editor has no settings panel yet")?;
    shown.set_value(Settings::new(options));
    world.trigger(ParamsChanged {
        entity: panel,
        is_final: true,
    });
    Ok(())
}

/// The first panel of `Settings<T>` at or below `editor`, outside nested
/// panels, looking breadth first.
fn find_panel<T>(world: &World, editor: Entity) -> Option<Entity>
where
    T: Send + Sync + 'static,
{
    let mut queue = std::collections::VecDeque::from([editor]);
    while let Some(entity) = queue.pop_front() {
        let Ok(found) = world.get_entity(entity) else {
            continue;
        };
        if found.contains::<ParamPanel<Settings<T>>>() && !found.contains::<NestedPanel>() {
            return Some(entity);
        }
        if let Some(children) = found.get::<Children>() {
            queue.extend(children.iter());
        }
    }
    None
}
