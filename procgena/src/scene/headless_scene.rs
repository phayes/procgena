//! The scene alone, without the studio or a window, for the scene's GPU tests.
//!
//! [`HeadlessScene`] runs the scene on the calling thread. It updates only
//! when asked, and reads each rendered frame back to the CPU as a [`Frame`].

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::app::PluginsState;
use bevy::prelude::*;
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::tasks::tick_global_task_pools_on_main_thread;

use super::headless::{headless_plugins, unpad_rows};
use super::viewport::target_image;
use super::{
    OrbitCamera, ScenePlugin, ViewerCommand, ViewerEvent, ViewportActivity, ViewportCamera,
    ViewportInput, ViewportTarget,
};

/// Time slept per extra frame rendered after the scene goes idle while
/// waiting for the readbacks of frames it asked for, so the GPU can catch up.
const DRAIN_PACE: Duration = Duration::from_millis(2);

/// The longest any wait in this module may take before it panics.
const LIMIT: Duration = Duration::from_secs(120);

/// One rendered frame, tightly packed BGRA8 (sRGB), top row first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes in B, G, R, A order.
    pub bgra: Vec<u8>,
}

/// Frames read back so far, numbered from 1 in the order they were rendered.
#[derive(Default)]
struct FrameLog {
    delivered: u64,
    frames: VecDeque<(u64, Frame)>,
}

/// Where readbacks are stored for the [`HeadlessScene`] to collect.
#[derive(Resource, Clone)]
struct FrameSink {
    log: Arc<Mutex<FrameLog>>,
    size: UVec2,
}

/// Where scene events are stored for the [`HeadlessScene`] to collect.
#[derive(Resource, Clone, Default)]
struct EventSink(Arc<Mutex<VecDeque<ViewerEvent>>>);

/// The scene running without a window on the calling thread.
///
/// Nothing happens between calls: each method updates the app as much as it
/// needs to. Commands and input sent with [`send`](Self::send) and
/// [`input`](Self::input) are applied on the next update.
pub struct HeadlessScene {
    app: App,
    frames: Arc<Mutex<FrameLog>>,
    events: Arc<Mutex<VecDeque<ViewerEvent>>>,
    /// Frames rendered so far.
    rendered: u64,
}

impl HeadlessScene {
    /// Starts an empty scene, with no subject shown, rendering at `size`
    /// pixels. Show a subject by sending [`ViewerCommand::Show`]. Clumps are
    /// drawn in their plain material colours, without textures.
    ///
    /// # Panics
    ///
    /// Panics if Bevy cannot start its renderer, for example without a GPU.
    pub fn new(size: UVec2) -> Self {
        Self::with_sources(size, super::add_asset_sources)
    }

    /// Like [`new`](Self::new), but registers the asset sources with
    /// `sources` instead of [`add_asset_sources`](super::add_asset_sources).
    pub fn with_sources(size: UVec2, sources: impl FnOnce(&mut App)) -> Self {
        Self::build(ScenePlugin::default(), size, sources)
    }

    /// Like [`new`](Self::new), but trees are drawn at their farthest mesh
    /// level at every distance beyond it, never as impostors.
    pub fn without_impostors(size: UVec2) -> Self {
        Self::build(
            ScenePlugin::default().without_impostors(),
            size,
            super::add_asset_sources,
        )
    }

    /// Runs `scene`, with asset sources registered by `sources`, rendering
    /// at `size` pixels.
    fn build(scene: ScenePlugin, size: UVec2, sources: impl FnOnce(&mut App)) -> Self {
        let size = size.max(UVec2::ONE);
        let frames = Arc::new(Mutex::new(FrameLog::default()));
        let events = EventSink::default();

        // Build the app and finish its plugins; the renderer starts
        // asynchronously. Clumps are drawn in flat colours, which tests
        // recognise more reliably than textures. Impostors are baked afresh,
        // whatever the user's cache folder holds.
        let mut app = App::new();
        sources(&mut app);
        let scene = scene.without_clump_textures().without_impostor_cache();
        app.add_plugins((headless_plugins(), scene))
            .insert_resource(FrameSink {
                log: frames.clone(),
                size,
            })
            .insert_resource(events.clone())
            .add_systems(Startup, spawn_readback)
            .add_systems(Last, collect_events);
        while app.plugins_state() == PluginsState::Adding {
            tick_global_task_pools_on_main_thread();
        }
        app.finish();
        app.cleanup();

        // Render at the requested size.
        let image = app.world().resource::<ViewportTarget>().image.clone();
        let _ = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .insert(image.id(), target_image(size));

        Self {
            app,
            frames,
            events: events.0,
            rendered: 0,
        }
    }

