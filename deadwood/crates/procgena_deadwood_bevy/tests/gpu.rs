//! Renders dead wood at every detail level on a real GPU and checks that
//! every shader pipeline compiles.
//!
//! Needs a GPU, so it is ignored by default. Run it with:
//!
//! ```bash
//! cargo test -p procgena_deadwood_bevy --test gpu -- --include-ignored
//! ```
//!
//! Bark and end-grain textures are used when `deadwood/assets/textures`
//! holds them (`uv run --project tools textures download deadwood`).

use std::time::{Duration, Instant};

use bevy::app::{PluginsState, TerminalCtrlCHandlerPlugin};
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::RenderApp;
use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;
use bevy::render::render_resource::{CachedPipelineState, PipelineCache, TextureFormat};
use bevy::tasks::tick_global_task_pools_on_main_thread;
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use procgena_core::Quality;
use procgena_deadwood::{DeadwoodPreset, ScatterOptions, default_lod, lod_size};
use procgena_deadwood_bevy::{
    DeadwoodGenerating, DeadwoodParams, DeadwoodPart, DeadwoodPartKind, DeadwoodPlugin,
    DeadwoodScatterParams, ScatterGround,
};

/// The longest the test waits for pieces and shaders before failing.
const LIMIT: Duration = Duration::from_secs(180);

/// Frames with no pipeline compiling that count as finished.
const SETTLED_FRAMES: u32 = 10;

/// The repository's dead wood assets, which hold the textures.
const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../deadwood/assets");

/// An app that renders into an image instead of a window.
fn headless_app() -> App {
    let textured = [
        "textures/bark/bark_brown_02-1k-color-bc7.ktx2",
        "textures/end/TreeEnd003-1k-color-bc7.ktx2",
        "textures/end/TreeEnd005-1k-color-bc7.ktx2",
    ]
    .iter()
    .all(|file| std::path::Path::new(ASSETS).join(file).exists());
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
            })
            .set(AssetPlugin {
                file_path: ASSETS.to_owned(),
                ..default()
            }),
        DeadwoodPlugin {
            texture_root: textured.then(|| "textures".to_owned()),
            ..DeadwoodPlugin::default()
        },
    ));

    // The renderer starts in the background; wait for it.
    while app.plugins_state() == PluginsState::Adding {
        tick_global_task_pools_on_main_thread();
    }
    app.finish();
    app.cleanup();
    app
}

/// Spawns a camera drawing into an image, a shadow-casting light, a stick
/// scatter, and copies of a branch with strips at distances that fall in
/// each of its detail levels.
fn spawn_scene(app: &mut App) {
    let image = Image::new_target_texture(320, 240, TextureFormat::Rgba8UnormSrgb, None);
    let image = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    app.world_mut().spawn((
        Camera3d::default(),
        RenderTarget::Image(image.into()),
        Transform::from_xyz(0.0, 0.5, 1.5).looking_at(Vec3::new(0.0, 0.0, -5.0), Vec3::Y),
    ));
    app.world_mut().spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(1.0, 3.0, 1.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // A dead branch has bark, a sawn and a snapped end, and side twigs that
    // become strips; one copy per level, midway through it.
    let options = DeadwoodPreset::DeadBranch.options();
    let lod = default_lod();
    let factor = lod.factor(lod_size(&options), Quality::High);
    let mut starts: Vec<f32> = lod.levels.iter().map(|l| l.distance * factor).collect();
    starts.push(lod.cull_distance.expect("the default levels cull") * factor);
    for pair in starts.windows(2) {
        let z = -(pair[0] + pair[1]) * 0.5;
        app.world_mut().spawn((
            DeadwoodParams::new(options.clone()),
            Transform::from_xyz(0.0, 0.0, z + 1.5),
        ));
    }

    // A hollow log near the camera, and kindling around it.
    app.world_mut().spawn((
        DeadwoodParams::preset(DeadwoodPreset::HollowLog),
        Transform::from_xyz(1.0, 0.0, -1.0),
    ));
    app.world_mut().spawn(DeadwoodScatterParams {
        scatter: ScatterOptions {
            size: [3.0, 3.0],
            ..ScatterOptions::kindling()
        },
        ground: ScatterGround::Flat,
    });
}

/// Runs frames until every piece is built and no pipeline has compiled for
/// a while.
fn run_until_settled(app: &mut App) {
    let deadline = Instant::now() + LIMIT;
    let mut settled = 0;
    while settled < SETTLED_FRAMES {
        assert!(
            Instant::now() < deadline,
            "dead wood or shaders did not finish"
        );
        app.update();
        let world = app.world_mut();
        let building = world
            .query_filtered::<(), With<DeadwoodGenerating>>()
            .iter(world)
            .next()
            .is_some();
        let parts = world.query::<&DeadwoodPart>().iter(world).count();
        let cache = app.sub_app(RenderApp).world().resource::<PipelineCache>();
        let compiling = cache.waiting_pipelines().next().is_some();
        settled = if building || parts == 0 || compiling {
            0
        } else {
            settled + 1
        };
    }
}

#[test]
#[ignore = "needs a GPU"]
fn every_detail_level_compiles_its_shaders() {
    let mut app = headless_app();
    spawn_scene(&mut app);
    run_until_settled(&mut app);

    // Every level, including the strips, was spawned.
    let world = app.world_mut();
    let parts: Vec<DeadwoodPart> = world
        .query::<&DeadwoodPart>()
        .iter(world)
        .copied()
        .collect();
    for lod in 0..3 {
        assert!(parts.iter().any(|p| p.lod == lod), "no part at level {lod}");
    }
    assert!(
        parts.iter().any(|p| p.kind == DeadwoodPartKind::Ribbon),
        "no strips"
    );

    // No pipeline failed: a shader error shows up here.
    let cache = app.sub_app(RenderApp).world().resource::<PipelineCache>();
    let errors: Vec<String> = cache
        .pipelines()
        .filter_map(|p| match &p.state {
            CachedPipelineState::Err(e) => Some(e.to_string()),
            _ => None,
        })
        .collect();
    assert!(
        errors.is_empty(),
        "pipelines failed:\n{}",
        errors.join("\n")
    );
}
