//! Pool generation on a real GPU, and the meshes it lists for the shared
//! instanced renderer.
//!
//! Renders headless frames and reads generated draw arguments back. Needs a
//! GPU, so the tests are ignored by default:
//!
//! ```text
//! cargo test -p procgena_stone_bevy --features gpu -- --ignored gpu
//! ```
//!
//! Which copies each view draws at which level is the shared renderer's
//! job, tested in `procgena_core_bevy`.

use std::time::Duration;

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::RenderApp;
use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;
use bevy::render::render_resource::{
    Buffer, BufferDescriptor, BufferUsages, CommandEncoderDescriptor, MapMode, PollType,
    TextureFormat,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::time::TimeUpdateStrategy;
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use procgena_core::chain::Step;
use procgena_core_bevy::instanced::{ExternalMeshes, InstancedScatter};
use procgena_stone::{Stone, StoneDetail, StoneOptions, TextureSize};

use super::pools::RenderPools;
use crate::pool::external_mesh_id;
use crate::{PooledStone, StonePlugin, StonePool};

/// Frames to wait for generation and its status read-back.
const MAX_FRAMES: usize = 600;
/// Words per indexed indirect draw, and the index count's word within one.
const DRAW_WORDS: usize = 5;
const INDEX_COUNT_WORD: usize = 0;

/// Copies a buffer to the CPU as `u32` words, waiting for the GPU.
fn read_words(device: &RenderDevice, queue: &RenderQueue, source: &Buffer) -> Vec<u32> {
    let size = source.size();
    let staging = device.create_buffer(&BufferDescriptor {
        label: Some("procgena_stone_test_staging"),
        size,
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
    encoder.copy_buffer_to_buffer(source, 0, &staging, 0, size);
    queue.submit([encoder.finish()]);
    staging
        .slice(..)
        .map_async(MapMode::Read, |result| result.expect("map"));
    device.poll(PollType::wait_indefinitely()).expect("poll");
    let words = staging
        .slice(..)
        .get_mapped_range()
        .expect("mapped")
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&b| u32::from_le_bytes(b))
        .collect();
    staging.unmap();
    words
}

/// A small game level, so generation in tests is quick: 24 cells per axis,
/// with a 12-cell lower mesh.
fn two_levels() -> StoneDetail {
    StoneDetail::new(24, TextureSize::Size512)
}

/// A headless app rendering to an image, with a camera looking down the
/// negative z axis and one pooled rock of each variant at `positions`.
fn headless_app(positions: &[Vec3]) -> App {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .disable::<WinitPlugin>()
            .disable::<PipelinedRenderingPlugin>(),
    )
    .add_plugins(StonePlugin {
        texture_root: None,
        ..default()
    })
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 60.0,
    )));

    let world = app.world_mut();
    let target = world
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            320,
            180,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
    let mut options = StoneOptions::default();
    options.mesh.game_level = two_levels();
    let pool = StonePool::new(options, positions.len() as u32);
    let pool = world.resource_mut::<Assets<StonePool>>().add(pool);
    world.spawn((
        Camera3d::default(),
        RenderTarget::Image(target.into()),
        Transform::from_xyz(0.0, 1.5, 0.0).looking_at(Vec3::new(0.0, 0.0, -10.0), Vec3::Y),
    ));
    for (variant, &position) in positions.iter().enumerate() {
        world.spawn((
            PooledStone {
                pool: pool.clone(),
                variant: variant as u32,
            },
            Transform::from_translation(position),
        ));
    }
    app.finish();
    app.cleanup();
    app
}

/// Runs frames until every pool has no pending generation or read-back.
/// Panics after `MAX_FRAMES`.
fn run_until_generated(app: &mut App) {
    for _ in 0..MAX_FRAMES {
        app.update();
        let world = app.sub_app_mut(RenderApp).world_mut();
        let pools = world.resource::<RenderPools>();
        if !pools.pools.is_empty() && pools.pools.values().all(|p| p.status_read()) {
            // One more frame lists the final generation's meshes.
            app.update();
            return;
        }
    }
    panic!("the pool never finished generating");
}

#[test]
#[ignore = "needs a GPU"]
fn gpu_pool_lists_every_mesh_for_the_shared_renderer() {
    let positions = [Vec3::new(0.0, 0.0, -8.0), Vec3::new(0.0, 0.0, -40.0)];
    let mut app = headless_app(&positions);
    run_until_generated(&mut app);

    // The main world draws the rocks through one Scatter with a slot per
    // variant.
    let mut scatters = app.world_mut().query::<&InstancedScatter>();
    let scatter = scatters.single(app.world()).expect("one pool drawer");
    assert_eq!(scatter.slots.len(), 2);
    assert_eq!(scatter.copies.len(), 2);

    // The render world lists every variant's every level, each with triangles.
    let world = app.sub_app_mut(RenderApp).world_mut();
    let device = world.resource::<RenderDevice>().clone();
    let queue = world.resource::<RenderQueue>().clone();
    let pools = world.resource::<RenderPools>();
    let pool = pools.pools.values().next().expect("a pool");
    let external = world.resource::<ExternalMeshes>();
    assert_eq!(external.0.len(), 4, "two variants at two levels");
    for mesh in 0..pool.mesh_count() {
        let listed = external
            .0
            .get(&external_mesh_id(pool.version(), mesh))
            .unwrap_or_else(|| panic!("mesh {mesh} is not listed"));
        let args = read_words(&device, &queue, &listed.args);
        let at = listed.args_offset as usize / 4;
        assert!(
            args[at + INDEX_COUNT_WORD] > 0,
            "mesh {mesh} has no triangles"
        );
    }
}

