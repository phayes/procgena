//! Explore rock shapes, texture sets and distance-based mesh detail.
//!
//! Run `cargo run -p procgena_stone_bevy --example viewer` from the repository.
//! The viewer uses the repository's `assets/textures` folder with BC7 textures;
//! download them first with `uv run --project tools textures download stone`.
//!
//! Controls:
//! - Left / Right: choose the previous / next seed.
//! - Tab: switch between marching cubes and dual contouring.
//! - T: show the next texture set; Shift+T shows the previous one.
//! - [ / ]: make the rock smaller / larger. Detail levels switch at distances
//!   proportional to the rock's size.
//! - F: switch between one rock and a 9 × 9 field to inspect detail transitions.
//! - P: switch to a GPU stone pool: 64 shapes generated on the GPU, drawn as
//!   10 000 rocks on a 100 × 100 grid. P again returns to the CPU rocks.
//! - G: toggle burial for CPU rocks using each rock's suggested embed depth.
//!   GPU pool rocks are unaffected.
//! - C: switch to a stone scatter: the boulder field preset spread over 60 m
//!   of rolling ground, tilted to the slope and partly buried, drawn from
//!   one GPU stone pool per class. Left / Right change its seed. C again
//!   returns to the CPU rocks.
//! - Hold the right mouse button to look around (M keeps the cursor captured).
//!   Use WASD to fly, E / Q to rise / sink, and hold left Shift to fly faster.
//!
//! Pass options after `--`, for example:
//! `cargo run -p procgena_stone_bevy --example viewer -- --seed 3 --texture lichen_rock_02 --field`.
//! Use `--dc` to start with dual contouring, `--size <metres>` to set the
//! rock size, and `--untextured` to draw material colours only. Use `--pool`
//! to start with the stone pool, `--scatter` to start with the stone scatter,
//! and `--twin` to show a CPU rock (left) next to
//! the same shape drawn from a stone pool (right). Use `--embed` to start with
//! CPU rocks buried to their suggested depth (the same setting as G). Use
//! `--prepass` to add depth, normal and motion-vector prepasses, or
//! `--deferred` for deferred rendering. Set camera positions with
//! `--eye x y z` and `--target x y z`. Use `--resolution <width> <height>` to set
//! the window or screenshot size in pixels (default: 1280 × 720).
//! Use `--screenshot <file.png>` to save a PNG image and exit. The viewer waits
//! for CPU rocks and textures to load, then allows a few more frames before
//! capturing the image. Pool and scatter rocks get an additional six-second wait for the
//! graphics processor (GPU) to generate their shapes; completion is not checked.
//! The run reports an error if a texture fails to load, saving fails, or the
//! scene remains unready for two minutes after CPU rock generation finishes.
//! Add `--headless` to save the image without opening a window;
//! it requires `--screenshot`.

use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::camera::RenderTarget;
use bevy::camera_controller::free_camera::{FreeCamera, FreeCameraPlugin};
use bevy::core_pipeline::prepass::{
    DeferredPrepass, DepthPrepass, MotionVectorPrepass, NormalPrepass,
};
use bevy::pbr::DefaultOpaqueRendererMethod;
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use procgena_stone::{
    Ground, HeightGrid, Mesher, ScatterArea, ScatterInput, StoneOptions, StoneScatter,
    StoneScatterOptions, TEXTURE_SETS,
};
use procgena_stone_bevy::{
    PooledStone, StoneEmbedDepth, StoneGenerating, StoneMaterial, StoneParams, StonePart,
    StonePlugin, StonePool,
};

