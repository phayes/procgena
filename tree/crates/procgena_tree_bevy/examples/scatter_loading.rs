//! Prepares a grove behind a loading screen, then plants it: every tree the
//! grove can use is generated and its impostor baked first, so the grove
//! appears complete within a few frames.
//!
//! ```sh
//! cargo run -p procgena_tree_bevy --example scatter_loading
//! cargo run -p procgena_tree_bevy --example scatter_loading -- --cache target/impostors
//! ```
//!
//! - `--preset <key>`: the tree Scatter preset (default `oak_woodland`).
//! - `--size <m>`: width and depth of the grove, in metres (default 240).
//! - `--cache <dir>`: keep baked impostors in this folder. Later runs read
//!   them back instead of baking, so preparing is quicker.
//! - `--headless --screenshot <file.png>`: render without a window and save
//!   one picture once the grove is planted.
//!
//! The times taken to prepare and to plant are printed.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::app::ScheduleRunnerPlugin;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use procgena_impostor::{ImpostorPlugin, ImpostorSettings};
use procgena_tree::TreeScatterPreset;
use procgena_tree_bevy::{
    PrepareTreeScatter, TreePlugin, TreeScatterFinished, TreeScatterParams, TreeScatterPreparing,
    TreeScatterReady,
};

/// Frames to wait after the grove is planted before the headless picture.
const SETTLE_FRAMES: u32 = 30;
/// Size of the headless picture, in pixels.
const PICTURE: UVec2 = UVec2::new(1280, 720);

/// What the example does, from the command line.
#[derive(Resource)]
struct Options {
    preset: &'static TreeScatterPreset,
    size: f32,
    screenshot: Option<String>,
}

/// Where the run has got to.
#[derive(Resource)]
struct Progress {
    started: Instant,
    /// When the grove was prepared and planting began.
    prepared: Option<Instant>,
    /// Frames since the grove was planted.
    planted: Option<u32>,
}

/// The loading screen's text.
#[derive(Component)]
struct LoadingText;

/// The image the camera draws into when headless.
#[derive(Resource)]
struct Offscreen(Handle<Image>);

fn main() -> AppExit {
    // Read the command line.
    let mut options = Options {
        preset: TreeScatterPreset::oak_woodland(),
        size: 240.0,
        screenshot: None,
    };
    let mut headless = false;
    let mut cache: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().expect("a value after the option");
        match arg.as_str() {
            "--preset" => {
                options.preset =
                    TreeScatterPreset::named(&value()).expect("a tree Scatter preset key");
            }
            "--size" => options.size = value().parse().expect("--size needs metres"),
            "--cache" => cache = Some(PathBuf::from(value())),
            "--headless" => headless = true,
            "--screenshot" => options.screenshot = Some(value()),
            other => panic!("unknown argument {other}"),
        }
    }
    assert!(
        !headless || options.screenshot.is_some(),
        "--headless needs --screenshot <file.png>"
    );

    // With `--headless`, run without a window.
    let defaults = DefaultPlugins.set(AssetPlugin {
        file_path: "../../assets".to_owned(),
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
            ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(1.0 / 60.0)),
        ));
    } else {
        app.add_plugins(defaults);
    }

    // Impostors are baked by the impostor plugin; add it first to choose
    // where they are kept.
    app.add_plugins((
        ImpostorPlugin {
            settings: ImpostorSettings {
                cache_dir: cache,
                ..default()
            },
        },
        TreePlugin {
            texture_root: Some("textures".to_owned()),
            ..default()
        },
    ))
    .insert_resource(options)
    .insert_resource(Progress {
        started: Instant::now(),
        prepared: None,
        planted: None,
    })
    .add_systems(Startup, setup)
    .add_systems(Update, (show_progress, screenshot))
    .add_observer(plant)
    .add_observer(planted);
    if headless {
        app.add_systems(PreStartup, render_offscreen);
    }
    app.run()
}

