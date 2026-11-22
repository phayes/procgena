//! Where the scene camera draws, how pointer input reaches it, and when it renders.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::camera::RenderTarget;
use bevy::input::mouse::MouseScrollUnit;
use bevy::picking::events::{PointerDrag, PointerScroll};
use bevy::picking::pointer::PointerButton as PickingButton;
use bevy::prelude::*;
use bevy::render::render_resource::{PipelineCache, TextureFormat, TextureUsages};
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::ui::widget::ViewportNode;
use bevy::window::RequestRedraw;

/// Pixels scrolled per line, for mouse wheels that report whole lines.
const SCROLL_LINE_HEIGHT: f32 = 16.0;

/// Marks the camera that shows the scene. The scene spawns it.
///
/// Its render target is set to the [`ViewportTarget`] image, replacing any
/// target set on it.
#[derive(Component, Default, Debug, Clone, Copy)]
pub struct ViewportCamera;

/// Marks the UI node that shows the scene.
///
/// Add it to one [`Node`]. The scene draws into that node at the node's size,
/// and drags and scrolls over it move the camera.
#[derive(Component, Default, Debug, Clone, Copy)]
#[require(Node)]
pub struct SceneViewport;

/// Pointer input that moves the scene camera, delivered as a Bevy message.
///
/// Drags and scrolls over the [`SceneViewport`] node are sent for you. Write
/// these yourself to move the camera from elsewhere, for example in tests.
/// Positions and deltas are in logical pixels.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub enum ViewportInput {
    /// The pointer moved by `delta` while `button` was held.
    Drag {
        /// The held button.
        button: PointerButton,
        /// Movement since the previous event.
        delta: Vec2,
        /// Whether Shift was held.
        shift: bool,
    },
    /// The wheel or trackpad scrolled by `delta` (positive y scrolls up / away from the user).
    Scroll {
        /// Scroll amount.
        delta: Vec2,
    },
}

/// A mouse button reported by [`ViewportInput`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerButton {
    /// Primary button.
    Left,
    /// Secondary button.
    Right,
    /// Wheel button.
    Middle,
}

/// Controls when the scene renders.
///
/// The app only redraws while the scene is active, so an idle scene costs
/// nothing. It is active while [`continuous`](Self::continuous) is set, or until
/// the frames asked for with [`request_frames`](Self::request_frames) have been
/// rendered. The scene already asks for frames after startup, after every
/// [`ViewerCommand`](super::ViewerCommand), after camera moves, and while
/// shaders compile.
///
/// Systems with other work in flight (for example an asset still loading)
/// should call `request_frames(ViewportActivity::SETTLE_FRAMES)` every frame
/// until that work is done.
#[derive(Resource, Debug, Default)]
pub struct ViewportActivity {
    /// Render every frame, for animation such as wind or an auto-rotating camera.
    pub continuous: bool,
    frames_left: u32,
}

impl ViewportActivity {
    /// Enough frames for a change to be rendered and shown.
    pub const SETTLE_FRAMES: u32 = 3;

    /// Renders at least `frames` more frames.
    pub fn request_frames(&mut self, frames: u32) {
        self.frames_left = self.frames_left.max(frames);
    }

    /// Whether the scene should render a frame now.
    pub fn is_active(&self) -> bool {
        self.continuous || self.frames_left > 0
    }

    /// Counts one rendered frame against the requested frames.
    fn frame_rendered(&mut self) {
        self.frames_left = self.frames_left.saturating_sub(1);
    }
}

/// The image the scene camera renders into.
///
/// A [`SceneViewport`] node resizes it to fit. Without one (for example when
/// rendering headless) it stays at the size it was given; replace the image
/// in `Assets<Image>` to change it. The image is usable as a copy source, so
/// it can be read back to the CPU.
#[derive(Resource, Debug, Clone)]
pub struct ViewportTarget {
    /// The render target image.
    pub image: Handle<Image>,
}

/// Whether any render pipeline is still compiling, shared with the render world.
#[derive(Resource, Clone, Default)]
struct ShadersPending(Arc<AtomicBool>);

/// Creates the render target, connects the camera and UI node to it, turns
/// pointer events into [`ViewportInput`], and requests redraws while active.
pub(crate) struct ViewportPlugin;

impl Plugin for ViewportPlugin {
    fn build(&self, app: &mut App) {
        let image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(target_image(UVec2::ONE));

        app.insert_resource(ViewportTarget { image })
            .init_resource::<ViewportActivity>()
            .init_resource::<ShadersPending>()
            .add_message::<ViewportInput>()
            .add_systems(Startup, request_startup_frames)
            .add_systems(PreUpdate, keep_rendering_while_compiling)
            .add_systems(PostUpdate, (attach_camera, attach_viewport_node).chain())
            .add_systems(Last, request_redraws)
            .add_observer(drag_viewport)
            .add_observer(scroll_viewport);
    }

