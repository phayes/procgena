//! What a document's settings are now, as last saved, and the edits to
//! undo and redo between them.
//!
//! The document is the authority on its settings: [`DocumentSettings`]
//! holds them as a settings tree in file form, and the editor in the
//! Inspector shows them. Every change the editor makes is recorded as one
//! undo step of [`Edit`]s; a slider drag is one step. Undo and redo load
//! the settings back into the editor.

use bevy::prelude::*;
use procgena_core::meta::{Edit, SettingPath};
use serde_json::{Map, Value};

use super::{Document, Selection};
use crate::studio::inspector::InspectorEdited;
use crate::studio::kinds::Kinds;

/// A document's settings: as the editor has them now, and as last saved
/// or opened, with the steps to undo and redo.
///
/// Added once the document's editor shows its preset. Kinds that can't
/// read their editor's settings (see
/// [`Kind::settings`](crate::studio::kinds::Kind::settings)) have none.
#[derive(Component, Debug, Clone)]
pub struct DocumentSettings {
    value: Value,
    saved: Value,
    undo: Vec<Vec<Edit>>,
    redo: Vec<Vec<Edit>>,
    /// Whether the last undo step is a drag still going on, which the next
    /// change joins.
    dragging: bool,
}

impl DocumentSettings {
    /// Settings `value`, saved as they are, with nothing to undo.
    pub fn new(value: Value) -> Self {
        Self {
            saved: value.clone(),
            value,
            undo: Vec::new(),
            redo: Vec::new(),
            dragging: false,
        }
    }

    /// The settings as the editor has them, in file form.
    pub fn value(&self) -> &Value {
        &self.value
    }

    /// Whether the settings differ from those last saved or opened.
    pub fn is_unsaved(&self) -> bool {
        self.value != self.saved
    }

    /// Marks the settings as saved as they are now.
    pub fn mark_saved(&mut self) {
        self.saved = self.value.clone();
    }

    /// Records the editor's settings `new` as one undo step, or as part of
    /// the step of a drag still going on. `dragging` says whether this
    /// change is part of a drag that hasn't ended. Clears the steps to redo.
    /// Records nothing when `new` equals the settings already held.
    fn record(&mut self, new: Value, dragging: bool) {
        let edits = edits_between(&self.value, &new);
        if edits.is_empty() {
            return;
        }
        match self.undo.last_mut() {
            Some(step) if self.dragging => join(step, edits),
            _ => self.undo.push(edits),
        }
        self.value = new;
        self.redo.clear();
        self.dragging = dragging;
    }

    /// Takes back the last step and returns the settings before it, or
    /// `None` with nothing to undo.
    fn undo(&mut self) -> Option<Value> {
        let step = self.undo.pop()?;
        for edit in step.iter().rev() {
            edit.path.set(&mut self.value, edit.old.clone());
        }
        self.redo.push(step);
        self.dragging = false;
        Some(self.value.clone())
    }

    /// Makes the last undone step again and returns the settings after it,
    /// or `None` with nothing to redo.
    fn redo(&mut self) -> Option<Value> {
        let step = self.redo.pop()?;
        for edit in &step {
            edit.path.set(&mut self.value, edit.new.clone());
        }
        self.undo.push(step);
        self.dragging = false;
        Some(self.value.clone())
    }
}

/// Adds `edits` to `step`: a setting `step` changed already keeps its old
/// value and takes the new one.
fn join(step: &mut Vec<Edit>, edits: Vec<Edit>) {
    for edit in edits {
        match step.iter_mut().find(|known| known.path == edit.path) {
            Some(known) => known.new = edit.new,
            None => step.push(edit),
        }
    }
}

/// The edits that turn `old` into `new`: one for each differing setting,
/// and one for a whole group whose settings were added or removed. Setting
/// each edit's `new` in order turns `old` into `new`; setting each `old` in
/// reverse order turns it back.
pub fn edits_between(old: &Value, new: &Value) -> Vec<Edit> {
    let mut edits = Vec::new();
    collect_edits(&SettingPath::default(), old, new, &mut edits);
    edits
}

