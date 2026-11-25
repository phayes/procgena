//! What a document's settings are now, as last saved, and the edits to
//! undo and redo between them.
//!
//! The document is the authority on its settings: [`DocumentSettings`]
//! holds them, and the editor in the Inspector shows them. Every change the
//! editor makes is recorded as one undo step of [`Edit`]s; a slider drag is
//! one step. Undo and redo load the settings back into the editor.

use bevy::prelude::*;
use procgena_core::meta::{Edit, SettingPath};
use procgena_core::preset::Registry;
use serde_json::{Map, Value};

use super::pickers::PresetPicked;
use super::{Document, Selection};
use crate::studio::inspector::InspectorEdited;
use crate::studio::kinds::Kinds;

/// A document's settings: as the editor has them now, and as last saved
/// or opened, with the steps to undo and redo.
///
/// The settings are held as the editor reads them, with every preset they
/// refer to written out (see [`EditorSettings::value`]). The file form,
/// which refers to other presets by key as the preset's file wrote them,
/// is kept as last saved or opened, and [`file_value`](Self::file_value)
/// carries it forward to the settings now.
///
/// Added once the document's editor shows its preset. Kinds that can't
/// read their editor's settings (see
/// [`Kind::settings`](crate::studio::kinds::Kind::settings)) have none.
#[derive(Component, Debug, Clone)]
pub struct DocumentSettings {
    value: Value,
    saved: Value,
    saved_file: Value,
    undo: Vec<Vec<Edit>>,
    redo: Vec<Vec<Edit>>,
    /// Whether the last undo step is a drag still going on, which the next
    /// change joins.
    dragging: bool,
    /// Presets picked in the editor: where, the reference, and the
    /// settings the picker put there.
    picked: Vec<(SettingPath, String, Value)>,
}

impl DocumentSettings {
    /// Settings `value`, as the editor reads them, saved as they are in
    /// file form `file`, with nothing to undo.
    pub fn new(value: Value, file: Value) -> Self {
        Self {
            saved: value.clone(),
            saved_file: file,
            value,
            undo: Vec::new(),
            redo: Vec::new(),
            dragging: false,
            picked: Vec::new(),
        }
    }

    /// The settings in file form, to save: each part unchanged since the
    /// last save or opening is written as the file wrote it, so references
    /// to other presets stay as they were; changed parts are written out,
    /// except that a preset picked in the editor, still unchanged since, is
    /// written as its reference.
    pub fn file_value(&self) -> Value {
        let mut file = carry_forward(&self.saved_file, &self.saved, &self.value);
        for (path, reference, picked) in &self.picked {
            if path.get(&self.value) == Some(picked) {
                path.set(&mut file, Value::String(reference.clone()));
            }
        }
        file
    }

    /// Remembers that the preset `reference` was picked for the setting at
    /// `path`, which holds its settings now.
    fn pick(&mut self, path: SettingPath, reference: String) {
        let Some(picked) = path.get(&self.value).cloned() else {
            return;
        };
        self.picked.retain(|(known, ..)| *known != path);
        self.picked.push((path, reference, picked));
    }

    /// Whether the settings differ from those last saved or opened.
    pub fn is_unsaved(&self) -> bool {
        self.value != self.saved
    }