#[test]
#[ignore = "needs a GPU"]
fn gpu_pool_copies_take_the_size_the_gpu_measured() {
    let positions = [Vec3::new(0.0, 0.0, -8.0), Vec3::new(2.0, 0.0, -8.0)];
    let mut app = headless_app(&positions);
    run_until_generated(&mut app);
    // The main world takes the measured sizes on the next frame.
    app.update();

    // Each copy's size is its variant's game-level bounding diameter, as the
    // CPU measures the same rock, to within two grid cells, and smaller than
    // the bound of any rock from the pool's settings.
    let pool = {
        let pools = app.world().resource::<Assets<StonePool>>();
        pools.iter().next().expect("a pool").1.clone()
    };
    let mut scatters = app.world_mut().query::<&InstancedScatter>();
    let scatter = scatters.single(app.world()).expect("one pool drawer");
    let cell = pool.options.scale / two_levels().resolution as f32;
    for copy in scatter.copies.iter() {
        let cpu = Stone::new(pool.variant_options(copy.slot))
            .unwrap()
            .bounding_diameter();
        assert!(
            (copy.size - cpu).abs() <= 2.0 * cell,
            "variant {}: GPU size {} vs CPU {cpu}",
            copy.slot,
            copy.size
        );
        assert!(copy.size < pool.options.bounding_diameter());
    }
}

#[test]
#[ignore = "needs a GPU"]
fn gpu_pool_regenerates_meshes_that_overflowed() {
    let mut app = headless_app(&[Vec3::new(0.0, 0.0, -8.0)]);
    run_until_generated(&mut app);
    {
        let world = app.sub_app_mut(RenderApp).world_mut();
        let mut pools = world.resource_mut::<RenderPools>();
        let pool = pools.pools.values_mut().next().expect("a pool");
        assert_eq!(pool.attempts(), 1, "the first generation should fit");
        pool.regenerate_too_small();
    }

    // The too-small generation is read back and the pool generated again.
    for _ in 0..MAX_FRAMES {
        app.update();
        let world = app.sub_app_mut(RenderApp).world_mut();
        if world
            .resource::<RenderPools>()
            .pools
            .values()
            .next()
            .is_some_and(|pool| pool.attempts() >= 3)
        {
            break;
        }
    }
    run_until_generated(&mut app);

    let world = app.sub_app_mut(RenderApp).world_mut();
    let device = world.resource::<RenderDevice>().clone();
    let queue = world.resource::<RenderQueue>().clone();
    let pools = world.resource::<RenderPools>();
    let pool = pools.pools.values().next().expect("a pool");
    assert_eq!(
        pool.attempts(),
        3,
        "one too-small generation, then one that fits"
    );
    for batch in &pool.batches {
        let args = read_words(&device, &queue, &batch.draw_args);
        for mesh in 0..batch.mesh_count as usize {
            assert!(
                args[mesh * DRAW_WORDS + INDEX_COUNT_WORD] > 0,
                "mesh {} has no triangles",
                batch.first_mesh as usize + mesh
            );
        }
    }
}

#[test]
#[ignore = "needs a GPU"]
fn gpu_pool_variants_gain_their_baked_impostor_as_the_last_level() {
    let positions = [Vec3::new(0.0, 0.0, -8.0), Vec3::new(0.0, 0.0, -40.0)];
    let mut app = headless_app(&positions);
    run_until_generated(&mut app);

    // Each variant's impostor is baked in the background, then drawn
    // after its two mesh steps.
    let impostor_level = |app: &mut App| {
        let mut scatters = app.world_mut().query::<&InstancedScatter>();
        let scatter = scatters.single(app.world()).expect("one pool drawer");
        scatter.slots.iter().all(|slot| slot.impostor)
    };
    for _ in 0..MAX_FRAMES {
        if impostor_level(&mut app) {
            break;
        }
        app.update();
    }
    assert!(impostor_level(&mut app), "every variant has an impostor");
    let mut scatters = app.world_mut().query::<&InstancedScatter>();
    let scatter = scatters.single(app.world()).expect("one pool drawer");
    for slot in scatter.slots.iter() {
        let steps: Vec<Step> = slot.parts.iter().map(|part| part.step).collect();
        assert_eq!(steps, Step::ALL, "the mesh steps, then the impostor");
    }
}

#[test]
#[ignore = "needs a GPU"]
fn gpu_pool_regenerates_when_its_asset_changes() {
    let mut app = headless_app(&[Vec3::new(0.0, 0.0, -8.0)]);
    run_until_generated(&mut app);
    let seed = |app: &mut App| {
        let world = app.sub_app_mut(RenderApp).world_mut();
        let pools = world.resource::<RenderPools>();
        let pool = pools.pools.values().next().expect("a pool");
        (pool.seed(), pool.attempts())
    };
    let first = seed(&mut app).0;
    assert_eq!(seed(&mut app).1, 1, "adding a pool generates it once");

    // Change the pool in place.
    let world = app.world_mut();
    let mut pools = world.resource_mut::<Assets<StonePool>>();
    let (_, pool) = pools.iter_mut().next().expect("a pool");
    pool.options.default_seed += 100;
    app.update();
    run_until_generated(&mut app);
    assert_eq!(seed(&mut app), (first + 100, 1));

    // Only the new generation's meshes are listed.
    let world = app.sub_app_mut(RenderApp).world_mut();
    assert_eq!(world.resource::<ExternalMeshes>().0.len(), 2);
}
