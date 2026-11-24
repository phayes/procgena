//! Measures how fast a grove draws: grows one grove, circles a camera over it
//! without a window, and reports frame times.
//!
//! ```sh
//! cargo run -p procgena_tree_bevy --example scatter_bench -- --size 120
//! cargo run -p procgena_tree_bevy --example scatter_bench -- --size 200 --csv grove.csv
//! ```
//!
//! - `--size <m>`: width and depth of the grove, in metres (default 120).
//! - `--preset <key>`: the tree Scatter preset (default `oak_woodland`).
//! - `--frames <n>`: frames to measure after the warm-up (default 600).
//! - `--csv <file>`: also write every measured frame time to a CSV file.
//! - `--screenshot <file.png>`: also save the last frame.
//! - `--textures`: use the repository's bark and leaf textures.
//! - `--entities`: draw the same trees as one tree entity each instead of
//!   through the grove, to compare the two.
//! - `--no-impostor`: draw far trees with their farthest mesh level instead
//!   of as impostors. Otherwise measuring starts once every variant's
//!   impostor is baked.
//! - `--cache <dir>`: keep baked impostors in this folder, and reuse them on
//!   later runs.
//!
//! The camera path follows the frame count, not the clock, so every run sees
//! the same frames. Compare runs on the same machine and build profile only.
//!
//! Columns, in milliseconds:
//!
//! - `frame_ms`: the whole frame. Rendering waits for the GPU to keep up, so
//!   this includes drawing.
//! - `main_ms`: the app's own systems, from `First` to `Last`. This is where
//!   Bevy checks the visibility of every entity.
//! - `render_ms`: the render world preparing the frame, from extraction's
//!   commands to just before the GPU work is recorded. This is where every
//!   drawn entity is specialized, queued and batched.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::app::ScheduleRunnerPlugin;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::renderer::render_system;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::time::Real;
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use procgena_impostor::{ImpostorBakeCamera, ImpostorLevel, ImpostorPlugin, ImpostorSettings};
use procgena_tree::{TreeScatterPreset, default_lod};
use procgena_tree_bevy::{
    TreeParams, TreePlugin, TreeScatterAssets, TreeScatterFinished, TreeScatterInstances,
    TreeScatterParams, TreeScatterStats, default_tree_impostor,
};

/// Frames after the grove is ready before measuring starts: pipeline
/// compilation and the first uploads.
const WARMUP: u32 = 120;
/// Seconds to wait for the grove before giving up.
const TIMEOUT: f32 = 300.0;
/// Frames for one full circle of the camera.
const CIRCLE_FRAMES: f32 = 600.0;

/// What to measure, from the command line.
#[derive(Resource)]
struct Bench {
    size: f32,
    preset: &'static TreeScatterPreset,
    frames: u32,
    csv: Option<String>,
    screenshot: Option<String>,
    entities: bool,
    /// Where far trees switch to impostors, or `None` without them.
    impostor: Option<ImpostorLevel>,
}

/// The image the camera draws into.
#[derive(Resource)]
struct Target(Handle<Image>);

/// Progress of the run.
#[derive(Resource, Default)]
struct Run {
    /// Trees in the grove, once it is ready.
    trees: Option<usize>,
    /// Frames since the grove was ready.
    frame: u32,
    /// Each measured frame's times, in milliseconds: frame, main, render.
    rows: Vec<[f32; 3]>,
}

/// CPU times of the latest frame, shared by the app and its render world.
#[derive(Resource, Clone, Default)]
struct CpuClock(Arc<Mutex<CpuTimes>>);

#[derive(Default)]
struct CpuTimes {
    main_start: Option<Instant>,
    main_ms: f32,
    render_start: Option<Instant>,
    render_ms: f32,
}

fn start_main(clock: Res<CpuClock>) {
    clock.0.lock().unwrap().main_start = Some(Instant::now());
}

fn end_main(clock: Res<CpuClock>) {
    let mut times = clock.0.lock().unwrap();
    if let Some(start) = times.main_start {
        times.main_ms = start.elapsed().as_secs_f32() * 1000.0;
    }
}

fn start_render(clock: Res<CpuClock>) {
    clock.0.lock().unwrap().render_start = Some(Instant::now());
}

fn end_render(clock: Res<CpuClock>) {
    let mut times = clock.0.lock().unwrap();
    if let Some(start) = times.render_start {
        times.render_ms = start.elapsed().as_secs_f32() * 1000.0;
    }
}

