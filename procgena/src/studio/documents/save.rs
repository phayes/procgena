//! Saving documents into the project: Save, Save as new with its dialog,
//! Reset to base, and the keys and Inspector buttons for them and for undo.

use bevy::feathers::controls::{
    ButtonVariant, FeathersButton, FeathersDialog, FeathersDialogBody, FeathersDialogFooter,
    FeathersDialogHeader, FeathersTextInput, FeathersTextInputContainer,
};
use bevy::feathers::display::caption;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, TextEdit};
use bevy::ui_widgets::{Activate, RequestClose};
use procgena_core::preset::{BUILTIN, Document as PresetFile, Layer, Reference, Registry};

use super::history::{DocumentSettings, Redo, Undo, queue_load};
use super::{Document, Origin, Selection};
use crate::studio::kinds::Kinds;
use crate::studio::project::Presets;
use crate::ui::toast::{ShowToast, ToastKind};

/// Makes Save, Save as new, Reset to base, Undo and Redo work from their
/// keys and buttons.
pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (shortcut_keys, fill_key_field, show_buttons))
            .add_observer(press_button);
    }
}

/// What a header button does.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HeaderButton {
    /// Saves the selected document.
    #[default]
    Save,
    /// Saves the selected document as a new project preset.
    SaveAsNew,
    /// Undoes the selected document's last change.
    Undo,
    /// Redoes the selected document's last undone change.
    Redo,
}

/// The Inspector header's buttons: Save, Save as new, Undo and Redo.
pub fn header_buttons() -> impl Scene {
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Wrap,
            column_gap: px(4),
            row_gap: px(4),
            flex_shrink: 0.0,
        }
        Children [
            @FeathersButton { @caption: bsn! { @caption("Save") } }
            HeaderButton::Save
            --
            @FeathersButton { @caption: bsn! { @caption("Save as new") } }
            HeaderButton::SaveAsNew
            --
            @FeathersButton { @caption: bsn! { @caption("Undo") } }
            HeaderButton::Undo
            --
            @FeathersButton { @caption: bsn! { @caption("Redo") } }
            HeaderButton::Redo
        ]
    }
}

/// Runs the clicked header button's command.
fn press_button(activate: On<Activate>, buttons: Query<&HeaderButton>, mut commands: Commands) {
    let Ok(button) = buttons.get(activate.event_target()) else {
        return;
    };
    let run: fn(&mut World) = match button {
        HeaderButton::Save => save,
        HeaderButton::SaveAsNew => save_as_new,
        HeaderButton::Undo => undo_selected,
        HeaderButton::Redo => redo_selected,
    };
    commands.queue(run);
}

/// Shows Save as the main button while the selected document is unsaved,
/// and dims the buttons that would do nothing.
fn show_buttons(
    selection: Res<Selection>,
    documents: Query<(&Document, Option<&DocumentSettings>)>,
    mut buttons: Query<(&HeaderButton, &mut ButtonVariant)>,
) {
    let found = selection
        .document
        .and_then(|document| documents.get(document).ok());
    let state = found.and_then(|(_, state)| state);
    for (button, mut variant) in &mut buttons {
        let wanted = match button {
            HeaderButton::Save if state.is_some_and(DocumentSettings::is_unsaved) => {
                ButtonVariant::Primary
            }
            _ => ButtonVariant::Normal,
        };
        if *variant != wanted {
            *variant = wanted;
        }
    }
}

/// Cmd (Ctrl elsewhere) with Z undoes, with Shift and Z redoes, with S
/// saves, and with Shift and S saves as new. Text fields keep their own
/// keys.
fn shortcut_keys(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    text_fields: Query<(), With<EditableText>>,
    mut commands: Commands,
) {
    let command = keys.any_pressed([
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ]);
    if !command
        || focus
            .get()
            .is_some_and(|entity| text_fields.contains(entity))
    {
        return;
    }
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let run: fn(&mut World) = if keys.just_pressed(KeyCode::KeyZ) {
        if shift { redo_selected } else { undo_selected }
    } else if keys.just_pressed(KeyCode::KeyS) {
        if shift { save_as_new } else { save }
    } else {
        return;
    };
    commands.queue(run);
}

/// Undoes the selected document's last change.
pub fn undo_selected(world: &mut World) {
    if let Some(document) = world.resource::<Selection>().document {
        world.trigger(Undo(document));
    }
}

/// Redoes the selected document's last undone change.
pub fn redo_selected(world: &mut World) {
    if let Some(document) = world.resource::<Selection>().document {
        world.trigger(Redo(document));
    }
}

