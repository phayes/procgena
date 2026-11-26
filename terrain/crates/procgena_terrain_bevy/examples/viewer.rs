//! Streams a world of terrain around a free camera.
//!
//! ```bash
//! cargo run -p procgena_terrain_bevy --example viewer
//! ```
//!
//! Move with the free camera: W, A, S and D to fly, E and Q for up and down,
//! and the mouse to look around while the right button is held (or after
//! pressing M). L tints chunks by level, B draws chunk borders, T draws
//! every chunk's triangle edges (the wireframe).
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
//! - `--cell-pixels <n>`: the largest a terrain cell is drawn, in pixels of
//!   a 720-pixel-high screen (default about 7); smaller is finer and
//!   draws more triangles;
//! - `--rough-from <level>`: draw chunk levels from this one up at the
//!   rough tier only, without roads;
//! - `--mid-from <level>`: draw chunk levels from this one up (below
//!   `--rough-from`) at the mid tier, with roads in outline;
//! - `--levels`, `--borders`, `--wireframe`: start with chunk levels
//!   tinted, chunk borders drawn, or the wireframe drawn;
//! - `--textures`: draw the ground with its texture sets, read from
//!   `terrain/assets/textures` (download them first with
//!   `uv run --project tools textures download terrain --sizes 128,512,1k`);
//!   without it the ground has flat colours;
//! - `--screenshot <file.png>`: save an image once every chunk is drawn,
//!   then exit;
//! - `--measure <frames>`: once every chunk is drawn, time this many frames
//!   and print, per chunk level, the chunks and triangles drawn and how
//!   wide one cell of the nearest and farthest chunk is on screen; then
//!   exit (after the screenshot, with `--screenshot`). Frames are not
//!   capped with `--headless`; in a window they are capped by vsync;
//! - `--fly <m/s>` (with `--measure`; also `--fly-seconds <s>`, default
//!   10): after measuring, fly straight ahead at the camera's height and
//!   print how long coarse ground stayed under and near the camera; then
//!   exit (with `--screenshot`, after an image taken at the end of the
//!   flight, still moving);
//! - `--tin`: draw the finest chunks from their canonical meshes (see
//!   `TerrainStream::tin`); `--measure` then also counts those chunks;
//! - `--reads <n>`: read up to `n` chunks at once, on `n` background
//!   threads (by default 4 reads on Bevy's usual threads);
//! - `--headless`: with `--screenshot` or `--measure`, render a
//!   1280 × 720 image without opening a window.

use std::sync::Arc;
use std::time::Duration;

use bevy::app::{ScheduleRunnerPlugin, TaskPoolThreadAssignmentPolicy};
use bevy::camera::RenderTarget;
use bevy::camera::primitives::Aabb;
use bevy::camera_controller::free_camera::{FreeCamera, FreeCameraPlugin};
use bevy::pbr::wireframe::{WireframeConfig, WireframePlugin};
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::render_resource::{TextureFormat, WgpuFeatures};
use bevy::render::settings::WgpuSettings;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use glam::DVec2;
use procgena_terrain::tin::TinTolerance;
use procgena_terrain::{
    ChunkDetail, ChunkKey, ChunkLevels, Elevation, RegionId, TerrainPreset, WorldTerrain,
    WorldTerrainOptions,
};
use procgena_terrain_bevy::{
    CanonicalChunk, GroundTextureSettings, TerrainChunk, TerrainDebug, TerrainPlugin,
    TerrainProgress, TerrainStream, TerrainViewer, TerrainWater,
};

/// Frames to wait after every chunk is drawn before a screenshot, so
/// shaders are ready.
const SETTLE_FRAMES: u32 = 30;

/// Seconds to wait for the terrain before a screenshot gives up.
const SCREENSHOT_TIMEOUT: f32 = 900.0;

/// Where to save a screenshot, from `--screenshot <file.png>`.
#[derive(Resource)]
struct ScreenshotPath(Option<String>);