/// Adds to `edits` what differs between `old` and `new` at `path`.
fn collect_edits(path: &SettingPath, old: &Value, new: &Value, edits: &mut Vec<Edit>) {
    if old == new {
        return;
    }
    match (old, new) {
        (Value::Object(old_map), Value::Object(new_map)) if same_keys(old_map, new_map) => {
            for (key, old_value) in old_map {
                collect_edits(&path.child(key), old_value, &new_map[key], edits);
            }
        }
        _ => edits.push(Edit {
            path: path.clone(),
            old: old.clone(),
            new: new.clone(),
        }),
    }
}

/// Whether two maps have the same keys.
fn same_keys(a: &Map<String, Value>, b: &Map<String, Value>) -> bool {
    a.len() == b.len() && a.keys().all(|key| b.contains_key(key))
}

/// How a kind's documents read and set the settings their editor shows.
#[derive(Clone, Copy)]
pub struct EditorSettings {
    /// The settings the editor `editor` shows, in file form, or `None`
    /// before the editor is ready.
    pub value: fn(&World, Entity) -> Option<Value>,
    /// Shows `value` in the editor `editor`, as an edit would: the editor
    /// rebuilds what it shows.
    ///
    /// # Errors
    ///
    /// Fails, changing nothing, when `value` isn't settings of the kind or
    /// the editor isn't ready.
    pub load: fn(&mut World, Entity, &Value) -> Result<(), String>,
}

/// Undoes the last step of the document `0`.
#[derive(Event, Debug, Clone, Copy)]
pub struct Undo(pub Entity);

/// Redoes the last undone step of the document `0`.
#[derive(Event, Debug, Clone, Copy)]
pub struct Redo(pub Entity);

/// Whether the last Inspector edit is part of a drag still going on.
#[derive(Resource, Debug, Default)]
struct Dragging(bool);

/// Records documents' edits for undo, redo and unsaved state.
pub struct HistoryPlugin;

impl Plugin for HistoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Dragging>()
            .add_observer(note_drag)
            .add_observer(undo)
            .add_observer(redo)
            .add_systems(PostUpdate, (start_settings, record_edits).chain());
    }
}

/// Remembers whether the last Inspector edit ended its drag.
fn note_drag(edited: On<InspectorEdited>, mut dragging: ResMut<Dragging>) {
    dragging.0 = !edited.is_final;
}

/// Settings to load into a new document's editor once it is ready, such
/// as a project preset's, which the editor doesn't start with.
#[derive(Component, Debug, Clone)]
pub struct PendingLoad(pub Value);