fn main() -> AppExit {
    // Read the command line.
    let mut bench = Bench {
        size: 120.0,
        preset: TreeScatterPreset::oak_woodland(),
        frames: 600,
        csv: None,
        screenshot: None,
        entities: false,
        impostor: Some(default_tree_impostor()),
    };
    let mut textures = false;
    let mut cache = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().expect("a value after the option");
        match arg.as_str() {
            "--size" => bench.size = value().parse().expect("--size needs metres"),
            "--preset" => {
                bench.preset =
                    TreeScatterPreset::named(&value()).expect("a tree Scatter preset key");
            }
            "--frames" => bench.frames = value().parse().expect("--frames needs a count"),
            "--csv" => bench.csv = Some(value()),
            "--screenshot" => bench.screenshot = Some(value()),
            "--textures" => textures = true,
            "--entities" => bench.entities = true,
            "--no-impostor" => bench.impostor = None,
            "--cache" => cache = Some(PathBuf::from(value())),
            other => panic!("unknown argument {other}"),
        }
    }

    // Run without a window, as fast as frames can be made.
    let clock = CpuClock::default();
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .set(AssetPlugin {
                file_path: "../../assets".to_owned(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .disable::<WinitPlugin>(),
        ScheduleRunnerPlugin::run_loop(Duration::ZERO),
        ImpostorPlugin {
            settings: ImpostorSettings {
                cache_dir: cache,
                ..default()
            },
        },
        TreePlugin {
            texture_root: textures.then(|| "textures".to_owned()),
            impostor: bench.impostor,
            ..default()
        },
    ))
    .insert_resource(bench)
    .insert_resource(clock.clone())
    .init_resource::<Run>()
    .add_observer(grove_finished)
    .add_systems(Startup, setup)
    .add_systems(First, start_main)
    .add_systems(Update, (fly, record).chain())
    .add_systems(Last, end_main);

    // Time the render world's preparation, up to recording the GPU work.
    app.sub_app_mut(RenderApp)
        .insert_resource(clock)
        .add_systems(
            Render,
            (
                start_render.in_set(RenderSystems::ExtractCommands),
                end_render
                    .in_set(RenderSystems::Render)
                    .before(render_system),
            ),
        );
    app.run()
}

fn setup(
    mut commands: Commands,
    bench: Res<Bench>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // A 1920 × 1080 image for the camera to draw into.
    let image = images.add(Image::new_target_texture(
        1920,
        1080,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    commands.spawn((
        Camera3d::default(),
        RenderTarget::Image(image.clone().into()),
        Transform::default(),
    ));
    commands.insert_resource(Target(image));
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

    // Flat ground under the grove, and the grove.
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(bench.size, bench.size))),
        MeshMaterial3d(materials.add(Color::srgb(0.32, 0.42, 0.22))),
    ));
    commands.spawn(TreeScatterParams::preset(
        bench.preset,
        Vec2::splat(bench.size),
    ));
}

/// Counts the grove's trees when it is ready, or exits if it failed. With
/// `--entities`, hides the grove and spawns its trees as tree entities.
fn grove_finished(
    event: On<TreeScatterFinished>,
    mut commands: Commands,
    bench: Res<Bench>,
    groves: Query<(&TreeScatterStats, &TreeScatterInstances, &TreeScatterAssets)>,
    mut run: ResMut<Run>,
    mut exit: MessageWriter<AppExit>,
) {
    if let Err(error) = &event.result {
        error!("grove failed: {error}");
        exit.write(AppExit::error());
        return;
    }
    let Ok((stats, instances, assets)) = groves.get(event.entity) else {
        return;
    };
    run.trees = Some(stats.trees);
    if bench.entities {
        commands.entity(event.entity).insert(Visibility::Hidden);
        for tree in instances.trees() {
            let options = assets.of(tree).options().clone();
            commands.spawn((TreeParams::Options(options), tree.transform));
        }
    }
}

/// Circles the camera over the grove: 25 m up, half the grove's width out,
/// looking at its middle.
fn fly(
    run: Res<Run>,
    bench: Res<Bench>,
    mut cameras: Query<&mut Transform, (With<Camera3d>, Without<ImpostorBakeCamera>)>,
) {
    let angle = run.frame as f32 / CIRCLE_FRAMES * std::f32::consts::TAU;
    let radius = 0.5 * bench.size;
    let eye = Vec3::new(radius * angle.cos(), 25.0, radius * angle.sin());
    for mut transform in &mut cameras {
        *transform = Transform::from_translation(eye).looking_at(Vec3::ZERO, Vec3::Y);
    }
}

