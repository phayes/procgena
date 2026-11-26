//! Where each frame's time goes, for `--profile`.
//!
//! While [`Capture`](crate::capture::Capture) measures frames, this times
//! the main schedule (the game's systems) and the render schedule (the
//! render thread's work) of each frame.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use bevy::app::MainScheduleOrder;
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
use bevy::render::{Render, RenderApp, RenderSystems};

/// Times each frame's schedules, as the module describes.
pub struct ProfilePlugin;

impl Plugin for ProfilePlugin {
    fn build(&self, app: &mut App) {
        // The main schedule's time, from before `First` to after `Last`.
        let profile = Profile::default();
        app.insert_resource(profile.clone())
            .init_schedule(FrameStart)
            .init_schedule(FrameEnd)
            .add_systems(FrameStart, start_main)
            .add_systems(FrameEnd, end_main);
        let mut order = app.world_mut().resource_mut::<MainScheduleOrder>();
        order.insert_before(First, FrameStart);
        order.insert_after(Last, FrameEnd);

        // The render schedule's time, on the render thread.
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.insert_resource(profile).add_systems(
                Render,
                (
                    start_render.in_set(RenderSystems::ExtractCommands),
                    end_render.in_set(RenderSystems::PostCleanup),
                ),
            );
        }
    }
}

/// Runs before the main schedule each frame.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct FrameStart;

/// Runs after the main schedule each frame.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct FrameEnd;

/// What has been timed, shared by the main and render worlds.
#[derive(Resource, Clone, Default)]
pub struct Profile(Arc<Mutex<Timings>>);

/// The times gathered while recording.
#[derive(Default)]
struct Timings {
    /// Whether frames are being recorded now.
    recording: bool,
    /// When this frame's main schedule began, and the render schedule.
    main_started: Option<Instant>,
    render_started: Option<Instant>,
    /// Each recorded frame's main and render schedule time, in seconds.
    main: Vec<f32>,
    render: Vec<f32>,
}

impl Profile {
    /// Starts recording, from the next frame.
    pub fn start(&self) {
        self.lock().recording = true;
    }

    /// Stops recording and prints the main and render schedules' times
    /// over the recorded frames.
    pub fn report(&self) {
        let mut timings = self.lock();
        timings.recording = false;
        println!(
            "main schedule: {} | render schedule: {}",
            summary(&timings.main),
            summary(&timings.render)
        );
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Timings> {
        self.0
            .lock()
            .expect("no profile timer panics while holding it")
    }
}

/// The mean, median and worst of `times` (seconds), in milliseconds.
fn summary(times: &[f32]) -> String {
    if times.is_empty() {
        return "no frames".to_owned();
    }
    let mut sorted = times.to_vec();
    sorted.sort_by(f32::total_cmp);
    let mean = sorted.iter().sum::<f32>() / sorted.len() as f32;
    format!(
        "mean {:.2} ms, median {:.2} ms, worst {:.2} ms",
        mean * 1000.0,
        sorted[sorted.len() / 2] * 1000.0,
        sorted[sorted.len() - 1] * 1000.0
    )
}

fn start_main(profile: Res<Profile>) {
    profile.lock().main_started = Some(Instant::now());
}

fn end_main(profile: Res<Profile>) {
    let mut timings = profile.lock();
    if let Some(started) = timings.main_started.take()
        && timings.recording
    {
        timings.main.push(started.elapsed().as_secs_f32());
    }
}

fn start_render(profile: Res<Profile>) {
    profile.lock().render_started = Some(Instant::now());
}

fn end_render(profile: Res<Profile>) {
    let mut timings = profile.lock();
    if let Some(started) = timings.render_started.take()
        && timings.recording
    {
        timings.render.push(started.elapsed().as_secs_f32());
    }
}
