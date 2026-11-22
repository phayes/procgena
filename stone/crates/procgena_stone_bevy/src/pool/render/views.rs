//! Per-view culling: every camera, and every shadow view of the lights it sees,
//! chooses each frame which pooled rocks it draws and at which detail level.
//!
//! Shadow views cull with their own frustum, so rocks outside the camera's view
//! still cast shadows into it, but choose detail levels by distance to the
//! camera, so a rock's shadow matches its drawn shape.

use std::collections::{HashMap, HashSet};

use bevy::asset::AssetId;
use bevy::camera::Camera3d;
use bevy::ecs::prelude::*;
use bevy::math::{Mat4, Vec3, Vec4};
use bevy::pbr::{LightEntity, ViewLightEntities};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, Buffer, BufferDescriptor, BufferUsages, CachedComputePipelineId,
    ComputePassDescriptor, ComputePipelineDescriptor, PipelineCache, ShaderType, UniformBuffer,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue};
use bevy::render::view::{ExtractedView, RetainedViewEntity};

use super::pools::{RenderPool, RenderPools, draw_args_offset};
use super::{PoolLayouts, PoolShaders};
use crate::pool::{MAX_POOL_LEVELS, StonePool};

/// One view's culling settings for one pool. Mirrors `CullView` in `pool_cull.wesl`.
#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub(crate) struct CullUniform {
    planes: [Vec4; 6],
    lod_origin: Vec4,
    lod_ranges: [Vec4; MAX_POOL_LEVELS],
    instance_count: u32,
    variant_count: u32,
    lod_count: u32,
    shadow: u32,
    skip_near: u32,
    radius: f32,
}

/// Bytes per entry of a view's visible-rock list: a rock index and a dither level.
pub(crate) const VISIBLE_ENTRY_SIZE: u64 = 8;
/// Threads per workgroup in `pool_cull.wesl`.
const WORKGROUP_SIZE: u32 = 64;

/// Every view's culling state, by view and pool.
#[derive(Resource, Default)]
pub(crate) struct PoolViews {
    pub views: HashMap<(RetainedViewEntity, AssetId<StonePool>), PoolView>,
}

/// One view's culling of one pool: the draws it makes and the rocks they read.
pub(crate) struct PoolView {
    uniform: UniformBuffer<CullUniform>,
    /// One indexed indirect draw per pool mesh, with this view's instance counts.
    pub draw_args: Buffer,
    /// The rocks each draw reads, as instance-rate vertex data.
    pub visible: Buffer,
    mesh_capacity: u32,
    visible_capacity: u32,
    cull_group: Option<BindGroup>,
    /// Updated for this frame by `prepare_views`.
    seen: bool,
    /// Culled this frame, so its draw arguments are current.
    pub culled: bool,
}

impl PoolView {
    fn new(device: &RenderDevice, meshes: u32, entries: u32) -> Self {
        let buffer = |label: &'static str, size: u64, usage: BufferUsages| {
            device.create_buffer(&BufferDescriptor {
                label: Some(label),
                size: size.max(4),
                usage,
                mapped_at_creation: false,
            })
        };
        Self {
            uniform: UniformBuffer::default(),
            draw_args: buffer(
                "ze_stone_pool_view_draw_args",
                draw_args_offset(meshes),
                // The GPU tests copy the arguments out.
                BufferUsages::STORAGE
                    | BufferUsages::INDIRECT
                    | BufferUsages::COPY_DST
                    | BufferUsages::COPY_SRC,
            ),
            visible: buffer(
                "ze_stone_pool_visible",
                u64::from(entries) * VISIBLE_ENTRY_SIZE,
                BufferUsages::STORAGE | BufferUsages::VERTEX,
            ),
            mesh_capacity: meshes,
            visible_capacity: entries,
            cull_group: None,
            seen: false,
            culled: false,
        }
    }
}

/// How one view culls and picks detail levels.
#[derive(Clone, Copy)]
struct CullSpec {
    clip_from_world: Mat4,
    /// Where detail levels are measured from: the camera.
    lod_origin: Vec3,
    shadow: bool,
    /// Keep rocks between a directional light and its shadow cascade.
    skip_near: bool,
}

