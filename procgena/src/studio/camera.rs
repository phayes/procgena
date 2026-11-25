//! Camera controls on the view frame's bar: the Orbit and Fly modes, Auto
//! Rotate and its speed while orbiting, the flying keys and speed while
//! flying, and Reset Camera. [`CAMERA_MODE_KEY`] switches the mode too.
//!
//! The mode is a view-frame setting for the whole studio, kept while the
//! studio runs and never saved: every document is orbited or flown alike.
//! The rotation settings are part of the shared [`EnvironmentSettings`], so
//! those controls edit the Environment panel's value and send it on.

use bevy::feathers::controls::{FeathersButton, FeathersCheckbox, FeathersRadio, FeathersSlider};
use bevy::feathers::display::{caption, label_dim};
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui::Checked;
use bevy::ui_widgets::{
    Activate, RadioGroup, SliderPrecision, SliderStep, SliderValue, ValueChange,
};

use crate::params::ParamPanel;
use crate::scene::{CameraMode, EnvironmentSettings, FlySpeed, ViewKeys, ViewerCommand};

/// Fastest auto-rotation offered, in degrees per second.
const MAX_ROTATE_SPEED: f32 = 90.0;

/// The key that switches between orbiting and flying, while the keyboard
/// belongs to the 3D view.
pub const CAMERA_MODE_KEY: KeyCode = KeyCode::KeyC;

/// Marks the Orbit and Fly radio buttons.
#[derive(Component, Default, Clone)]
struct ModeRadio;

/// Marks the Fly radio button, beside [`ModeRadio`].
#[derive(Component, Default, Clone)]
struct FlyRadio;

/// Marks the controls shown only while orbiting.
#[derive(Component, Default, Clone)]
struct OrbitControls;

/// Marks the flying keys and speed, shown only while flying.
#[derive(Component, Default, Clone)]
struct FlyHint;

/// Marks the Auto Rotate checkbox.
#[derive(Component, Default, Clone)]
struct AutoRotateBox;

/// Marks the rotation speed slider, shown while Auto Rotate is on.
#[derive(Component, Default, Clone)]
struct RotateSpeedSlider;

/// The camera controls, for the view frame's bar.
///
/// Choosing a mode or resetting the camera gives the keyboard back to the
/// 3D view, so Space and the flying keys don't also press the control just
/// clicked.
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
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(8),
            }
            RadioGroup
            on(choose_mode)
            Children [
                @FeathersRadio { @caption: bsn! { @caption("Orbit") } }
                ModeRadio
                Checked
                --
                @FeathersRadio { @caption: bsn! { @caption("Fly") } }
                ModeRadio
                FlyRadio
            ]
            --
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(8),
            }
            OrbitControls
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
            ]
            --
            @label_dim("")
            Node { display: Display::None }
            FlyHint
            --
            @FeathersButton { @caption: bsn! { @caption("Reset Camera") } }
            on(reset_camera)
        ]
    }
}

/// Gives the keyboard back to the 3D view, if a UI element has it.
fn release_focus(focus: Option<ResMut<InputFocus>>) {
    if let Some(mut focus) = focus
        && focus.get().is_some()
    {
        focus.clear();
    }
}

/// Switches to the mode of the radio button chosen.
fn choose_mode(
    change: On<ValueChange<Entity>>,
    fly: Query<(), With<FlyRadio>>,
    focus: Option<ResMut<InputFocus>>,
    mut viewer: MessageWriter<ViewerCommand>,
) {
    let mode = if fly.contains(change.value) {
        CameraMode::Fly
    } else {
        CameraMode::Orbit
    };
    viewer.write(ViewerCommand::SetCameraMode(mode));
    release_focus(focus);
}

/// Fits the camera to the subject again.
fn reset_camera(
    _: On<Activate>,
    focus: Option<ResMut<InputFocus>>,
    mut viewer: MessageWriter<ViewerCommand>,
) {
    viewer.write(ViewerCommand::ResetCamera);
    release_focus(focus);
}

