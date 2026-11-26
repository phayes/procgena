//! Flying the scene camera: W and S fly where it looks, A and D sideways,
//! drag to look, Space and Shift to rise and drop.

use bevy::ecs::system::SystemParam;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui_widgets::ModalDialogBarrier;

use super::orbit::{ORBIT_PER_PIXEL, OrbitCamera};
use super::viewport::{ViewportActivity, ViewportCamera, ViewportInput};

/// Flying speed, in metres per second, per metre of orbit distance when
/// flying starts.
const SPEED_PER_DISTANCE: f32 = 0.5;
/// How much one wheel notch (16 logical pixels) changes the speed, as a
/// fraction of the current speed.
const SPEED_STEP_PER_NOTCH: f32 = 0.10;
/// Logical pixels in one wheel notch.
const PIXELS_PER_NOTCH: f32 = 16.0;
/// The slowest and fastest the camera can fly, in metres per second.
const SPEED_LIMITS: (f32, f32) = (0.05, 500.0);
/// The longest frame counted when moving, in seconds, so the first frame
/// after the window wakes from sleep doesn't jump.
const MAX_STEP: f32 = 0.1;

/// How the pointer and keyboard move the scene camera. The scene starts in
/// [`Orbit`](Self::Orbit); change it with
/// [`ViewerCommand::SetCameraMode`](super::ViewerCommand::SetCameraMode).
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CameraMode {
    /// Turn around a focus point: drag to orbit, Shift-drag or drag with
    /// another button to pan, scroll to zoom.
    #[default]
    Orbit,
    /// Fly freely: W and S fly where the camera looks (up or down
    /// included), A and D move sideways, Space rises and Shift drops,
    /// dragging with any button looks around, and scrolling changes the
    /// speed by a tenth per wheel notch. Keys move the camera only while no
    /// UI element has keyboard focus; clicking the 3D view takes it back.
    Fly,
}

/// How fast the camera flies.
///
/// Each time flying starts, and each time the camera is set while flying
/// (for example when another subject is shown), the speed is fitted to how
/// far the camera is from what it looks at, so a terrain flies far faster
/// than a mushroom. Scrolling scales that speed, and the scale is kept. The
/// speed always stays between 0.05 and 500 metres per second.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct FlySpeed {
    /// The speed fitted to the view, in metres per second.
    base: f32,
    /// The factor the wheel has set.
    scale: f32,
}

impl Default for FlySpeed {
    fn default() -> Self {
        Self {
            base: 1.0,
            scale: 1.0,
        }
    }
}

impl FlySpeed {
    /// The flying speed, in metres per second.
    pub fn speed(&self) -> f32 {
        let (slowest, fastest) = SPEED_LIMITS;
        (self.base * self.scale).clamp(slowest, fastest)
    }

    /// Fits the speed to a view `distance` metres from what it looks at,
    /// keeping the wheel's factor.
    pub(crate) fn fit(&mut self, distance: f32) {
        self.base = SPEED_PER_DISTANCE * distance.max(f32::EPSILON);
    }

    /// Changes the speed for a scroll of `amount` logical pixels; positive
    /// values fly faster. Each wheel notch changes it by a tenth of the
    /// current speed.
    pub(crate) fn scroll(&mut self, amount: f32) {
        // Scale per notch, compounded, so a notch up then down returns.
        let notches = amount / PIXELS_PER_NOTCH;
        self.scale *= (1.0 + SPEED_STEP_PER_NOTCH).powf(notches);
        // Keep the factor where the speed stays within its limits.
        let (slowest, fastest) = SPEED_LIMITS;
        let base = self.base.max(f32::EPSILON);
        self.scale = self.scale.clamp(slowest / base, fastest / base);
    }
}

/// The keyboard as the 3D view sees it.
///
/// Keys count only while the keyboard belongs to the 3D view: no UI
/// element (a text field, a slider, a button) has keyboard focus, no
/// dialog is open, and neither Ctrl nor Cmd is held, so shortcuts such as
/// Cmd+S don't also move the camera.
#[derive(SystemParam)]
pub struct ViewKeys<'w, 's> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    focus: Option<Res<'w, InputFocus>>,
    windows: Query<'w, 's, (), With<Window>>,
    dialogs: Query<'w, 's, (), With<ModalDialogBarrier>>,
}

