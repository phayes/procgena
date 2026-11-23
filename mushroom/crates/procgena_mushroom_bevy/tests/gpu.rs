//! Renders mushrooms at every detail level on a real GPU and checks that every
//! shader pipeline compiles.
//!
//! Needs a GPU, so it is ignored by default. Run it with:
//!
//! ```bash
//! cargo test -p procgena_mushroom_bevy --test gpu -- --include-ignored
//! ```

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
use procgena_mushroom::{MushroomOptions, MushroomPreset};
use procgena_mushroom_bevy::{MushroomGenerating, MushroomPart, MushroomPlugin};

/// The longest the test waits for mushrooms and shaders before failing.
const LIMIT: Duration = Duration::from_secs(120);

/// Frames with no pipeline compiling that count as finished.
const SETTLED_FRAMES: u32 = 10;

/// An app that renders into an image instead of a window.
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
        MushroomPlugin::default(),
    ));

    // The renderer starts in the background; wait for it.
    while app.plugins_state() == PluginsState::Adding {
        tick_global_task_pools_on_main_thread();
    }
    app.finish();
    app.cleanup();
    app
}

/// Spawns a camera drawing into an image, a shadow-casting light, and
/// mushrooms near enough and far enough for every default detail level.
fn spawn_scene(app: &mut App) {
    let image = Image::new_target_texture(320, 240, TextureFormat::Rgba8UnormSrgb, None);
    let image = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    app.world_mut().spawn((
        Camera3d::default(),
        RenderTarget::Image(image.into()),
        Transform::from_xyz(0.0, 0.3, 1.0).looking_at(Vec3::new(0.0, 0.0, -5.0), Vec3::Y),
    ));
    app.world_mut().spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(1.0, 3.0, 1.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // An 8 cm mushroom switches at 1.5 m, 4 m, 10 m and 18 m: one per
    // level. The fly agaric adds an annulus and warts; the porcini a cap
    // pattern and no gills.
    let near = |preset: MushroomPreset| MushroomOptions {
        size: 0.08,
        ..preset.options()
    };
    for (z, options) in [
        (0.0, near(MushroomPreset::FlyAgaric)),
        (-2.5, near(MushroomPreset::Porcini)),
        (-6.0, near(MushroomPreset::FlyAgaric)),
        (-15.0, near(MushroomPreset::Porcini)),
        (-25.0, near(MushroomPreset::FlyAgaric)),
    ] {
        app.world_mut().spawn((
            procgena_mushroom_bevy::MushroomParams::new(options),
            Transform::from_xyz(0.0, 0.0, z),
        ));
    }
}

/// Runs frames until every mushroom is built and no pipeline has compiled for
/// a while.
fn run_until_settled(app: &mut App) {
    let deadline = Instant::now() + LIMIT;
    let mut settled = 0;
    while settled < SETTLED_FRAMES {
        assert!(
            Instant::now() < deadline,
            "mushrooms or shaders did not finish"
        );
        app.update();
        let world = app.world_mut();
        let building = world
            .query_filtered::<(), With<MushroomGenerating>>()
            .iter(world)
            .next()
            .is_some();
        let parts = world.query::<&MushroomPart>().iter(world).count();
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

    // Every level, including the camera-facing shapes, was spawned.
    let world = app.world_mut();
    let parts: Vec<MushroomPart> = world
        .query::<&MushroomPart>()
        .iter(world)
        .copied()
        .collect();
    for lod in 0..5 {
        assert!(parts.iter().any(|p| p.lod == lod), "no part at level {lod}");
    }
    for lod in [3, 4] {
        assert!(
            parts.iter().any(|p| p.billboard && p.lod == lod),
            "no camera-facing shapes at level {lod}"
        );
    }

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