/// Timing and detail figures, from `--measure <frames>`.
#[derive(Resource)]
struct Measure {
    /// Frames to time.
    frames: usize,
    /// Each timed frame's length in seconds.
    times: Vec<f32>,
    /// Settled frames counted before timing starts.
    settled: u32,
    /// Whether the report is printed.
    done: bool,
}

/// A flight after `--measure`, from `--fly <m/s>`: the camera moves
/// straight ahead at a fixed height, and the report says how long coarse
/// ground stays near it.
#[derive(Resource)]
struct Fly {
    /// Metres a second.
    speed: f32,
    /// How long to fly, in seconds.
    seconds: f32,
    /// The chunk levels, to tell which drawn chunks are coarser than their
    /// distance wants.
    levels: ChunkLevels,
    /// Seconds flown so far.
    flown: f32,
    /// Seconds with coarse ground (or none) under the camera, in all and
    /// the longest stretch, and the current stretch.
    coarse_underfoot: f32,
    longest_coarse: f32,
    coarse_now: f32,
    /// Seconds since the drawn chunks last changed, and the longest such
    /// gap.
    since_change: f32,
    longest_unchanged: f32,
    /// Per frame: drawn chunks coarser than their distance wants.
    too_coarse: Vec<usize>,
}

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
    mid_from: Option<u8>,
    /// Draw the finest chunks from their canonical meshes.
    tin: bool,
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
    // The chunk levels: the usual detail, or cells of `--cell-pixels` on a
    // 720-pixel-high screen.
    let detail = value("--cell-pixels").map_or(ChunkDetail::DEFAULT, |v| ChunkDetail {
        cell_on_screen: v.parse::<f32>().expect("--cell-pixels needs a number") / 720.0,
        ..ChunkDetail::DEFAULT
    });
    let levels = detail.levels(view);
    let reads: Option<usize> = value("--reads").map(|n| n.parse().expect("--reads needs a number"));
    let mut plugin =
        TerrainPlugin::detailed(detail, view).expect("--view needs a positive distance");
    if let Some(reads) = reads {
        plugin = plugin.with_max_reads(reads);
    }
    let flight = value("--fly").map(|speed| Fly {
        speed: speed.parse().expect("--fly needs a speed in m/s"),
        seconds: value("--fly-seconds")
            .map_or(10.0, |s| s.parse().expect("--fly-seconds needs a number")),
        levels,
        flown: 0.0,
        coarse_underfoot: 0.0,
        longest_coarse: 0.0,
        coarse_now: 0.0,
        since_change: 0.0,
        longest_unchanged: 0.0,
        too_coarse: Vec::new(),
    });
    assert!(
        flight.is_none() || has("--measure"),
        "--fly needs --measure <frames>"
    );

    // With `--headless`, run without a window, on a fixed 60 Hz loop, or
    // uncapped when measuring.
    let headless = has("--headless");
    let measuring = value("--measure").map(|n| Measure {
        frames: n.parse().expect("--measure needs a number of frames"),
        times: Vec::new(),
        settled: 0,
        done: false,
    });
    assert!(
        !headless || has("--screenshot") || measuring.is_some(),
        "--headless needs --screenshot <file.png> or --measure <frames>"
    );
    let frame = if measuring.is_some() {
        Duration::ZERO
    } else {
        Duration::from_secs_f64(1.0 / 60.0)
    };
    // Assets are read from the workspace, where the ground textures live.
    // With `--reads <n>`, read up to `n` chunks at once on `n` threads.
    // The wireframe needs the GPU to draw lines.
    let mut defaults = DefaultPlugins
        .set(AssetPlugin {
            file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../..").to_owned(),
            ..default()
        })
        .set(RenderPlugin {
            render_creation: WgpuSettings {
                features: WgpuFeatures::POLYGON_MODE_LINE,
                ..default()
            }
            .into(),
            ..default()
        });
    if let Some(reads) = reads {
        defaults = defaults.set(TaskPoolPlugin {
            task_pool_options: TaskPoolOptions {
                async_compute: TaskPoolThreadAssignmentPolicy {
                    min_threads: reads,
                    max_threads: reads,
                    percent: 1.0,
                    on_thread_spawn: None,
                    on_thread_destroy: None,
                },
                ..default()
            },
        });
    }
    let mut app = App::new();
    app.insert_resource(GroundTextureSettings {
        root: has("--textures").then(|| "terrain/assets/textures".to_owned()),
        ..default()
    });
    if headless {
        app.add_plugins((
            defaults
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .disable::<WinitPlugin>(),
            ScheduleRunnerPlugin::run_loop(frame),
        ));
    } else {
        app.add_plugins((defaults, FreeCameraPlugin));
    }
    if let Some(measuring) = measuring {
        app.insert_resource(measuring);
    }
    if let Some(flight) = flight {
        app.insert_resource(flight);
    }
    app.add_plugins((plugin, WireframePlugin::default()))
        .insert_resource(WireframeConfig {
            global: has("--wireframe"),
            default_color: Color::srgb(0.1, 0.15, 0.1),
            ..default()
        })
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
            mid_from: value("--mid-from").map(|v| v.parse().expect("--mid-from needs a level")),
            tin: has("--tin"),
            headless,
        })
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (water, toggle_debug, measure, fly, screenshot).chain(),
        )
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
        mid_from: scene.mid_from,
        tin: scene.tin.then_some(TinTolerance::GENERAL),
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

