//! Renders mushrooms at every step of the drawing chain on a real GPU, bakes
//! their impostors, and checks that every shader pipeline compiles.
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
use std::sync::Arc;

use bevy::math::DVec2;
use procgena_core::FlatGround;
use procgena_core::chain::Step;
use procgena_core::scatter::ScatterArea;
use procgena_core_bevy::instanced::InstancedScatter;
use procgena_mushroom::{MushroomOptions, MushroomPreset, ScatterOptions};
use procgena_mushroom_bevy::{
    MushroomGenerating, MushroomGroundScatterParams, MushroomPart, MushroomPlugin,
};

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
/// mushrooms near enough and far enough for every step of the drawing chain.
/// Returns how many single mushrooms it spawned.
fn spawn_scene(app: &mut App) -> usize {
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

    // 8 cm mushrooms from near to far, so every step is on screen. The fly
    // agaric adds an annulus and warts; the porcini a cap pattern and no
    // gills.
    let near = |preset: &MushroomPreset| MushroomOptions {
        size: 0.08,
        ..preset.mushroom.clone()
    };
    let singles = [
        (0.0, near(MushroomPreset::fly_agaric())),
        (-2.5, near(MushroomPreset::porcini())),
        (-6.0, near(MushroomPreset::fly_agaric())),
        (-15.0, near(MushroomPreset::porcini())),
        (-25.0, near(MushroomPreset::fly_agaric())),
    ];
    let count = singles.len();
    for (z, options) in singles {
        app.world_mut().spawn((
            procgena_mushroom_bevy::MushroomParams::new(options.clone()),
            Transform::from_xyz(0.0, 0.0, z),
        ));

        // A small ground scatter beside each, drawn by the shared
        // instanced renderer at the same level.
        app.world_mut().spawn((
            MushroomGroundScatterParams {
                mushroom: Arc::new(options),
                scatter: ScatterOptions {
                    density: 20.0,
                    ..ScatterOptions::default()
                },
                ground: Arc::new(FlatGround),
                area: ScatterArea::new(DVec2::ZERO, Vec2::splat(0.5)),
                mask: None,
            },
            Transform::from_xyz(0.4, 0.0, z),
        ));
    }
    count
}

/// Runs frames until every mushroom is built, the `singles` single
/// mushrooms and every scattered body draw their impostors, and no pipeline
/// has compiled for a while.
fn run_until_settled(app: &mut App, singles: usize) {
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
        let impostors = world
            .query::<&MushroomPart>()
            .iter(world)
            .filter(|part| part.step == Step::Impostor)
            .count();
        let baking = impostors < singles
            || world
                .query::<&InstancedScatter>()
                .iter(world)
                .flat_map(|scatter| scatter.slots.iter())
                .any(|slot| slot.lod.levels.len() < Step::ALL.len());
        let cache = app.sub_app(RenderApp).world().resource::<PipelineCache>();
        let compiling = cache.waiting_pipelines().next().is_some();
        settled = if building || baking || compiling {
            0
        } else {
            settled + 1
        };
    }
}

#[test]
#[ignore = "needs a GPU"]
fn every_step_compiles_its_shaders_and_bakes() {
    let mut app = headless_app();
    let singles = spawn_scene(&mut app);
    run_until_settled(&mut app, singles);

    // Every step was spawned for every single mushroom.
    let world = app.world_mut();
    let parts: Vec<MushroomPart> = world
        .query::<&MushroomPart>()
        .iter(world)
        .copied()
        .collect();
    for step in Step::ALL {
        let count = parts.iter().filter(|p| p.step == step).count();
        assert_eq!(count, singles, "parts at {step:?}");
    }

    // Every scatter draws through the shared instanced renderer.
    let scattered = world
        .query::<&InstancedScatter>()
        .iter(world)
        .filter(|scatter| !scatter.copies.is_empty())
        .count();
    assert_eq!(scattered, 5, "a scatter has no instanced copies");

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
