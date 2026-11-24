//! Streams a world of terrain around a free camera.
//!
//! ```bash
//! cargo run -p procgena_terrain_bevy --example viewer
//! ```
//!
//! Move with the free camera: W, A, S and D to fly, E and Q for up and down,
//! and the mouse to look around while the right button is held (or after
//! pressing M). L tints chunks by level, B draws chunk borders.
//!
//! Options:
//!
//! - `--preset <key>`: the terrain every region gets (`plains`,
//!   `rolling_hills` or `mountains`); by default plains where the world
//!   stands low, hills higher up and mountains highest;
//! - `--seed <n>`: the world seed (default 1);
//! - `--eye x y z` and `--target x y z`: where the camera starts and what it
//!   looks at, with each `y` in metres above the ground there;
//! - `--view <metres>`: the view distance (default 4000);
//! - `--rough-from <level>`: draw chunk levels from this one up at the
//!   rough tier only, without roads;
//! - `--levels`, `--borders`: start with chunk levels tinted, or chunk
//!   borders drawn;
//! - `--screenshot <file.png>`: save an image once every chunk is drawn,
//!   then exit;
//! - `--headless`: with `--screenshot`, render a 1280 × 720 image without
//!   opening a window.

use std::sync::Arc;
use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::camera::RenderTarget;
use bevy::camera_controller::free_camera::{FreeCamera, FreeCameraPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use glam::DVec2;
use procgena_terrain::{
    Elevation, RegionId, TerrainPreset, WorldTerrain, WorldTerrainOptions, chunk_lod,
};
use procgena_terrain_bevy::{
    TerrainDebug, TerrainPlugin, TerrainProgress, TerrainStream, TerrainViewer, TerrainWater,
};

/// Frames to wait after every chunk is drawn before a screenshot, so
/// shaders are ready.
const SETTLE_FRAMES: u32 = 30;

/// Seconds to wait for the terrain before a screenshot gives up.
const SCREENSHOT_TIMEOUT: f32 = 900.0;

/// Where to save a screenshot, from `--screenshot <file.png>`.
#[derive(Resource)]
struct ScreenshotPath(Option<String>);

/// The image the camera draws into with `--headless`.
#[derive(Resource)]
struct Offscreen(Handle<Image>);

/// The world and where the camera starts.
#[derive(Resource)]
struct Scene {
    /// The world, until the terrain is spawned from it.
    world: Option<WorldTerrain>,
    eye: Vec3,
    target: Vec3,
    rough_from: Option<u8>,
    headless: bool,
}

/// The water material given to every chunk's water.
#[derive(Resource)]
struct WaterMaterial(Handle<StandardMaterial>);

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let vector = |flag: &str| -> Option<Vec3> {
        let i = args.iter().position(|a| a == flag)?;
        let n: Vec<f32> = args[i + 1..i + 4]
            .iter()
            .map(|v| v.parse().expect("three numbers"))
            .collect();
        Some(Vec3::new(n[0], n[1], n[2]))
    };
    let has = |flag: &str| args.iter().any(|a| a == flag);

    // The world: one preset everywhere, or a mix by elevation.
    let preset = value("--preset").map(|key| {
        TerrainPreset::named(&key)
            .unwrap_or_else(|| panic!("no terrain preset called {key:?}"))
            .terrain
            .clone()
    });
    let source = move |_: RegionId, site: DVec2, elevation: &Elevation| {
        if let Some(options) = &preset {
            return options.clone();
        }
        match elevation.height(site) {
            h if h < 0.0 => TerrainPreset::plains().terrain.clone(),
            h if h < 150.0 => TerrainPreset::rolling_hills().terrain.clone(),
            _ => TerrainPreset::mountains().terrain.clone(),
        }
    };
    let options = WorldTerrainOptions {
        seed: value("--seed").map_or(1, |s| s.parse().expect("--seed needs a number")),
        ..WorldTerrainOptions::default()
    };
    let world = WorldTerrain::new(options, Arc::new(source)).expect("a valid world");

    // The camera's eye and target, standing on the ground.
    let on_ground = |p: Vec3| {
        let ground = world.height(DVec2::new(f64::from(p.x), f64::from(p.z)));
        Vec3::new(
            p.x,
            p.y + if ground.is_finite() { ground } else { 0.0 },
            p.z,
        )
    };
    let eye = on_ground(vector("--eye").unwrap_or(Vec3::new(0.0, 120.0, 0.0)));
    let target = on_ground(vector("--target").unwrap_or(Vec3::new(900.0, 0.0, -300.0)));

    let view: f32 = value("--view").map_or(4000.0, |v| v.parse().expect("--view needs a number"));
    let plugin = TerrainPlugin::new(chunk_lod(view)).expect("--view needs a positive distance");

    // With `--headless`, run without a window on a fixed 60 Hz loop.
    let headless = has("--headless");
    assert!(
        !headless || has("--screenshot"),
        "--headless needs --screenshot <file.png>"
    );
    let mut app = App::new();
    if headless {
        app.add_plugins((
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .disable::<WinitPlugin>(),
            ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(1.0 / 60.0)),
        ));
    } else {
        app.add_plugins((DefaultPlugins, FreeCameraPlugin));
    }
    app.add_plugins(plugin)
        .insert_resource(ClearColor(Color::srgb(0.62, 0.72, 0.8)))
        .insert_resource(TerrainDebug {
            levels: has("--levels"),
            borders: has("--borders"),
        })
        .insert_resource(ScreenshotPath(value("--screenshot")))
        .insert_resource(Scene {
            world: Some(world),
            eye,
            target,
            rough_from: value("--rough-from")
                .map(|v| v.parse().expect("--rough-from needs a level")),
            headless,
        })
        .add_systems(Startup, setup)
        .add_systems(Update, (water, toggle_debug, screenshot))
        .run();
}

