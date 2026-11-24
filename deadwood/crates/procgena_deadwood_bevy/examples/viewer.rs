//! Shows every dead wood preset laid out on the ground, with kindling and
//! twig litter scattered behind them.
//!
//! ```bash
//! cargo run -p procgena_deadwood_bevy --example viewer
//! ```
//!
//! Move with the free camera: W, A, S and D to fly, E and Q for up and down,
//! and the mouse to look around while the right button is held (or after
//! pressing M). Move away to watch pieces switch to simpler detail levels.
//!
//! Options:
//!
//! - `--preset <name>`: show only one preset, such as `hollow_log`, with
//!   the camera framed on it;
//! - `--scatter <kindling|twig_litter|fallen_logs>`: show only that
//!   scatter;
//! - `--closeup`: start close to what the camera looks at;
//! - `--distance <metres>`: start this far from what the camera looks at;
//! - `--orbit <degrees>`: start the camera turned around what it looks at;
//! - `--flat`: draw everything at the cheapest detail at every distance;
//! - `--no-textures`: use vertex colours instead of bark images;
//! - `--screenshot <file.png>`: save an image once everything has been
//!   generated, then exit;
//! - `--headless`: with `--screenshot`, render a 1280 × 720 image without
//!   opening a window.
//!
//! Bark and end-grain images are read from the repository's
//! `deadwood/assets/textures` when
//! `uv run --project tools textures download deadwood` has filled it.

