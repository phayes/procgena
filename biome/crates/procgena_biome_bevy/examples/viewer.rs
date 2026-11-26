//! Draws one Ecotope over a patch of rolling ground.
//!
//! ```bash
//! cargo run -p procgena_biome_bevy --example viewer
//! ```
//!
//! Move with the free camera: W, A, S and D to fly, E and Q for up and down,
//! and the mouse to look around while the right button is held (or after
//! pressing M).
//!
//! Options:
//!
//! - `--preset <name>`: the Ecotope preset to draw, `pine_forest_floor` by
//!   default;
//! - `--file <path.ecotope.yml>`: draw an Ecotope preset file instead;
//! - `--seed <n>`: place with this seed instead of the preset's own;
//! - `--size <metres>`: the side of the square patch, 40 by default;
//! - `--eye <x> <y> <z>` and `--target <x> <y> <z>`: where the camera stands
//!   and what it looks at;
//! - `--screenshot <file.png>`: save an image once everything has been
//!   generated, then exit;
//! - `--headless`: with `--screenshot`, render a 1280 × 720 image without
//!   opening a window.
//!
//! - `--textures`: draw with each asset's texture images, read from
//!   `tree/assets/textures`, `stone/assets/textures` and so on. Download
//!   them first with `uv run --project tools textures download <project>`
//!   for `tree`, `stone`, `deadwood` and `mushroom`: an asset whose images
//!   are missing is not drawn. Without it, trees and stones get plain
//!   colours. The ground is always plain grey.

use std::sync::Arc;
use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::camera::RenderTarget;
use bevy::camera_controller::free_camera::{FreeCamera, FreeCameraPlugin};
use bevy::math::DVec2;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use procgena_biome::{Ecotope, EcotopePreset, ScatterKind};
use procgena_biome_bevy::{
    EcotopeFinished, EcotopeParams, EcotopePlacing, EcotopePlugin, GroundPatch, rolling_hills,
};
use procgena_core::scatter::ScatterArea;
use procgena_core::{Ground, HeightGrid};
use procgena_deadwood_bevy::{DeadwoodGenerating, DeadwoodPart, DeadwoodPlugin};
use procgena_mushroom_bevy::{MushroomGenerating, MushroomPart, MushroomPlugin};
use procgena_plant_bevy::PlantPlugins;
use procgena_stone_bevy::{StoneGenerating, StonePart, StonePlugin};
use procgena_tree_bevy::{TreeGenerating, TreePart, TreePlugin};

/// Frames to wait after generation finishes before a screenshot, so meshes
/// and shaders are ready.
const SETTLE_FRAMES: u32 = 90;

/// Seconds to wait for every asset before a screenshot gives up.
const SCREENSHOT_TIMEOUT: f32 = 300.0;

/// Spacing of the ground's height samples, in metres.
const GROUND_CELL: f64 = 0.5;

/// Extra ground beyond the patch on each side, in metres, so copies at the
/// edge have ground under them.
const GROUND_MARGIN: f64 = 4.0;

/// Where to save a screenshot, from `--screenshot <file.png>`.
#[derive(Resource)]
struct ScreenshotPath(Option<String>);

/// The image the camera draws into with `--headless`.
#[derive(Resource)]
struct Offscreen(Handle<Image>);

/// What to draw and where the camera starts, from the command line.
#[derive(Resource)]
struct Scene {
    ecotope: Ecotope,
    seed: u32,
    size: f32,
    eye: Option<Vec3>,
    target: Option<Vec3>,
    headless: bool,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let vector = |flag: &str| {
        let i = args.iter().position(|a| a == flag)?;
        let v: Vec<f32> = args[i + 1..i + 4]
            .iter()
            .map(|a| {
                a.parse()
                    .unwrap_or_else(|_| panic!("{flag} needs three numbers"))
            })
            .collect();
        Some(Vec3::new(v[0], v[1], v[2]))
    };
    let has = |flag: &str| args.iter().any(|a| a == flag);

    // The Ecotope: a file, or a built-in preset.
    let preset = match value("--file") {
        Some(path) => {
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
            let name = std::path::Path::new(&path)
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".ecotope.yml"))
                .unwrap_or("file")
                .to_owned();
            EcotopePreset::from_yaml(&name, &text).unwrap_or_else(|e| panic!("{path}: {e}"))
        }
        None => {
            let name = value("--preset").unwrap_or_else(|| "pine_forest_floor".to_owned());
            EcotopePreset::named(&name)
                .unwrap_or_else(|| panic!("no Ecotope preset called {name:?}"))
                .clone()
        }
    };
    let seed = value("--seed")
        .map(|s| s.parse().expect("--seed needs a whole number"))
        .unwrap_or(preset.ecotope.default_seed);

    // With `--headless`, run without a window on a fixed 60 Hz loop.
    let headless = has("--headless");
    assert!(
        !headless || has("--screenshot"),
        "--headless needs --screenshot <file.png>"
    );
    // `--textures` reads each asset's images from its folder in the
    // repository; without it, assets get plain colours.
    let textured = has("--textures");
    let root = |project: &str| textured.then(|| format!("{project}/assets/textures"));
    let mut ecotope = preset.ecotope;
    if !textured {
        plain_colours(&mut ecotope);
    }
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
            ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(1.0 / 60.0)),
        ));
    } else {
        app.add_plugins((defaults, FreeCameraPlugin));
    }
    app.add_plugins((
        TreePlugin {
            texture_root: root("tree"),
            ..default()
        },
        StonePlugin {
            texture_root: root("stone"),
            ..default()
        },
        DeadwoodPlugin {
            texture_root: root("deadwood"),
            ..default()
        },
        MushroomPlugin {
            texture_root: root("mushroom"),
            ..default()
        },
        PlantPlugins,
        EcotopePlugin,
    ))
    .insert_resource(ClearColor(Color::srgb(0.62, 0.72, 0.8)))
    .insert_resource(ScreenshotPath(value("--screenshot")))
    .insert_resource(Scene {
        ecotope,
        seed,
        size: value("--size")
            .map(|s| s.parse().expect("--size needs a number"))
            .unwrap_or(40.0),
        eye: vector("--eye"),
        target: vector("--target"),
        headless,
    })
    .add_systems(Startup, setup)
    .add_systems(Update, screenshot)
    .run();
}