/// Number of rows and columns in the square rock field.
const FIELD_SIZE: i32 = 9;
/// Number of rows and columns of rocks in pool mode.
const POOL_GRID: i32 = 100;
/// Number of shapes in the viewer's stone pool.
const POOL_VARIANTS: u32 = 64;
/// Seconds to wait after pooled rocks are spawned before a screenshot, for GPU
/// generation, shaders and textures to finish. Not checked.
const POOL_SCREENSHOT_DELAY: f32 = 6.0;
/// Extra frames before capture to give newly loaded textures time to appear.
const SETTLE_FRAMES: u32 = 10;
/// Maximum wait in seconds for the scene to become ready for a screenshot.
/// The wait starts over whenever CPU rock generation is in progress.
const SCREENSHOT_TIMEOUT: f32 = 120.0;
/// Distance between neighbouring rocks in the field, as a multiple of the rock size.
const FIELD_SPACING: f32 = 2.5;
/// Factor applied to the rock size for each press of [ or ].
const SIZE_STEP: f32 = 1.25;
/// Width and depth in metres of the ground in scatter mode.
const SCATTER_SIZE: f32 = 60.0;
/// Metres between ground heights sampled in scatter mode.
const SCATTER_CELL: f32 = 0.5;
/// Linear grey used for rocks without textures.
const UNTEXTURED_COLOR: [f32; 3] = [0.35, 0.33, 0.3];

/// How the camera renders opaque surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderMode {
    /// Forward rendering without prepasses.
    Forward,
    /// Forward rendering with depth, normal and motion-vector prepasses.
    Prepass,
    /// Deferred rendering.
    Deferred,
}

#[derive(Resource)]
struct Viewer {
    seed: u32,
    mesher: Mesher,
    texture: usize,
    size: f32,
    field: bool,
    /// Draw a GPU stone pool instead of CPU rocks.
    pool: bool,
    /// Show a CPU rock next to the same shape from a stone pool.
    twin: bool,
    /// Draw a stone scatter over rolling ground instead of CPU rocks.
    scatter: bool,
    /// Apply suggested burial to CPU rocks; leave pool rocks at ground level.
    embed: bool,
    textured: bool,
    screenshot: Option<String>,
    /// Capture a screenshot without opening a window.
    headless: bool,
}

#[derive(Component)]
struct Shown;

/// Image that receives the camera's view when there is no window.
#[derive(Resource)]
struct Offscreen(Handle<Image>);

#[derive(Resource)]
struct CameraSetup {
    eye: Vec3,
    target: Vec3,
    mode: RenderMode,
    /// Width and height in pixels for the window or headless screenshot.
    resolution: UVec2,
}

