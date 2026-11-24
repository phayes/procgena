//! Grow clump presets on a rock, a tree or a ground patch.
//!
//! Run `cargo run -p procgena_clump_bevy --example viewer` from the repository.
//! The viewer reads textures from the repository's `stone/assets/textures`,
//! `tree/assets/textures` and `clump/assets/textures` folders when they have
//! been downloaded (`uv run --project tools textures download <stone|tree|clump>`),
//! and uses plain colours otherwise.
//!
//! Controls:
//! - H: grow on the next host: a rock, a tree, or a ground patch.
//! - Tab: show the next clump preset; Shift+Tab shows the previous one.
//! - Left / Right: choose the previous / next clump seed.
//! - Up / Down: choose the next / previous host seed.
//! - R: turn the host a quarter turn about the x axis, to see that snow
//!   still falls downward.
//! - Hold the right mouse button to look around (M keeps the cursor captured).
//!   Use WASD to fly, E / Q to rise / sink, and hold left Shift to fly faster.
//!
//! Pass options after `--`, for example:
//! `cargo run -p procgena_clump_bevy --example viewer -- --host tree --preset moss_patches`.
//! `--host` is `rock`, `tree` or `ground`; `--preset` takes a preset name such
//! as `fresh_snow_cap`. Use `--untextured` to draw plain colours only, `--eye x y z`
//! and `--target x y z` to place the camera, and `--screenshot <file.png>`
//! to save a PNG once the clump has grown, then exit. Add `--headless` to
//! save the image without opening a window; it requires `--screenshot`.

use std::path::Path;
use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::camera::RenderTarget;
use bevy::camera_controller::free_camera::{FreeCamera, FreeCameraPlugin};
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use procgena_clump::ClumpPreset;
use procgena_clump_bevy::{
    ClumpFinished, ClumpGenerating, ClumpParams, ClumpPart, ClumpPlugin, ObjectTags,
};
use procgena_specimen::tags;
use procgena_stone::StoneOptions;
use procgena_stone_bevy::{StoneParams, StonePlugin};
use procgena_tree::TreePreset;
use procgena_tree_bevy::{TreeParams, TreePlugin};

/// Frames to wait after the clump appears before a screenshot, for textures
/// and shadows to settle.
const SETTLE_FRAMES: u32 = 20;
/// Longest wait in seconds for a screenshot's clump to appear.
const SCREENSHOT_TIMEOUT: f32 = 120.0;

/// What the clump grows on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Host {
    Rock,
    Tree,
    Ground,
}

impl Host {
    fn next(self) -> Self {
        match self {
            Self::Rock => Self::Tree,
            Self::Tree => Self::Ground,
            Self::Ground => Self::Rock,
        }
    }
}

#[derive(Resource)]
struct Viewer {
    host: Host,
    host_seed: u32,
    preset: usize,
    clump_seed: u32,
    /// Quarter turns of the host about the x axis.
    turns: u32,
    screenshot: Option<String>,
    headless: bool,
}

#[derive(Resource)]
struct CameraSetup {
    eye: Vec3,
    target: Vec3,
}

/// Image that receives the camera's view when there is no window.
#[derive(Resource)]
struct Offscreen(Handle<Image>);

/// The current host entity.
#[derive(Component)]
struct Shown;