/// L toggles the level tint, B the chunk borders, T the wireframe.
fn toggle_debug(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut debug: ResMut<TerrainDebug>,
    mut wireframe: ResMut<WireframeConfig>,
) {
    let Some(keys) = keys else { return };
    if keys.just_pressed(KeyCode::KeyL) {
        debug.levels = !debug.levels;
    }
    if keys.just_pressed(KeyCode::KeyB) {
        debug.borders = !debug.borders;
    }
    if keys.just_pressed(KeyCode::KeyT) {
        wireframe.global = !wireframe.global;
    }
}

/// One chunk level's figures in the `--measure` report.
#[derive(Default)]
struct LevelFigures {
    chunks: usize,
    triangles: usize,
    /// Chunks drawn from their canonical meshes, and their triangles.
    canonical: usize,
    canonical_triangles: usize,
    /// The nearest point of the nearest and of the farthest chunk, in
    /// metres from the camera.
    nearest: f32,
    farthest: f32,
}

/// With `--measure`, times frames once every chunk is drawn, then prints
/// the report, and exits unless a screenshot is still to be taken.
#[allow(clippy::too_many_arguments)]
fn measure(
    measure: Option<ResMut<Measure>>,
    path: Res<ScreenshotPath>,
    time: Res<Time<Real>>,
    progress: Query<&TerrainProgress>,
    chunks: Query<(
        &TerrainChunk,
        &Mesh3d,
        &GlobalTransform,
        &Aabb,
        Has<CanonicalChunk>,
    )>,
    meshes: Res<Assets<Mesh>>,
    camera: Query<(&GlobalTransform, &Projection, &Camera), With<TerrainViewer>>,
    fly: Option<Res<Fly>>,
    mut exit: MessageWriter<AppExit>,
) {
    let flying = fly.is_some();
    let Some(mut measure) = measure else { return };
    if measure.done {
        return;
    }

    // Wait until every chunk is drawn and shaders are ready; start again if
    // the terrain changes while timing.
    if progress.is_empty() || !progress.iter().all(|p| p.settled && p.drawn > 0) {
        measure.settled = 0;
        measure.times.clear();
        return;
    }
    if measure.settled < SETTLE_FRAMES {
        measure.settled += 1;
        return;
    }
    if measure.times.len() < measure.frames {
        measure.times.push(time.delta_secs());
        return;
    }

    // Chunks, triangles and distances per level.
    let Ok((eye, projection, view)) = camera.single() else {
        return;
    };
    let eye = eye.translation();
    let mut levels: std::collections::BTreeMap<u8, LevelFigures> = default();
    for (chunk, mesh, transform, aabb, canonical) in &chunks {
        let triangles = meshes
            .get(&mesh.0)
            .and_then(|m| m.indices())
            .map_or(0, |i| i.len() / 3);
        let centre = transform.transform_point(Vec3::from(aabb.center));
        let half = Vec3::from(aabb.half_extents);
        let distance = (eye - eye.clamp(centre - half, centre + half)).length();
        let row = levels.entry(chunk.key.level).or_insert(LevelFigures {
            nearest: f32::INFINITY,
            ..default()
        });
        row.chunks += 1;
        row.triangles += triangles;
        if canonical {
            row.canonical += 1;
            row.canonical_triangles += triangles;
        }
        row.nearest = row.nearest.min(distance);
        row.farthest = row.farthest.max(distance);
    }

    // How wide one cell is on screen at distance `d`, facing the camera.
    let fov = match projection {
        Projection::Perspective(p) => p.fov,
        _ => panic!("--measure needs a perspective camera"),
    };
    let height = view.physical_viewport_size().map_or(720.0, |s| s.y as f32);
    let pixels = |cell: f64, d: f32| cell as f32 * height / (2.0 * d.max(1.0) * (fov / 2.0).tan());

    // The report.
    println!(
        "terrain measure: {height} px high, vertical fov {:.1}°",
        fov.to_degrees()
    );
    println!("level  cell m  chunks  triangles  nearest m  farthest m  px/cell near  px/cell far");
    let (mut chunks_all, mut triangles_all) = (0, 0);
    for (level, row) in &levels {
        let cell = ChunkKey::new(0, 0, *level).step();
        println!(
            "{level:>5}  {cell:>6}  {:>6}  {:>9}  {:>9.0}  {:>10.0}  {:>12.1}  {:>11.1}",
            row.chunks,
            row.triangles,
            row.nearest,
            row.farthest,
            pixels(cell, row.nearest),
            pixels(cell, row.farthest),
        );
        chunks_all += row.chunks;
        triangles_all += row.triangles;
        if row.canonical > 0 {
            println!(
                "       level {level}: {} chunks canonical, {} triangles (skirts and split sides included)",
                row.canonical, row.canonical_triangles
            );
        }
    }
    println!("total  {chunks_all} chunks, {triangles_all} triangles");
    let mut times = measure.times.clone();
    times.sort_by(f32::total_cmp);
    let ms = |q: f32| 1000.0 * times[((times.len() - 1) as f32 * q).round() as usize];
    let mean = 1000.0 * times.iter().sum::<f32>() / times.len() as f32;
    println!(
        "frames {}: mean {mean:.2} ms, median {:.2} ms, p95 {:.2} ms, max {:.2} ms",
        times.len(),
        ms(0.5),
        ms(0.95),
        ms(1.0)
    );
    measure.done = true;
    if path.0.is_none() && !flying {
        exit.write(AppExit::Success);
    }
}

