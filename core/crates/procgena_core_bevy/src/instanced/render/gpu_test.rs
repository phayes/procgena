//! The instanced renderer on a real GPU.
//!
//! Renders headless frames and reads each view's draws back to check which
//! copies it draws at which level, and that the instanced pipelines build.
//! Needs a GPU, so the tests are ignored by default:
//!
//! ```text
//! cargo test -p procgena_core_bevy --lib -- --ignored gpu
//! ```

use std::collections::HashMap;
use std::time::Duration;

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::RenderApp;
use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;
use bevy::render::render_resource::{
    BufferDescriptor, BufferUsages, CommandEncoderDescriptor, MapMode, PollType, TextureFormat,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::time::TimeUpdateStrategy;
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use procgena_core::{GeneratorKind, Lod, LodLevel, OptionsHash, PoolId, SpecimenId};

use super::*;
use crate::instanced::{
    InstancedCopy, InstancedPart, InstancedPlugin, InstancedScatter, InstancedSlot,
    InstancedStandardMaterial, PartMesh,
};

/// Frames to wait for pipelines and culling.
const MAX_FRAMES: usize = 600;
/// Words per indexed indirect draw, and the instance count's word within one.
const DRAW_WORDS: usize = 5;
const INSTANCE_COUNT_WORD: usize = 1;

/// Copies a buffer to the CPU as `u32` words, waiting for the GPU.
fn read_words(device: &RenderDevice, queue: &RenderQueue, source: &Buffer) -> Vec<u32> {
    let size = source.size();
    let staging = device.create_buffer(&BufferDescriptor {
        label: Some("procgena_instanced_test_staging"),
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

/// A headless app rendering to an image, with a camera at (0, 1.5, 0)
/// looking down the negative z axis, a directional light shining forward
/// and down, and one Scatter with a slot per position, each with one copy
/// there. Every slot has two levels: up to 20 m, and beyond, fading over
/// 18 to 22 m; the second level casts no shadows when `far_shadows` is
/// false.
fn headless_app(positions: &[Vec3], far_shadows: bool) -> App {
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
    .add_plugins(InstancedPlugin)
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
    let mesh = world
        .resource_mut::<Assets<Mesh>>()
        .add(Cuboid::from_length(1.0));
    let material = world
        .resource_mut::<Assets<InstancedStandardMaterial>>()
        .add(InstancedStandardMaterial::default());
    let mut far = LodLevel::new(20.0, 0.1, ());
    far.casts_shadows = far_shadows;
    let lod = Lod::new(1.0, vec![LodLevel::new(0.0, 0.1, ()), far]);
    let kind = GeneratorKind::from_key("procgena_core_bevy.test.kind");
    let pool = PoolId::new(kind, OptionsHash(1));
    let slots: Vec<InstancedSlot> = (0..positions.len() as u32)
        .map(|slot| InstancedSlot {
            specimen: SpecimenId::new(0, pool, slot),
            piece: 0,
            lod: lod.clone(),
            parts: (0..2)
                .map(|level| InstancedPart {
                    level,
                    mesh: PartMesh::Mesh(mesh.clone()),
                    material: material.clone().untyped(),
                })
                .collect(),
            bounds: (Vec3::ZERO, 0.9),
        })
        .collect();
    let copies: Vec<InstancedCopy> = positions
        .iter()
        .enumerate()
        .map(|(slot, &position)| InstancedCopy {
            slot: slot as u32,
            transform: Affine3A::from_translation(position),
            size: 1.0,
            wind_phase: 0.0,
        })
        .collect();
    world.spawn(InstancedScatter {
        slots: slots.into(),
        copies: copies.into(),
    });
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
    app.finish();
    app.cleanup();
    app
}

/// The camera's view in the render world.
fn camera_view(world: &mut World) -> Option<RetainedViewEntity> {
    let mut cameras = world.query_filtered::<&ExtractedView, With<Camera3d>>();
    cameras
        .iter(world)
        .next()
        .map(|view| view.retained_view_entity)
}

/// Runs frames until the camera's main-pass pipelines are compiled and it
/// has been culled. Panics after `MAX_FRAMES`.
fn run_until_drawn(app: &mut App) {
    for _ in 0..MAX_FRAMES {
        app.update();
        let world = app.sub_app_mut(RenderApp).world_mut();
        let camera = camera_view(world);
        let cache = world.resource::<PipelineCache>();
        let views = world.resource::<ScatterViews>();
        let ready = views.0.iter().any(|((view, _), state)| {
            Some(*view) == camera
                && state.cull_group.is_some()
                && !state.pipelines[pass::MAIN as usize].is_empty()
                && state.pipelines[pass::MAIN as usize]
                    .iter()
                    .all(|id| id.is_some_and(|id| cache.get_render_pipeline(id).is_some()))
        });
        if ready {
            // One more frame runs the cull with the pipelines in place.
            app.update();
            return;
        }
    }

    // Say which stage never finished.
    let world = app.sub_app_mut(RenderApp).world_mut();
    let camera = camera_view(world);
    let cache = world.resource::<PipelineCache>();
    let scatters = world.resource::<Scatters>();
    let views = world.resource::<ScatterViews>();
    let state: Vec<String> = views
        .0
        .iter()
        .filter(|((view, _), _)| Some(*view) == camera)
        .map(|(_, state)| {
            let main = &state.pipelines[pass::MAIN as usize];
            format!(
                "cull group: {}, main pipelines: {} of {} chosen, {} compiled",
                state.cull_group.is_some(),
                main.iter().flatten().count(),
                main.len(),
                main.iter()
                    .flatten()
                    .filter(|id| cache.get_render_pipeline(**id).is_some())
                    .count(),
            )
        })
        .collect();
    let meshes = world.resource::<RenderAssets<RenderMesh>>();
    let materials = world.resource::<ErasedRenderAssets<PreparedMaterial>>();
    let registered = world.resource::<InstancedMaterials>();
    let parts: Vec<String> = scatters
        .0
        .values()
        .flat_map(|gpu| &gpu.parts)
        .map(|part| {
            let MeshRef::Asset(mesh) = part.mesh else {
                return "external".to_owned();
            };
            format!(
                "mesh ready: {}, material ready: {}, registered: {}",
                meshes.get(mesh).is_some(),
                materials.get(part.material).is_some(),
                if part.attached {
                    registered.attached.contains_key(&part.type_id)
                } else {
                    registered.copies.contains_key(&part.type_id)
                },
            )
        })
        .collect();
    panic!(
        "the camera never drew the Scatter: {} Scatters, parts: {parts:?}, {} views, camera: {state:?}",
        scatters.0.len(),
        views.0.len(),
    );
}

/// The draws of every view that culled the Scatter, by view.
fn view_draw_args(app: &mut App) -> HashMap<RetainedViewEntity, Vec<u32>> {
    let world = app.sub_app_mut(RenderApp).world_mut();
    let device = world.resource::<RenderDevice>().clone();
    let queue = world.resource::<RenderQueue>().clone();
    world
        .resource::<ScatterViews>()
        .0
        .iter()
        .filter_map(|((view, _), state)| {
            let buffers = state.buffers.as_ref()?;
            state.cull_group.as_ref()?;
            Some((*view, read_words(&device, &queue, &buffers.args)))
        })
        .collect()
}

/// The light kind of each shadow view.
fn light_kinds(app: &mut App) -> HashMap<RetainedViewEntity, &'static str> {
    let world = app.sub_app_mut(RenderApp).world_mut();
    let mut lights = world.query::<(&ExtractedView, &LightEntity)>();
    lights
        .iter(world)
        .map(|(view, light)| {
            let kind = match light {
                LightEntity::Directional { .. } => "directional",
                LightEntity::Point { .. } => "point",
                LightEntity::Spot { .. } => "spot",
            };
            (view.retained_view_entity, kind)
        })
        .collect()
}

/// The instance count of slot `slot`'s part at `level` (parts are listed
/// by slot, then level).
fn count(args: &[u32], slot: usize, level: usize) -> u32 {
    args[(slot * 2 + level) * DRAW_WORDS + INSTANCE_COUNT_WORD]
}

#[test]
#[ignore = "needs a GPU"]
fn gpu_views_draw_the_copies_they_see_at_the_right_levels() {
    // Copies 0 to 2 are ahead of the camera: near, far, and in the fade band
    // between the two levels (20 m away). Copy 3 is behind the camera. Copy 4
    // is behind and above the camera, where the light casts its shadow into
    // view.
    let positions = [
        Vec3::new(0.0, 0.0, -8.0),
        Vec3::new(0.0, 0.0, -40.0),
        Vec3::new(0.0, 0.0, -(20.0f32.powi(2) - 1.5f32.powi(2)).sqrt()),
        Vec3::new(0.0, 0.0, 8.0),
        Vec3::new(0.0, 8.0, 4.0),
    ];
    let mut app = headless_app(&positions, true);
    run_until_drawn(&mut app);
    let camera = camera_view(app.sub_app_mut(RenderApp).world_mut()).expect("a camera");
    let mut views = view_draw_args(&mut app);
    let camera_args = views.remove(&camera).expect("the camera culled nothing");

    // The camera draws each visible copy at its level, both levels in the band.
    let expected = [[1, 0], [0, 1], [1, 1], [0, 0], [0, 0]];
    for (slot, levels) in expected.iter().enumerate() {
        for (level, &want) in levels.iter().enumerate() {
            assert_eq!(
                count(&camera_args, slot, level),
                want,
                "camera: slot {slot}, level {level}"
            );
        }
    }

    // Shadow views draw one level per copy, and some cascade draws the copy
    // whose shadow falls into view although the camera cannot see it.
    assert!(!views.is_empty(), "no shadow view culled the Scatter");
    for args in views.values() {
        for slot in 0..positions.len() {
            assert!(
                count(args, slot, 0) + count(args, slot, 1) <= 1,
                "a shadow view drew slot {slot} twice"
            );
        }
    }
    assert!(
        views.values().any(|args| count(args, 4, 0) == 1),
        "no shadow view drew the copy behind the camera"
    );
}

#[test]
#[ignore = "needs a GPU"]
fn gpu_levels_without_shadows_cast_none() {
    // A far copy, drawn at the second level, which casts no shadows.
    let mut app = headless_app(&[Vec3::new(0.0, 0.0, -40.0)], false);
    run_until_drawn(&mut app);
    let camera = camera_view(app.sub_app_mut(RenderApp).world_mut()).expect("a camera");
    let mut views = view_draw_args(&mut app);
    let camera_args = views.remove(&camera).expect("the camera culled nothing");
    assert_eq!(count(&camera_args, 0, 1), 1);
    for args in views.values() {
        assert_eq!(count(args, 0, 0) + count(args, 0, 1), 0);
    }
}

#[test]
#[ignore = "needs a GPU"]
fn gpu_point_and_spot_lights_cull_their_own_shadow_views() {
    let mut app = headless_app(&[Vec3::new(0.0, 0.0, -8.0)], true);
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
    run_until_drawn(&mut app);
    app.update();

    // The point light's six cube faces and the spot light each cull the
    // Scatter; the copy sits below the point light and in the spot light's
    // beam.
    let kinds = light_kinds(&mut app);
    let views = view_draw_args(&mut app);
    let of = |kind| {
        views
            .iter()
            .filter(|(view, _)| kinds.get(*view) == Some(&kind))
            .map(|(_, args)| args)
            .collect::<Vec<_>>()
    };
    let draws = |args: &[u32]| count(args, 0, 0) + count(args, 0, 1);
    assert_eq!(of("point").len(), 6, "point light cube faces");
    assert_eq!(of("spot").len(), 1, "spot light views");
    assert!(of("point").iter().any(|args| draws(args) == 1));
    assert!(of("spot").iter().all(|args| draws(args) == 1));
}

#[test]
#[ignore = "needs a GPU"]
fn gpu_quality_scales_the_switch_distance() {
    // At 15 m the copy is at the first level at High quality, and at the
    // second at Low (switching at 20 × 0.4 = 8 m).
    let mut app = headless_app(&[Vec3::new(0.0, 0.0, -15.0)], true);
    app.insert_resource(crate::LodQuality(procgena_core::Quality::Low));
    run_until_drawn(&mut app);
    let camera = camera_view(app.sub_app_mut(RenderApp).world_mut()).expect("a camera");
    let args = view_draw_args(&mut app).remove(&camera).expect("culled");
    assert_eq!((count(&args, 0, 0), count(&args, 0, 1)), (0, 1));
}

/// The rows of `attached_app`'s host slot: two that every host level draws,
/// and one only host level 0 draws.
fn attached_rows() -> Vec<AttachedRow> {
    let row = |position: Vec3, host_levels: u32| AttachedRow {
        slot: 1,
        position,
        rotation: Quat::from_rotation_y(0.5),
        scale: 0.5,
        size: 1.0,
        wind: Vec3::new(0.1, 0.2, 0.3),
        host_levels,
    };
    vec![
        row(Vec3::new(0.5, 0.5, 0.0), AttachedRow::ALL_LEVELS),
        row(Vec3::new(-0.5, 0.5, 0.0), AttachedRow::ALL_LEVELS),
        row(Vec3::new(0.0, 1.0, 0.0), 1),
    ]
}

/// A headless app like `headless_app`, whose camera also has a depth
/// prepass, with one Scatter of two slots: slot 0, a host with the levels
/// of `headless_app`, copied at `hosts`; and slot 1, drawn only by the rows
/// of [`attached_rows`] on each host copy, with levels up to 10 m and
/// beyond, fading over 9 to 11 m, and culled from `row_cull` if given.
fn attached_app(hosts: &[Vec3], row_cull: Option<f32>) -> App {
    let mut app = headless_app(&[], true);
    let world = app.world_mut();
    let mesh = world
        .resource_mut::<Assets<Mesh>>()
        .add(Cuboid::from_length(0.4));
    let material = world
        .resource_mut::<Assets<InstancedStandardMaterial>>()
        .add(InstancedStandardMaterial::default());
    let kind = GeneratorKind::from_key("procgena_core_bevy.test.attached");
    let pool = PoolId::new(kind, OptionsHash(2));
    let slot = |index: u32, lod: Lod| InstancedSlot {
        specimen: SpecimenId::new(0, pool, index),
        piece: 0,
        lod,
        parts: (0..2)
            .map(|level| InstancedPart {
                level,
                mesh: PartMesh::Mesh(mesh.clone()),
                material: material.clone().untyped(),
            })
            .collect(),
        bounds: (Vec3::ZERO, 0.4),
    };
    let host_lod = Lod::new(
        1.0,
        vec![LodLevel::new(0.0, 0.1, ()), LodLevel::new(20.0, 0.1, ())],
    );
    let mut row_lod = Lod::new(
        1.0,
        vec![LodLevel::new(0.0, 0.1, ()), LodLevel::new(10.0, 0.1, ())],
    );
    if let Some(cull) = row_cull {
        row_lod = row_lod.with_cull(cull);
    }
    let copies: Vec<InstancedCopy> = hosts
        .iter()
        .map(|&position| InstancedCopy {
            slot: 0,
            transform: Affine3A::from_translation(position),
            size: 1.0,
            wind_phase: 0.0,
        })
        .collect();
    world.spawn((
        InstancedScatter {
            slots: vec![slot(0, host_lod), slot(1, row_lod)].into(),
            copies: copies.into(),
        },
        InstancedAttached {
            tables: vec![Arc::from(attached_rows())].into(),
        },
    ));
    let mut cameras = world.query_filtered::<Entity, With<Camera3d>>();
    let camera = cameras.single(world).expect("one camera");
    world
        .entity_mut(camera)
        .insert(bevy::core_pipeline::prepass::DepthPrepass);
    app
}

/// Runs frames until some view of the Scatter has every pipeline it chose
/// in `pass` compiled, and has chosen one for every part. Panics after
/// `MAX_FRAMES`.
fn run_until_compiled(app: &mut App, pass: u8) {
    for _ in 0..MAX_FRAMES {
        app.update();
        let world = app.sub_app_mut(RenderApp).world_mut();
        let cache = world.resource::<PipelineCache>();
        let ready = world.resource::<ScatterViews>().0.values().any(|state| {
            let pipelines = &state.pipelines[pass as usize];
            !pipelines.is_empty()
                && pipelines
                    .iter()
                    .all(|id| id.is_some_and(|id| cache.get_render_pipeline(id).is_some()))
        });
        if ready {
            app.update();
            return;
        }
    }
    panic!("no view compiled every pipeline of pass {pass}");
}

/// One attached entry as the cull writes it: host record, dither, and the
/// row's place, turn and wind, as raw words.
type Entry = [u32; 16];

/// What the cull should write for row `row` of the host with mesh record
/// `record`, drawn with `dither`.
fn expected_entry(record: u32, dither: i32, row: &AttachedRow) -> Entry {
    let mut entry = [0; 16];
    entry[0] = record;
    entry[1] = dither as u32;
    let floats = [
        row.position.extend(row.scale),
        Vec4::from(row.rotation),
        row.wind.extend(0.0),
    ];
    for (i, value) in floats.iter().flat_map(|v| v.to_array()).enumerate() {
        entry[4 + i] = value.to_bits();
    }
    entry
}

#[test]
#[ignore = "needs a GPU"]
fn gpu_attached_rows_are_drawn_at_their_own_levels() {
    // Host 0 is 8 m ahead, at its level 0: its rows are tested one by one
    // (their distances straddle the rows' fade band) and all drawn at row
    // level 0. Host 1 is 40 m ahead, at its level 1: its rows all lie
    // inside row level 1, so they are drawn whole there, without the row
    // that only host level 0 draws. Host 2 is behind the camera.
    let hosts = [
        Vec3::new(0.0, 0.0, -8.0),
        Vec3::new(0.0, 0.0, -40.0),
        Vec3::new(0.0, 0.0, 8.0),
    ];
    let mut app = attached_app(&hosts, None);
    run_until_drawn(&mut app);
    run_until_compiled(&mut app, pass::PREPASS);
    run_until_compiled(&mut app, pass::SHADOW);
    let camera = camera_view(app.sub_app_mut(RenderApp).world_mut()).expect("a camera");
    let mut views = view_draw_args(&mut app);
    let camera_args = views.remove(&camera).expect("the camera culled nothing");

    // The hosts as before, and the rows by level.
    assert_eq!(count(&camera_args, 0, 0), 1, "near host");
    assert_eq!(count(&camera_args, 0, 1), 1, "far host");
    assert_eq!(count(&camera_args, 1, 0), 3, "rows at level 0");
    assert_eq!(count(&camera_args, 1, 1), 2, "rows at level 1");

    // The entries the camera's lists hold, against a CPU reference. Copies
    // keep their order, so host `i` has mesh record `i`.
    let rows = attached_rows();
    let world = app.sub_app_mut(RenderApp).world_mut();
    let device = world.resource::<RenderDevice>().clone();
    let queue = world.resource::<RenderQueue>().clone();
    let (words, host_state, attached_count) = {
        let scatters = world.resource::<Scatters>();
        let gpu = scatters.0.values().next().expect("a Scatter");
        let state = world
            .resource::<ScatterViews>()
            .0
            .iter()
            .find(|((view, _), _)| *view == camera)
            .map(|(_, state)| state)
            .expect("the camera's state");
        let buffers = state.buffers.as_ref().expect("buffers");
        (
            read_words(&device, &queue, &buffers.attached_visible),
            read_words(&device, &queue, &buffers.host_state),
            gpu.attached_count,
        )
    };
    assert_eq!(attached_count, 9, "three hosts of three rows");

    // The host step: the near host's rows one by one at host level 0, the
    // far host's whole at row level 1 and host level 1, the host behind
    // skipped. (Modes as in `instanced_attach.wesl`.)
    let (each_row, whole) = (1, 2);
    assert_eq!(
        host_state[..3],
        [each_row, (whole + 1) | (1 << 16), 0],
        "host states"
    );
    let entries = |level: u32, len: usize| {
        let mut list: Vec<Entry> = (0..len)
            .map(|i| {
                let at = (level * attached_count) as usize + i;
                words[at * 16..][..16].try_into().unwrap()
            })
            .collect();
        list.sort();
        list
    };
    let mut near: Vec<Entry> = rows.iter().map(|row| expected_entry(0, 0, row)).collect();
    near.sort();
    let mut far: Vec<Entry> = rows[..2]
        .iter()
        .map(|row| expected_entry(1, 0, row))
        .collect();
    far.sort();
    assert_eq!(entries(0, 3), near, "rows of the near host");
    assert_eq!(entries(1, 2), far, "rows of the far host");

    // Shadow views draw each row at one level at most, and some cascade
    // draws the near host's rows.
    for args in views.values() {
        assert!(count(args, 1, 0) + count(args, 1, 1) <= 9);
    }
    assert!(
        views.values().any(|args| count(args, 1, 0) == 3),
        "no shadow view drew the near host's rows"
    );
}

#[test]
#[ignore = "needs a GPU"]
fn gpu_attached_rows_beyond_their_reach_are_skipped() {
    // Rows are culled from 50 m, so a host 200 m away draws none of them,
    // while it still draws itself.
    let mut app = attached_app(&[Vec3::new(0.0, 0.0, -200.0)], Some(50.0));
    run_until_drawn(&mut app);
    let camera = camera_view(app.sub_app_mut(RenderApp).world_mut()).expect("a camera");
    let args = view_draw_args(&mut app).remove(&camera).expect("culled");
    assert_eq!(count(&args, 0, 1), 1, "the host");
    assert_eq!(count(&args, 1, 0) + count(&args, 1, 1), 0, "its rows");
}