fn main() -> AppExit {
    let mut viewer = Viewer {
        seed: StoneOptions::default().default_seed,
        mesher: Mesher::MarchingCubes,
        texture: TEXTURE_SETS
            .iter()
            .position(|&t| t == StoneOptions::default().material.texture)
            .unwrap(),
        size: StoneOptions::default().scale,
        field: false,
        pool: false,
        twin: false,
        scatter: false,
        embed: false,
        textured: true,
        screenshot: None,
        headless: false,
    };
    let mut camera = CameraSetup {
        eye: Vec3::new(2.5, 1.8, 4.0),
        target: Vec3::new(0.0, 0.3, 0.0),
        mode: RenderMode::Forward,
        resolution: UVec2::new(1280, 720),
    };

    // Apply command-line choices before creating the scene.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    let number = |args: &[String], i: usize| args[i].parse::<f32>().expect("a number");
    while i < args.len() {
        match args[i].as_str() {
            "--seed" => {
                viewer.seed = args[i + 1].parse().expect("a whole number");
                i += 1;
            }
            "--dc" => viewer.mesher = Mesher::DualContouring,
            "--texture" => {
                viewer.texture = TEXTURE_SETS
                    .iter()
                    .position(|&t| t == args[i + 1])
                    .unwrap_or_else(|| panic!("unknown texture set {}", args[i + 1]));
                i += 1;
            }
            "--size" => {
                viewer.size = number(&args, i + 1);
                i += 1;
            }
            "--field" => viewer.field = true,
            "--pool" => viewer.pool = true,
            "--twin" => viewer.twin = true,
            "--scatter" => viewer.scatter = true,
            "--embed" => viewer.embed = true,
            "--untextured" => viewer.textured = false,
            "--prepass" => camera.mode = RenderMode::Prepass,
            "--deferred" => camera.mode = RenderMode::Deferred,
            "--eye" => {
                camera.eye = Vec3::new(
                    number(&args, i + 1),
                    number(&args, i + 2),
                    number(&args, i + 3),
                );
                i += 3;
            }
            "--target" => {
                camera.target = Vec3::new(
                    number(&args, i + 1),
                    number(&args, i + 2),
                    number(&args, i + 3),
                );
                i += 3;
            }
            "--screenshot" => {
                viewer.screenshot = Some(args[i + 1].clone());
                i += 1;
            }
            "--headless" => viewer.headless = true,
            "--resolution" => {
                let n = |k: usize| args[i + k].parse::<u32>().expect("a whole number");
                camera.resolution = UVec2::new(n(1), n(2));
                i += 2;
            }
            other => panic!("unknown option {other}"),
        }
        i += 1;
    }

    assert!(
        !viewer.headless || viewer.screenshot.is_some(),
        "--headless requires --screenshot <file.png>"
    );

    // Without a window, use a timed loop to keep rendering until capture finishes.
    let defaults = DefaultPlugins
        .set(AssetPlugin {
            file_path: "../../assets".to_owned(),
            ..default()
        })
        .set(RenderPlugin {
            synchronous_pipeline_compilation: true,
            ..default()
        });
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
        app.add_plugins((
            defaults.set(WindowPlugin {
                primary_window: Some(Window {
                    resolution: camera.resolution.into(),
                    ..default()
                }),
                ..default()
            }),
            FreeCameraPlugin,
        ));
    }
    app.add_plugins(StonePlugin {
        texture_root: viewer.textured.then(|| "textures".to_owned()),
        ..default()
    });
    if camera.mode == RenderMode::Deferred {
        app.insert_resource(DefaultOpaqueRendererMethod::deferred());
    }
    app.insert_resource(viewer)
        .insert_resource(camera)
        .add_systems(Startup, setup)
        .add_systems(Update, (controls, show_rocks, sink_rocks, screenshot))
        .run()
}

fn setup(
    mut commands: Commands,
    camera: Res<CameraSetup>,
    viewer: Res<Viewer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    // Create an image to hold the camera's view for a headless screenshot.
    let offscreen = viewer.headless.then(|| {
        let image = Image::new_target_texture(
            camera.resolution.x,
            camera.resolution.y,
            TextureFormat::Rgba8UnormSrgb,
            None,
        );
        let handle = images.add(image);
        commands.insert_resource(Offscreen(handle.clone()));
        handle
    });

    let mut entity = commands.spawn((
        Camera3d::default(),
        Transform::from_translation(camera.eye).looking_at(camera.target, Vec3::Y),
        FreeCamera {
            walk_speed: 5.0,
            run_speed: 25.0,
            ..default()
        },
    ));
    if let Some(handle) = &offscreen {
        entity.insert(RenderTarget::Image(handle.clone().into()));
    }
    match camera.mode {
        RenderMode::Forward => {}
        RenderMode::Prepass => {
            entity.insert((DepthPrepass, NormalPrepass, MotionVectorPrepass));
        }
        RenderMode::Deferred => {
            entity.insert((
                DepthPrepass,
                MotionVectorPrepass,
                DeferredPrepass,
                Msaa::Off,
            ));
        }
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
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(400.0, 400.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.32, 0.42, 0.22))),
    ));
}

