//! Screenshots and frame timing from the command line.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};

use crate::GameState;
use crate::camera::Offscreen;

/// Frames to wait after loading before a screenshot or measuring, so
/// shaders, textures and the copies' assets are ready.
const SETTLE_FRAMES: u32 = 120;

/// What to capture once the demo has loaded and settled.
#[derive(Resource, Debug, Clone, Default)]
pub struct Capture {
    /// Save an image here, then exit (after measuring, if asked).
    pub screenshot: Option<String>,
    /// Time this many frames and print their times, then exit.
    pub measure: Option<u32>,
    /// Don't exit after capturing: something else ends the run.
    pub stay: bool,
}

/// Saves the screenshot and measures frames, as [`Capture`] asks.
pub struct CapturePlugin;

impl Plugin for CapturePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Frames>()
            .add_systems(Update, capture.run_if(in_state(GameState::Playing)));
    }
}

/// Where capturing has got to.
#[derive(Resource, Default)]
struct Frames {
    /// Frames since loading finished.
    settled: u32,
    /// The measured frame times, in seconds.
    times: Vec<f32>,
    /// Whether everything asked for has been done.
    done: bool,
}

/// Waits for the scene to settle, measures the frames asked for, then
/// takes the screenshot and exits once it is written (or at once, without
/// one), unless [`Capture::stay`].
fn capture(
    mut commands: Commands,
    request: Res<Capture>,
    offscreen: Option<Res<Offscreen>>,
    time: Res<Time<Real>>,
    mut frames: ResMut<Frames>,
    mut exit: MessageWriter<AppExit>,
) {
    if frames.done || (request.screenshot.is_none() && request.measure.is_none()) {
        return;
    }
    frames.settled += 1;
    if frames.settled < SETTLE_FRAMES {
        return;
    }

    // Time the frames asked for.
    if let Some(count) = request.measure {
        frames.times.push(time.delta_secs());
        if frames.times.len() < count as usize {
            return;
        }
        report(&frames.times);
    }
    frames.done = true;

    // Then take the screenshot.
    let stay = request.stay;
    let Some(path) = &request.screenshot else {
        if !stay {
            exit.write(AppExit::Success);
        }
        return;
    };
    let target = match offscreen {
        Some(offscreen) => Screenshot::image(offscreen.0.clone()),
        None => Screenshot::primary_window(),
    };
    commands
        .spawn(target)
        .observe(save_to_disk(path.clone()))
        .observe(
            move |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                if !stay {
                    exit.write(AppExit::Success);
                }
            },
        );
}

/// Prints the mean, median, 99th percentile and worst of `times` (in
/// seconds), and how many frames took longer than 16.7 ms and 33 ms.
fn report(times: &[f32]) {
    let mut sorted = times.to_vec();
    sorted.sort_by(f32::total_cmp);
    let ms = |s: f32| s * 1000.0;
    let at = |q: f32| sorted[((sorted.len() - 1) as f32 * q).round() as usize];
    let mean = sorted.iter().sum::<f32>() / sorted.len() as f32;
    let over = |limit: f32| sorted.iter().filter(|t| **t > limit).count();
    println!(
        "frames: {} | mean {:.2} ms ({:.0} fps) | median {:.2} ms | p99 {:.2} ms | worst {:.2} ms \
         | over 16.7 ms: {} | over 33 ms: {}",
        sorted.len(),
        ms(mean),
        1.0 / mean,
        ms(at(0.5)),
        ms(at(0.99)),
        ms(*sorted.last().expect("at least one frame")),
        over(1.0 / 60.0),
        over(1.0 / 30.0),
    );
}