/// Makes a picture to draw into instead of a window.
fn render_offscreen(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let image =
        Image::new_target_texture(PICTURE.x, PICTURE.y, TextureFormat::Rgba8UnormSrgb, None);
    commands.insert_resource(Offscreen(images.add(image)));
}

fn setup(
    mut commands: Commands,
    options: Res<Options>,
    offscreen: Option<Res<Offscreen>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // A camera at one edge of the grove, looking across it, so the far
    // trees are drawn as impostors.
    let half = options.size / 2.0;
    let mut camera = commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 30.0, half + 20.0)
            .looking_at(Vec3::new(0.0, 0.0, -half / 3.0), Vec3::Y),
    ));
    if let Some(offscreen) = offscreen {
        camera.insert(RenderTarget::Image(offscreen.0.clone().into()));
    }
    let camera = camera.id();
    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(40.0, 80.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 400.0,
        ..default()
    });
    commands.insert_resource(ClearColor(Color::srgb(0.62, 0.76, 0.9)));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(options.size, options.size))),
        MeshMaterial3d(materials.add(Color::srgb(0.32, 0.42, 0.22))),
    ));

    // The loading screen, and the grove being prepared behind it.
    commands.spawn((
        Text::new("Preparing the grove"),
        TextFont::from_font_size(32.0),
        Node {
            position_type: PositionType::Absolute,
            left: px(24),
            bottom: px(24),
            ..default()
        },
        UiTargetCamera(camera),
        LoadingText,
    ));
    commands.spawn(PrepareTreeScatter::preset(options.preset));
}

/// Shows how far preparing has got.
fn show_progress(
    preparing: Query<&TreeScatterPreparing, Changed<TreeScatterPreparing>>,
    mut text: Query<&mut Text, With<LoadingText>>,
) {
    for progress in &preparing {
        for mut text in &mut text {
            text.0 = format!(
                "Preparing the grove: {:.0}% ({} of {} steps)",
                progress.fraction() * 100.0,
                progress.done,
                progress.total
            );
        }
    }
}

/// Plants the grove once its trees are ready, and hides the loading screen.
fn plant(
    _: On<TreeScatterReady>,
    mut commands: Commands,
    options: Res<Options>,
    mut progress: ResMut<Progress>,
    text: Query<Entity, With<LoadingText>>,
) {
    let now = Instant::now();
    info!(
        "prepared in {:.1} s",
        (now - progress.started).as_secs_f32()
    );
    progress.prepared = Some(now);
    commands.spawn(TreeScatterParams::preset(
        options.preset,
        Vec2::splat(options.size),
    ));
    for entity in &text {
        commands.entity(entity).despawn();
    }
}

/// Reports how long planting took, and lets go of the prepared trees: the
/// planted grove holds the ones it uses from now on.
fn planted(
    event: On<TreeScatterFinished>,
    mut commands: Commands,
    mut progress: ResMut<Progress>,
    preparers: Query<Entity, With<PrepareTreeScatter>>,
) {
    if let Err(error) = &event.result {
        error!("grove failed: {error}");
    }
    if let Some(prepared) = progress.prepared {
        info!("planted in {:.2} s", prepared.elapsed().as_secs_f32());
    }
    progress.planted.get_or_insert(0);
    for entity in &preparers {
        commands.entity(entity).despawn();
    }
}

/// With `--screenshot`, saves a picture once the grove has settled and
/// exits.
fn screenshot(
    mut commands: Commands,
    options: Res<Options>,
    offscreen: Option<Res<Offscreen>>,
    mut progress: ResMut<Progress>,
) {
    let (Some(path), Some(frames)) = (&options.screenshot, progress.planted.as_mut()) else {
        return;
    };
    *frames += 1;
    if *frames != SETTLE_FRAMES {
        return;
    }
    let shot = match offscreen {
        Some(offscreen) => Screenshot::image(offscreen.0.clone()),
        None => Screenshot::primary_window(),
    };
    commands
        .spawn(shot)
        .observe(save_to_disk(path.clone()))
        .observe(
            |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                exit.write(AppExit::Success);
            },
        );
}