impl CullSpec {
    fn new(view: &ExtractedView, lod_origin: Vec3, shadow: bool, skip_near: bool) -> Self {
        Self {
            clip_from_world: view.clip_from_world.unwrap_or_else(|| {
                view.clip_from_view * view.world_from_view.to_matrix().inverse()
            }),
            lod_origin,
            shadow,
            skip_near,
        }
    }
}

/// Inward-facing, normalized frustum planes of a clip-from-world matrix:
/// left, right, bottom, top, near, far. Bevy uses reversed depth, so clip `z`
/// runs from `w` (near) down to 0 (far); an infinite far plane comes out
/// degenerate and is replaced by one that passes everything.
fn frustum_planes(clip_from_world: &Mat4) -> [Vec4; 6] {
    let row = |i| clip_from_world.row(i);
    let raw = [
        row(3) + row(0),
        row(3) - row(0),
        row(3) + row(1),
        row(3) - row(1),
        row(3) - row(2),
        row(2),
    ];
    raw.map(|plane| {
        let length = plane.truncate().length();
        if length < 1e-6 {
            Vec4::W
        } else {
            plane / length
        }
    })
}

/// Creates, resizes and updates the culling state of every camera and every
/// shadow view, for every drawable pool. Drops state for views and pools that
/// are gone.
pub(crate) fn prepare_views(
    mut views: ResMut<PoolViews>,
    pools: Res<RenderPools>,
    cameras: Query<(&ExtractedView, Option<&ViewLightEntities>), With<Camera3d>>,
    light_views: Query<(&ExtractedView, &LightEntity)>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    for view in views.views.values_mut() {
        view.seen = false;
        view.culled = false;
    }
    let mut done = HashSet::new();
    let mut update = |retained: RetainedViewEntity, spec: CullSpec| {
        // A light view is culled once, for the first camera that reaches it.
        if !done.insert(retained) {
            return;
        }
        for (id, pool) in &pools.pools {
            if pool.batches.is_empty() || pool.instance_count == 0 {
                continue;
            }
            update_view(
                &mut views.views,
                (retained, *id),
                pool,
                spec,
                &device,
                &queue,
            );
        }
    };
    // Each camera, and the directional shadow cascades Bevy lists for it.
    let mut first_camera = None;
    for (camera, lights) in &cameras {
        let origin = camera.world_from_view.translation();
        first_camera.get_or_insert(origin);
        update(
            camera.retained_view_entity,
            CullSpec::new(camera, origin, false, false),
        );
        for &light in lights.map_or(&[][..], |l| l.lights.as_slice()) {
            if let Ok((view, kind)) = light_views.get(light) {
                let directional = matches!(kind, LightEntity::Directional { .. });
                update(
                    view.retained_view_entity,
                    CullSpec::new(view, origin, true, directional),
                );
            }
        }
    }

    // Point and spot shadow views, which all cameras share, choose detail
    // levels from the first camera.
    if let Some(origin) = first_camera {
        for (view, kind) in &light_views {
            let directional = matches!(kind, LightEntity::Directional { .. });
            update(
                view.retained_view_entity,
                CullSpec::new(view, origin, true, directional),
            );
        }
    }
    views.views.retain(|_, view| view.seen);
}

/// Makes sure `key`'s state has room for `pool`, and writes its culling settings.
fn update_view(
    views: &mut HashMap<(RetainedViewEntity, AssetId<StonePool>), PoolView>,
    key: (RetainedViewEntity, AssetId<StonePool>),
    pool: &RenderPool,
    spec: CullSpec,
    device: &RenderDevice,
    queue: &RenderQueue,
) {
    let meshes = pool.mesh_count();
    let entries = pool.instance_count * pool.lod_count;
    let view = views
        .entry(key)
        .and_modify(|view| {
            if view.mesh_capacity < meshes || view.visible_capacity < entries {
                *view = PoolView::new(device, meshes, entries);
            }
        })
        .or_insert_with(|| PoolView::new(device, meshes, entries));

    let mut lod_ranges = [Vec4::ZERO; MAX_POOL_LEVELS];
    lod_ranges[..pool.lod_ranges.len()].copy_from_slice(&pool.lod_ranges);
    view.uniform.set(CullUniform {
        planes: frustum_planes(&spec.clip_from_world),
        lod_origin: spec.lod_origin.extend(1.0),
        lod_ranges,
        instance_count: pool.instance_count,
        variant_count: pool.variant_count,
        lod_count: pool.lod_count,
        shadow: u32::from(spec.shadow),
        skip_near: u32::from(spec.skip_near),
        radius: pool.radius,
    });
    view.uniform.write_buffer(device, queue);
    view.seen = true;
}

