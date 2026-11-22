//! A camera that orbits a focus point, driven by pointer input over the viewport.

use std::f32::consts::FRAC_PI_2;

use super::viewport::{PointerButton, ViewportActivity, ViewportInput};
use bevy::prelude::*;

use super::environment::EnvironmentSettings;

/// Radians of orbit per logical pixel dragged.
const ORBIT_PER_PIXEL: f32 = 0.006;
/// Focus movement per logical pixel dragged, as a fraction of the camera distance.
const PAN_PER_PIXEL: f32 = 0.0015;
/// Zoom rate per logical pixel scrolled; each pixel scales the distance by `exp(-ZOOM_PER_PIXEL)`.
const ZOOM_PER_PIXEL: f32 = 0.002;
/// Keeps the camera just short of straight up or down, where "up" is undefined.
const MAX_PITCH: f32 = FRAC_PI_2 - 0.01;

/// How far the camera may zoom and tilt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrbitLimits {
    /// Closest and farthest camera distance from the focus point, in world units.
    pub distance: (f32, f32),
    /// Lowest and highest camera pitch in radians; 0 is level with the focus
    /// point and positive values look down from above. Clamped to just under ±90°.
    pub pitch: (f32, f32),
}

/// A camera orbiting a focus point. The camera entity's [`Transform`] follows it.
///
/// Left-drag orbits, right-drag, middle-drag or Shift+left-drag pans, and
/// scrolling zooms.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct OrbitCamera {
    /// The point the camera looks at.
    pub focus: Vec3,
    /// Angle around the vertical axis, in radians. 0 places the camera on the +Z side.
    pub yaw: f32,
    /// Angle above the focus point, in radians. Positive values look down.
    pub pitch: f32,
    /// Distance from the focus point, in world units.
    pub distance: f32,
    /// Zoom and tilt limits applied after every change.
    pub limits: OrbitLimits,
}

impl OrbitCamera {
    /// Rotates the camera for a drag of `delta` logical pixels.
    ///
    /// Dragging right swings the camera left around the focus point, so the
    /// subject appears to turn with the pointer. Dragging down raises the camera.
    pub fn orbit(&mut self, delta: Vec2) {
        self.yaw -= delta.x * ORBIT_PER_PIXEL;
        self.pitch += delta.y * ORBIT_PER_PIXEL;
        self.clamp();
    }

    /// Moves the focus point in the view plane for a drag of `delta` logical
    /// pixels, so the scene follows the pointer. The step grows with distance.
    pub fn pan(&mut self, delta: Vec2) {
        let rotation = self.rotation();
        let right = rotation * Vec3::X;
        let up = rotation * Vec3::Y;
        let scale = self.distance * PAN_PER_PIXEL;
        self.focus += (-delta.x * right + delta.y * up) * scale;
    }

    /// Zooms for a scroll of `amount` logical pixels; positive values move closer.
    pub fn zoom(&mut self, amount: f32) {
        self.distance *= (-amount * ZOOM_PER_PIXEL).exp();
        self.clamp();
    }

    /// Spins the camera around the vertical axis by `radians`.
    pub fn rotate(&mut self, radians: f32) {
        self.yaw = (self.yaw + radians).rem_euclid(std::f32::consts::TAU);
    }

    /// The camera transform for the current orbit.
    pub fn transform(&self) -> Transform {
        let rotation = self.rotation();
        Transform {
            translation: self.focus + rotation * Vec3::new(0.0, 0.0, self.distance),
            rotation,
            ..default()
        }
    }

    fn rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, -self.pitch, 0.0)
    }

    fn clamp(&mut self) {
        let (near, far) = self.limits.distance;
        self.distance = self.distance.clamp(near, far);
        let (low, high) = self.limits.pitch;
        self.pitch = self.pitch.clamp(low.max(-MAX_PITCH), high.min(MAX_PITCH));
    }
}