/// With `--fly`, once `--measure` is done, moves the camera straight ahead
/// each frame and keeps the flight's figures; prints them and exits when
/// the flight ends.
#[allow(clippy::too_many_arguments)]
fn fly(
    fly: Option<ResMut<Fly>>,
    measure: Option<Res<Measure>>,
    time: Res<Time<Real>>,
    mut camera: Query<&mut Transform, With<TerrainViewer>>,
    chunks: Query<(&TerrainChunk, &GlobalTransform, &Aabb)>,
    added: Query<(), Added<TerrainChunk>>,
    mut removed: RemovedComponents<TerrainChunk>,
    path: Res<ScreenshotPath>,
    mut exit: MessageWriter<AppExit>,
) {
    let shooting = path.0.is_some();
    let Some(mut fly) = fly else { return };
    if !measure.is_some_and(|m| m.done) || fly.flown >= fly.seconds {
        return;
    }
    let dt = time.delta_secs();
    let Ok(mut transform) = camera.single_mut() else {
        return;
    };

    // Move straight ahead, level.
    let ahead = (transform.forward().as_vec3() * Vec3::new(1.0, 0.0, 1.0)).normalize_or_zero();
    transform.translation += ahead * fly.speed * dt;
    fly.flown += dt;
    let eye = transform.translation;

    // Coarse ground under the camera: no chunk there, or one coarser than
    // level 0.
    let under = ChunkKey::containing(DVec2::new(f64::from(eye.x), f64::from(eye.z)), 0);
    let underfoot = chunks
        .iter()
        .map(|(c, _, _)| c.key)
        .find(|k| k.level == 0 && *k == under);
    if underfoot.is_none() {
        fly.coarse_underfoot += dt;
        fly.coarse_now += dt;
        fly.longest_coarse = fly.longest_coarse.max(fly.coarse_now);
    } else {
        fly.coarse_now = 0.0;
    }

    // Time since the drawn chunks last changed.
    let changed = !added.is_empty() || removed.read().count() > 0;
    fly.since_change = if changed { 0.0 } else { fly.since_change + dt };
    fly.longest_unchanged = fly.longest_unchanged.max(fly.since_change);

    // Drawn chunks coarser than their distance wants, measured as chunks
    // are chosen (across the ground, and the height above the chunk's
    // middle), allowing for the levels' hysteresis.
    let coarsest = |d: f64| {
        fly.levels
            .levels
            .iter()
            .rposition(|l| f64::from(l.start * (1.0 - l.hysteresis)) <= d)
            .unwrap_or(0)
    };
    let too_coarse = chunks
        .iter()
        .filter(|(chunk, transform, aabb)| {
            let middle = transform.transform_point(Vec3::from(aabb.center)).y;
            let across = chunk
                .key
                .distance_to(DVec2::new(f64::from(eye.x), f64::from(eye.z)));
            let d = across.hypot(f64::from(eye.y - middle));
            usize::from(chunk.key.level) > coarsest(d)
        })
        .count();
    fly.too_coarse.push(too_coarse);

    if fly.flown < fly.seconds {
        return;
    }

    // The report.
    let frames = fly.too_coarse.len();
    let mean = fly.too_coarse.iter().sum::<usize>() as f32 / frames as f32;
    let max = fly.too_coarse.iter().copied().max().unwrap_or(0);
    println!(
        "fly {} m/s for {:.1} s ({frames} frames, {:.0} m)",
        fly.speed,
        fly.flown,
        fly.speed * fly.flown
    );
    println!(
        "  coarse underfoot {:.1} s ({:.0}%), longest {:.2} s",
        fly.coarse_underfoot,
        100.0 * fly.coarse_underfoot / fly.flown,
        fly.longest_coarse
    );
    println!("  longest without a change {:.2} s", fly.longest_unchanged);
    println!("  chunks too coarse for their distance: mean {mean:.1}, max {max}");
    if !shooting {
        exit.write(AppExit::Success);
    }
}