/// Builds each view's culling bind group.
pub(crate) fn prepare_view_bind_groups(
    mut views: ResMut<PoolViews>,
    pools: Res<RenderPools>,
    layouts: Res<PoolLayouts>,
    pipeline_cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
) {
    for ((_, id), view) in &mut views.views {
        view.cull_group = None;
        let Some(pool) = pools.pools.get(id) else {
            continue;
        };
        let (Some(uniform), Some(instances), Some(starts)) = (
            view.uniform.binding(),
            pool.instances.as_ref().and_then(|b| b.binding()),
            pool.variant_start.binding(),
        ) else {
            continue;
        };
        view.cull_group = Some(device.create_bind_group(
            "ze_stone_pool_cull",
            &pipeline_cache.get_bind_group_layout(&layouts.cull),
            &BindGroupEntries::sequential((
                uniform,
                instances,
                starts,
                view.draw_args.as_entire_binding(),
                view.visible.as_entire_binding(),
            )),
        ));
    }
}

/// The culling compute pipelines.
#[derive(Resource, Debug)]
pub(crate) struct PoolCullPipelines {
    reset_counts: CachedComputePipelineId,
    cull: CachedComputePipelineId,
}

pub(crate) fn init_cull_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    layouts: Res<PoolLayouts>,
    shaders: Res<PoolShaders>,
) {
    let queue = |entry: &'static str| {
        pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some(format!("ze_stone_pool_{entry}").into()),
            layout: vec![layouts.cull.clone()],
            shader: shaders.cull.clone(),
            entry_point: Some(entry.into()),
            ..Default::default()
        })
    };
    commands.insert_resource(PoolCullPipelines {
        reset_counts: queue("reset_counts"),
        cull: queue("cull"),
    });
}

/// Records every view's culling: copies the pool's draws into the view's, then
/// counts and lists the rocks each draw shows.
pub(crate) fn run_cull(
    mut ctx: RenderContext,
    pipelines: Res<PoolCullPipelines>,
    pipeline_cache: Res<PipelineCache>,
    pools: Res<RenderPools>,
    mut views: ResMut<PoolViews>,
) {
    let (Some(reset_counts), Some(cull)) = (
        pipeline_cache.get_compute_pipeline(pipelines.reset_counts),
        pipeline_cache.get_compute_pipeline(pipelines.cull),
    ) else {
        return;
    };
    for ((_, id), view) in &mut views.views {
        let (Some(pool), Some(group)) = (pools.pools.get(id), view.cull_group.as_ref()) else {
            continue;
        };
        let encoder = ctx.command_encoder();
        for batch in &pool.batches {
            encoder.copy_buffer_to_buffer(
                &batch.draw_args,
                0,
                &view.draw_args,
                draw_args_offset(batch.first_mesh),
                draw_args_offset(batch.mesh_count),
            );
        }
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("ze_stone_pool_cull"),
            ..Default::default()
        });
        pass.set_bind_group(0, group, &[]);
        pass.set_pipeline(reset_counts);
        pass.dispatch_workgroups(pool.mesh_count().div_ceil(WORKGROUP_SIZE), 1, 1);
        pass.set_pipeline(cull);
        pass.dispatch_workgroups(pool.instance_count.div_ceil(WORKGROUP_SIZE), 1, 1);
        drop(pass);
        view.culled = true;
    }
}
