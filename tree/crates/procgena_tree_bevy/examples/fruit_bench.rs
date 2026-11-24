//! Measures what drawing fruit costs in a large grove: about 1000 Spring
//! Pussy Willows with their catkins, seen from a near, a middle and a far
//! camera, rendered headless.
//!
//! ```sh
//! cargo run -p procgena_tree_bevy --example fruit_bench
//! cargo run -p procgena_tree_bevy --example fruit_bench -- --no-fruit
//! ```
//!
//! For each camera it prints:
//! - the GPU time of each culling stage of the shared instanced renderer
//!   (when the GPU can time compute passes);
//! - how many copies and attached copies (catkin clusters) every camera and
//!   shadow view drew, and the bytes the culling wrote for them;
//! - the time per frame with the CPU waiting for the GPU to finish.
//!
//! `--no-fruit` grows the same grove without catkins, to compare. `--trees
//! <n>` changes how many trees the grove has (roughly), and `--frames <n>`
//! how many frames each camera is measured over. Bark and leaves use plain
//! colours, without textures.

use std::time::{Duration, Instant};

use bevy::app::{PluginsState, TerminalCtrlCHandlerPlugin};
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::RenderApp;
use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;
use bevy::render::render_resource::{PipelineCache, PollType, TextureFormat};
use bevy::render::renderer::RenderDevice;
use bevy::tasks::tick_global_task_pools_on_main_thread;
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use procgena_core::ChildKey;
use procgena_core_bevy::instanced::{InstancedScatter, InstancedTimings, read_instance_counts};
use procgena_tree::{ScatterTree, TreePreset, TreeScatter};
use procgena_tree_bevy::{
    TreePlugin, TreeScatterGenerating, TreeScatterInstances, TreeScatterParams, default_tree_lod,
};

/// Trees per hectare.
const DENSITY: f32 = 400.0;
/// Smallest distance between trunks, in metres.
const SPACING: f32 = 2.5;
/// The longest the grove, its impostors and shaders may take.
const LIMIT: Duration = Duration::from_secs(900);

struct Options {
    fruit: bool,
    trees: f32,
    frames: usize,
}

fn options() -> Options {
    let mut options = Options {
        fruit: true,
        trees: 1000.0,
        frames: 120,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--no-fruit" => options.fruit = false,
            "--trees" => {
                options.trees = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .expect("--trees <n>")
            }
            "--frames" => {
                options.frames = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .expect("--frames <n>")
            }
            other => panic!("unknown option {other}"),
        }
    }
    options
}

fn main() {
    let options = options();
    let side = (options.trees / DENSITY * 10_000.0).sqrt();
    let mut app = headless_app();

    // The grove, a camera and a shadow-casting sun.
    let mut willow = TreePreset::spring_pussy_willow().tree.clone();
    if !options.fruit {
        willow.fruit = None;
    }
    let key = ChildKey::new("willows").expect("a child key");
    let grove = TreeScatter::new(vec![ScatterTree::custom(key, willow, DENSITY, SPACING)]);
    let image = Image::new_target_texture(1280, 720, TextureFormat::Rgba8UnormSrgb, None);
    let image = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    let camera = app
        .world_mut()
        .spawn((Camera3d::default(), RenderTarget::Image(image.into())))
        .id();
    app.world_mut().spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(1.0, 3.0, 1.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    let grove = app
        .world_mut()
        .spawn(TreeScatterParams::new(grove, Vec2::splat(side)))
        .id();
    wait_until_settled(&mut app, grove);
    let trees = app
        .world()
        .get::<TreeScatterInstances>(grove)
        .map_or(0, |instances| instances.trees().len());
    app.sub_app_mut(RenderApp)
        .world_mut()
        .init_resource::<InstancedTimings>();

    println!(
        "{trees} willows on {side:.0} m × {side:.0} m, fruit {}",
        if options.fruit { "on" } else { "off" }
    );
    let half = side / 2.0;
    let views = [
        (
            "near",
            Vec3::new(-half + 2.0, 1.7, 0.0),
            Vec3::new(0.0, 2.0, 0.0),
        ),
        ("middle", Vec3::new(0.0, 25.0, half + 30.0), Vec3::ZERO),
        ("far", Vec3::new(0.0, 80.0, half + 250.0), Vec3::ZERO),
    ];
    for (name, eye, target) in views {
        *app.world_mut().get_mut::<Transform>(camera).unwrap() =
            Transform::from_translation(eye).looking_at(target, Vec3::Y);
        measure(&mut app, name, options.frames);
    }
}

/// An app that renders into an image instead of a window, with impostors
/// on and no textures.
fn headless_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .build()
            .disable::<PipelinedRenderingPlugin>()
            .disable::<TerminalCtrlCHandlerPlugin>()
            .disable::<WinitPlugin>()
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            }),
        TreePlugin {
            texture_root: None,
            ..TreePlugin::default()
        },
    ));
    while app.plugins_state() == PluginsState::Adding {
        tick_global_task_pools_on_main_thread();
    }
    app.finish();
    app.cleanup();
    app
}

