//! Pool generation and culling on a real GPU.
//!
//! Renders headless frames and reads the per-view draw arguments back to check
//! which rocks each view draws at which detail level. Needs a GPU, so the tests
//! are ignored by default:
//!
//! ```text
//! cargo test -p procgena_stone_bevy -- --ignored gpu
//! ```

use std::collections::HashMap;
use std::time::Duration;

use bevy::camera::RenderTarget;
use bevy::pbr::LightEntity;
use bevy::prelude::*;
use bevy::render::RenderApp;
use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;
use bevy::render::render_resource::{
    Buffer, BufferDescriptor, BufferUsages, CommandEncoderDescriptor, MapMode, PollType,
    TextureFormat,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::view::{ExtractedView, RetainedViewEntity};
use bevy::time::TimeUpdateStrategy;
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use procgena_stone::{DetailLevel, StoneDetail, StoneOptions, TextureSize};

use super::pools::RenderPools;
use super::views::PoolViews;
use crate::{PooledStone, StonePool, ZeStonePlugin};

/// Frames to wait for generation, pipeline compilation and culling.
const MAX_FRAMES: usize = 600;
/// Words per indexed indirect draw, and the instance count's word within one.
const DRAW_WORDS: usize = 5;
const INSTANCE_COUNT_WORD: usize = 1;
const INDEX_COUNT_WORD: usize = 0;

/// Copies a buffer to the CPU as `u32` words, waiting for the GPU.
fn read_words(device: &RenderDevice, queue: &RenderQueue, source: &Buffer) -> Vec<u32> {
    let size = source.size();
    let staging = device.create_buffer(&BufferDescriptor {
        label: Some("ze_stone_test_staging"),
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

/// Two detail levels for a rock of the default size: 24 cells per axis up to
/// 20 units from the camera, 12 cells beyond, fading over 18 to 22 units.
fn two_levels() -> StoneDetail {
    StoneDetail::new(
        [(0.0, 24), (20.0, 12)]
            .map(|(distance, resolution)| DetailLevel {
                resolution,
                distance,
                hysteresis: 0.1,
                texture_size: TextureSize::Size512,
            })
            .to_vec(),
    )
}

/// A headless app rendering to an image, with a camera at the origin looking
/// down the negative z axis, a directional light shining forward and down, and
/// one pooled rock of each variant at `positions`.
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
    .add_plugins(ZeStonePlugin {
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
    options.mesh.detail = two_levels();
    let pool = StonePool::new(options, positions.len() as u32);
    let pool = world.resource_mut::<Assets<StonePool>>().add(pool);
    world.spawn((
        Camera3d::default(),
        RenderTarget::Image(target.into()),
        Transform::from_xyz(0.0, 1.5, 0.0).looking_at(Vec3::new(0.0, 0.0, -10.0), Vec3::Y),
    ));
    world.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(0.0, 10.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
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

/// Runs frames until pools have no pending generation or readback and the
/// camera has checked visibility afterward. Panics after `MAX_FRAMES`.
/// Waiting allows visibility checks to use the generated enclosing radius.
fn run_until_culled(app: &mut App) {
    for _ in 0..MAX_FRAMES {
        app.update();
        let world = app.sub_app_mut(RenderApp).world_mut();
        let pools = world.resource::<RenderPools>();
        if pools.pools.is_empty() || !pools.pools.values().all(|p| p.status_read()) {
            continue;
        }
        // Give visibility checks a frame to use the updated enclosing radius.
        app.update();
        let world = app.sub_app_mut(RenderApp).world_mut();
        let camera = camera_view(world);
        let views = world.resource::<PoolViews>();
        if views
            .views
            .iter()
            .any(|((view, _), state)| Some(*view) == camera && state.culled)
        {
            return;
        }
    }
    panic!("the camera never culled the pool");
}

/// The camera's view in the render world.
fn camera_view(world: &mut World) -> Option<RetainedViewEntity> {
    let mut cameras = world.query_filtered::<&ExtractedView, With<Camera3d>>();
    cameras
        .iter(world)
        .next()
        .map(|view| view.retained_view_entity)
}

/// The draw arguments of every view that culled the pool, the camera's first.
fn view_draw_args(app: &mut App) -> Vec<(bool, Vec<u32>)> {
    let world = app.sub_app_mut(RenderApp).world_mut();
    let camera = camera_view(world);
    let device = world.resource::<RenderDevice>().clone();
    let queue = world.resource::<RenderQueue>().clone();
    let views = world.resource::<PoolViews>();
    let mut out: Vec<(bool, Vec<u32>)> = views
        .views
        .iter()
        .filter(|(_, state)| state.culled)
        .map(|((view, _), state)| {
            (
                Some(*view) == camera,
                read_words(&device, &queue, &state.draw_args),
            )
        })
        .collect();
    out.sort_by_key(|(is_camera, _)| !is_camera);
    out
}

/// The instance count of the draw of `variant` at detail level `lod`.
fn count(args: &[u32], variant: usize, lod: usize) -> u32 {
    args[(variant * 2 + lod) * DRAW_WORDS + INSTANCE_COUNT_WORD]
}

#[test]
#[ignore = "needs a GPU"]
fn gpu_views_draw_the_rocks_they_see_at_the_right_levels() {
    // Rocks 0 to 2 are ahead of the camera: near, far, and in the fade band
    // between the two levels (about 20 units away). Rock 3 is behind the
    // camera. Rock 4 is behind and above the camera, where the light casts its
    // shadow into view.
    let positions = [
        Vec3::new(0.0, 0.0, -8.0),
        Vec3::new(0.0, 0.0, -40.0),
        Vec3::new(0.0, 0.0, -(20.0f32.powi(2) - 1.5f32.powi(2)).sqrt()),
        Vec3::new(0.0, 0.0, 8.0),
        Vec3::new(0.0, 8.0, 4.0),
    ];
    let mut app = headless_app(&positions);
    run_until_culled(&mut app);
    let views = view_draw_args(&mut app);
    let (true, camera) = &views[0] else {
        panic!("the camera culled nothing");
    };

    // The camera draws each visible rock at its level, both levels in the band.
    let expected = [[1, 0], [0, 1], [1, 1], [0, 0], [0, 0]];
    for (variant, levels) in expected.iter().enumerate() {
        for (lod, &want) in levels.iter().enumerate() {
            assert_eq!(
                count(camera, variant, lod),
                want,
                "camera: variant {variant}, level {lod}"
            );
        }
    }

    // Shadow views draw one level per rock, and some cascade draws the rock
    // whose shadow falls into view although the camera cannot see it.
    let shadows = &views[1..];
    assert!(!shadows.is_empty(), "no shadow view culled the pool");
    for (_, args) in shadows {
        for variant in 0..positions.len() {
            assert!(
                count(args, variant, 0) + count(args, variant, 1) <= 1,
                "a shadow view drew variant {variant} twice"
            );
        }
    }
    assert!(
        shadows.iter().any(|(_, args)| count(args, 4, 0) == 1),
        "no shadow view drew the rock behind the camera"
    );
}

#[test]
#[ignore = "needs a GPU"]
fn gpu_pool_regenerates_meshes_that_overflowed() {
    let mut app = headless_app(&[Vec3::new(0.0, 0.0, -8.0)]);
    run_until_culled(&mut app);
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
    run_until_culled(&mut app);

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

/// The draw arguments of every shadow view that culled the pool, by light kind.
fn shadow_draw_args(app: &mut App) -> Vec<(&'static str, Vec<u32>)> {
    let world = app.sub_app_mut(RenderApp).world_mut();
    let mut lights = world.query::<(&ExtractedView, &LightEntity)>();
    let kinds: HashMap<RetainedViewEntity, &'static str> = lights
        .iter(world)
        .map(|(view, light)| {
            let kind = match light {
                LightEntity::Directional { .. } => "directional",
                LightEntity::Point { .. } => "point",
                LightEntity::Spot { .. } => "spot",
            };
            (view.retained_view_entity, kind)
        })
        .collect();
    let device = world.resource::<RenderDevice>().clone();
    let queue = world.resource::<RenderQueue>().clone();
    world
        .resource::<PoolViews>()
        .views
        .iter()
        .filter(|(_, state)| state.culled)
        .filter_map(|((view, _), state)| {
            let kind = kinds.get(view)?;
            Some((*kind, read_words(&device, &queue, &state.draw_args)))
        })
        .collect()
}

#[test]
#[ignore = "needs a GPU"]
fn gpu_point_and_spot_lights_cull_their_own_shadow_views() {
    let mut app = headless_app(&[Vec3::new(0.0, 0.0, -8.0)]);
    app.world_mut().spawn((
        PointLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(0.0, 4.0, -8.0),
    ));
    app.world_mut().spawn((
        SpotLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(3.0, 5.0, -4.0).looking_at(Vec3::new(0.0, 0.0, -8.0), Vec3::Y),
    ));
    run_until_culled(&mut app);
    app.update();

    // The point light's six cube faces and the spot light each cull the pool;
    // the rock sits below the point light and in the spot light's beam.
    let shadows = shadow_draw_args(&mut app);
    let views = |kind| {
        shadows
            .iter()
            .filter(|(k, _)| *k == kind)
            .collect::<Vec<_>>()
    };
    let draws = |args: &[u32]| count(args, 0, 0) + count(args, 0, 1);
    assert_eq!(views("point").len(), 6, "point light cube faces");
    assert_eq!(views("spot").len(), 1, "spot light views");
    assert!(views("point").iter().any(|(_, args)| draws(args) == 1));
    assert!(views("spot").iter().all(|(_, args)| draws(args) == 1));
}

#[test]
#[ignore = "needs a GPU"]
fn gpu_pool_regenerates_when_its_asset_changes() {
    let mut app = headless_app(&[Vec3::new(0.0, 0.0, -8.0)]);
    run_until_culled(&mut app);
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
    pool.options.seed += 100;
    app.update();
    run_until_culled(&mut app);
    assert_eq!(seed(&mut app), (first + 100, 1));
}