/// Applies pointer input and auto-rotation to every [`OrbitCamera`].
pub(crate) fn drive_orbit(
    mut input: MessageReader<ViewportInput>,
    settings: Res<EnvironmentSettings>,
    time: Res<Time>,
    mut cameras: Query<&mut OrbitCamera>,
) {
    let inputs: Vec<ViewportInput> = input.read().copied().collect();
    for mut orbit in &mut cameras {
        // Pointer input.
        for event in &inputs {
            match *event {
                ViewportInput::Drag {
                    button: PointerButton::Left,
                    delta,
                    shift: false,
                } => orbit.orbit(delta),
                ViewportInput::Drag { delta, .. } => orbit.pan(delta),
                ViewportInput::Scroll { delta } => orbit.zoom(delta.y),
            }
        }

        // Turntable spin.
        if settings.auto_rotate {
            orbit.rotate(settings.rotate_speed.to_radians() * time.delta_secs());
        }
    }
}

/// Moves each orbit camera to its orbit, rendering a frame whenever one moves.
pub(crate) fn follow_orbit(
    mut cameras: Query<(&OrbitCamera, &mut Transform), Changed<OrbitCamera>>,
    mut activity: ResMut<ViewportActivity>,
) {
    for (orbit, mut transform) in &mut cameras {
        *transform = orbit.transform();
        activity.request_frames(ViewportActivity::SETTLE_FRAMES);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> OrbitCamera {
        OrbitCamera {
            focus: Vec3::new(0.0, 2.0, 0.0),
            yaw: 0.0,
            pitch: 0.0,
            distance: 10.0,
            limits: OrbitLimits {
                distance: (1.0, 100.0),
                pitch: (-0.5, 1.0),
            },
        }
    }

    #[test]
    fn camera_looks_at_its_focus_from_its_distance() {
        let mut orbit = camera();
        orbit.yaw = 0.7;
        orbit.pitch = 0.4;
        let transform = orbit.transform();
        assert!((transform.translation.distance(orbit.focus) - 10.0).abs() < 1e-4);
        let toward_focus = (orbit.focus - transform.translation).normalize();
        assert!(transform.forward().dot(toward_focus) > 0.9999);
        // Positive pitch puts the camera above the focus point.
        assert!(transform.translation.y > orbit.focus.y);
    }

    #[test]
    fn zero_yaw_and_pitch_view_from_positive_z() {
        let transform = camera().transform();
        assert!(
            transform
                .translation
                .abs_diff_eq(Vec3::new(0.0, 2.0, 10.0), 1e-4)
        );
    }

    #[test]
    fn zoom_and_tilt_stay_within_limits() {
        let mut orbit = camera();
        orbit.zoom(1e6);
        assert_eq!(orbit.distance, 1.0);
        orbit.zoom(-1e6);
        assert_eq!(orbit.distance, 100.0);
        orbit.orbit(Vec2::new(0.0, 1e6));
        assert_eq!(orbit.pitch, 1.0);
        orbit.orbit(Vec2::new(0.0, -1e6));
        assert_eq!(orbit.pitch, -0.5);
    }

    #[test]
    fn pitch_limits_never_reach_straight_up() {
        let mut orbit = camera();
        orbit.limits.pitch = (-10.0, 10.0);
        orbit.orbit(Vec2::new(0.0, 1e6));
        assert!(orbit.pitch < FRAC_PI_2);
    }

    #[test]
    fn scrolling_up_moves_closer() {
        let mut orbit = camera();
        orbit.zoom(50.0);
        assert!(orbit.distance < 10.0);
    }

    #[test]
    fn panning_moves_focus_across_the_view() {
        let mut orbit = camera();
        let before = orbit.transform();
        orbit.pan(Vec2::new(100.0, 0.0));
        // Dragging right moves the focus to the camera's left, keeping height and depth.
        let moved = orbit.focus - Vec3::new(0.0, 2.0, 0.0);
        assert!(moved.dot(*before.left()) > 0.0);
        assert!(moved.y.abs() < 1e-5 && moved.z.abs() < 1e-5);
        // The camera keeps its orientation.
        assert!(
            orbit
                .transform()
                .rotation
                .abs_diff_eq(before.rotation, 1e-5)
        );
    }
}