/// The other camera mode.
pub fn toggled(mode: CameraMode) -> CameraMode {
    match mode {
        CameraMode::Orbit => CameraMode::Fly,
        CameraMode::Fly => CameraMode::Orbit,
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

/// Keeps the camera controls in step with the scene's camera mode and the
/// Environment panel's settings, and switches the mode on [`CAMERA_MODE_KEY`].
pub struct CameraControlsPlugin;

impl Plugin for CameraControlsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, camera_mode_key).add_systems(
            PostUpdate,
            (show_camera_settings, show_camera_mode, show_fly_speed),
        );
    }
}

/// Switches between orbiting and flying when [`CAMERA_MODE_KEY`] is pressed.
fn camera_mode_key(
    keys: ViewKeys,
    mode: Res<CameraMode>,
    mut viewer: MessageWriter<ViewerCommand>,
) {
    if keys.just_pressed(CAMERA_MODE_KEY) {
        viewer.write(ViewerCommand::SetCameraMode(toggled(*mode)));
    }
}

/// Checks the radio button of the scene's camera mode, and shows the
/// controls for that mode only.
fn show_camera_mode(
    mode: Res<CameraMode>,
    radios: Query<(Entity, Has<FlyRadio>, Has<Checked>), With<ModeRadio>>,
    mut orbit_controls: Query<&mut Node, (With<OrbitControls>, Without<FlyHint>)>,
    mut fly_hint: Query<&mut Node, (With<FlyHint>, Without<OrbitControls>)>,
    mut commands: Commands,
) {
    if !mode.is_changed() {
        return;
    }
    let flying = *mode == CameraMode::Fly;
    for (radio, is_fly, checked) in &radios {
        let wanted = is_fly == flying;
        if wanted && !checked {
            commands.entity(radio).insert(Checked);
        } else if !wanted && checked {
            commands.entity(radio).remove::<Checked>();
        }
    }
    let shown = |on: bool| if on { Display::Flex } else { Display::None };
    for mut node in &mut orbit_controls {
        node.display = shown(!flying);
    }
    for mut node in &mut fly_hint {
        node.display = shown(flying);
    }
}

/// Shows the flying keys and the flying speed.
fn show_fly_speed(speed: Res<FlySpeed>, mut hint: Query<&mut Text, With<FlyHint>>) {
    if !speed.is_changed() {
        return;
    }
    for mut text in &mut hint {
        text.0 = format!("WASD · Space/Shift · wheel {}", speed_label(speed.speed()));
    }
}