impl ViewKeys<'_, '_> {
    /// Whether the keyboard belongs to the 3D view.
    pub fn free(&self) -> bool {
        let focused = self
            .focus
            .as_ref()
            .and_then(|focus| focus.get())
            .is_some_and(|entity| !self.windows.contains(entity));
        let command = self.keys.any_pressed([
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
        ]);
        !focused && !command && self.dialogs.is_empty()
    }

    /// Whether `key` was pressed this frame, and the keyboard belongs to
    /// the 3D view.
    pub fn just_pressed(&self, key: KeyCode) -> bool {
        self.free() && self.keys.just_pressed(key)
    }

    /// Which way the held keys fly, in the camera's own frame: x to the
    /// right, y up and z forward, each -1, 0 or 1.
    fn direction(&self) -> Vec3 {
        if !self.free() {
            return Vec3::ZERO;
        }
        let axis = |plus: &[KeyCode], minus: &[KeyCode]| {
            f32::from(u8::from(self.keys.any_pressed(plus.iter().copied())))
                - f32::from(u8::from(self.keys.any_pressed(minus.iter().copied())))
        };
        Vec3::new(
            axis(&[KeyCode::KeyD], &[KeyCode::KeyA]),
            axis(
                &[KeyCode::Space],
                &[KeyCode::ShiftLeft, KeyCode::ShiftRight],
            ),
            axis(&[KeyCode::KeyW], &[KeyCode::KeyS]),
        )
    }
}

/// The world-space step for flying in `direction` (camera frame: x right,
/// y up, z forward) from a camera facing `forward` and turned so that
/// `right` is its right, `distance` metres long.
///
/// Forward flies exactly where the camera looks, up or down included.
/// Sideways follows the camera's right, and up is always straight up.
/// Diagonals are no faster than straight lines.
fn fly_step(forward: Vec3, right: Vec3, direction: Vec3, distance: f32) -> Vec3 {
    let step = right * direction.x + Vec3::Y * direction.y + forward * direction.z;
    step.normalize_or_zero() * distance
}

/// Applies the keys and pointer input to the scene camera while it is in
/// [`CameraMode::Fly`].
pub(crate) fn drive_fly(
    mut input: MessageReader<ViewportInput>,
    mode: Res<CameraMode>,
    keys: ViewKeys,
    time: Res<Time>,
    mut speed: ResMut<FlySpeed>,
    mut cameras: Query<&mut OrbitCamera, With<ViewportCamera>>,
    mut activity: ResMut<ViewportActivity>,
) {
    // Read the input in either mode, so none is left over for later.
    let inputs: Vec<ViewportInput> = input.read().copied().collect();
    if *mode != CameraMode::Fly {
        return;
    }
    let Ok(mut camera) = cameras.single_mut() else {
        return;
    };

    // Look and change speed with the pointer.
    for event in inputs {
        match event {
            ViewportInput::Drag { delta, .. } => {
                let (yaw, pitch) = (camera.yaw, camera.pitch);
                camera.look(
                    yaw - delta.x * ORBIT_PER_PIXEL,
                    pitch + delta.y * ORBIT_PER_PIXEL,
                );
            }
            ViewportInput::Scroll { delta } => speed.scroll(delta.y),
        }
    }

    // Move with the keys, drawing every frame while one is held.
    let direction = keys.direction();
    if direction != Vec3::ZERO {
        activity.request_frames(ViewportActivity::SETTLE_FRAMES);
        let step = time.delta_secs().min(MAX_STEP) * speed.speed();
        let pose = camera.transform();
        let offset = fly_step(*pose.forward(), *pose.right(), direction, step);
        camera.focus += offset;
    }
}

/// Switches the scene camera to `mode`.
///
/// Flying starts from the orbit camera's pose, at a speed fitted to its
/// distance. Orbiting starts around the point the flying camera looks at,
/// as far in front of it as the orbit was; the camera stays where it is,
/// turning only as far as the orbit's tilt limits need.
pub(crate) fn set_mode(world: &mut World, mode: CameraMode) {
    if *world.resource::<CameraMode>() == mode {
        return;
    }
    let mut cameras = world.query_filtered::<&mut OrbitCamera, With<ViewportCamera>>();
    if let Ok(mut camera) = cameras.single_mut(world) {
        match mode {
            CameraMode::Fly => {
                let distance = camera.distance;
                world.resource_mut::<FlySpeed>().fit(distance);
            }
            CameraMode::Orbit => camera.fit_limits_in_place(),
        }
    }
    *world.resource_mut::<CameraMode>() = mode;
}