fn main() -> AppExit {
    let mut viewer = Viewer {
        host: Host::Rock,
        host_seed: 0,
        preset: 0,
        clump_seed: ClumpPreset::fresh_snow_cap().clump.default_seed,
        turns: 0,
        screenshot: None,
        headless: false,
    };
    let mut camera = CameraSetup {
        eye: Vec3::new(3.0, 2.5, 4.0),
        target: Vec3::new(0.0, 0.6, 0.0),
    };
    let mut textured = true;

    // Read command-line options.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let vec3 = |at: usize| {
            let n = |k: usize| args[at + k].parse::<f32>().expect("expected a number");
            Vec3::new(n(1), n(2), n(3))
        };
        match args[i].as_str() {
            "--host" => {
                i += 1;
                viewer.host = match args[i].as_str() {
                    "rock" => Host::Rock,
                    "tree" => Host::Tree,
                    "ground" => Host::Ground,
                    other => panic!("unknown host {other}"),
                };
            }
            "--preset" => {
                i += 1;
                let preset = ClumpPreset::named(&args[i]).expect("unknown preset");
                viewer.preset = ClumpPreset::all().position(|p| p == preset).unwrap();
                viewer.clump_seed = preset.clump.default_seed;
            }
            "--untextured" => textured = false,
            "--eye" => {
                camera.eye = vec3(i);
                i += 3;
            }
            "--target" => {
                camera.target = vec3(i);
                i += 3;
            }
            "--screenshot" => {
                i += 1;
                viewer.screenshot = Some(args[i].clone());
            }
            "--headless" => viewer.headless = true,
            other => panic!("unknown option {other}"),
        }
        i += 1;
    }
    assert!(
        !viewer.headless || viewer.screenshot.is_some(),
        "--headless requires --screenshot <file.png>"
    );

    // Read assets from the repository root, so all three texture folders are
    // in reach. Use each folder only when its textures have been downloaded.
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../..");
    let downloaded = |folder: &str, set: &str| {
        (textured && Path::new(root).join(folder).join(set).is_dir()).then(|| folder.to_owned())
    };
    let defaults = DefaultPlugins
        .set(AssetPlugin {
            file_path: root.to_owned(),
            ..default()
        })
        .set(RenderPlugin {
            synchronous_pipeline_compilation: true,
            ..default()
        });

    // Without a window, use a timed loop to keep rendering until capture finishes.
    let mut app = App::new();
    if viewer.headless {
        app.add_plugins((
            defaults
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .disable::<WinitPlugin>(),
            ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(1.0 / 60.0)),
        ));
    } else {
        app.add_plugins((defaults, FreeCameraPlugin));
    }
    app.add_plugins((
        StonePlugin {
            texture_root: downloaded("stone/assets/textures", "rock"),
            ..default()
        },
        TreePlugin {
            texture_root: downloaded("tree/assets/textures", "bark"),
            ..default()
        },
        ClumpPlugin {
            texture_root: downloaded("clump/assets/textures", "snow"),
            ..default()
        },
    ))
    .insert_resource(viewer)
    .insert_resource(camera)
    .add_systems(Startup, setup)
    .add_systems(Update, (controls, show_host, screenshot).chain())
    .add_observer(report)
    .run()
}

fn setup(
    mut commands: Commands,
    camera: Res<CameraSetup>,
    viewer: Res<Viewer>,
    mut images: ResMut<Assets<Image>>,
) {
    // Create an image to hold the camera's view for a headless screenshot.
    let mut entity = commands.spawn((
        Camera3d::default(),
        Transform::from_translation(camera.eye).looking_at(camera.target, Vec3::Y),
        FreeCamera {
            walk_speed: 3.0,
            run_speed: 12.0,
            ..default()
        },
    ));
    if viewer.headless {
        let image = Image::new_target_texture(1280, 720, TextureFormat::Rgba8UnormSrgb, None);
        let handle = images.add(image);
        entity.insert(RenderTarget::Image(handle.clone().into()));
        commands.insert_resource(Offscreen(handle));
    }

    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 400.0,
        ..default()
    });
    commands.insert_resource(ClearColor(Color::srgb(0.62, 0.76, 0.9)));
}

/// Changes the host, preset and seeds from the keyboard.
fn controls(keys: Res<ButtonInput<KeyCode>>, mut viewer: ResMut<Viewer>) {
    let presets = ClumpPreset::all().count();
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if keys.just_pressed(KeyCode::KeyH) {
        viewer.host = viewer.host.next();
    }
    if keys.just_pressed(KeyCode::Tab) {
        viewer.preset = if shift {
            (viewer.preset + presets - 1) % presets
        } else {
            (viewer.preset + 1) % presets
        };
    }
    if keys.just_pressed(KeyCode::ArrowRight) {
        viewer.clump_seed = viewer.clump_seed.wrapping_add(1);
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        viewer.clump_seed = viewer.clump_seed.wrapping_sub(1);
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        viewer.host_seed = viewer.host_seed.wrapping_add(1);
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        viewer.host_seed = viewer.host_seed.wrapping_sub(1);
    }
    if keys.just_pressed(KeyCode::KeyR) {
        viewer.turns = (viewer.turns + 1) % 4;
    }
}