/// Runs frames until the grove is placed, every variant's impostor is drawn
/// and no pipeline has compiled for a while.
fn wait_until_settled(app: &mut App, grove: Entity) {
    let deadline = Instant::now() + LIMIT;
    let mesh_levels = default_tree_lod().levels.len();
    let mut settled = 0;
    while settled < 30 {
        assert!(Instant::now() < deadline, "the grove did not settle");
        app.update();
        let building = app
            .world()
            .entity(grove)
            .contains::<TreeScatterGenerating>();
        let impostors = app
            .world()
            .get::<InstancedScatter>(grove)
            .is_some_and(|scatter| {
                let variants = scatter
                    .slots
                    .iter()
                    .filter(|slot| slot.lod.levels.len() > 3)
                    .count();
                variants > 0
                    && scatter
                        .slots
                        .iter()
                        .filter(|slot| slot.lod.levels.len() > 3)
                        .all(|slot| slot.lod.levels.len() == mesh_levels + 1)
            });
        let cache = app.sub_app(RenderApp).world().resource::<PipelineCache>();
        let compiling = cache.waiting_pipelines().next().is_some();
        settled = if building || !impostors || compiling {
            0
        } else {
            settled + 1
        };
    }
}

/// Measures `frames` frames from the camera as placed, after a warm-up,
/// and prints what was drawn and how long it took.
fn measure(app: &mut App, name: &str, frames: usize) {
    for _ in 0..30 {
        app.update();
    }
    let mut frame_times = Vec::with_capacity(frames);
    let mut cull_times = Vec::with_capacity(frames);
    for _ in 0..frames {
        let start = Instant::now();
        app.update();
        let render = app.sub_app(RenderApp).world();
        render
            .resource::<RenderDevice>()
            .poll(PollType::wait_indefinitely())
            .expect("wait for the GPU");
        frame_times.push(start.elapsed());
        if let Some(times) = render.resource::<InstancedTimings>().last() {
            cull_times.push(times);
        }
    }
    let render = app.sub_app(RenderApp).world();
    let counts = read_instance_counts(render);
    let supported = render.resource::<InstancedTimings>().is_supported();

    frame_times.sort();
    let median = |times: &[Duration]| times.get(times.len() / 2).copied().unwrap_or_default();
    let mean = |pick: fn(&procgena_core_bevy::instanced::CullTimes) -> Duration| {
        let total: Duration = cull_times.iter().map(pick).sum();
        total / cull_times.len().max(1) as u32
    };
    println!("\n{name} camera:");
    println!(
        "  frame (CPU waits for GPU): median {:.2} ms",
        median(&frame_times).as_secs_f64() * 1e3
    );
    if supported && !cull_times.is_empty() {
        println!(
            "  cull GPU (mean of {} frames): copies {:.3} ms, hosts {:.3} ms, rows {:.3} ms, args {:.3} ms, total {:.3} ms",
            cull_times.len(),
            mean(|t| t.copies).as_secs_f64() * 1e3,
            mean(|t| t.hosts).as_secs_f64() * 1e3,
            mean(|t| t.rows).as_secs_f64() * 1e3,
            mean(|t| t.args).as_secs_f64() * 1e3,
            mean(|t| t.total()).as_secs_f64() * 1e3,
        );
    } else {
        println!("  cull GPU: not timed (no TIMESTAMP_QUERY)");
    }
    println!(
        "  drawn over {} views: {} copies, {} attached copies; {:.2} MB written per frame",
        counts.views,
        counts.copies,
        counts.attached,
        counts.bytes() as f64 / 1e6
    );
}
