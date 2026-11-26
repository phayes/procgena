//! The lumberjack and the wind: a showcase demo built on procgena.
//!
//! ```bash
//! cargo run -p procgena_demo_lumberjack --release
//! ```
//!
//! Walk with W, A, S and D, run with Shift, jump with Space, and look around
//! with the mouse. V switches between the third-person and first-person
//! camera; Escape pauses.
//!
//! The lumberjack is read from `demo/assets/lumberjack/lumberjack.glb`,
//! which isn't in the repository: make it once with the Blender script in
//! `demo/crates/procgena_demo_lumberjack/tools/prepare_lumberjack.py` (its
//! opening comment says how).
//!
//! Options:
//!
//! - `--seed <n>`: the World seed, instead of the demo's own;
//! - `--settings <file.demo.yml>`: read the demo's tunables from this file
//!   instead of its own `presets/lumberjack.demo.yml`;
//! - `--quality <low|medium|high|ultra>`: how far each level of detail
//!   reaches (default high);
//! - `--screenshot <file.png>`: once everything is loaded and has settled,
//!   save an image and exit;
//! - `--measure <frames>`: once everything has settled, time this many
//!   frames, print the frame times and exit (after the screenshot, with
//!   `--screenshot`);
//! - `--autowalk`: instead of reading the keys, run the lumberjack around
//!   the start, to the nearest road and along it; print whether he ever
//!   sank through the ground, got stuck or went into a trunk, and exit
//!   (with an error if he did). A screenshot or measurement is taken
//!   during the walk;
//! - `--view <behind|front|eyes>`: the view play starts in: behind him (the
//!   default), in front of him looking at his face, or through his eyes;
//! - `--time-placing <runs>`: don't open the demo; place the forest's tile
//!   at the start `runs` times with its Scatters added one at a time, print
//!   what each Scatter costs, and exit;
//! - `--load-limit <seconds>`: exit with an error if loading takes longer;
//! - `--headless`: with `--screenshot`, `--measure` or `--autowalk`, draw
//!   into an image instead of a window, with frames uncapped;
//! - `--resolution <w> <h>`: the image size with `--headless` (default
//!   1280 × 720);
//! - `--profile`: with `--measure`, also print how long the main and
//!   render schedules took;
//! - `--without <keys>`: leave these Scatters of the forest out, such as
//!   `--without pines,undergrowth`;
//! - `--no-shadows`: the sun casts no shadows.

use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use procgena_core::Quality;

mod animation;
mod autowalk;
mod camera;
mod capture;
mod land;
mod player;
mod profile;
mod settings;
mod sky;
mod timing;

use autowalk::AutowalkPlugin;
use camera::CameraPlugin;
use capture::{Capture, CapturePlugin};
use land::LandPlugin;
use player::{PlayerPlugin, View};
use profile::ProfilePlugin;
use settings::DemoOptions;
use sky::SkyPlugin;

/// Where the demo is: loading its land, playing, or paused.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    /// Building the land and placing the forest, behind a loading screen.
    #[default]
    Loading,
    /// Walking about.
    Playing,
    /// Time stands still.
    Paused,
}

/// What the command line asked for.
#[derive(Resource, Debug, Clone)]
pub struct Launch {
    /// The World seed, if not the demo's own.
    pub seed: Option<u32>,
    /// The drawing quality.
    pub quality: Quality,
    /// Draw into an image instead of a window.
    pub headless: bool,
    /// The image size with `headless`.
    pub resolution: UVec2,
    /// The forest's Scatters to leave out, by key.
    pub without: Vec<String>,
    /// The sun casts shadows.
    pub shadows: bool,
}

fn main() -> AppExit {
    let args: Vec<String> = std::env::args().collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let has = |flag: &str| args.iter().any(|a| a == flag);

    // Read the command line.
    let autowalk = has("--autowalk");
    let capture = Capture {
        screenshot: value("--screenshot"),
        measure: value("--measure").map(|n| n.parse().expect("--measure needs a number of frames")),
        stay: autowalk,
        load_limit: value("--load-limit")
            .map(|s| s.parse().expect("--load-limit needs a number of seconds")),
    };
    let headless = has("--headless");
    assert!(
        !headless || capture.screenshot.is_some() || capture.measure.is_some() || autowalk,
        "--headless needs --screenshot <file.png>, --measure <frames> or --autowalk"
    );
    let resolution = match args.iter().position(|a| a == "--resolution") {
        Some(i) => {
            let n = |k: usize| -> u32 {
                args.get(i + k)
                    .and_then(|v| v.parse().ok())
                    .expect("--resolution needs a width and a height")
            };
            UVec2::new(n(1), n(2))
        }
        None => UVec2::new(1280, 720),
    };
    let quality = match value("--quality").as_deref() {
        None | Some("high") => Quality::High,
        Some("medium") => Quality::Medium,
        Some("low") => Quality::Low,
        Some("ultra") => Quality::Ultra,
        Some(other) => panic!("--quality is low, medium, high or ultra, not {other:?}"),
    };
    let launch = Launch {
        seed: value("--seed").map(|s| s.parse().expect("--seed needs a whole number")),
        quality,
        headless,
        resolution,
        without: value("--without")
            .map(|keys| keys.split(',').map(str::to_owned).collect())
            .unwrap_or_default(),
        shadows: !has("--no-shadows"),
    };
    let profile = has("--profile");
    assert!(
        !profile || capture.measure.is_some(),
        "--profile needs --measure <frames>"
    );

    // The demo's tunables.
    let options = match value("--settings") {
        Some(path) => DemoOptions::read(&path).unwrap_or_else(|e| panic!("{e}")),
        None => DemoOptions::lumberjack(),
    };

    // Only time placing, without Bevy, if asked.
    if let Some(runs) = value("--time-placing") {
        let runs = runs.parse().expect("--time-placing needs a number of runs");
        let land = land::Land::new(&launch, &options.land);
        timing::time_placing(&land, &options, runs);
        return AppExit::Success;
    }

    // Bevy, without a window when headless.
    let defaults = DefaultPlugins.set(AssetPlugin {
        file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../..").to_owned(),
        ..default()
    });
    let mut app = App::new();
    if headless {
        app.add_plugins((
            defaults
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .disable::<WinitPlugin>(),
            ScheduleRunnerPlugin::run_loop(Duration::ZERO),
        ));
    } else {
        app.add_plugins(defaults.set(WindowPlugin {
            primary_window: Some(Window {
                title: "The lumberjack and the wind".to_owned(),
                ..default()
            }),
            ..default()
        }));
    }

    // The view play starts in.
    let mut view = View::default();
    match value("--view").as_deref() {
        None | Some("behind") => {}
        Some("front") => view.yaw = std::f32::consts::PI,
        Some("eyes") => view.first_person = true,
        Some(other) => panic!("--view is behind, front or eyes, not {other:?}"),
    }
    app.insert_resource(view);

    // The demo's own plugins.
    app.init_state::<GameState>()
        .insert_resource(options)
        .insert_resource(launch)
        .insert_resource(capture)
        .add_plugins((
            CameraPlugin,
            SkyPlugin,
            LandPlugin,
            PlayerPlugin {
                input: !headless && !autowalk,
            },
            CapturePlugin,
        ));
    if profile {
        app.add_plugins(ProfilePlugin);
    }
    if autowalk {
        app.add_plugins(AutowalkPlugin);
    }
    app.run()
}