fn controls(keys: Res<ButtonInput<KeyCode>>, mut viewer: ResMut<Viewer>) {
    if keys.just_pressed(KeyCode::ArrowRight) {
        viewer.seed = viewer.seed.wrapping_add(1);
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        viewer.seed = viewer.seed.wrapping_sub(1);
    }
    if keys.just_pressed(KeyCode::Tab) {
        viewer.mesher = match viewer.mesher {
            Mesher::MarchingCubes => Mesher::DualContouring,
            Mesher::DualContouring => Mesher::MarchingCubes,
        };
    }
    if keys.just_pressed(KeyCode::KeyT) {
        let count = TEXTURE_SETS.len();
        let back = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        viewer.texture = if back {
            (viewer.texture + count - 1) % count
        } else {
            (viewer.texture + 1) % count
        };
    }
    if keys.just_pressed(KeyCode::BracketLeft) {
        viewer.size /= SIZE_STEP;
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        viewer.size *= SIZE_STEP;
    }
    if keys.just_pressed(KeyCode::KeyF) {
        viewer.field = !viewer.field;
    }
    if keys.just_pressed(KeyCode::KeyP) {
        viewer.pool = !viewer.pool;
    }
    if keys.just_pressed(KeyCode::KeyG) {
        viewer.embed = !viewer.embed;
    }
    if keys.just_pressed(KeyCode::KeyC) {
        viewer.scatter = !viewer.scatter;
    }
}

/// Applies burial once when a CPU rock receives its suggested depth, if enabled.
/// Viewer changes respawn rocks, so toggling burial does not accumulate offsets.
fn sink_rocks(
    viewer: Res<Viewer>,
    mut rocks: Query<(&StoneEmbedDepth, &mut Transform), Added<StoneEmbedDepth>>,
) {
    if !viewer.embed {
        return;
    }
    for (depth, mut transform) in &mut rocks {
        transform.translation.y -= depth.0;
    }
}

/// Rebuilds the displayed rocks when viewer controls change.
fn show_rocks(
    mut commands: Commands,
    viewer: Res<Viewer>,
    shown: Query<Entity, With<Shown>>,
    mut pools: ResMut<Assets<StonePool>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !viewer.is_changed() {
        return;
    }
    for entity in &shown {
        commands.entity(entity).despawn();
    }

    if viewer.scatter {
        show_scatter(
            &mut commands,
            &viewer,
            &mut pools,
            &mut meshes,
            &mut materials,
        );
        return;
    }

    let mut options = StoneOptions {
        default_seed: viewer.seed,
        scale: viewer.size,
        ..StoneOptions::default()
    };
    options.mesh.mesher = viewer.mesher;
    options.material.texture = TEXTURE_SETS[viewer.texture].to_owned();
    if !viewer.textured {
        options.material.color = UNTEXTURED_COLOR;
    }
    info!(
        "seed {}, {:?}, texture {}, size {:.2}",
        viewer.seed, viewer.mesher, options.material.texture, viewer.size
    );
    // Start at ground level; sink_rocks applies optional burial after CPU generation.
    // A CPU rock on the left and the same shape from a pool on the right.
    if viewer.twin {
        let pool = StonePool::new(options.clone(), 1);
        let offset = Vec3::X * viewer.size * 0.6;
        commands.spawn((
            StoneParams::new(pool.variant_options(0)),
            Transform::from_translation(-offset),
            Shown,
        ));
        commands.spawn((
            PooledStone {
                pool: pools.add(pool),
                variant: 0,
            },
            Transform::from_translation(offset),
            Shown,
        ));
        return;
    }

    // Pool mode: a grid of rocks with assorted shapes, turns and sizes.
    if viewer.pool {
        let mut pool = StonePool::new(options, POOL_VARIANTS);
        pool.stretch_jitter = 0.3;
        let pool = pools.add(pool);
        let half = POOL_GRID / 2;
        for x in -half..half {
            for z in -half..half {
                let n = ((x + half) * POOL_GRID + (z + half)) as u32;
                let hash = n.wrapping_mul(0x9e37_79b9).rotate_left(13);
                let size = 0.6 + (hash % 1000) as f32 / 1000.0 * 0.8;
                let position = Vec3::new(x as f32, 0.0, z as f32) * viewer.size * FIELD_SPACING;
                commands.spawn((
                    PooledStone {
                        pool: pool.clone(),
                        variant: hash % POOL_VARIANTS,
                    },
                    Transform::from_translation(position)
                        .with_rotation(Quat::from_rotation_y(hash as f32))
                        .with_scale(Vec3::splat(size)),
                    Shown,
                ));
            }
        }
        return;
    }

    // Spawn grounded rocks, varying seeds and turns across the field.
    let half = if viewer.field { FIELD_SIZE / 2 } else { 0 };
    for x in -half..=half {
        for z in -half..=half {
            let mut rock = options.clone();
            rock.default_seed = viewer
                .seed
                .wrapping_add(((x + half) * FIELD_SIZE + (z + half)) as u32);
            let position = Vec3::new(x as f32, 0.0, z as f32) * viewer.size * FIELD_SPACING;
            let turn = Quat::from_rotation_y((x * 7 + z * 13) as f32);
            commands.spawn((
                StoneParams::new(rock),
                Transform::from_translation(position).with_rotation(turn),
                Shown,
            ));
        }
    }
}