/// With `--screenshot`, saves one image once every chunk is drawn (and
/// after `--measure` is done), or at the end of a `--fly` flight, still
/// moving; then exits. Exits with an error if the terrain isn't drawn
/// within [`SCREENSHOT_TIMEOUT`].
#[allow(clippy::too_many_arguments)]
fn screenshot(
    mut commands: Commands,
    path: Res<ScreenshotPath>,
    offscreen: Option<Res<Offscreen>>,
    measure: Option<Res<Measure>>,
    fly: Option<Res<Fly>>,
    time: Res<Time>,
    progress: Query<&TerrainProgress>,
    mut settled: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = &path.0 else { return };
    if measure.is_some_and(|m| !m.done) {
        return;
    }
    if let Some(fly) = fly {
        // At the end of the flight, once.
        if fly.flown < fly.seconds || *settled > 0 {
            return;
        }
        *settled = SETTLE_FRAMES;
    } else if !progress.iter().all(|p| p.settled && p.drawn > 0) || progress.is_empty() {
        *settled = 0;
        if time.elapsed_secs() > SCREENSHOT_TIMEOUT {
            error!("screenshot failed: the terrain wasn't drawn in {SCREENSHOT_TIMEOUT} seconds");
            exit.write(AppExit::error());
        }
        return;
    } else {
        *settled += 1;
        if *settled != SETTLE_FRAMES {
            return;
        }
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
