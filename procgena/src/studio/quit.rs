//! Quitting: Escape asks first, Cmd+Q (Ctrl+Q elsewhere) quits at once.

use bevy::feathers::controls::{
    ButtonVariant, FeathersButton, FeathersDialog, FeathersDialogBody, FeathersDialogFooter,
    FeathersDialogHeader, FeathersMenuPopup,
};
use bevy::feathers::display::caption;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{Activate, ModalDialogBarrier, RequestClose};

/// Marks the quit confirmation dialog.
#[derive(Component, Default, Clone)]
struct QuitDialog;

/// Handles the quit keys.
pub struct QuitPlugin;

impl Plugin for QuitPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, quit_keys);
    }
}

/// Quits on Cmd+Q or Ctrl+Q. On Escape, asks whether to quit, unless a dialog
/// or menu is open (they close themselves) or a text field has focus (Escape
/// leaves the field instead, so a second Escape asks).
#[allow(clippy::too_many_arguments)]
fn quit_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut focus: ResMut<InputFocus>,
    dialogs: Query<(), With<ModalDialogBarrier>>,
    popups: Query<&Visibility, With<FeathersMenuPopup>>,
    text_fields: Query<(), With<EditableText>>,
    mut menu_was_open: Local<bool>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
) {
    // A menu that Escape closes this frame may already be hidden, so also
    // count one that was open last frame.
    let menu_open = popups.iter().any(|v| *v == Visibility::Visible);
    let menu_recently_open = menu_open || *menu_was_open;
    *menu_was_open = menu_open;

    let command = keys.any_pressed([
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ]);
    if command && keys.just_pressed(KeyCode::KeyQ) {
        exit.write(AppExit::Success);
        return;
    }
    if !keys.just_pressed(KeyCode::Escape) || !dialogs.is_empty() || menu_recently_open {
        return;
    }
    if focus
        .get()
        .is_some_and(|entity| text_fields.contains(entity))
    {
        focus.clear();
        return;
    }
    commands.spawn_scene(quit_dialog());
}

/// The dialog asking whether to quit.
fn quit_dialog() -> impl Scene {
    bsn! {
        @FeathersDialog {
            @width: px(320),
            @contents: bsn_list! {
                @FeathersDialogHeader Children [ @caption("Quit procgena?") ]
                --
                @FeathersDialogBody Children [
                    @caption("Settings you have not saved will be lost.")
                ]
                --
                @FeathersDialogFooter Children [
                    @FeathersButton { @caption: bsn! { @caption("Cancel") } }
                    on(|activate: On<Activate>, mut commands: Commands| {
                        commands.trigger(RequestClose { source: activate.event_target() });
                    })
                    --
                    @FeathersButton {
                        @caption: bsn! { @caption("Quit") },
                        @variant: ButtonVariant::Primary,
                    }
                    on(|_: On<Activate>, mut exit: MessageWriter<AppExit>| {
                        exit.write(AppExit::Success);
                    })
                ]
            }
        }
        QuitDialog
        on(|close: On<RequestClose>, mut commands: Commands| {
            commands.entity(close.event_target()).despawn();
        })
    }
}