use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::camera::RenderTarget;
use bevy::camera_controller::free_camera::{FreeCamera, FreeCameraPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use procgena_core_bevy::instanced::InstancedScatter;
use procgena_deadwood::{
    DEFAULT_LOD_SIZE, DeadwoodDetail, DeadwoodLod, DeadwoodPreset, LodLevel, PieceKind,
    ScatterOptions, default_lod,
};
use procgena_deadwood_bevy::{
    DeadwoodGenerating, DeadwoodParams, DeadwoodPart, DeadwoodPlugin, DeadwoodScatterParams,
    ScatterGround,
};

/// Frames to wait after generation finishes before a screenshot, so meshes,
/// textures and shaders are ready.
const SETTLE_FRAMES: u32 = 90;

/// Gap between pieces in the preset layout, in metres.
const SPACING: f32 = 1.2;

/// Seconds to wait for every piece before a screenshot gives up.
const SCREENSHOT_TIMEOUT: f32 = 180.0;

/// Where to save a screenshot, from `--screenshot <file.png>`.
#[derive(Resource)]
struct ScreenshotPath(Option<String>);

/// The image the camera draws into with `--headless`.
#[derive(Resource)]
struct Offscreen(Handle<Image>);

/// What to show and where the camera starts, from the command line.
#[derive(Resource)]
struct Scene {
    /// The one preset to show, from `--preset`, or `None` for all.
    preset: Option<&'static DeadwoodPreset>,
    /// Angle around the target in degrees, from `--orbit`.
    orbit: f32,
    /// The one scatter to show, from `--scatter`, and the width of its
    /// square of ground in metres, or `None`.
    scatter: Option<(ScatterOptions, f32)>,
    /// True to start close to the target, from `--closeup`.
    closeup: bool,
    /// Camera distance from the target, from `--distance`, replacing the
    /// layout's own.
    distance: Option<f32>,
    /// True to draw into an image instead of a window, from `--headless`.
    headless: bool,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let preset = value("--preset").map(|name| {
        DeadwoodPreset::named(&name).unwrap_or_else(|| panic!("no preset called {name:?}"))
    });

    // `--flat` uses only the cheapest detail, drawn out to 200 m.
    let lod = if has("--flat") {
        DeadwoodLod::new(
            DEFAULT_LOD_SIZE,
            vec![LodLevel::new(0.0, 0.1, DeadwoodDetail::LOW)],
        )
        .with_cull(200.0)
    } else {
        default_lod()
    };

    // Read bark and end-grain images from the repository's dead wood
    // assets when they are there; otherwise every piece uses vertex colours.
    let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../deadwood/assets");
    let textured = !has("--no-textures")
        && [
            "textures/bark/bark_brown_02-1k-color-bc7.ktx2",
            "textures/end/TreeEnd003-1k-color-bc7.ktx2",
            "textures/end/TreeEnd005-1k-color-bc7.ktx2",
        ]
        .iter()
        .all(|file| std::path::Path::new(assets).join(file).exists());
    if !textured {
        info!("no textures; run `uv run --project tools textures download deadwood`");
    }
    let plugin = DeadwoodPlugin {
        lod,
        texture_root: textured.then(|| "textures".to_owned()),
        ..DeadwoodPlugin::default()
    };
    // With `--headless`, run without a window on a fixed 60 Hz loop.
    let headless = has("--headless");
    assert!(
        !headless || has("--screenshot"),
        "--headless needs --screenshot <file.png>"
    );
    let defaults = DefaultPlugins.set(AssetPlugin {
        file_path: assets.to_owned(),
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
        app.add_plugins((defaults, FreeCameraPlugin));
    }
    app.add_plugins(plugin)
        .insert_resource(ClearColor(Color::srgb(0.62, 0.72, 0.8)))
        .insert_resource(ScreenshotPath(value("--screenshot")))
        .insert_resource(Scene {
            preset,
            orbit: value("--orbit").and_then(|v| v.parse().ok()).unwrap_or(0.0),
            scatter: value("--scatter").map(|name| match name.as_str() {
                "kindling" => (ScatterOptions::kindling(), 6.0),
                "twig_litter" => (ScatterOptions::twig_litter(), 4.0),
                "fallen_logs" => (ScatterOptions::fallen_logs(), 20.0),
                _ => panic!("no scatter called {name:?}"),
            }),
            closeup: has("--closeup"),
            distance: value("--distance").map(|v| v.parse().expect("--distance needs a number")),
            headless,
        })
        .add_systems(Startup, setup)
        .add_systems(Update, screenshot)
        .run();
}

/// Entities that draw dead wood: a single piece's parts, or a scatter.
type Drawn = Or<(With<DeadwoodPart>, With<InstancedScatter>)>;

/// With `--screenshot`, saves one image once every piece has been built
/// and drawn for a moment, then exits. Exits with an error if nothing is
/// built within [`SCREENSHOT_TIMEOUT`].
#[expect(clippy::too_many_arguments, reason = "one Bevy system")]
fn screenshot(
    mut commands: Commands,
    path: Res<ScreenshotPath>,
    offscreen: Option<Res<Offscreen>>,
    time: Res<Time>,
    generating: Query<(), With<DeadwoodGenerating>>,
    parts: Query<(), Drawn>,
    mut settled: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = &path.0 else { return };
    if !generating.is_empty() || parts.is_empty() {
        *settled = 0;
        if time.elapsed_secs() > SCREENSHOT_TIMEOUT {
            error!("screenshot failed: nothing built after {SCREENSHOT_TIMEOUT} seconds");
            exit.write(AppExit::error());
        }
        return;
    }
    *settled += 1;
    if *settled != SETTLE_FRAMES {
        return;
    }

    // Save the image, then exit once it is written.
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

/// Spawns the camera, light, ground, pieces and scatters.
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    scene: Res<Scene>,
) {
    // The pieces: one preset at the origin, or every preset in rows of
    // logs, branches and stumps.
    // Each layout gives the point to look at, the direction from it to the
    // camera, and the camera's distance.
    let (target, direction, distance) = if let Some((scatter, size)) = &scene.scatter {
        // One scatter around the origin.
        commands.spawn(DeadwoodScatterParams {
            scatter: scatter.clone(),
            size: Vec2::splat(*size),
            ground: ScatterGround::Flat,
        });
        let distance = if scene.closeup { 1.0 } else { *size };
        (Vec3::ZERO, Vec3::new(0.0, 0.45, 1.0), distance)
    } else if let Some(preset) = scene.preset {
        commands.spawn(DeadwoodParams::preset(preset));
        let o = &preset.deadwood;
        let (d, l) = (o.diameter, o.length);
        match (o.kind == PieceKind::Stump, scene.closeup) {
            // Stumps: from above, to see the top.
            (true, closeup) => (
                Vec3::Y * l * 0.8,
                Vec3::new(0.0, 1.0, 0.8),
                d.max(l) * if closeup { 1.6 } else { 3.0 },
            ),
            // Logs and branches lie along X; up close, look at the +X end.
            (false, true) => (
                Vec3::new(l * 0.5, d * 0.5, 0.0),
                Vec3::new(1.0, 0.6, 0.5),
                (d * 3.0).max(0.3),
            ),
            (false, false) => (Vec3::Y * d * 0.5, Vec3::new(0.0, 0.45, 1.0), l * 1.2),
        }
    } else {
        // Logs and branches lie roughly along X, so they go in columns, one
        // behind the other; stumps go in a row in front.
        for (kind, start, step) in [
            (
                PieceKind::Log,
                Vec3::new(-2.5, 0.0, 0.0),
                Vec3::Z * -SPACING,
            ),
            (PieceKind::Branch, Vec3::new(2.5, 0.0, 0.0), Vec3::Z * -1.8),
            (
                PieceKind::Stump,
                Vec3::new(-1.8, 0.0, 2.0),
                Vec3::X * SPACING,
            ),
        ] {
            let presets = DeadwoodPreset::all().filter(|p| p.deadwood.kind == kind);
            for (i, preset) in presets.enumerate() {
                commands.spawn((
                    DeadwoodParams::preset(preset),
                    Transform::from_translation(start + step * i as f32),
                ));
            }
        }

        // Kindling and twig litter behind the pieces.
        commands.spawn((
            DeadwoodScatterParams {
                scatter: ScatterOptions::kindling(),
                size: Vec2::splat(6.0),
                ground: ScatterGround::Flat,
            },
            Transform::from_xyz(-3.0, 0.0, -11.0),
        ));
        commands.spawn((
            DeadwoodScatterParams {
                scatter: ScatterOptions::twig_litter(),
                size: Vec2::splat(4.0),
                ground: ScatterGround::Flat,
            },
            Transform::from_xyz(3.0, 0.0, -11.0),
        ));
        let distance = if scene.closeup { 5.0 } else { 12.0 };
        (
            Vec3::new(0.0, 0.2, -2.5),
            Vec3::new(0.0, 0.45, 1.0),
            distance,
        )
    };

    // Camera looking at the pieces, turned around them by `--orbit`.
    let offset = direction.normalize() * scene.distance.unwrap_or(distance);
    let eye = target + Quat::from_rotation_y(scene.orbit.to_radians()) * offset;
    let mut camera = commands.spawn((
        Camera3d::default(),
        FreeCamera::default(),
        Transform::from_translation(eye).looking_at(target, Vec3::Y),
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
            illuminance: 2500.0,
            ..default()
        },
        Transform::from_xyz(3.0, 5.0, 2.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 150.0,
        ..default()
    });

    // A forest-floor brown ground plane, just below the pieces.
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(60.0, 60.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.24, 0.2, 0.14),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.002, -8.0),
    ));
}
