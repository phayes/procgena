//! Shows every mushroom preset in a row, two clusters, and porcini and fairy
//! rings scattered over a patch of ground.
//!
//! ```bash
//! cargo run -p procgena_mushroom_bevy --example viewer
//! ```
//!
//! Move with the free camera: W, A, S and D to fly, E and Q for up and down,
//! and the mouse to look around while the right button is held (or after
//! pressing M). Move away from the mushrooms to watch them switch to simpler
//! detail levels.
//!
//! Add `--screenshot <file.png>` to save an image once everything has been
//! generated, then exit. Add `--flat` to draw everything with open caps and
//! camera-facing stem strips at every distance, `--billboard` to draw
//! everything at the cheapest detail (one camera-facing triangle each for
//! stem and cap),
//! `--closeup` to start next to the enoki cluster instead of the preset row,
//! `--preset <key>` (such as `porcini` or `fly_agaric`) to start next to one
//! preset, and `--orbit <degrees>` to start the camera turned around what it
//! looks at. With `--screenshot`, add `--headless` to render a 1280 × 720
//! image without opening a window.

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
use procgena_core::scatter::ScatterArea;
use procgena_core::{Ground, HeightGrid, Lod, LodLevel};
use procgena_core_bevy::instanced::InstancedScatter;
use procgena_mushroom::{
    DEFAULT_LOD_SIZE, MushroomDetail, MushroomPreset, ScatterOptions, default_lod,
};
use procgena_mushroom_bevy::{
    MushroomGenerating, MushroomGroundScatterParams, MushroomParams, MushroomPart, MushroomPlugin,
};

/// Frames to wait after generation finishes before a screenshot, so meshes
/// and shaders are ready.
const SETTLE_FRAMES: u32 = 60;

/// Seconds to wait for every mushroom before a screenshot gives up.
const SCREENSHOT_TIMEOUT: f32 = 180.0;

/// Where to save a screenshot, from `--screenshot <file.png>`.
#[derive(Resource)]
struct ScreenshotPath(Option<String>);

/// The image the camera draws into with `--headless`.
#[derive(Resource)]
struct Offscreen(Handle<Image>);

/// Where the enoki cluster stands.
const ENOKI_CLUSTER: Vec3 = Vec3::new(-0.4, 0.0, -0.4);

/// Where the camera starts, from the command line.
#[derive(Resource)]
struct CameraStart {
    /// Angle around the target in degrees, from `--orbit`.
    orbit: f32,
    /// True to look at the enoki cluster rather than the preset row, from
    /// `--closeup`.
    closeup: bool,
    /// Index in [`MushroomPreset::all`] of one preset to look at closely,
    /// from `--preset <key>`.
    preset: Option<usize>,
    /// True to draw into an image instead of a window, from `--headless`.
    headless: bool,
}

/// Spacing between the presets in the row, in metres.
const ROW_SPACING: f32 = 0.2;

