//! Draws a grove through the shared instanced renderer on a real GPU and
//! checks that every pipeline it needs compiles: the tree materials of both
//! mesh steps, and the impostor after them once baked.
//!
//! Needs a GPU, so it is ignored by default. Run it with:
//!
//! ```bash
//! cargo test -p procgena_tree_bevy --test gpu -- --include-ignored
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
use procgena_core::chain::Step;
use procgena_core_bevy::instanced::InstancedScatter;
use procgena_tree::{ScatterTree, TreePreset, TreeScatter};
use procgena_tree_bevy::{TreePlugin, TreeScatterGenerating, TreeScatterParams};

/// The longest the test waits for the grove, its impostors and shaders.
const LIMIT: Duration = Duration::from_secs(300);

/// Frames with no pipeline compiling that count as finished.
const SETTLED_FRAMES: u32 = 10;

/// An app that renders into an image instead of a window, with impostors on
/// and no textures.
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

    // The renderer starts in the background; wait for it.
    while app.plugins_state() == PluginsState::Adding {
        tick_global_task_pools_on_main_thread();
    }
    app.finish();
    app.cleanup();
    app
}

/// Spawns a camera drawing into an image, a shadow-casting light, and a
/// small grove of two quick-to-build species in front of the camera.
fn spawn_scene(app: &mut App) -> Entity {
    let image = Image::new_target_texture(320, 240, TextureFormat::Rgba8UnormSrgb, None);
    let image = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    app.world_mut().spawn((
        Camera3d::default(),
        RenderTarget::Image(image.into()),
        Transform::from_xyz(0.0, 3.0, 12.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    app.world_mut().spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(1.0, 3.0, 1.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    let grove = TreeScatter::new(vec![
        ScatterTree::new(TreePreset::bush_2())
            .with_variants(2)
            .scaled(0.4),
        ScatterTree::new(TreePreset::pine_small())
            .with_variants(2)
            .scaled(0.2),
    ]);
    app.world_mut()
        .spawn(TreeScatterParams::new(grove, Vec2::splat(12.0)))
        .id()
}

/// Whether every slot of `grove`'s scatter has its impostor step.
fn impostors_drawn(app: &mut App, grove: Entity) -> bool {
    app.world()
        .entity(grove)
        .get::<InstancedScatter>()
        .is_some_and(|scatter| {
            !scatter.copies.is_empty()
                && scatter.slots.iter().all(|slot| {
                    slot.impostor && slot.parts.iter().any(|p| p.step == Step::Impostor)
                })
        })
}

/// Runs frames until the grove is placed, every impostor is drawn and no
/// pipeline has compiled for a while.
fn run_until_settled(app: &mut App, grove: Entity) {
    let deadline = Instant::now() + LIMIT;
    let mut settled = 0;
    while settled < SETTLED_FRAMES {
        assert!(
            Instant::now() < deadline,
            "the grove, its impostors or shaders did not finish"
        );
        app.update();
        let building = app
            .world()
            .entity(grove)
            .contains::<TreeScatterGenerating>();
        let ready = impostors_drawn(app, grove);
        let cache = app.sub_app(RenderApp).world().resource::<PipelineCache>();
        let compiling = cache.waiting_pipelines().next().is_some();
        settled = if building || !ready || compiling {
            0
        } else {
            settled + 1
        };
    }
}

#[test]
#[ignore = "needs a GPU"]
fn a_grove_draws_every_step_and_its_impostors() {
    let mut app = headless_app();
    let grove = spawn_scene(&mut app);
    run_until_settled(&mut app, grove);

    // Every slot draws parts at both mesh steps, then its impostor.
    let scatter = app.world().entity(grove).get::<InstancedScatter>().unwrap();
    for slot in scatter.slots.iter() {
        for step in Step::ALL {
            assert!(
                slot.parts.iter().any(|p| p.step == step),
                "a slot has no part at step {step:?}"
            );
        }
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