    fn finish(&self, app: &mut App) {
        // Report compiling pipelines from the render world.
        let pending = app.world().resource::<ShadersPending>().clone();
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .insert_resource(pending)
                .add_systems(Render, check_pipelines.in_set(RenderSystems::Cleanup));
        }
    }
}

/// A `size` BGRA8 sRGB image usable as a camera target and as a copy source.
pub(crate) fn target_image(size: UVec2) -> Image {
    let mut image = Image::new_target_texture(
        size.x.max(1),
        size.y.max(1),
        TextureFormat::Bgra8UnormSrgb,
        None,
    );
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    image
}

/// Renders the first frames unprompted so startup work is drawn.
fn request_startup_frames(mut activity: ResMut<ViewportActivity>) {
    activity.request_frames(ViewportActivity::SETTLE_FRAMES);
}

/// Points each new scene camera at the target image, rendering before the UI.
fn attach_camera(
    mut commands: Commands,
    target: Res<ViewportTarget>,
    mut cameras: Query<(Entity, &mut Camera), Added<ViewportCamera>>,
) {
    for (entity, mut camera) in &mut cameras {
        camera.order = -1;
        commands
            .entity(entity)
            .insert(RenderTarget::Image(target.image.clone().into()));
    }
}

/// Shows the scene camera in each [`SceneViewport`] node that does not show it yet.
fn attach_viewport_node(
    mut commands: Commands,
    nodes: Query<Entity, (With<SceneViewport>, Without<ViewportNode>)>,
    camera: Query<Entity, With<ViewportCamera>>,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    for node in &nodes {
        commands.entity(node).insert(ViewportNode::new(camera));
    }
}

/// Turns drags over the [`SceneViewport`] node into [`ViewportInput::Drag`].
fn drag_viewport(
    drag: On<PointerDrag>,
    nodes: Query<(), With<SceneViewport>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut input: MessageWriter<ViewportInput>,
) {
    if !nodes.contains(drag.entity) {
        return;
    }
    let button = match drag.button {
        PickingButton::Primary => PointerButton::Left,
        PickingButton::Secondary => PointerButton::Right,
        PickingButton::Middle => PointerButton::Middle,
    };
    input.write(ViewportInput::Drag {
        button,
        delta: drag.delta,
        shift: keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
    });
}

/// Turns scrolling over the [`SceneViewport`] node into [`ViewportInput::Scroll`].
fn scroll_viewport(
    mut scroll: On<PointerScroll>,
    nodes: Query<(), With<SceneViewport>>,
    mut input: MessageWriter<ViewportInput>,
) {
    if !nodes.contains(scroll.entity) {
        return;
    }
    scroll.propagate(false);
    let scale = match scroll.unit {
        MouseScrollUnit::Line => SCROLL_LINE_HEIGHT,
        MouseScrollUnit::Pixel => 1.0,
    };
    input.write(ViewportInput::Scroll {
        delta: Vec2::new(scroll.x, scroll.y) * scale,
    });
}

/// Records in the render world whether any pipeline is still compiling.
fn check_pipelines(cache: Res<PipelineCache>, pending: Res<ShadersPending>) {
    let waiting = cache.waiting_pipelines().next().is_some();
    pending.0.store(waiting, Ordering::Relaxed);
}

/// Keeps rendering while shaders compile, since objects appear only once they have.
fn keep_rendering_while_compiling(
    pending: Res<ShadersPending>,
    mut activity: ResMut<ViewportActivity>,
) {
    if pending.0.load(Ordering::Relaxed) {
        activity.request_frames(ViewportActivity::SETTLE_FRAMES);
    }
}

/// Asks the window for another frame while the scene is active, and counts
/// this frame against the requested ones.
fn request_redraws(
    mut activity: ResMut<ViewportActivity>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    if activity.is_active() {
        redraw.write(RequestRedraw);
        activity.frame_rendered();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_frames_count_down_to_idle() {
        let mut activity = ViewportActivity::default();
        assert!(!activity.is_active());
        activity.request_frames(2);
        activity.request_frames(1);
        assert!(activity.is_active());
        activity.frame_rendered();
        assert!(activity.is_active());
        activity.frame_rendered();
        assert!(!activity.is_active());
    }

    #[test]
    fn continuous_rendering_stays_active() {
        let mut activity = ViewportActivity {
            continuous: true,
            ..default()
        };
        activity.frame_rendered();
        assert!(activity.is_active());
    }
}