/// Returns the row position of preset number `i`.
fn row_x(i: usize) -> f32 {
    (i as f32 - (MushroomPreset::all().count() as f32 - 1.0) * 0.5) * ROW_SPACING
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let orbit = value("--orbit").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    // `--preset fly_agaric` picks the preset with that key.
    let preset = value("--preset").map(|key| {
        MushroomPreset::all()
            .position(|p| p.key == key)
            .unwrap_or_else(|| panic!("no preset with key {key:?}"))
    });

    // `--flat` and `--billboard` use only that one detail, drawn out to
    // 100 m for an 8 cm cap.
    let only =
        |detail| Lod::new(DEFAULT_LOD_SIZE, vec![LodLevel::new(0.0, 0.1, detail)]).with_cull(100.0);
    let lod = if args.iter().any(|a| a == "--flat") {
        only(MushroomDetail::FLAT)
    } else if args.iter().any(|a| a == "--billboard") {
        only(MushroomDetail::BILLBOARD)
    } else {
        default_lod()
    };
    // Read assets from the repository's `mushroom/assets`. The fly agaric's
    // cap texture is used when its files have been converted or downloaded
    // there; otherwise every mushroom uses vertex colours.
    let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
    let textured = std::path::Path::new(assets)
        .join("textures/cap/mushroom_top_001-1k-color-bc7.ktx2")
        .exists();
    if !textured {
        info!("no cap textures found; run `uv run --project tools textures download mushroom`");
    }
    let plugin = MushroomPlugin {
        lod,
        texture_root: textured.then(|| "textures".to_owned()),
        ..MushroomPlugin::default()
    };
    // With `--headless`, run without a window on a fixed 60 Hz loop.
    let headless = args.iter().any(|a| a == "--headless");
    assert!(
        !headless || value("--screenshot").is_some(),
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
        .insert_resource(CameraStart {
            orbit,
            closeup: args.iter().any(|a| a == "--closeup"),
            preset,
            headless,
        })
        .add_systems(Startup, setup)
        .add_systems(Update, screenshot)
        .run();
}

/// Entities that draw mushrooms: a mushroom's or cluster's parts, or a
/// scatter.
type Drawn = Or<(With<MushroomPart>, With<InstancedScatter>)>;

/// With `--screenshot`, saves one image once every mushroom has been built
/// and drawn for a moment, then exits. Exits with an error if nothing is
/// built within [`SCREENSHOT_TIMEOUT`].
#[expect(clippy::too_many_arguments, reason = "one Bevy system")]
fn screenshot(
    mut commands: Commands,
    path: Res<ScreenshotPath>,
    offscreen: Option<Res<Offscreen>>,
    time: Res<Time>,
    generating: Query<(), With<MushroomGenerating>>,
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

/// Spawns the camera, light, mushrooms, clusters and the scattered ground
/// patch.
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    start: Res<CameraStart>,
) {
    // Camera looking at the preset row or the enoki cluster, turned around
    // it by `--orbit`, and light.
    let (target, offset) = if let Some(i) = start.preset {
        (Vec3::new(row_x(i), 0.06, 0.0), Vec3::new(0.0, 0.03, 0.28))
    } else if start.closeup {
        (ENOKI_CLUSTER + Vec3::Y * 0.05, Vec3::new(0.0, 0.07, 0.3))
    } else {
        (Vec3::new(0.0, 0.05, 0.0), Vec3::new(0.0, 0.45, 1.8))
    };
    let eye = target + Quat::from_rotation_y(start.orbit.to_radians()) * offset;
    let mut camera = commands.spawn((
        Camera3d::default(),
        FreeCamera::default(),
        Transform::from_translation(eye).looking_at(target, Vec3::Y),
    ));
    if start.headless {
        let image = Image::new_target_texture(1280, 720, TextureFormat::Rgba8UnormSrgb, None);
        let handle = images.add(image);
        camera.insert(RenderTarget::Image(handle.clone().into()));
        commands.insert_resource(Offscreen(handle));
    }
    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(2.0, 4.0, 1.5).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Every preset in a row, each as a single mushroom.
    for (i, preset) in MushroomPreset::all().enumerate() {
        let mut options = preset.mushroom.clone();
        options.cluster.stems = 1;
        commands.spawn((
            MushroomParams::new(options),
            Transform::from_xyz(row_x(i), 0.0, 0.0),
        ));
    }

    // The enoki preset's cluster, whose long stalks sweep outward from a
    // tight base, and a tuft of buttons.
    commands.spawn((
        MushroomParams::preset(MushroomPreset::enoki()),
        Transform::from_translation(ENOKI_CLUSTER),
    ));
    let mut buttons = MushroomPreset::button().mushroom.clone();
    buttons.cluster.stems = 5;
    commands.spawn((
        MushroomParams::new(buttons),
        Transform::from_xyz(0.4, 0.0, -0.4),
    ));

    // A patch of gently rolling ground that mushrooms are scattered over,
    // sampled once into a height grid.
    let centre = DVec2::new(0.0, f64::from(GROUND_Z));
    let area = ScatterArea::new(centre, Vec2::splat(GROUND_SIZE));
    let half = DVec2::splat(f64::from(GROUND_SIZE) * 0.5);
    let ground: Arc<dyn Ground> = Arc::new(HeightGrid::from_fn(
        centre - half,
        centre + half,
        f64::from(GROUND_CELL),
        rolling,
    ));
    commands.spawn((
        Mesh3d(meshes.add(ground_mesh(&*ground, centre))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.28, 0.22, 0.14),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.001, GROUND_Z),
    ));
    commands.spawn((
        MushroomGroundScatterParams {
            mushroom: Arc::new(MushroomPreset::porcini().mushroom.clone()),
            scatter: ScatterOptions {
                density: 6.0,
                ..ScatterOptions::default()
            },
            ground: ground.clone(),
            area,
            mask: None,
        },
        Transform::from_xyz(0.0, 0.0, GROUND_Z),
    ));
    // A couple of fairy rings on the same ground, each one whole copy whose
    // members follow the ground.
    commands.spawn((
        MushroomGroundScatterParams {
            mushroom: Arc::new(MushroomPreset::fairy_ring().mushroom.clone()),
            scatter: ScatterOptions {
                default_seed: 4,
                density: 0.06,
                min_spacing: 2.0,
                scale_min: 0.9,
                scale_max: 1.1,
                specimens: 2,
                ..ScatterOptions::default()
            },
            ground,
            area,
            mask: None,
        },
        Transform::from_xyz(0.0, 0.0, GROUND_Z),
    ));
}

/// Width and depth of the scattered ground patch, in metres.
const GROUND_SIZE: f32 = 6.0;
/// Spacing of the ground's height samples, in metres.
const GROUND_CELL: f32 = 0.25;
/// World Z of the ground patch's centre, behind the preset row.
const GROUND_Z: f32 = -4.0;

/// Gently rolling ground, a few centimetres high, in World metres.
fn rolling(p: DVec2) -> f32 {
    let (x, z) = (p.x as f32, p.y as f32);
    0.06 * (1.3 * x).sin() + 0.05 * (0.9 * z + 0.4 * x).cos() - 0.05
}

/// Builds a mesh of `ground` over the patch around `centre`, relative to
/// the centre, with one vertex per height sample.
fn ground_mesh(ground: &dyn Ground, centre: DVec2) -> Mesh {
    let cuts = (GROUND_SIZE / GROUND_CELL) as u32 - 1;
    let mut mesh = Plane3d::default()
        .mesh()
        .size(GROUND_SIZE, GROUND_SIZE)
        .subdivisions(cuts)
        .build();
    if let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    {
        for p in positions.iter_mut() {
            let at = centre + DVec2::new(f64::from(p[0]), f64::from(p[2]));
            p[1] = ground.height(at);
        }
    }
    mesh.compute_smooth_normals();
    mesh
}