/// Spawns the terrain, the camera and the light.
fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut scene: ResMut<Scene>,
) {
    let world = scene.world.take().expect("set up once");
    commands.spawn(TerrainStream {
        rough_from: scene.rough_from,
        ..TerrainStream::world(world)
    });

    let mut camera = commands.spawn((
        Camera3d::default(),
        TerrainViewer,
        FreeCamera {
            walk_speed: 60.0,
            run_speed: 400.0,
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            far: 20_000.0,
            ..default()
        }),
        DistanceFog {
            color: Color::srgb(0.62, 0.72, 0.8),
            falloff: FogFalloff::from_visibility(9000.0),
            ..default()
        },
        Transform::from_translation(scene.eye).looking_at(scene.target, Vec3::Y),
    ));
    if scene.headless {
        let image = Image::new_target_texture(1280, 720, TextureFormat::Rgba8UnormSrgb, None);
        let handle = images.add(image);
        camera.insert(RenderTarget::Image(handle.clone().into()));
        commands.insert_resource(Offscreen(handle));
    }

    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            illuminance: 9000.0,
            ..default()
        },
        Transform::from_xyz(0.3, 1.0, 0.5).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 600.0,
        ..default()
    });
    commands.insert_resource(WaterMaterial(materials.add(StandardMaterial {
        base_color: Color::srgba(0.12, 0.3, 0.42, 0.8),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.1,
        ..default()
    })));
}

/// Gives every chunk's water this example's water material.
fn water(
    mut commands: Commands,
    material: Res<WaterMaterial>,
    added: Query<Entity, Added<TerrainWater>>,
) {
    for entity in &added {
        commands
            .entity(entity)
            .insert(MeshMaterial3d(material.0.clone()));
    }
}

/// L toggles the level tint, B the chunk borders.
fn toggle_debug(keys: Option<Res<ButtonInput<KeyCode>>>, mut debug: ResMut<TerrainDebug>) {
    let Some(keys) = keys else { return };
    if keys.just_pressed(KeyCode::KeyL) {
        debug.levels = !debug.levels;
    }
    if keys.just_pressed(KeyCode::KeyB) {
        debug.borders = !debug.borders;
    }
}

/// With `--screenshot`, saves one image once every chunk is drawn, then
/// exits. Exits with an error if the terrain isn't drawn within
/// [`SCREENSHOT_TIMEOUT`].
fn screenshot(
    mut commands: Commands,
    path: Res<ScreenshotPath>,
    offscreen: Option<Res<Offscreen>>,
    time: Res<Time>,
    progress: Query<&TerrainProgress>,
    mut settled: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = &path.0 else { return };
    if !progress.iter().all(|p| p.settled && p.drawn > 0) || progress.is_empty() {
        *settled = 0;
        if time.elapsed_secs() > SCREENSHOT_TIMEOUT {
            error!("screenshot failed: the terrain wasn't drawn in {SCREENSHOT_TIMEOUT} seconds");
            exit.write(AppExit::error());
        }
        return;
    }
    *settled += 1;
    if *settled != SETTLE_FRAMES {
        return;
    }
    let target = match offscreen {
        Some(offscreen) => Screenshot::image(offscreen.0.clone()),
        None => Screenshot::primary_window(),
    };
    commands
        .spawn(target)
        .observe(save_to_disk(path.clone()))
        .observe(
            |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                exit.write(AppExit::Success);
            },
        );
}