/// Spawns the boulder field preset over rolling ground, with one stone pool
/// per class.
fn show_scatter(
    commands: &mut Commands,
    viewer: &Viewer,
    pools: &mut Assets<StonePool>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    // Rolling ground with a steeper ridge, sampled once into a grid.
    let half = f64::from(SCATTER_SIZE) * 0.5;
    let hills = |p: bevy::math::DVec2| {
        let (x, z) = (p.x as f32, p.y as f32);
        0.1 + 2.5 * (1.0 + (0.25 * x).sin()) + 0.8 * (1.0 + (0.11 * z + 0.05 * x).cos())
    };
    let ground = HeightGrid::from_fn(
        bevy::math::DVec2::splat(-half),
        bevy::math::DVec2::splat(half),
        f64::from(SCATTER_CELL),
        hills,
    );

    // Scatter the stones over the whole grid.
    let options = StoneScatterOptions::boulder_field();
    let area = ScatterArea::new(bevy::math::DVec2::ZERO, Vec2::splat(SCATTER_SIZE));
    let input = ScatterInput::new(&ground, area, viewer.seed);
    let scatter = match StoneScatter::new(&options, &input) {
        Ok(scatter) => scatter,
        Err(error) => {
            error!("stone scatter failed: {error}");
            return;
        }
    };
    info!(
        "scatter seed {}: {} stones",
        viewer.seed,
        scatter.instances().len()
    );

    // Draw the ground as a mesh following the grid.
    let cuts = (SCATTER_SIZE / SCATTER_CELL) as u32 - 1;
    let mut mesh = Plane3d::default()
        .mesh()
        .size(SCATTER_SIZE, SCATTER_SIZE)
        .subdivisions(cuts)
        .build();
    if let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    {
        for p in positions.iter_mut() {
            let at = bevy::math::DVec2::new(f64::from(p[0]), f64::from(p[2]));
            p[1] = ground.height(at);
        }
    }
    mesh.compute_smooth_normals();
    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(Color::srgb(0.36, 0.44, 0.26))),
        Shown,
    ));

    // One pool per class, and one pooled stone per placed stone.
    let handles: Vec<Handle<StonePool>> = options
        .classes
        .iter()
        .map(|class| {
            let mut stone = class.stone.clone();
            if !viewer.textured {
                stone.material.color = UNTEXTURED_COLOR;
            }
            let mut pool = StonePool::new(stone, class.variants);
            pool.stretch_jitter = class.stretch_jitter;
            // The scatter's seed is the World seed its shapes grow from.
            pool.world_seed = viewer.seed;
            pools.add(pool)
        })
        .collect();
    for stone in scatter.instances() {
        commands.spawn((
            PooledStone {
                pool: handles[stone.class].clone(),
                variant: stone.variant,
            },
            Transform::from_matrix(Mat4::from(stone.transform)),
            Shown,
        ));
    }
}