    /// Sends `command` to the scene, to be applied on the next update.
    pub fn send(&mut self, command: ViewerCommand) {
        self.app.world_mut().write_message(command);
    }

    /// Sends pointer input to the scene, to be applied on the next update.
    pub fn input(&mut self, input: ViewportInput) {
        self.app.world_mut().write_message(input);
    }

    /// The scene camera's current view.
    ///
    /// # Panics
    ///
    /// Panics before the first update, when the camera does not exist yet.
    pub fn camera_view(&mut self) -> OrbitCamera {
        let world = self.app.world_mut();
        let mut cameras = world.query_filtered::<&OrbitCamera, With<ViewportCamera>>();
        *cameras
            .single(world)
            .expect("the scene has exactly one camera")
    }

    /// The scene's world, for checking its entities directly.
    pub fn world_mut(&mut self) -> &mut World {
        self.app.world_mut()
    }

    /// Takes every event the scene has reported and not yet been taken, oldest first.
    pub fn take_events(&mut self) -> Vec<ViewerEvent> {
        lock(&self.events).drain(..).collect()
    }

    /// Returns the next reported event for which `wanted` holds, updating the
    /// scene until one arrives. Events before it are discarded.
    ///
    /// # Panics
    ///
    /// Panics if no such event arrives within two minutes.
    pub fn next_event(&mut self, wanted: impl Fn(&ViewerEvent) -> bool) -> ViewerEvent {
        let deadline = Instant::now() + LIMIT;
        loop {
            while let Some(event) = lock(&self.events).pop_front() {
                if wanted(&event) {
                    return event;
                }
            }
            assert!(
                Instant::now() < deadline,
                "no matching event within {LIMIT:?}"
            );
            self.update();
        }
    }

    /// Renders until the scene goes idle, and returns every frame it asked for,
    /// in order. The next update always counts as asked for, so changes sent
    /// just before are drawn.
    ///
    /// Frames rendered after the scene went idle, only to collect the
    /// readbacks of earlier frames, are not returned.
    ///
    /// # Panics
    ///
    /// Panics if the scene is still active, or the frames it asked for are
    /// not read back, within two minutes, for example with wind on.
    pub fn settle(&mut self) -> Vec<Frame> {
        let deadline = Instant::now() + LIMIT;
        let mut last_asked = self.rendered + 1;
        loop {
            assert!(
                Instant::now() < deadline,
                "the scene did not go idle, and its frames were not read back, within {LIMIT:?}"
            );
            let asked = self.rendered < last_asked
                || self.app.world().resource::<ViewportActivity>().is_active();
            self.update();
            if asked {
                last_asked = self.rendered;
            } else {
                // Readbacks can trail rendering by many frames after heavy
                // work; wait for the last asked-for frame's.
                if lock(&self.frames).delivered >= last_asked {
                    break;
                }
                std::thread::sleep(DRAIN_PACE);
            }
        }

        // Keep only the frames the scene asked for.
        let mut log = lock(&self.frames);
        log.frames
            .drain(..)
            .filter(|(number, _)| *number <= last_asked)
            .map(|(_, frame)| frame)
            .collect()
    }

    /// Renders until the scene goes idle and returns the last frame it asked for.
    ///
    /// # Panics
    ///
    /// Panics as [`settle`](Self::settle) does, or if no frame could be read back.
    pub fn settled_frame(&mut self) -> Frame {
        self.settle()
            .pop()
            .expect("no frame was read back from the scene")
    }

    /// Runs one frame.
    pub fn update(&mut self) {
        self.app.update();
        self.rendered += 1;
    }
}

/// Locks `mutex`, recovering the data if a panic poisoned it.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Starts copying the render target back to the CPU every frame.
fn spawn_readback(mut commands: Commands, target: Res<ViewportTarget>) {
    commands
        .spawn(Readback::texture(target.image.clone()))
        .observe(store_frame);
}

/// Stores each completed readback that matches the render size.
fn store_frame(event: On<ReadbackComplete>, sink: Res<FrameSink>) {
    let mut log = lock(&sink.log);
    log.delivered += 1;
    let number = log.delivered;
    let UVec2 {
        x: width,
        y: height,
    } = sink.size;
    if let Some(bgra) = unpad_rows(&event.data, width, height) {
        log.frames.push_back((
            number,
            Frame {
                width,
                height,
                bgra,
            },
        ));
    }
}

/// Copies this frame's scene events to the [`EventSink`].
fn collect_events(mut events: MessageReader<ViewerEvent>, sink: Res<EventSink>) {
    lock(&sink.0).extend(events.read().cloned());
}