/// Respawns the host and its clump whenever the viewer's choices change.
fn show_host(
    mut commands: Commands,
    viewer: Res<Viewer>,
    shown: Query<Entity, With<Shown>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !viewer.is_changed() {
        return;
    }
    for entity in &shown {
        commands.entity(entity).despawn();
    }

    // Spawn the host, turned about its centre.
    let turn = Quat::from_rotation_x(viewer.turns as f32 * std::f32::consts::FRAC_PI_2);
    let mut host = match viewer.host {
        Host::Rock => {
            let rock = StoneOptions {
                default_seed: viewer.host_seed,
                scale: 1.5,
                ..StoneOptions::default()
            };
            commands.spawn(StoneParams::new(rock))
        }
        Host::Tree => {
            let mut tree = TreePreset::OakSmall.options();
            tree.seed = tree.seed.wrapping_add_unsigned(viewer.host_seed);
            commands.spawn(TreeParams::new(tree))
        }
        Host::Ground => {
            let ground = Plane3d::default().mesh().size(3.0, 3.0).subdivisions(30);
            commands.spawn((
                Mesh3d(meshes.add(ground)),
                MeshMaterial3d(materials.add(Color::srgb(0.32, 0.42, 0.22))),
                ObjectTags([tags::GROUND].into()),
            ))
        }
    };
    host.insert((Shown, Transform::from_rotation(turn)));

    // Grow the chosen preset on it.
    let preset = ClumpPreset::all()
        .nth(viewer.preset)
        .expect("the viewer's preset index is in range");
    let mut options = preset.clump.clone();
    options.default_seed = viewer.clump_seed;
    host.with_child(ClumpParams::new(options));
    info!(
        "host {:?} seed {}, clump {} seed {}",
        viewer.host, viewer.host_seed, preset.key, viewer.clump_seed
    );
}

/// Logs each finished clump build.
fn report(event: On<ClumpFinished>) {
    match &event.result {
        Ok(built) => info!(
            "clump grown: {} triangles in {:.0} ms ({:?})",
            built.triangles,
            built.build_time.as_secs_f64() * 1000.0,
            built.stats
        ),
        Err(error) => error!("clump failed: {error}"),
    }
}

/// Saves a screenshot once the clump has grown and settled, then exits.
#[allow(clippy::too_many_arguments)]
fn screenshot(
    mut commands: Commands,
    viewer: Res<Viewer>,
    time: Res<Time>,
    offscreen: Option<Res<Offscreen>>,
    parts: Query<(), With<ClumpPart>>,
    generating: Query<(), With<ClumpGenerating>>,
    mut exit: MessageWriter<AppExit>,
    mut settled: Local<u32>,
    mut taken: Local<bool>,
) {
    let Some(path) = &viewer.screenshot else {
        return;
    };
    if *taken {
        return;
    }

    // Wait for the clump, then a few more frames.
    if parts.is_empty() || !generating.is_empty() {
        *settled = 0;
        if time.elapsed_secs() > SCREENSHOT_TIMEOUT {
            error!("screenshot failed: no clump after {SCREENSHOT_TIMEOUT} seconds");
            exit.write(AppExit::error());
            *taken = true;
        }
        return;
    }
    *settled += 1;
    if *settled < SETTLE_FRAMES {
        return;
    }
    *taken = true;
    let target = match offscreen {
        Some(offscreen) => Screenshot::image(offscreen.0.clone()),
        None => Screenshot::primary_window(),
    };

    // Exit after saving, reporting whether the PNG was written.
    let path = path.clone();
    commands.spawn(target).observe(
        move |captured: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
            let saved = captured
                .image
                .clone()
                .try_into_dynamic()
                .map_err(|e| e.to_string())
                .and_then(|image| {
                    image
                        .to_rgb8()
                        .save_with_format(&path, image::ImageFormat::Png)
                        .map_err(|e| e.to_string())
                });
            match saved {
                Ok(()) => {
                    info!("screenshot saved to {path}");
                    exit.write(AppExit::Success);
                }
                Err(message) => {
                    error!("screenshot failed: could not save {path}: {message}");
                    exit.write(AppExit::error());
                }
            }
        },
    );
}