/// A flying speed in metres per second, to two or three significant figures.
fn speed_label(speed: f32) -> String {
    let decimals = match speed {
        s if s < 0.1 => 3,
        s if s < 1.0 => 2,
        s if s < 10.0 => 1,
        _ => 0,
    };
    format!("{speed:.decimals$} m/s")
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::text::EditableText;

    use super::*;
    use crate::headless::{DEFAULT_SIZE, HeadlessStudio};
    use crate::scene::{OrbitCamera, SceneViewport, ViewportCamera};

    fn camera(studio: &mut HeadlessStudio) -> OrbitCamera {
        let world = studio.app_mut().world_mut();
        *world
            .query_filtered::<&OrbitCamera, With<ViewportCamera>>()
            .single(world)
            .unwrap()
    }

    fn mode(studio: &mut HeadlessStudio) -> CameraMode {
        *studio.app_mut().world().resource::<CameraMode>()
    }

    fn speed(studio: &mut HeadlessStudio) -> f32 {
        studio.app_mut().world().resource::<FlySpeed>().speed()
    }

    #[test]
    fn speeds_read_with_a_few_figures() {
        assert_eq!(speed_label(0.0123), "0.012 m/s");
        assert_eq!(speed_label(0.25), "0.25 m/s");
        assert_eq!(speed_label(5.0), "5.0 m/s");
        assert_eq!(speed_label(450.4), "450 m/s");
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn the_view_frame_the_key_and_commands_switch_modes_keeping_the_pose() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        assert_eq!(mode(&mut studio), CameraMode::Orbit);
        assert!(studio.find("Auto Rotate").is_ok());
        let orbit = camera(&mut studio);

        // Fly from the view frame, from the orbit's pose, with the flying
        // keys shown in place of Auto Rotate.
        studio.click("Fly").unwrap();
        assert_eq!(mode(&mut studio), CameraMode::Fly);
        assert_eq!(camera(&mut studio), orbit);
        assert!(studio.find("Auto Rotate").is_err());
        assert!(
            studio.shows_text_starting("WASD · Space/Shift"),
            "{:?}",
            studio.texts()
        );

        // Clicking Fly left the keyboard to the 3D view: W flies level along
        // the view, Space straight up.
        let flying = speed(&mut studio);
        let focus = studio.app_mut().world().resource::<InputFocus>().get();
        assert_eq!(
            focus, None,
            "clicking Fly gives the keyboard to the 3D view"
        );
        studio.hold(&[KeyCode::KeyW], Duration::from_millis(500));
        let moved = camera(&mut studio).eye() - orbit.eye();
        let expected = 0.5 * flying;
        assert!(
            (moved.length() - expected).abs() < 0.05 * expected,
            "{moved} for {expected}"
        );
        assert!(moved.y.abs() < 1e-4);
        assert!(moved.dot(*orbit.transform().forward()) > 0.0);
        let before = camera(&mut studio).eye();
        studio.hold(&[KeyCode::Space], Duration::from_millis(500));
        let risen = camera(&mut studio).eye() - before;
        assert!(
            risen.y > 0.4 * flying && risen.xz().length() < 1e-4,
            "{risen}"
        );

        // The key orbits again around the point in front, without moving
        // the camera; the command and the key fly again.
        let flown = camera(&mut studio);
        studio.key(None, CAMERA_MODE_KEY);
        assert_eq!(mode(&mut studio), CameraMode::Orbit);
        assert!(camera(&mut studio).eye().abs_diff_eq(flown.eye(), 1e-3));
        assert!(studio.find("Auto Rotate").is_ok());
        studio.command("toggle-camera-mode").unwrap();
        assert_eq!(mode(&mut studio), CameraMode::Fly);
        studio.command("orbit-camera").unwrap();
        assert_eq!(mode(&mut studio), CameraMode::Orbit);
        studio.command("fly-camera").unwrap();
        assert_eq!(mode(&mut studio), CameraMode::Fly);

        // Another kind of subject keeps the mode, and flies at a speed fitted
        // to its view: an Ecotope is far bigger than a tree.
        studio.select("ecotope/pine_forest_floor").unwrap();
        assert_eq!(mode(&mut studio), CameraMode::Fly);
        assert!(speed(&mut studio) > 2.0 * flying);
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn flying_keys_wait_while_a_text_field_has_the_keyboard() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        studio.select("ecotope/pine_forest_floor").unwrap();
        studio.command("fly-camera").unwrap();
        studio.click("+ Add Scatter").unwrap();

        // Type into the new Scatter's key field: the camera stays put, and
        // the mode key types a letter instead of switching modes.
        let world = studio.app_mut().world_mut();
        let field = world
            .query::<(Entity, &EditableText)>()
            .iter(world)
            .find(|(_, text)| text.value() == "trees")
            .map(|(entity, _)| entity)
            .unwrap();
        let at = studio.center_of(field).unwrap();
        studio.click_at(at);
        let start = camera(&mut studio);
        studio.type_text("wasd c");
        studio.hold(&[KeyCode::KeyW], Duration::from_millis(300));
        studio.hold(&[KeyCode::Space], Duration::from_millis(300));
        assert_eq!(camera(&mut studio), start);
        assert_eq!(mode(&mut studio), CameraMode::Fly);
        let typed = studio
            .app_mut()
            .world()
            .get::<EditableText>(field)
            .unwrap()
            .value()
            .to_string();
        assert!(typed.contains("wasd c"), "{typed:?}");

        // Clicking the 3D view takes the keyboard back.
        let world = studio.app_mut().world_mut();
        let viewport = world
            .query_filtered::<Entity, With<SceneViewport>>()
            .single(world)
            .unwrap();
        let at = studio.center_of(viewport).unwrap();
        studio.click_at(at);
        studio.hold(&[KeyCode::KeyW], Duration::from_millis(300));
        assert_ne!(camera(&mut studio).eye(), start.eye());
    }
}