/// Saves the selected document: a project preset is written over its file;
/// a built-in preset asks for a key to save it as new.
pub fn save(world: &mut World) {
    let Some((document, found)) = selected(world) else {
        return;
    };
    if found.origin == Origin::BuiltIn {
        save_as_new(world);
        return;
    }
    let result = update(world, document, &found);
    report(world, result.map(|()| format!("Saved {}.", found.key)));
}

/// Asks for a key, then saves the selected document as a new project
/// preset written over the preset it shows.
pub fn save_as_new(world: &mut World) {
    let Some((document, found)) = selected(world) else {
        return;
    };
    let Some(project) = world.resource::<Presets>().project() else {
        report(world, Err("No project is open to save into.".to_owned()));
        return;
    };
    let proposed = project.free_key(found.kind, &found.key);
    world
        .commands()
        .spawn_scene(save_as_dialog(document, proposed));
}

/// Sets the selected document's settings back to those of the preset it is
/// written over: its file's base for a project preset, the preset itself
/// for a built-in one. The change can be undone.
pub fn reset_to_base(world: &mut World) {
    let Some((document, found)) = selected(world) else {
        return;
    };
    match base_settings(world, &found) {
        Ok(value) => queue_load(world, document, value),
        Err(error) => report(world, Err(error)),
    }
}

/// The selected document's kind, key and origin, if it can be saved.
fn selected(world: &mut World) -> Option<(Entity, Found)> {
    let document = world.resource::<Selection>().document?;
    let found = world.get::<Document>(document)?;
    let found = Found {
        kind: found.kind,
        key: found.key.clone(),
        origin: found.origin,
    };
    if world.get::<DocumentSettings>(document).is_none() {
        report(
            world,
            Err("This kind of preset can't be saved yet.".to_owned()),
        );
        return None;
    }
    Some((document, found))
}

/// What saving needs to know of a document.
struct Found {
    kind: &'static str,
    key: String,
    origin: Origin,
}

/// The full settings of the preset `found` is written over.
fn base_settings(world: &World, found: &Found) -> Result<serde_json::Value, String> {
    let registry = world.resource::<Presets>().registry();
    let resolve = world
        .resource::<Kinds>()
        .get(found.kind)
        .and_then(|kind| kind.settings)
        .ok_or("This kind of preset can't be reset yet.")?
        .resolve;
    let base = match found.origin {
        Origin::BuiltIn => Reference::qualified(BUILTIN, &found.key),
        Origin::Project => {
            let file = registry
                .project_preset(found.kind, &found.key)
                .ok_or("The preset's file is gone.")?;
            match &file.document.base {
                Some(base) => base_reference(registry, found.kind, &found.key, base),
                // Without a base, the base is the kind's defaults.
                None => {
                    return registry
                        .document_settings(found.kind, &found.key, &PresetFile::default())
                        .map_err(|error| error.to_string());
                }
            }
        }
    };
    resolve(registry, &base)
}

/// The reference, as any lookup reads it, to the preset that the project
/// file `key` of kind `kind` names as its `base`: a project file sees the
/// project's presets first, but never itself.
fn base_reference(registry: &Registry, kind: &str, key: &str, base: &str) -> String {
    if base.contains(':') {
        return base.to_owned();
    }
    if base != key && registry.project_preset(kind, base).is_some() {
        return registry.namespace(Layer::Project).map_or_else(
            || base.to_owned(),
            |namespace| Reference::qualified(namespace, base),
        );
    }
    Reference::qualified(BUILTIN, base)
}

/// Writes the project preset `found` again from the document's settings,
/// keeping its file's base, name, description and group.
fn update(world: &mut World, document: Entity, found: &Found) -> Result<(), String> {
    let value = world
        .get::<DocumentSettings>(document)
        .map(DocumentSettings::file_value)
        .ok_or("The document has no settings yet.")?;
    let mut presets = world.resource_mut::<Presets>();
    let project = presets.project_mut().ok_or("No project is open.")?;
    let file = project
        .registry()
        .project_preset(found.kind, &found.key)
        .ok_or("The preset's file is gone.")?
        .document
        .clone();
    let file = PresetFile {
        settings: Some(value.clone()),
        ..file
    };
    project
        .update(found.kind, &found.key, &file)
        .map_err(|error| error.to_string())?;
    if let Some(mut state) = world.get_mut::<DocumentSettings>(document) {
        state.mark_saved(value);
    }
    Ok(())
}