/// Sets the scene camera to `view` in either mode. While flying, the speed
/// is fitted to the new view; while orbiting, the view is kept within its
/// limits.
pub(crate) fn set_view(world: &mut World, mut view: OrbitCamera) {
    let mode = *world.resource::<CameraMode>();
    match mode {
        CameraMode::Fly => world.resource_mut::<FlySpeed>().fit(view.distance),
        CameraMode::Orbit => view.fit_limits_in_place(),
    }
    let mut cameras = world.query_filtered::<&mut OrbitCamera, With<ViewportCamera>>();
    if let Ok(mut camera) = cameras.single_mut(world) {
        *camera = view;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::scene::orbit::OrbitLimits;

    fn orbit() -> OrbitCamera {
        OrbitCamera {
            focus: Vec3::new(0.0, 1.0, 0.0),
            yaw: 0.6,
            pitch: 0.2,
            distance: 10.0,
            limits: OrbitLimits {
                distance: (0.5, 60.0),
                pitch: (-0.05, 1.5),
            },
        }
    }

    /// A scene camera with only the flying systems, at 1/10 s a frame.
    fn app(mode: CameraMode) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                100,
            )))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<InputFocus>()
            .init_resource::<ViewportActivity>()
            .init_resource::<FlySpeed>()
            .init_resource::<CameraMode>()
            .add_message::<ViewportInput>()
            .add_systems(Update, drive_fly);
        app.world_mut().spawn((ViewportCamera, orbit()));
        set_mode(app.world_mut(), mode);
        // The first frame has no time step.
        app.update();
        app
    }

    fn camera(app: &mut App) -> OrbitCamera {
        let world = app.world_mut();
        *world
            .query_filtered::<&OrbitCamera, With<ViewportCamera>>()
            .single(world)
            .unwrap()
    }

    fn hold(app: &mut App, key: KeyCode, frames: usize) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        for _ in 0..frames {
            app.update();
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(key);
    }

    #[test]
    fn forward_flies_where_the_camera_looks_and_up_is_straight_up() {
        let pose = orbit().transform();
        let (facing, right) = (*pose.forward(), *pose.right());
        // The test camera looks down a little, so forward drops.
        let forward = fly_step(facing, right, Vec3::Z, 2.0);
        assert!((forward.length() - 2.0).abs() < 1e-5);
        assert!(forward.normalize().dot(facing) > 0.9999);
        assert!(forward.y < 0.0);

        let sideways = fly_step(facing, right, Vec3::X, 1.0);
        assert!(sideways.dot(right) > 0.9999);
        assert!(fly_step(facing, right, Vec3::Y, 3.0).abs_diff_eq(Vec3::new(0.0, 3.0, 0.0), 1e-6));
        assert!(fly_step(facing, right, Vec3::NEG_Y, 1.0).y < 0.0);

        // A diagonal is as long as a straight step.
        assert!(
            (fly_step(facing, right, Vec3::new(1.0, 1.0, 1.0), 1.0).length() - 1.0).abs() < 1e-5
        );
    }

    #[test]
    fn each_wheel_notch_changes_the_speed_by_a_tenth_within_limits() {
        let mut speed = FlySpeed::default();
        speed.fit(10.0);
        let start = speed.speed();
        speed.scroll(PIXELS_PER_NOTCH);
        assert!((speed.speed() / start - 1.1).abs() < 1e-4);
        speed.scroll(-PIXELS_PER_NOTCH);
        assert!((speed.speed() / start - 1.0).abs() < 1e-4);

        // Scrolling far either way stops at the limits, and turns back at
        // once.
        let (slowest, fastest) = SPEED_LIMITS;
        speed.scroll(PIXELS_PER_NOTCH * 1000.0);
        assert!((speed.speed() - fastest).abs() < 1e-2);
        speed.scroll(-PIXELS_PER_NOTCH);
        assert!(speed.speed() < fastest);
        speed.scroll(-PIXELS_PER_NOTCH * 1000.0);
        assert!((speed.speed() - slowest).abs() < 1e-5);
    }

    #[test]
    fn flying_starts_from_the_orbit_pose_at_a_speed_fitted_to_its_distance() {
        let mut app = app(CameraMode::Orbit);
        let before = camera(&mut app).transform();
        set_mode(app.world_mut(), CameraMode::Fly);
        assert_eq!(camera(&mut app).transform(), before);
        let near = app.world().resource::<FlySpeed>().speed();

        // A view ten times farther out flies ten times faster.
        let mut far = orbit();
        far.distance = 100.0;
        set_view(app.world_mut(), far);
        let speed = app.world().resource::<FlySpeed>().speed();
        assert!((speed / near - 10.0).abs() < 1e-4, "{speed} vs {near}");
    }

    #[test]
    fn keys_move_the_camera_while_flying_only() {
        let mut app = app(CameraMode::Orbit);
        let start = camera(&mut app);
        hold(&mut app, KeyCode::KeyW, 5);
        assert_eq!(camera(&mut app), start, "orbiting ignores WASD");

        set_mode(app.world_mut(), CameraMode::Fly);
        let speed = app.world().resource::<FlySpeed>().speed();
        let facing = start.transform().forward();
        hold(&mut app, KeyCode::KeyW, 5);
        let moved = camera(&mut app).eye() - start.eye();
        // Five frames of 1/10 s, straight along the view.
        assert!((moved.length() - 0.5 * speed).abs() < 1e-3, "{moved}");
        assert!(moved.normalize().dot(*facing) > 0.9999);

        let before = camera(&mut app).eye();
        hold(&mut app, KeyCode::Space, 2);
        let risen = camera(&mut app).eye() - before;
        assert!((risen - Vec3::Y * 0.2 * speed).length() < 1e-3, "{risen}");
        hold(&mut app, KeyCode::ShiftLeft, 2);
        assert!((camera(&mut app).eye() - before).length() < 1e-3);
    }

    #[test]
    fn keys_do_nothing_while_a_ui_element_has_focus() {
        let mut app = app(CameraMode::Fly);
        let start = camera(&mut app);

        // A text field or Inspector control has the keyboard.
        let field = app.world_mut().spawn_empty().id();
        app.insert_resource(InputFocus::from_entity(field));
        hold(&mut app, KeyCode::KeyW, 3);
        hold(&mut app, KeyCode::Space, 3);
        assert_eq!(camera(&mut app), start);

        // A focused window is no UI element.
        let window = app.world_mut().spawn(Window::default()).id();
        app.insert_resource(InputFocus::from_entity(window));
        hold(&mut app, KeyCode::KeyW, 3);
        assert_ne!(camera(&mut app), start);

        // Neither do keys held with Cmd, which are shortcuts.
        let moved = camera(&mut app);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::SuperLeft);
        hold(&mut app, KeyCode::KeyS, 3);
        assert_eq!(camera(&mut app), moved);
    }

    #[test]
    fn dragging_looks_around_without_moving_and_the_wheel_sets_the_speed() {
        let mut app = app(CameraMode::Fly);
        let start = camera(&mut app);
        let speed = app.world().resource::<FlySpeed>().speed();
        app.world_mut().write_message(ViewportInput::Drag {
            button: crate::scene::viewport::PointerButton::Left,
            delta: Vec2::new(100.0, -300.0),
            shift: false,
        });
        app.world_mut().write_message(ViewportInput::Scroll {
            delta: Vec2::new(0.0, 100.0),
        });
        app.update();

        let looked = camera(&mut app);
        assert!(looked.eye().abs_diff_eq(start.eye(), 1e-4));
        // Dragging right turns right; dragging up looks up, past the
        // orbit's tilt limit.
        assert!(looked.transform().forward().dot(*start.transform().right()) > 0.0);
        assert!(looked.pitch < start.limits.pitch.0);
        assert!(app.world().resource::<FlySpeed>().speed() > speed);
    }

    #[test]
    fn orbiting_again_pivots_in_front_of_the_camera_without_moving_it() {
        let mut app = app(CameraMode::Fly);
        let mut flown = camera(&mut app);
        flown.focus += Vec3::new(30.0, 5.0, -12.0);
        flown.look(2.0, -0.6);
        set_view(app.world_mut(), flown);

        set_mode(app.world_mut(), CameraMode::Orbit);
        let orbit = camera(&mut app);
        assert!(orbit.eye().abs_diff_eq(flown.eye(), 1e-3));
        // Looking up was beyond the orbit's limits, so it tilts down to them.
        assert_eq!(orbit.pitch, orbit.limits.pitch.0);
        assert_eq!(orbit.yaw, flown.yaw);
        let ahead = orbit.focus - orbit.eye();
        assert!((ahead.length() - flown.distance).abs() < 1e-3);
        assert!(ahead.normalize().dot(*orbit.transform().forward()) > 0.9999);
    }
}
