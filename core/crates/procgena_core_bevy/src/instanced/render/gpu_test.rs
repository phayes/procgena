//! The instanced renderer on a real GPU.
//!
//! Renders headless frames and reads each view's draws back to check which
//! copies it draws at which step, and that the instanced pipelines build.
//!
//! Slots use [`DistanceChain::DEFAULT`], and copies are sized so that their
//! lower mesh takes over at round distances (see [`size_switching_at`]).
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
use procgena_core::chain::{DistanceChain, Step, ViewScale};
use procgena_core::{GeneratorKind, OptionsHash, PoolId, SpecimenId};

use super::*;
use crate::instanced::{
    InstancedCopy, InstancedPart, InstancedPlugin, InstancedScatter, InstancedSlot,
    InstancedStandardMaterial, PartMesh,
};

/// The size of a copy whose lower mesh takes over `distance` metres from a
/// default camera at High quality, under [`DistanceChain::DEFAULT`]. Its
/// impostor then takes over 2.5 times as far, and it is culled 50 times as
/// far.
fn size_switching_at(distance: f32) -> f32 {
    distance * DistanceChain::DEFAULT.lower_below / ViewScale::DEFAULT.get()
}

/// Parts at each of `steps`, all drawing `mesh` with `material`.
fn parts(
    steps: &[Step],
    mesh: &Handle<Mesh>,
    material: &Handle<InstancedStandardMaterial>,
) -> Vec<InstancedPart> {
    steps
        .iter()
        .map(|&step| InstancedPart {
            step,
            mesh: PartMesh::Mesh(mesh.clone()),
            material: material.clone().untyped(),
        })
        .collect()
}

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
/// there. Every slot has its game level up to 20 m and its lower mesh
/// beyond, fading over 18 to 22 m. With `impostor`, slots also have an
/// impostor part, taking over at 50 m (45 to 55 m).
fn headless_app(positions: &[Vec3], impostor: bool) -> App {
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
    let steps: &[Step] = if impostor {
        &Step::ALL
    } else {
        &[Step::Game, Step::Lower]
    };
    let kind = GeneratorKind::from_key("procgena_core_bevy.test.kind");
    let pool = PoolId::new(kind, OptionsHash(1));
    let slots: Vec<InstancedSlot> = (0..positions.len() as u32)
        .map(|slot| InstancedSlot {
            specimen: SpecimenId::new(0, pool, slot),
            piece: 0,
            chain: DistanceChain::DEFAULT,
            cull: true,
            impostor,
            parts: parts(steps, &mesh, &material),
            bounds: (Vec3::ZERO, 0.9),
        })
        .collect();
    let copies: Vec<InstancedCopy> = positions
        .iter()
        .enumerate()
        .map(|(slot, &position)| InstancedCopy {
            slot: slot as u32,
            transform: Affine3A::from_translation(position),
            size: size_switching_at(20.0),
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

/// The instance count of slot `slot`'s part at `level`, the index of its
/// step (parts are listed by slot, then step, two per slot; a slot with an
/// impostor part must be the last).
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
    let mut app = headless_app(&positions, false);
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
fn gpu_impostors_cast_no_shadows() {
    // A far copy, drawn as its impostor, which casts no shadows.
    let mut app = headless_app(&[Vec3::new(0.0, 0.0, -80.0)], true);
    run_until_drawn(&mut app);
    let camera = camera_view(app.sub_app_mut(RenderApp).world_mut()).expect("a camera");
    let mut views = view_draw_args(&mut app);
    let camera_args = views.remove(&camera).expect("the camera culled nothing");
    assert_eq!(count(&camera_args, 0, Step::Impostor.index()), 1);
    for args in views.values() {
        let drawn: u32 = (0..3).map(|level| count(args, 0, level)).sum();
        assert_eq!(drawn, 0);
    }
}

#[test]
#[ignore = "needs a GPU"]
fn gpu_narrow_lenses_keep_detail_further_away() {
    // At 30 m the copy is past its 20 m switch through the default lens,
    // but a lens a third as wide makes it look three times larger, so it
    // keeps its game level (switching at 60 m).
    let mut app = headless_app(&[Vec3::new(0.0, 0.0, -30.0)], false);
    let world = app.world_mut();
    let mut cameras = world.query_filtered::<&mut Projection, With<Camera3d>>();
    let mut projection = cameras.single_mut(world).expect("one camera");
    let Projection::Perspective(perspective) = &mut *projection else {
        panic!("the test camera is perspective");
    };
    let narrow = 2.0 * ((perspective.fov * 0.5).tan() / 3.0).atan();
    perspective.fov = narrow;
    run_until_drawn(&mut app);
    let camera = camera_view(app.sub_app_mut(RenderApp).world_mut()).expect("a camera");
    let args = view_draw_args(&mut app).remove(&camera).expect("culled");
    assert_eq!((count(&args, 0, 0), count(&args, 0, 1)), (1, 0));
}

#[test]
#[ignore = "needs a GPU"]
fn gpu_point_and_spot_lights_cull_their_own_shadow_views() {
    let mut app = headless_app(&[Vec3::new(0.0, 0.0, -8.0)], false);
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
    let mut app = headless_app(&[Vec3::new(0.0, 0.0, -15.0)], false);
    app.insert_resource(crate::LodQuality(procgena_core::Quality::Low));
    run_until_drawn(&mut app);
    let camera = camera_view(app.sub_app_mut(RenderApp).world_mut()).expect("a camera");
    let args = view_draw_args(&mut app).remove(&camera).expect("culled");
    assert_eq!((count(&args, 0, 0), count(&args, 0, 1)), (0, 1));
}

/// The rows of `attached_app`'s host slot: two that every host step draws,
/// and one only the host's game level draws. Each switches to its lower
/// mesh at 10 m.
fn attached_rows() -> Vec<AttachedRow> {
    let row = |position: Vec3, host_steps: u32| AttachedRow {
        slot: 1,
        position,
        rotation: Quat::from_rotation_y(0.5),
        scale: 0.5,
        size: size_switching_at(10.0),
        wind: Vec3::new(0.1, 0.2, 0.3),
        host_steps,
    };
    vec![
        row(Vec3::new(0.5, 0.5, 0.0), AttachedRow::ALL_STEPS),
        row(Vec3::new(-0.5, 0.5, 0.0), AttachedRow::ALL_STEPS),
        row(Vec3::new(0.0, 1.0, 0.0), AttachedRow::steps([Step::Game])),
    ]
}

/// A headless app like `headless_app`, whose camera also has a depth
/// prepass, with one Scatter of two slots: slot 0, a host with the steps
/// of `headless_app`, copied at `hosts`; and slot 1, drawn only by the rows
/// of [`attached_rows`] on each host copy, at its game level up to 10 m and
/// its lower mesh beyond, fading over 9 to 11 m, and culled from
/// `row_cull` metres if given (500 m otherwise).
fn attached_app(hosts: &[Vec3], row_cull: Option<f32>) -> App {
    let mut app = headless_app(&[], false);
    let world = app.world_mut();
    let mesh = world
        .resource_mut::<Assets<Mesh>>()
        .add(Cuboid::from_length(0.4));
    let material = world
        .resource_mut::<Assets<InstancedStandardMaterial>>()
        .add(InstancedStandardMaterial::default());
    let kind = GeneratorKind::from_key("procgena_core_bevy.test.attached");
    let pool = PoolId::new(kind, OptionsHash(2));
    let slot = |index: u32, chain: DistanceChain| InstancedSlot {
        specimen: SpecimenId::new(0, pool, index),
        piece: 0,
        chain,
        cull: true,
        impostor: false,
        parts: parts(&[Step::Game, Step::Lower], &mesh, &material),
        bounds: (Vec3::ZERO, 0.4),
    };
    let mut row_chain = DistanceChain::DEFAULT;
    if let Some(cull) = row_cull {
        // Rows are sized to switch at 10 m, so a threshold `t` falls at
        // 10 × lower_below / t metres. The impostor's threshold sits
        // halfway, so the bands stay apart.
        row_chain.cull_below = 10.0 * row_chain.lower_below / cull;
        row_chain.impostor_below = (row_chain.lower_below * row_chain.cull_below).sqrt();
    }
    let copies: Vec<InstancedCopy> = hosts
        .iter()
        .map(|&position| InstancedCopy {
            slot: 0,
            transform: Affine3A::from_translation(position),
            size: size_switching_at(20.0),
            wind_phase: 0.0,
        })
        .collect();
    world.spawn((
        InstancedScatter {
            slots: vec![slot(0, DistanceChain::DEFAULT), slot(1, row_chain)].into(),
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

    // The host step dropped it, so no row was looked at.
    let world = app.sub_app_mut(RenderApp).world_mut();
    let state = camera_buffer(world, camera, |buffers| &buffers.host_state);
    assert_eq!(state[0], 0, "skipped");
}

/// Reads back the camera's `buffer` of its only Scatter.
fn camera_buffer(
    world: &mut World,
    camera: RetainedViewEntity,
    buffer: impl Fn(&ViewBuffers) -> &Buffer,
) -> Vec<u32> {
    let device = world.resource::<RenderDevice>().clone();
    let queue = world.resource::<RenderQueue>().clone();
    let views = world.resource::<ScatterViews>();
    let state = views
        .0
        .iter()
        .find(|((view, _), _)| *view == camera)
        .map(|(_, state)| state)
        .expect("the camera's state");
    read_words(
        &device,
        &queue,
        buffer(state.buffers.as_ref().expect("buffers")),
    )
}

#[test]
#[ignore = "needs a GPU"]
fn gpu_attached_rows_find_their_host_at_every_offset() {
    // Host slots with tables of 1, 0, 3 and 70 rows (the last crosses a
    // 64-thread workgroup), copied 2, 1, 2 and 1 times, all near the camera
    // and drawn at the rows' first level. Row threads start at 0, 1, 2, 5
    // and 8, so the host search meets uneven boundaries.
    let row = |i: usize| AttachedRow {
        slot: 4,
        position: Vec3::new((i % 10) as f32 * 0.05, 0.5 + (i / 10) as f32 * 0.05, 0.0),
        rotation: Quat::from_rotation_y(i as f32 * 0.1),
        scale: 0.2,
        size: size_switching_at(10.0),
        wind: Vec3::new(i as f32, 0.0, 0.0),
        host_steps: AttachedRow::ALL_STEPS,
    };
    let tables: Vec<Vec<AttachedRow>> = vec![
        vec![row(0)],
        Vec::new(),
        (1..4).map(row).collect(),
        (4..74).map(row).collect(),
    ];
    let hosts: [(u32, Vec3); 6] = [
        (0, Vec3::new(-2.0, 0.0, -5.0)),
        (0, Vec3::new(-1.0, 0.0, -5.0)),
        (1, Vec3::new(0.0, 0.0, -5.0)),
        (2, Vec3::new(1.0, 0.0, -5.0)),
        (2, Vec3::new(2.0, 0.0, -5.0)),
        (3, Vec3::new(0.0, 0.0, -6.0)),
    ];
    let mut app = multi_host_app(&hosts, &tables);
    run_until_drawn(&mut app);
    let camera = camera_view(app.sub_app_mut(RenderApp).world_mut()).expect("a camera");

    // Every row of every host once, at level 0. Copies keep their order
    // within a slot, so mesh records follow `hosts`.
    let mut expected: Vec<Entry> = hosts
        .iter()
        .enumerate()
        .flat_map(|(record, (slot, _))| {
            tables[*slot as usize]
                .iter()
                .map(move |row| expected_entry(record as u32, 0, row))
        })
        .collect();
    expected.sort();
    assert_eq!(expected.len(), 2 + 6 + 70);
    let world = app.sub_app_mut(RenderApp).world_mut();
    let words = camera_buffer(world, camera, |buffers| &buffers.attached_visible);
    let mut drawn: Vec<Entry> = (0..expected.len())
        .map(|i| words[i * 16..][..16].try_into().unwrap())
        .collect();
    drawn.sort();
    assert_eq!(drawn, expected);
}

/// A headless app like `headless_app`, with one Scatter: host slots 0 to 3
/// copied as `hosts` gives (slot, position), with `tables` as their rows,
/// and slot 4, which the rows draw. Hosts and rows sized as
/// `size_switching_at(10.0)` switch to their lower mesh at 10 m.
fn multi_host_app(hosts: &[(u32, Vec3)], tables: &[Vec<AttachedRow>]) -> App {
    let mut app = headless_app(&[], false);
    let world = app.world_mut();
    let mesh = world
        .resource_mut::<Assets<Mesh>>()
        .add(Cuboid::from_length(0.2));
    let material = world
        .resource_mut::<Assets<InstancedStandardMaterial>>()
        .add(InstancedStandardMaterial::default());
    let kind = GeneratorKind::from_key("procgena_core_bevy.test.hosts");
    let pool = PoolId::new(kind, OptionsHash(3));
    let slots: Vec<InstancedSlot> = (0..5)
        .map(|index| InstancedSlot {
            specimen: SpecimenId::new(0, pool, index),
            piece: 0,
            chain: DistanceChain::DEFAULT,
            cull: true,
            impostor: false,
            parts: parts(&[Step::Game, Step::Lower], &mesh, &material),
            bounds: (Vec3::ZERO, 0.2),
        })
        .collect();
    let copies: Vec<InstancedCopy> = hosts
        .iter()
        .map(|&(slot, position)| InstancedCopy {
            slot,
            transform: Affine3A::from_translation(position),
            size: size_switching_at(10.0),
            wind_phase: 0.0,
        })
        .collect();
    world.spawn((
        InstancedScatter {
            slots: slots.into(),
            copies: copies.into(),
        },
        InstancedAttached {
            tables: tables.iter().map(|t| Arc::from(t.clone())).collect(),
        },
    ));
    app
}

#[test]
#[ignore = "needs a GPU"]
fn gpu_timings_report_every_stage_when_supported() {
    let mut app = attached_app(&[Vec3::new(0.0, 0.0, -8.0)], None);
    run_until_drawn(&mut app);
    app.sub_app_mut(RenderApp)
        .world_mut()
        .init_resource::<InstancedTimings>();
    for _ in 0..10 {
        app.update();
    }
    let world = app.sub_app(RenderApp).world();
    let timings = world.resource::<InstancedTimings>();
    if !timings.is_supported() {
        assert_eq!(timings.last(), None);
        return;
    }
    let times = timings.last().expect("a frame's times read back");
    assert!(times.total() > std::time::Duration::ZERO, "{times:?}");

    // The counts match the camera and shadow views' draws.
    let counts = read_instance_counts(world);
    assert!(counts.views >= 2, "camera and shadow views");
    assert!(counts.attached >= 3, "the near host's rows");
    assert_eq!(
        counts.bytes(),
        counts.copies * INSTANCE_SIZE + counts.attached * ATTACHED_SIZE
    );
}