/// Saves one PNG image and exits when `--screenshot` is set.
/// Waits for CPU rocks and textures, then gives them extra frames to appear.
/// Pool rocks get an additional delay because GPU generation is not checked.
/// Reports an error if loading or saving fails, or if the scene takes too long
/// to become ready after CPU rock generation finishes.
#[expect(clippy::too_many_arguments, reason = "Bevy system parameters")]
fn screenshot(
    mut commands: Commands,
    viewer: Res<Viewer>,
    offscreen: Option<Res<Offscreen>>,
    time: Res<Time>,
    asset_server: Res<AssetServer>,
    materials: Res<Assets<StoneMaterial>>,
    parts: Query<(), With<StonePart>>,
    generating: Query<(), With<StoneGenerating>>,
    pooled: Query<(), With<PooledStone>>,
    mut exit: MessageWriter<AppExit>,
    mut idle_since: Local<Option<f32>>,
    mut ready_since: Local<Option<f32>>,
    mut settled: Local<u32>,
    mut taken: Local<bool>,
) {
    let Some(path) = &viewer.screenshot else {
        return;
    };
    if *taken {
        return;
    }
    let now = time.elapsed_secs();

    // Allow CPU rock generation to finish before counting the readiness timeout.
    let idle_since = if generating.is_empty() {
        *idle_since.get_or_insert(now)
    } else {
        *idle_since = None;
        now
    };

    // Stop on a texture error; otherwise, wait for rocks and textures to be ready.
    let textures = match stone_textures_state(&materials, &asset_server) {
        Ok(loaded) => loaded,
        Err(message) => {
            error!("screenshot failed: {message}");
            exit.write(AppExit::error());
            *taken = true;
            return;
        }
    };
    let rocks = if viewer.pool || viewer.scatter {
        !pooled.is_empty()
    } else {
        !parts.is_empty() && generating.is_empty()
    };
    if !(textures && rocks) {
        *ready_since = None;
        *settled = 0;
        if now - idle_since > SCREENSHOT_TIMEOUT {
            error!(
                "screenshot failed: scene not ready {SCREENSHOT_TIMEOUT} seconds after \
                 rock generation finished"
            );
            exit.write(AppExit::error());
            *taken = true;
        }
        return;
    }

    // Allow time for textures to appear and for the GPU to generate pool rocks.
    let since = *ready_since.get_or_insert(now);
    *settled += 1;
    let pool_wait = if pooled.is_empty() {
        0.0
    } else {
        POOL_SCREENSHOT_DELAY
    };
    if *settled < SETTLE_FRAMES || now < since + pool_wait {
        return;
    }
    *taken = true;
    let target = match offscreen {
        Some(offscreen) => Screenshot::image(offscreen.0.clone()),
        None => Screenshot::primary_window(),
    };

    // Exit after saving, reporting whether the PNG was written successfully.
    let path = path.clone();
    commands.spawn(target).observe(
        move |captured: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| match save_png(
            &captured.image,
            &path,
        ) {
            Ok(()) => {
                info!("screenshot saved to {path}");
                exit.write(AppExit::Success);
            }
            Err(message) => {
                error!("screenshot failed: could not save {path}: {message}");
                exit.write(AppExit::error());
            }
        },
    );
}

/// Checks whether all rock textures have loaded: `Ok(true)` means ready and
/// `Ok(false)` means still waiting. On failure, returns an error with the texture
/// path when available.
fn stone_textures_state(
    materials: &Assets<StoneMaterial>,
    asset_server: &AssetServer,
) -> Result<bool, String> {
    let mut loaded = true;
    for (_, material) in materials.iter() {
        let extension = &material.extension;
        let textures = [&extension.color, &extension.normal, &extension.orh];
        for texture in textures.into_iter().flatten() {
            if asset_server.load_state(texture).is_failed() {
                let path = texture.path().map(ToString::to_string).unwrap_or_default();
                return Err(format!("texture {path} failed to load"));
            }
            loaded &= asset_server.is_loaded_with_dependencies(texture);
        }
    }
    Ok(loaded)
}

/// Saves the captured image as a PNG without transparency.
/// Returns an error message if conversion or saving fails.
fn save_png(frame: &Image, path: &str) -> Result<(), String> {
    let rgb = frame
        .clone()
        .try_into_dynamic()
        .map_err(|error| error.to_string())?
        .to_rgb8();
    rgb.save_with_format(path, image::ImageFormat::Png)
        .map_err(|error| error.to_string())
}