    /// Marks the settings as saved as they are now, with `file` the file
    /// form written.
    pub fn mark_saved(&mut self, file: Value) {
        self.saved = self.value.clone();
        self.saved_file = file;
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

/// The file form of settings `new`, given that settings `old` were written
/// as `file`: each part of `new` equal to the same part of `old` is
/// written as `file` wrote it, and the rest as `new` has it. List items are
/// matched by their `key` setting when they have one, so a reordered or
/// shortened list keeps its unchanged items' file text, and by position
/// otherwise.
pub fn carry_forward(file: &Value, old: &Value, new: &Value) -> Value {
    if old == new {
        return file.clone();
    }
    match (file, old, new) {
        (Value::Object(file), Value::Object(old), Value::Object(new)) => {
            let mut carried = Map::new();
            for (key, new_value) in new {
                let value = match (file.get(key), old.get(key)) {
                    (Some(file), Some(old)) => carry_forward(file, old, new_value),
                    _ => new_value.clone(),
                };
                carried.insert(key.clone(), value);
            }
            Value::Object(carried)
        }
        (Value::Array(file), Value::Array(old), Value::Array(new)) if file.len() == old.len() => {
            let carried = new
                .iter()
                .enumerate()
                .map(|(index, new_item)| {
                    let matched = match new_item.get("key") {
                        Some(key) => old.iter().position(|item| item.get("key") == Some(key)),
                        None => (index < old.len()).then_some(index),
                    };
                    match matched {
                        Some(at) => carry_forward(&file[at], &old[at], new_item),
                        None => new_item.clone(),
                    }
                })
                .collect();
            Value::Array(carried)
        }
        _ => new.clone(),
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
///
/// Settings here are as the editor reads them: a settings tree of the
/// kind's options, with any other preset they use written out or named by
/// a built-in key, never by a project key.
#[derive(Clone, Copy)]
pub struct EditorSettings {
    /// The settings the editor `editor` shows, or `None` before the editor
    /// is ready.
    pub value: fn(&mut World, Entity) -> Option<Value>,
    /// The settings of the preset `reference` names in `registry`, as the
    /// editor reads them.
    ///
    /// # Errors
    ///
    /// Fails when the reference names no preset that reads.
    pub resolve: fn(&Registry, &str) -> Result<Value, String>,
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
            .init_resource::<Picks>()
            .add_observer(note_drag)
            .add_observer(note_pick)
            .add_observer(undo)
            .add_observer(redo)
            .init_resource::<Queued>()
            // Loads happen first in a frame (see `Queued`); edits are
            // recorded once the frame's systems have made them.
            .add_systems(First, (start_settings, apply_queued).chain())
            .add_systems(PostUpdate, record_edits);
    }
}

/// Remembers whether the last Inspector edit ended its drag.
fn note_drag(edited: On<InspectorEdited>, mut dragging: ResMut<Dragging>) {
    dragging.0 = !edited.is_final;
}

/// Presets picked in editors since their documents last recorded edits.
#[derive(Resource, Debug, Default)]
struct Picks(Vec<PresetPicked>);

/// Keeps a picked preset until its document records the change.
fn note_pick(picked: On<PresetPicked>, mut picks: ResMut<Picks>) {
    picks.0.push(picked.event().clone());
}

/// What a new document starts its [`DocumentSettings`] with once its
/// editor is ready.
#[derive(Component, Debug, Clone, Default)]
pub struct PendingLoad {
    /// Settings to load into the editor, as it reads them, when they
    /// aren't those it starts with, such as a project preset's.
    pub load: Option<Value>,
    /// The preset's settings in file form, when they differ from how the
    /// editor reads them, such as references to other presets.
    pub file: Option<Value>,
}

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
        // Wait for the editor, and for settings to load, for it to wake:
        // some editors load only into the selected document's editor.
        let loads = world
            .get::<PendingLoad>(entity)
            .is_some_and(|pending| pending.load.is_some());
        if (settings.value)(world, editor).is_none()
            || (loads && world.get::<crate::studio::Dormant>(editor).is_some())
        {
            continue;
        }
        let pending = world
            .entity_mut(entity)
            .take::<PendingLoad>()
            .unwrap_or_default();
        if let Some(value) = &pending.load
            && let Err(error) = (settings.load)(world, editor, value)
        {
            error!("could not show the preset's settings: {error}");
        }
        if let Some(value) = (settings.value)(world, editor) {
            let file = pending.file.unwrap_or_else(|| value.clone());
            world
                .entity_mut(entity)
                .insert(DocumentSettings::new(value, file));
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
    let picks: Vec<PresetPicked> = std::mem::take(&mut world.resource_mut::<Picks>().0);
    if let Some(mut state) = world.get_mut::<DocumentSettings>(document) {
        if state.value != value {
            state.record(value, dragging);
        }

        // Presets picked in this document's editor, now in its settings.
        for picked in picks.into_iter().filter(|picked| picked.editor == editor) {
            state.pick(picked.path, picked.reference);
        }
    }
}

/// Undoes the document's last step and shows the result in its editor.
fn undo(undo: On<Undo>, mut queued: ResMut<Queued>) {
    queued.steps.push((undo.0, DocumentSettings::undo));
}

/// Redoes the document's last undone step and shows the result.
fn redo(redo: On<Redo>, mut queued: ResMut<Queued>) {
    queued.steps.push((redo.0, DocumentSettings::redo));
}

/// Steps through documents' histories, and settings to load into
/// documents' editors, waiting for the start of the next frame.
///
/// Loading settings can replace parts of an editor, such as a tree
/// Scatter's rows. Waiting until no other system has changes queued for
/// those parts keeps those changes from reaching parts already gone.
#[derive(Resource, Default)]
struct Queued {
    steps: Vec<(Entity, Step)>,
    loads: Vec<(Entity, Value)>,
}

/// One step through a document's history: undo or redo. Returns the
/// settings after it, or `None` with no step to take.
type Step = fn(&mut DocumentSettings) -> Option<Value>;

/// Loads `value` into the editor of `document` at the start of the next
/// frame, as an edit: it is recorded as one undo step.
pub fn queue_load(world: &mut World, document: Entity, value: Value) {
    world.resource_mut::<Queued>().loads.push((document, value));
}

/// Takes the queued history steps and loads.
fn apply_queued(world: &mut World) {
    let queued = std::mem::take(&mut *world.resource_mut::<Queued>());
    for (document, go) in queued.steps {
        step(world, document, go);
    }
    for (document, value) in queued.loads {
        let Some(found) = world.get::<Document>(document) else {
            continue;
        };
        let (kind, editor) = (found.kind, found.editor);
        let Some(settings) = world
            .resource::<Kinds>()
            .get(kind)
            .and_then(|kind| kind.settings)
        else {
            continue;
        };
        if let Err(error) = (settings.load)(world, editor, &value) {
            error!("could not show the settings: {error}");
        }
    }
}

/// Takes one step through `document`'s history with `go`, then loads the
/// settings it gives into the editor.
fn step(world: &mut World, document: Entity, go: Step) {
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
        let start = json!({ "size": 1.0, "rough": false });
        let mut state = DocumentSettings::new(start.clone(), start);
        assert!(!state.is_unsaved() && state.undo.is_empty());

        // A drag through three values, then a toggle.
        state.record(json!({ "size": 1.5, "rough": false }), true);
        state.record(json!({ "size": 2.0, "rough": false }), true);
        state.record(json!({ "size": 2.5, "rough": false }), false);
        state.record(json!({ "size": 2.5, "rough": true }), false);
        assert!(state.is_unsaved());
        state.mark_saved(state.file_value());
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

    #[test]
    fn unchanged_parts_keep_their_file_text() {
        // A Scatter whose file names its trees by key; the editor reads
        // them written out.
        let file = json!({ "seed": 1, "trees": [
            { "tree": "builtin:pine_small", "density_scale": 0.3 },
            { "key": "old", "tree": "tall_pine" },
        ]});
        let old = json!({ "seed": 1, "trees": [
            { "key": "pine_small", "tree": { "height": 4 }, "density_scale": 0.3 },
            { "key": "old", "tree": { "height": 9 }, "density_scale": 1.0 },
        ]});

        // Unchanged, the file text is kept whole.
        assert_eq!(carry_forward(&file, &old, &old), file);

        // A changed seed and density keep each tree's reference; a changed
        // tree is written out; the list's order follows the editor's.
        let new = json!({ "seed": 2, "trees": [
            { "key": "old", "tree": { "height": 10 }, "density_scale": 1.0 },
            { "key": "pine_small", "tree": { "height": 4 }, "density_scale": 0.5 },
        ]});
        assert_eq!(
            carry_forward(&file, &old, &new),
            json!({ "seed": 2, "trees": [
                { "key": "old", "tree": { "height": 10 }, "density_scale": 1.0 },
                { "key": "pine_small", "tree": "builtin:pine_small", "density_scale": 0.5 },
            ]})
        );
    }
}