/// Gives each document whose editor is ready its [`DocumentSettings`],
/// first loading any [`PendingLoad`] settings into the editor.
fn start_settings(world: &mut World) {
    let waiting: Vec<(Entity, &'static str, Entity)> = world
        .query_filtered::<(Entity, &Document), Without<DocumentSettings>>()
        .iter(world)
        .map(|(entity, document)| (entity, document.kind, document.editor))
        .collect();
    for (entity, kind, editor) in waiting {
        let Some(settings) = world
            .resource::<Kinds>()
            .get(kind)
            .and_then(|kind| kind.settings)
        else {
            continue;
        };
        if (settings.value)(world, editor).is_none() {
            continue;
        }
        if let Some(PendingLoad(value)) = world.entity_mut(entity).take::<PendingLoad>()
            && let Err(error) = (settings.load)(world, editor, &value)
        {
            error!("could not show the preset's settings: {error}");
        }
        if let Some(value) = (settings.value)(world, editor) {
            world
                .entity_mut(entity)
                .insert(DocumentSettings::new(value));
        }
    }
}

/// Records the selected document's editor's settings when they changed.
fn record_edits(world: &mut World) {
    let Some(document) = world.resource::<Selection>().document else {
        return;
    };
    let Some(found) = world.get::<Document>(document) else {
        return;
    };
    let (kind, editor) = (found.kind, found.editor);
    let Some(settings) = world
        .resource::<Kinds>()
        .get(kind)
        .and_then(|kind| kind.settings)
    else {
        return;
    };
    let Some(value) = (settings.value)(world, editor) else {
        return;
    };
    let dragging = world.resource::<Dragging>().0;
    if let Some(mut state) = world.get_mut::<DocumentSettings>(document)
        && state.value != value
    {
        state.record(value, dragging);
    }
}

/// Undoes the document's last step and shows the result in its editor.
fn undo(undo: On<Undo>, mut commands: Commands) {
    let document = undo.0;
    commands.queue(move |world: &mut World| step(world, document, DocumentSettings::undo));
}

/// Redoes the document's last undone step and shows the result.
fn redo(redo: On<Redo>, mut commands: Commands) {
    let document = redo.0;
    commands.queue(move |world: &mut World| step(world, document, DocumentSettings::redo));
}

/// Takes one step through `document`'s history with `go`, then loads the
/// settings it gives into the editor.
fn step(world: &mut World, document: Entity, go: fn(&mut DocumentSettings) -> Option<Value>) {
    let Some(found) = world.get::<Document>(document) else {
        return;
    };
    let (kind, editor) = (found.kind, found.editor);
    let Some(settings) = world
        .resource::<Kinds>()
        .get(kind)
        .and_then(|kind| kind.settings)
    else {
        return;
    };
    let Some(value) = world
        .get_mut::<DocumentSettings>(document)
        .and_then(|mut state| go(&mut state))
    else {
        return;
    };
    if let Err(error) = (settings.load)(world, editor, &value) {
        error!("could not show the undone settings: {error}");
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn edits_name_each_changed_setting_or_a_reshaped_group() {
        let old = json!({ "a": 1, "b": { "c": 2, "d": [1, 2] }, "e": { "kind": "x", "f": 1 } });
        let new = json!({ "a": 1, "b": { "c": 3, "d": [1] }, "e": { "kind": "y", "g": 2 } });
        let edits = edits_between(&old, &new);
        let paths: Vec<String> = edits.iter().map(|edit| edit.path.to_string()).collect();
        assert_eq!(paths, ["b.c", "b.d", "e"]);

        // They turn one into the other and back.
        let mut value = old.clone();
        for edit in &edits {
            edit.path.set(&mut value, edit.new.clone());
        }
        assert_eq!(value, new);
        for edit in edits.iter().rev() {
            edit.path.set(&mut value, edit.old.clone());
        }
        assert_eq!(value, old);
    }

    #[test]
    fn a_drag_is_one_step_and_undo_crosses_saves() {
        let mut state = DocumentSettings::new(json!({ "size": 1.0, "rough": false }));
        assert!(!state.is_unsaved() && state.undo.is_empty());

        // A drag through three values, then a toggle.
        state.record(json!({ "size": 1.5, "rough": false }), true);
        state.record(json!({ "size": 2.0, "rough": false }), true);
        state.record(json!({ "size": 2.5, "rough": false }), false);
        state.record(json!({ "size": 2.5, "rough": true }), false);
        assert!(state.is_unsaved());
        state.mark_saved();
        assert!(!state.is_unsaved());

        // Two steps back reach the start; the save doesn't stop it.
        assert_eq!(state.undo(), Some(json!({ "size": 2.5, "rough": false })));
        assert!(state.is_unsaved());
        assert_eq!(state.undo(), Some(json!({ "size": 1.0, "rough": false })));
        assert_eq!(state.undo(), None);

        // Redo goes forward again, and a new change drops what was undone.
        assert_eq!(state.redo(), Some(json!({ "size": 2.5, "rough": false })));
        state.record(json!({ "size": 3.0, "rough": false }), false);
        assert!(state.redo.is_empty());
        assert_eq!(state.undo(), Some(json!({ "size": 2.5, "rough": false })));
    }
}