/// Gives trees and stones plain colours in place of the texture images
/// their tints multiply. Colours don't change where anything is placed.
fn plain_colours(ecotope: &mut Ecotope) {
    for scatter in ecotope.scatters.values_mut() {
        match &mut scatter.scatter {
            ScatterKind::Tree(trees) => {
                for tree in &mut trees.trees {
                    tree.tree.bark.tint = 0x5a4632;
                    tree.tree.leaves.tint = 0x3d5a2a;
                }
            }
            ScatterKind::Stone(stones) => {
                for class in &mut stones.classes {
                    class.stone.material.color = [0.12, 0.115, 0.1];
                }
            }
            ScatterKind::Deadwood(_) | ScatterKind::Mushroom { .. } | ScatterKind::Plant(_) => {}
        }
    }
}

/// Spawns the ground, the Ecotope to place, the camera and the light.
fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, scene: Res<Scene>) {
    // The ground, sampled once into a grid a little wider than the patch.
    let half = f64::from(scene.size) * 0.5 + GROUND_MARGIN;
    let ground: Arc<dyn Ground> = Arc::new(HeightGrid::from_fn(
        DVec2::splat(-half),
        DVec2::splat(half),
        GROUND_CELL,
        rolling_hills,
    ));

    // The Ecotope, placed in the background and drawn once placed, over a
    // grey patch of its ground.
    commands
        .spawn(EcotopeParams {
            ecotope: Arc::new(scene.ecotope.clone()),
            seed: scene.seed,
            ground,
            area: ScatterArea::new(DVec2::ZERO, Vec2::splat(scene.size)),
            coverage: None,
            patch: Some(GroundPatch {
                margin: GROUND_MARGIN,
                cell: GROUND_CELL,
            }),
        })
        .observe(log_placement);

    // The camera, drawing into an image when headless.
    let target = scene.target.unwrap_or(Vec3::new(0.0, 1.0, 0.0));
    let eye = scene
        .eye
        .unwrap_or(Vec3::new(0.0, scene.size * 0.32, scene.size * 0.62));
    let mut camera = commands.spawn((
        Camera3d::default(),
        Transform::from_translation(eye).looking_at(target, Vec3::Y),
    ));
    if scene.headless {
        let image = Image::new_target_texture(1280, 720, TextureFormat::Rgba8UnormSrgb, None);
        let handle = images.add(image);
        camera.insert(RenderTarget::Image(handle.clone().into()));
        commands.insert_resource(Offscreen(handle));
    } else {
        camera.insert(FreeCamera::default());
    }

    // Sun and sky.
    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(30.0, 50.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 600.0,
        ..default()
    });
}

/// Logs how long placing took and each Scatter's copies, or exits with an
/// error if the Ecotope could not be placed.
fn log_placement(finished: On<EcotopeFinished>, mut exit: MessageWriter<AppExit>) {
    match &finished.result {
        Ok(summary) => {
            info!("placed in {:?}", summary.place_time);
            for layer in &summary.layers {
                info!(
                    "  {} (priority {}): {} copies",
                    layer.key, layer.priority, layer.copies
                );
            }
        }
        Err(error) => {
            error!("the Ecotope could not be placed: {error}");
            exit.write(AppExit::error());
        }
    }
}

/// Entities whose asset is still being built, or Ecotopes being placed.
type Generating = Or<(
    With<EcotopePlacing>,
    With<TreeGenerating>,
    With<StoneGenerating>,
    With<DeadwoodGenerating>,
    With<MushroomGenerating>,
)>;

/// Entities that draw a part of a built asset.
type Drawn = Or<(
    With<TreePart>,
    With<StonePart>,
    With<DeadwoodPart>,
    With<MushroomPart>,
)>;

/// With `--screenshot`, saves one image once every asset has been built and
/// drawn for a moment, then exits. Exits with an error if that takes longer
/// than [`SCREENSHOT_TIMEOUT`].
#[expect(clippy::too_many_arguments, reason = "one Bevy system")]
fn screenshot(
    mut commands: Commands,
    path: Res<ScreenshotPath>,
    offscreen: Option<Res<Offscreen>>,
    time: Res<Time>,
    generating: Query<(), Generating>,
    parts: Query<(), Drawn>,
    mut settled: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = &path.0 else { return };
    if !generating.is_empty() || parts.is_empty() {
        *settled = 0;
        if time.elapsed_secs() > SCREENSHOT_TIMEOUT {
            error!("screenshot failed: still building after {SCREENSHOT_TIMEOUT} seconds");
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
