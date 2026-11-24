//! Camera controls on the view frame's bar: Auto Rotate, its speed, and
//! Reset Camera.
//!
//! The rotation settings are part of the shared [`EnvironmentSettings`], so
//! these controls edit the Environment panel's value and send it on.

use bevy::feathers::controls::{FeathersButton, FeathersCheckbox, FeathersSlider};
use bevy::feathers::display::caption;
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui::Checked;
use bevy::ui_widgets::{Activate, SliderPrecision, SliderStep, SliderValue, ValueChange};

use crate::params::ParamPanel;
use crate::scene::{EnvironmentSettings, ViewerCommand};

/// Fastest auto-rotation offered, in degrees per second.
const MAX_ROTATE_SPEED: f32 = 90.0;

/// Marks the Auto Rotate checkbox.
#[derive(Component, Default, Clone)]
struct AutoRotateBox;

/// Marks the rotation speed slider, shown while Auto Rotate is on.
#[derive(Component, Default, Clone)]
struct RotateSpeedSlider;

/// The camera controls, for the view frame's bar.
pub fn camera_controls() -> impl Scene {
    let start = EnvironmentSettings::default();
    let speed = start.rotate_speed;
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(8),
        }
        Children [
            @FeathersCheckbox { @caption: bsn! { @caption("Auto Rotate") } }
            AutoRotateBox
            on(toggle_auto_rotate)
            --
            @FeathersSlider { @min: 0.0, @max: MAX_ROTATE_SPEED }
            Node { width: px(90), height: px(18), display: Display::None }
            SliderValue({speed})
            SliderStep(1.0)
            SliderPrecision(0)
            RotateSpeedSlider
            on(set_rotate_speed)
            --
            @FeathersButton { @caption: bsn! { @caption("Reset Camera") } }
            on(|_: On<Activate>, mut viewer: MessageWriter<ViewerCommand>| {
                viewer.write(ViewerCommand::ResetCamera);
            })
        ]
    }
}

/// Applies `edit` to the Environment panel's settings and sends them to the scene.
fn edit_environment(
    panel: &mut ParamPanel<EnvironmentSettings>,
    viewer: &mut MessageWriter<ViewerCommand>,
    edit: impl FnOnce(&mut EnvironmentSettings),
) {
    let mut settings = *panel.value();
    edit(&mut settings);
    panel.set_value(settings);
    viewer.write(ViewerCommand::SetEnvironment(settings));
}

/// Turns auto-rotation on or off.
fn toggle_auto_rotate(
    change: On<ValueChange<bool>>,
    mut panel: Single<&mut ParamPanel<EnvironmentSettings>>,
    mut viewer: MessageWriter<ViewerCommand>,
) {
    let on = change.value;
    edit_environment(&mut panel, &mut viewer, |e| e.auto_rotate = on);
}

/// Changes the auto-rotation speed.
fn set_rotate_speed(
    change: On<ValueChange<f32>>,
    mut panel: Single<&mut ParamPanel<EnvironmentSettings>>,
    mut viewer: MessageWriter<ViewerCommand>,
) {
    let speed = change.value;
    edit_environment(&mut panel, &mut viewer, |e| e.rotate_speed = speed);
}

/// Keeps the camera controls in step with the Environment panel's settings.
pub struct CameraControlsPlugin;

impl Plugin for CameraControlsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, show_camera_settings);
    }
}

/// Shows the Environment panel's rotation settings on the camera controls.
fn show_camera_settings(
    panel: Single<&ParamPanel<EnvironmentSettings>, Changed<ParamPanel<EnvironmentSettings>>>,
    checkbox: Single<(Entity, Has<Checked>), With<AutoRotateBox>>,
    mut slider: Single<(Entity, &SliderValue, &mut Node), With<RotateSpeedSlider>>,
    mut commands: Commands,
) {
    let settings = panel.value();

    // Tick the checkbox to match.
    let (checkbox, checked) = *checkbox;
    if settings.auto_rotate && !checked {
        commands.entity(checkbox).insert(Checked);
    } else if !settings.auto_rotate && checked {
        commands.entity(checkbox).remove::<Checked>();
    }

    // Show the speed only while rotating, at the current speed.
    let (slider, shown_speed, node) = &mut *slider;
    node.display = if settings.auto_rotate {
        Display::Flex
    } else {
        Display::None
    };
    if shown_speed.0 != settings.rotate_speed {
        commands
            .entity(*slider)
            .insert(SliderValue(settings.rotate_speed));
    }
}