/// Saves `document` as the new project preset `key`, written over the
/// preset it shows, and makes the document that preset.
fn save_new(world: &mut World, document: Entity, key: &str) -> Result<(), String> {
    let Some(found) = world.get::<Document>(document) else {
        return Err("The document is closed.".to_owned());
    };
    let (kind, from, layer) = (found.kind, found.key.clone(), found.origin.layer());
    let value = world
        .get::<DocumentSettings>(document)
        .map(DocumentSettings::file_value)
        .ok_or("The document has no settings yet.")?;

    // The file, written over the preset shown.
    let mut presets = world.resource_mut::<Presets>();
    let project = presets.project_mut().ok_or("No project is open.")?;
    let base = project.registry().base_for(kind, key, layer, &from);
    let file = PresetFile {
        base: Some(base),
        settings: Some(value.clone()),
        ..PresetFile::default()
    };
    project
        .save_new(kind, key, &file)
        .map_err(|error| error.to_string())?;

    // The document now shows the new preset.
    if let Some(mut found) = world.get_mut::<Document>(document) {
        found.key = key.to_owned();
        found.name = key.to_owned();
        found.origin = Origin::Project;
    }
    if let Some(mut state) = world.get_mut::<DocumentSettings>(document) {
        state.mark_saved(value);
    }
    Ok(())
}

/// Shows a toast saying how a save went.
fn report(world: &mut World, result: Result<String, String>) {
    let toast = match result {
        Ok(message) => ShowToast {
            kind: ToastKind::Success,
            message,
        },
        Err(message) => ShowToast {
            kind: ToastKind::Error,
            message,
        },
    };
    world.trigger(toast);
}

/// Marks the Save as new dialog, naming the document it saves.
#[derive(Component, Clone)]
struct SaveAsDialog(Entity);

impl Default for SaveAsDialog {
    fn default() -> Self {
        Self(Entity::PLACEHOLDER)
    }
}

/// Marks the dialog's key field, with the key to show in it once.
#[derive(Component, Default, Clone)]
struct NewKeyField(Option<String>);

/// The dialog asking for the new preset's key, proposing `proposed`.
fn save_as_dialog(document: Entity, proposed: String) -> impl Scene {
    bsn! {
        @FeathersDialog {
            @width: px(360),
            @contents: bsn_list! {
                @FeathersDialogHeader Children [ @caption("Save as new preset") ]
                --
                @FeathersDialogBody Children [
                    @caption("Key: lower-case letters, digits and underscores.")
                    --
                    @FeathersTextInputContainer
                    Node { flex_grow: 1.0 }
                    Children [ @FeathersTextInput NewKeyField({Some(proposed)}) ]
                ]
                --
                @FeathersDialogFooter Children [
                    @FeathersButton { @caption: bsn! { @caption("Cancel") } }
                    on(|activate: On<Activate>, mut commands: Commands| {
                        commands.trigger(RequestClose { source: activate.event_target() });
                    })
                    --
                    @FeathersButton {
                        @caption: bsn! { @caption("Save preset") },
                        @variant: ButtonVariant::Primary,
                    }
                    on(confirm_save_as)
                ]
            }
        }
        SaveAsDialog({document})
        on(|close: On<RequestClose>, mut commands: Commands| {
            commands.entity(close.event_target()).despawn();
        })
    }
}

/// Shows the proposed key in a newly spawned key field, selected so typing
/// replaces it.
fn fill_key_field(mut fields: Query<(&mut EditableText, &mut NewKeyField)>) {
    for (mut text, mut field) in &mut fields {
        if let Some(key) = field.0.take() {
            text.queue_edit(TextEdit::SelectAll);
            text.queue_edit(TextEdit::Insert(key.into()));
            text.queue_edit(TextEdit::SelectAll);
        }
    }
}

/// Saves with the typed key and closes the dialog, or shows why the key
/// can't be used and keeps it open.
fn confirm_save_as(
    activate: On<Activate>,
    dialogs: Query<(Entity, &SaveAsDialog)>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    fields: Query<&EditableText, With<NewKeyField>>,
    mut commands: Commands,
) {
    let Some((dialog, SaveAsDialog(document))) = parents
        .iter_ancestors(activate.event_target())
        .find_map(|ancestor| dialogs.get(ancestor).ok())
    else {
        return;
    };
    let Some(key) = children
        .iter_descendants(dialog)
        .find_map(|child| fields.get(child).ok())
        .map(|field| field.value().to_string())
    else {
        return;
    };
    let document = *document;
    commands.queue(move |world: &mut World| {
        let key = key.trim().to_owned();
        match save_new(world, document, &key) {
            Ok(()) => {
                world.entity_mut(dialog).despawn();
                report(world, Ok(format!("Saved as {key}.")));
            }
            Err(error) => report(world, Err(error)),
        }
    });
}