/// Records each measured frame and, after the last, prints a summary and
/// exits.
#[expect(clippy::too_many_arguments, reason = "one Bevy system")]
fn record(
    mut commands: Commands,
    bench: Res<Bench>,
    target: Res<Target>,
    clock: Res<CpuClock>,
    mut run: ResMut<Run>,
    time: Res<Time<Real>>,
    entities: Query<()>,
    groves: Query<(
        &TreeScatterInstances,
        &TreeScatterStats,
        &TreeScatterAssets,
        &GlobalTransform,
    )>,
    camera: Single<&GlobalTransform, (With<Camera3d>, Without<ImpostorBakeCamera>)>,
    mut exit: MessageWriter<AppExit>,
) {
    // Wait for the grove and its impostors, then for the warm-up.
    let impostors_baked = || {
        groves.iter().all(|(_, _, assets, _)| {
            assets
                .variants()
                .iter()
                .all(|v| !v.is_ready() || v.has_impostor())
        })
    };
    let ready =
        run.trees.is_some() && (run.frame > 0 || bench.impostor.is_none() || impostors_baked());
    let (true, Some(trees)) = (ready, run.trees) else {
        if time.elapsed_secs() > TIMEOUT {
            error!("the grove was not ready after {TIMEOUT} seconds");
            exit.write(AppExit::error());
        }
        return;
    };
    run.frame += 1;
    let last = WARMUP + bench.frames;
    if run.frame <= WARMUP || run.frame > last {
        return;
    }
    // The main and render times are the previous frame's, finished by now.
    let (main_ms, render_ms) = {
        let times = clock.0.lock().unwrap();
        (times.main_ms, times.render_ms)
    };
    run.rows
        .push([time.delta_secs() * 1000.0, main_ms, render_ms]);
    if run.frame < last {
        return;
    }

    // Summarise and finish.
    let mut summary = format!(
        "{} on {} m: {trees} trees, {} entities, {} frames",
        bench.preset.name,
        bench.size,
        entities.iter().count(),
        run.rows.len()
    );
    for (column, name) in ["frame_ms", "main_ms", "render_ms"].iter().enumerate() {
        let mut sorted: Vec<f32> = run.rows.iter().map(|row| row[column]).collect();
        sorted.sort_by(f32::total_cmp);
        let mean = sorted.iter().sum::<f32>() / sorted.len() as f32;
        let p95 = sorted[(sorted.len() - 1) * 95 / 100];
        let worst = sorted[sorted.len() - 1];
        write!(
            summary,
            "\n  {name:<10} mean {mean:7.3}   p95 {p95:7.3}   max {worst:7.3}"
        )
        .unwrap();
    }
    // Triangles drawn from the last frame's camera, per detail level, with
    // each tree on the level its distance picks (ignoring crossfades and
    // culling). An impostor is the level after the meshes, two triangles.
    let eye = camera.translation();
    let starts: Vec<f32> = default_lod().levels.iter().map(|l| l.distance).collect();
    let impostor_distance = bench.impostor.map_or(f32::INFINITY, |i| i.distance);
    for (instances, stats, _, transform) in &groves {
        let mut per_tree: Vec<f64> = stats
            .levels
            .iter()
            .map(|l| l.triangles as f64 / stats.trees.max(1) as f64)
            .collect();
        let impostor_level = per_tree.len();
        per_tree.push(2.0);
        let mut drawn = vec![0.0f64; per_tree.len()];
        for tree in instances.trees() {
            let distance = transform
                .transform_point(tree.transform.translation)
                .distance(eye);
            let level = if distance >= impostor_distance {
                impostor_level
            } else {
                let level = starts.iter().rposition(|s| distance >= *s).unwrap_or(0);
                level.min(impostor_level - 1)
            };
            drawn[level] += per_tree[level];
        }
        let total: f64 = drawn.iter().sum();
        write!(summary, "\n  triangles  {:.2} M drawn", total / 1e6).unwrap();
        for (level, triangles) in drawn.iter().enumerate() {
            let name = if level == impostor_level {
                "impostors".to_owned()
            } else {
                format!("level {level}")
            };
            write!(summary, ", {name} {:.1}%", 100.0 * triangles / total).unwrap();
        }
    }
    info!("{summary}");
    if let Some(path) = &bench.csv {
        let mut text = String::from("frame_ms,main_ms,render_ms\n");
        for [frame, main, render] in &run.rows {
            writeln!(text, "{frame:.4},{main:.4},{render:.4}").unwrap();
        }
        if let Err(error) = std::fs::write(path, text) {
            error!("could not write {path}: {error}");
        }
    }

    // Exit now, or once the last frame is saved.
    let Some(path) = &bench.screenshot else {
        exit.write(AppExit::Success);
        return;
    };
    commands
        .spawn(Screenshot::image(target.0.clone()))
        .observe(save_to_disk(path.clone()))
        .observe(
            |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                exit.write(AppExit::Success);
            },
        );
}
