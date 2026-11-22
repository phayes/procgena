//! Short messages stacked in the bottom-right corner of the window.
//!
//! Trigger [`ShowToast`] to show one. Successes hide themselves after a few
//! seconds; warnings and errors stay until the user closes them.

use std::time::Duration;

use bevy::feathers::constants::icons;
use bevy::feathers::controls::{ButtonVariant, FeathersButton};
use bevy::feathers::display::{icon, label};
use bevy::feathers::theme::ThemeBackgroundColor;
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use bevy::window::RequestRedraw;

/// How long a success message stays on screen.
const SUCCESS_TIME: Duration = Duration::from_secs(4);

/// Edge colours for each kind of message.
const SUCCESS_COLOR: Color = Color::srgb(0.35, 0.65, 0.3);
const WARNING_COLOR: Color = Color::srgb(0.85, 0.65, 0.2);
const ERROR_COLOR: Color = Color::srgb(0.85, 0.3, 0.3);

/// What kind of message a toast shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    /// Something finished as asked. Hides itself.
    Success,
    /// Something finished with limitations. Stays until closed.
    Warning,
    /// Something failed. Stays until closed.
    Error,
}

/// Shows `message` as a toast. Trigger it with `commands.trigger(ShowToast { .. })`.
#[derive(Event, Debug, Clone)]
pub struct ShowToast {
    /// The kind of message, which sets its colour and how long it stays.
    pub kind: ToastKind,
    /// The text to show. Line breaks are kept.
    pub message: String,
}

/// The node toasts are stacked in.
#[derive(Component)]
struct ToastStack;

/// A toast on screen; `expires` counts down for messages that hide themselves.
#[derive(Component)]
struct Toast {
    expires: Option<Timer>,
}

/// Shows [`ShowToast`] messages and hides them when they expire or are closed.
pub struct ToastPlugin;

impl Plugin for ToastPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_stack)
            .add_systems(Update, expire_toasts)
            .add_observer(show_toast)
            .add_observer(close_toast);
    }
}

/// Spawns the corner node the toasts appear in, above everything but dialogs.
fn spawn_stack(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            right: px(12),
            bottom: px(12),
            max_width: px(420),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::End,
            row_gap: px(6),
            ..default()
        },
        GlobalZIndex(98),
        Pickable::IGNORE,
        ToastStack,
    ));
}

/// Adds a toast for each [`ShowToast`] to the stack.
fn show_toast(
    show: On<ShowToast>,
    mut commands: Commands,
    stack: Single<Entity, With<ToastStack>>,
) {
    let (edge, expires) = match show.kind {
        ToastKind::Success => (
            SUCCESS_COLOR,
            Some(Timer::new(SUCCESS_TIME, TimerMode::Once)),
        ),
        ToastKind::Warning => (WARNING_COLOR, None),
        ToastKind::Error => (ERROR_COLOR, None),
    };
    let toast = commands
        .spawn((
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Start,
                column_gap: px(8),
                padding: UiRect::axes(px(10), px(6)),
                border: UiRect::left(px(3)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BorderColor::all(edge),
            ThemeBackgroundColor(tokens::DIALOG_BG),
            Toast {
                expires: expires.clone(),
            },
            ChildOf(*stack),
        ))
        .id();
    let message = show.message.clone();
    commands
        .spawn_scene(bsn! { Node { flex_shrink: 1.0 } Children [ @label(message) ] })
        .insert(ChildOf(toast));
    if expires.is_none() {
        commands
            .spawn_scene(bsn! {
                @FeathersButton {
                    @caption: bsn! { @icon(icons::X) },
                    @variant: ButtonVariant::Plain,
                }
                ToastClose
            })
            .insert(ChildOf(toast));
    }
}

/// Marks a toast's close button.
#[derive(Component, Default, Clone)]
struct ToastClose;

/// Removes the toast whose close button was pressed.
fn close_toast(
    activate: On<Activate>,
    buttons: Query<&ChildOf, With<ToastClose>>,
    mut commands: Commands,
) {
    if let Ok(child_of) = buttons.get(activate.event_target()) {
        commands.entity(child_of.parent()).despawn();
    }
}

/// Counts down self-hiding toasts and removes them when their time is up.
/// Keeps frames coming meanwhile, since the window otherwise sleeps while idle.
fn expire_toasts(
    time: Res<Time>,
    mut toasts: Query<(Entity, &mut Toast)>,
    mut commands: Commands,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    for (entity, mut toast) in &mut toasts {
        let Some(timer) = toast.expires.as_mut() else {
            continue;
        };
        if timer.tick(time.delta()).is_finished() {
            commands.entity(entity).despawn();
        } else {
            redraw.write(RequestRedraw);
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::time::TimeUpdateStrategy;

    use super::*;

    /// An app with only the toast stack, whose clock advances a fifth of a
    /// second per update (Bevy caps each step at a quarter second).
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<RequestRedraw>()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                200,
            )))
            .add_systems(Startup, spawn_stack)
            .add_systems(Update, expire_toasts)
            .add_observer(close_toast);
        app.update();
        app
    }

    /// Adds a toast directly, without its caption and button scenes.
    fn add_toast(app: &mut App, expires: Option<Duration>) -> Entity {
        app.world_mut()
            .spawn(Toast {
                expires: expires.map(|time| Timer::new(time, TimerMode::Once)),
            })
            .id()
    }

    #[test]
    fn successes_expire_and_problems_stay() {
        let mut app = app();
        let success = add_toast(&mut app, Some(Duration::from_secs(1)));
        let error = add_toast(&mut app, None);
        app.update();
        assert!(app.world().get_entity(success).is_ok());
        for _ in 0..6 {
            app.update();
        }
        assert!(app.world().get_entity(success).is_err());
        assert!(app.world().get_entity(error).is_ok());
    }

    #[test]
    fn the_close_button_removes_its_toast() {
        let mut app = app();
        let toast = add_toast(&mut app, None);
        let close = app.world_mut().spawn((ToastClose, ChildOf(toast))).id();
        app.world_mut().trigger(Activate { entity: close });
        app.update();
        assert!(app.world().get_entity(toast).is_err());
    }
}
