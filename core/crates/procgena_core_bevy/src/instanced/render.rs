//! The render world of the instanced renderer.
//!
//! Each frame the render world:
//!
//! 1. copies every visible [`InstancedScatter`] ([`extract_scatters`]);
//! 2. keeps, per Scatter, one Bevy mesh record (`MeshUniform`) per copy,
//!    which material shaders read through the mesh bind group, plus the
//!    culling inputs and each part's template draw ([`prepare_scatters`]);
//! 3. asks each part's material for its instanced pipeline in every pass a
//!    view needs ([`specialize_scatters`]), and queues one phase item per
//!    Scatter and phase ([`queue_scatters`]);
//! 4. culls every Scatter for every camera and shadow view on the GPU,
//!    filling the view's per-part indirect draws ([`run_cull`]);
//! 5. draws each part once, with one instance per visible copy
//!    ([`DrawParts`]).

use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::app::App;
use bevy::asset::{AssetId, UntypedAssetId};
use bevy::asset::{AssetServer, Handle};
use bevy::camera::Camera3d;
use bevy::camera::visibility::RenderLayers;
use bevy::core_pipeline::core_3d::{AlphaMask3d, Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey};
use bevy::core_pipeline::deferred::{AlphaMask3dDeferred, Opaque3dDeferred};
use bevy::core_pipeline::prepass::{
    AlphaMask3dPrepass, DeferredPrepass, Opaque3dPrepass, OpaqueNoLightmap3dBatchSetKey,
    OpaqueNoLightmap3dBinKey,
};
use bevy::core_pipeline::schedule::camera_driver;
use bevy::ecs::prelude::*;
use bevy::ecs::query::ROQueryItem;
use bevy::ecs::system::lifetimeless::{Read, SRes};
use bevy::ecs::system::{SystemParam, SystemParamItem};
use bevy::log::{error, warn};
use bevy::material::key::{ErasedMaterialPipelineKey, ErasedMeshPipelineKey};
use bevy::material::{AlphaMode, OpaqueRendererMethod};
use bevy::math::{Affine3, Affine3A, Mat4, Vec3, Vec3A, Vec4};
use bevy::mesh::{Mesh, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{
    LightEntity, LightKeyCache, MeshFlags, MeshPipeline, MeshPipelineKey, MeshTransforms,
    MeshUniform, PreparedMaterial, SetMeshViewBindGroup, SetMeshViewBindingArrayBindGroup,
    SetPrepassViewBindGroup, SetPrepassViewEmptyBindGroup, Shadow, ShadowBatchSetKey, ShadowBinKey,
    ViewKeyCache, ViewKeyPrepassCache, ViewLightEntities, alpha_mode_pipeline_key,
};
use bevy::prelude::{GlobalTransform, InheritedVisibility, Msaa};
use bevy::render::erased_render_asset::ErasedRenderAssets;
use bevy::render::material_bind_groups::{MaterialBindGroupAllocators, MaterialBindGroupSlot};
use bevy::render::mesh::allocator::{MeshAllocator, MeshSlabs};
use bevy::render::mesh::{
    MeshMetadataFallbackBuffer, MeshMetadataFallbackMesh, RenderMesh, RenderMeshBufferInfo,
};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_phase::{
    AddRenderCommand, BinnedPhaseItem, BinnedRenderPhase, BinnedRenderPhaseType, DrawFunctionId,
    DrawFunctions, InputUniformIndex, PhaseItem, RenderCommand, RenderCommandResult,
    TrackedRenderPass, ViewBinnedRenderPhases,
};
use bevy::render::render_resource::binding_types::{
    storage_buffer_read_only_sized, storage_buffer_sized, uniform_buffer,
};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, Buffer,
    BufferDescriptor, BufferUsages, CachedComputePipelineId, CachedRenderPipelineId,
    ComputePassDescriptor, ComputePipelineDescriptor, IndexFormat, PipelineCache, ShaderStages,
    ShaderType, StorageBuffer, UniformBuffer,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderGraph, RenderQueue};
use bevy::render::sync_world::MainEntity;
use bevy::render::view::{ExtractedView, RetainedViewEntity};
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::Shader;

#[cfg(test)]
mod cull_test;
#[cfg(test)]
mod gpu_test;

use super::material::{ATTACHED_SIZE, INSTANCE_SIZE, InstancedMaterials};
use super::{
    AttachedRow, ExternalMeshId, InstancedAttached, InstancedScatter, InstancedSlot, PartMesh,
};
use crate::LodQuality;
use procgena_core::LodRange;

/// The passes a Scatter is drawn in.
mod pass {
    /// The main forward pass.
    pub(super) const MAIN: u8 = 0;
    /// The depth, normal and motion-vector prepass of a forward camera.
    pub(super) const PREPASS: u8 = 1;
    /// The G-buffer pass of a deferred camera.
    pub(super) const DEFERRED: u8 = 2;
    /// A shadow map.
    pub(super) const SHADOW: u8 = 3;
    /// How many passes there are.
    pub(super) const COUNT: usize = 4;
}

/// Which parts a draw command draws: those in Bevy's opaque phases, those in
/// its alpha-mask phases, or all of them (in a shadow pass).
mod class {
    pub(super) const OPAQUE: u8 = 0;
    pub(super) const MASK: u8 = 1;
    pub(super) const ALL: u8 = 2;
}

/// Bytes per indexed indirect draw.
const DRAW_ARGS_SIZE: u64 = 20;
/// Threads per workgroup in `instanced_cull.wesl`.
const WORKGROUP_SIZE: u32 = 64;
/// Most detail levels a slot can have; one shadow bit each.
const MAX_LEVELS: usize = 32;

/// The renderer's system sets in the render world's `Render` schedule.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum InstancedSystems {
    /// Reads [`ExternalMeshes`] and every Scatter's parts, in
    /// `RenderSystems::Queue`. Plugins that list external meshes run before
    /// it.
    Prepare,
}

/// A mesh another plugin keeps on the GPU, drawn by parts with
/// [`PartMesh::External`].
#[derive(Clone, Debug)]
pub struct ExternalMesh {
    /// The vertices, laid out as `layout` says.
    pub vertices: Buffer,
    /// The indices.
    pub indices: Buffer,
    /// The type of `indices`.
    pub index_format: IndexFormat,
    /// Holds the mesh's indexed indirect draw at `args_offset`: five `u32`s
    /// (index count, instance count, first index, base vertex, first
    /// instance). The renderer copies it every frame and replaces the
    /// instance count, so the draw may be written on the GPU. The first
    /// instance must be 0.
    pub args: Buffer,
    /// Byte offset of the draw in `args`.
    pub args_offset: u64,
    /// The vertex layout, as a Bevy mesh with the same attributes would
    /// have, which the material's pipeline is built for. The mesh is drawn
    /// as a triangle list.
    pub layout: MeshVertexBufferLayoutRef,
}

/// The external meshes parts can draw, by id, in the render world.
///
/// The plugin that owns a mesh keeps its entry current, in systems that run
/// before [`InstancedSystems::Prepare`], and removes it when the mesh is
/// gone.
#[derive(Resource, Default, Debug)]
pub struct ExternalMeshes(pub HashMap<ExternalMeshId, ExternalMesh>);

/// The renderer's shaders.
#[derive(Resource, Clone)]
struct InstancedShaders {
    cull: Handle<Shader>,
    attach: Handle<Shader>,
}

/// Adds the render world's resources and systems.
pub(super) fn build(app: &mut App) {
    let assets = app.world().resource::<AssetServer>();
    let cull = assets.load("embedded://procgena_core_bevy/shaders/instanced_cull.wesl");
    let attach = assets.load("embedded://procgena_core_bevy/shaders/instanced_attach.wesl");
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .insert_resource(InstancedShaders { cull, attach })
        .insert_resource(CullLayout::new())
        .insert_resource(AttachLayout::new())
        .init_resource::<InstancedMaterials>()
        .init_resource::<ExternalMeshes>()
        .init_resource::<ExtractedScatters>()
        .init_resource::<Scatters>()
        .init_resource::<ScatterViews>()
        .init_resource::<PipelineMemo>()
        .add_render_command::<Opaque3d, DrawMain<{ class::OPAQUE }>>()
        .add_render_command::<AlphaMask3d, DrawMain<{ class::MASK }>>()
        .add_render_command::<Opaque3dPrepass, DrawPrepass<{ pass::PREPASS }, { class::OPAQUE }>>()
        .add_render_command::<AlphaMask3dPrepass, DrawPrepass<{ pass::PREPASS }, { class::MASK }>>()
        .add_render_command::<Opaque3dDeferred, DrawPrepass<{ pass::DEFERRED }, { class::OPAQUE }>>()
        .add_render_command::<AlphaMask3dDeferred, DrawPrepass<{ pass::DEFERRED }, { class::MASK }>>()
        .add_render_command::<Shadow, DrawPrepass<{ pass::SHADOW }, { class::ALL }>>()
        .add_systems(RenderStartup, init_cull_pipelines)
        .add_systems(ExtractSchedule, extract_scatters)
        .configure_sets(Render, InstancedSystems::Prepare.in_set(RenderSystems::Queue))
        .add_systems(
            Render,
            (
                (prepare_scatters, specialize_scatters, queue_scatters)
                    .chain()
                    .in_set(InstancedSystems::Prepare),
                prepare_views.in_set(RenderSystems::PrepareResources),
                prepare_bind_groups.in_set(RenderSystems::PrepareBindGroups),
            ),
        )
        .add_systems(RenderGraph, run_cull.before(camera_driver));
}

// --------------------------------------------------------------------------
// Extraction

/// A visible Scatter, copied from the main world this frame.
struct ExtractedScatter {
    world_from_scatter: Affine3A,
    scatter: InstancedScatter,
    /// The rows drawn on the Scatter's copies, if any.
    attached: Option<InstancedAttached>,
    /// The Scatter's render layers: cameras draw it only if they share one.
    layers: RenderLayers,
}

/// Every visible Scatter with copies, and the quality setting, copied from
/// the main world this frame.
#[derive(Resource, Default)]
struct ExtractedScatters {
    scatters: HashMap<MainEntity, ExtractedScatter>,
    quality_scale: f32,
}

#[expect(clippy::type_complexity, reason = "Bevy system parameters")]
fn extract_scatters(
    mut extracted: ResMut<ExtractedScatters>,
    scatters: Extract<
        Query<(
            Entity,
            &InstancedScatter,
            Option<&InstancedAttached>,
            &GlobalTransform,
            &InheritedVisibility,
            Option<&RenderLayers>,
        )>,
    >,
    quality: Extract<Option<Res<LodQuality>>>,
) {
    extracted.quality_scale = quality.as_ref().map_or(1.0, |q| q.0.distance_scale());
    extracted.scatters.clear();
    for (entity, scatter, attached, transform, visibility, layers) in &scatters {
        if !visibility.get() || scatter.copies.is_empty() {
            continue;
        }
        extracted.scatters.insert(
            MainEntity::from(entity),
            ExtractedScatter {
                world_from_scatter: transform.affine(),
                scatter: scatter.clone(),
                attached: attached.cloned(),
                layers: layers.cloned().unwrap_or_default(),
            },
        );
    }
}

// --------------------------------------------------------------------------
// Per-Scatter GPU data

/// One copy's culling input. Mirrors `Copy` in `instanced_cull.wesl`.
#[derive(Debug, Clone, Copy, ShaderType)]
struct GpuCopy {
    sphere: Vec4,
    origin: Vec4,
    slot: u32,
    record: u32,
}

/// One slot's culling input. Mirrors `Slot` in `instanced_cull.wesl`.
#[derive(Debug, Clone, Copy, Default, ShaderType)]
struct GpuSlot {
    start: u32,
    count: u32,
    level_count: u32,
    shadow_mask: u32,
    first_range: u32,
}

/// One host copy's input to the attached culling: a drawn copy whose slot
/// has rows. Mirrors `Host` in `instanced_attach.wesl`.
#[derive(Debug, Clone, Copy, ShaderType)]
struct GpuHost {
    /// The copy's world transform, by row.
    world: [Vec4; 3],
    sphere: Vec4,
    origin: Vec4,
    rows: Vec4,
    slot: u32,
    record: u32,
    first_row: u32,
    row_count: u32,
    first_thread: u32,
    whole_slot: u32,
    reach: f32,
    world_scale: f32,
}

/// One row of a host slot's table. Mirrors `Row` in `instanced_attach.wesl`.
#[derive(Debug, Clone, Copy, ShaderType)]
struct GpuRow {
    place: Vec4,
    turn: Vec4,
    wind: Vec4,
    sphere: Vec4,
    slot: u32,
    host_levels: u32,
}

/// `whole_slot` of a host whose rows' slots don't all share their levels.
const NO_SLOT: u32 = u32::MAX;

/// Where a part's mesh is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum MeshRef {
    Asset(AssetId<Mesh>),
    External(ExternalMeshId),
}

/// One part a Scatter draws.
struct GpuPart {
    mesh: MeshRef,
    material: UntypedAssetId,
    /// The material type.
    type_id: TypeId,
    /// Whether the part's level draws into shadow maps.
    casts_shadows: bool,
    /// Whether the part's slot fades between levels, so its pipelines read
    /// the dither.
    dithers: bool,
    /// Whether the part's slot is drawn by rows, from a view's attached
    /// entries with attached pipelines.
    attached: bool,
    /// The list the part draws: its first entry in a view's visible copies
    /// (or attached entries), and the most copies it can hold.
    list_start: u32,
    list_len: u32,
}

/// What the render world keeps for one Scatter between frames.
struct ScatterGpu {
    /// The Scatter, rows and transform the buffers were built for.
    scatter: InstancedScatter,
    attached: Option<InstancedAttached>,
    world_from_scatter: Affine3A,
    /// One mesh record per drawn copy, sorted by slot.
    records: StorageBuffer<Vec<MeshUniform>>,
    /// The mesh bind group over `records`, rebuilt every frame because
    /// Bevy's mesh metadata buffer may move.
    records_group: Option<BindGroup>,
    copies: StorageBuffer<Vec<GpuCopy>>,
    slots: StorageBuffer<Vec<GpuSlot>>,
    ranges: StorageBuffer<Vec<Vec4>>,
    part_lists: StorageBuffer<Vec<u32>>,
    parts: Vec<GpuPart>,
    /// Each part's indexed indirect draw with an instance count of 0.
    template_args: Option<Buffer>,
    /// Number of drawn copies.
    copy_count: u32,
    /// Number of lists: slots × `levels`.
    list_count: u32,
    /// The most detail levels of any slot.
    levels: u32,
    /// Host copies, and every host slot's rows, for the attached culling;
    /// empty without rows.
    hosts: StorageBuffer<Vec<GpuHost>>,
    rows: StorageBuffer<Vec<GpuRow>>,
    host_count: u32,
    /// Rows of every host copy: the attached culling's threads.
    row_threads: u32,
    /// Attached copies there can be, over all attached slots.
    attached_count: u32,
    layers: RenderLayers,
}

/// The render world's Scatters.
#[derive(Resource, Default)]
struct Scatters(HashMap<MainEntity, ScatterGpu>);

/// Brings each Scatter's records, culling inputs and parts up to date, and
/// writes the template draws of parts whose mesh is a Bevy mesh.
#[expect(clippy::too_many_arguments, reason = "one Bevy system")]
fn prepare_scatters(
    extracted: Res<ExtractedScatters>,
    mut scatters: ResMut<Scatters>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mesh_allocator: Res<MeshAllocator>,
    meshes: Res<RenderAssets<RenderMesh>>,
    fallback_mesh: Res<MeshMetadataFallbackMesh>,
    mut warned: Local<bool>,
) {
    scatters
        .0
        .retain(|entity, _| extracted.scatters.contains_key(entity));
    let metadata_index = mesh_allocator
        .mesh_metadata_slice(&fallback_mesh.0.id())
        .map(|slice| slice.range.start);

    for (entity, scatter) in &extracted.scatters {
        let changed = scatters.0.get(entity).is_none_or(|gpu| {
            !Arc::ptr_eq(&gpu.scatter.slots, &scatter.scatter.slots)
                || !Arc::ptr_eq(&gpu.scatter.copies, &scatter.scatter.copies)
                || gpu.world_from_scatter != scatter.world_from_scatter
                || match (&gpu.attached, &scatter.attached) {
                    (Some(old), Some(new)) => !Arc::ptr_eq(&old.tables, &new.tables),
                    (None, None) => false,
                    _ => true,
                }
        });
        if changed {
            let gpu = build_scatter(scatter, metadata_index, &device, &queue, &mut warned);
            scatters.0.insert(*entity, gpu);
        }
        let Some(gpu) = scatters.0.get_mut(entity) else {
            continue;
        };
        gpu.layers.clone_from(&scatter.layers);
        write_template_args(gpu, &mesh_allocator, &meshes, &queue);
    }
}

/// Builds a Scatter's GPU data from scratch.
fn build_scatter(
    extracted: &ExtractedScatter,
    metadata_index: Option<u32>,
    device: &RenderDevice,
    queue: &RenderQueue,
    warned: &mut bool,
) -> ScatterGpu {
    let scatter = &extracted.scatter;
    let world_from_scatter = extracted.world_from_scatter;
    let slots: &[InstancedSlot] = &scatter.slots;

    // Each slot's levels: valid slots only, with their ranges and shadows.
    let mut gpu_slots = vec![GpuSlot::default(); slots.len()];
    let mut ranges = Vec::new();
    let mut levels = 1u32;
    for (slot, gpu_slot) in slots.iter().zip(&mut gpu_slots) {
        let count = slot.lod.levels.len();
        if let Err(error) = slot.lod.validate() {
            if !*warned {
                *warned = true;
                warn!("procgena_core_bevy: an instanced slot is not drawn: {error}");
            }
            continue;
        }
        if count > MAX_LEVELS {
            continue;
        }
        gpu_slot.level_count = count as u32;
        gpu_slot.first_range = ranges.len() as u32;
        gpu_slot.shadow_mask = slot
            .lod
            .levels
            .iter()
            .enumerate()
            .filter(|(_, level)| level.casts_shadows)
            .fold(0, |mask, (l, _)| mask | (1 << l));
        ranges.extend(slot.lod.ranges(1.0).iter().map(LodRange::to_vec4));
        levels = levels.max(count as u32);
    }

    // Slots drawn by rows, and each host slot's rows.
    let no_tables: &[Arc<[AttachedRow]>] = &[];
    let tables = extracted
        .attached
        .as_ref()
        .map_or(no_tables, |a| &a.tables[..]);
    let is_attached = attached_slots(slots.len(), tables);
    let (gpu_rows, table_infos) = build_tables(slots, &gpu_slots, &ranges, tables, &is_attached);

    // Copies sorted by slot, skipping those with no such slot and those of
    // slots that rows draw.
    let mut order: Vec<usize> = (0..scatter.copies.len())
        .filter(|&i| {
            let slot = scatter.copies[i].slot as usize;
            slot < slots.len() && !is_attached[slot]
        })
        .collect();
    order.sort_by_key(|&i| scatter.copies[i].slot);
    for &i in &order {
        gpu_slots[scatter.copies[i].slot as usize].count += 1;
    }
    let mut start = 0;
    for gpu_slot in &mut gpu_slots {
        gpu_slot.start = start;
        start += gpu_slot.count;
    }
    let copy_count = order.len() as u32;

    // Attached slots count and place their copies among the attached
    // entries instead: each copy of a host slot adds its table's rows.
    let mut attached_counts = vec![0u32; slots.len()];
    for (host_slot, info) in table_infos.iter().enumerate() {
        let Some(info) = info else {
            continue;
        };
        let copies = gpu_slots[host_slot].count;
        for row in &gpu_rows[info.first_row as usize..][..info.row_count as usize] {
            attached_counts[row.slot as usize] += copies;
        }
    }
    let mut attached_count = 0;
    for (gpu_slot, (&attached, &count)) in gpu_slots
        .iter_mut()
        .zip(is_attached.iter().zip(&attached_counts))
    {
        if attached {
            gpu_slot.start = attached_count;
            gpu_slot.count = count;
            attached_count += count;
        }
    }

    // Mesh records and culling inputs, in sorted order, and the host copies
    // among them.
    let mut records = Vec::with_capacity(order.len());
    let mut copies = Vec::with_capacity(order.len());
    let mut hosts = Vec::new();
    let mut row_threads = 0;
    for (record, &i) in order.iter().enumerate() {
        let copy = &scatter.copies[i];
        let slot = &slots[copy.slot as usize];
        let world = world_from_scatter * copy.transform;
        records.push(copy_record(world, copy.wind_phase, metadata_index));
        let (center, radius) = slot.bounds;
        let factor = copy.size / slot.lod.reference_size;
        let origin = Vec3::from(world.translation).extend(factor);
        copies.push(GpuCopy {
            sphere: world
                .transform_point3a(Vec3A::from(center))
                .extend(radius * largest_scale(&world)),
            origin,
            slot: copy.slot,
            record: record as u32,
        });
        if let Some(Some(info)) = table_infos.get(copy.slot as usize) {
            hosts.push(host(&world, copy, origin, record as u32, info, row_threads));
            row_threads += info.row_count;
        }
    }
    let host_count = hosts.len() as u32;

    // Parts, each drawing one list.
    let mut parts = Vec::new();
    let mut part_lists = Vec::new();
    for (index, (slot, gpu_slot)) in slots.iter().zip(&gpu_slots).enumerate() {
        if gpu_slot.count == 0 {
            continue;
        }
        for part in &slot.parts {
            if part.level >= gpu_slot.level_count {
                continue;
            }
            let ranges = &ranges[gpu_slot.first_range as usize..][..gpu_slot.level_count as usize];
            let fades = ranges.len() > 1 || ranges.iter().any(|r| r.w < f32::MAX);
            let attached = is_attached[index];
            let entries = if attached { attached_count } else { copy_count };
            part_lists.push(index as u32 * levels + part.level);
            parts.push(GpuPart {
                mesh: match &part.mesh {
                    PartMesh::Mesh(handle) => MeshRef::Asset(handle.id()),
                    PartMesh::External(id) => MeshRef::External(*id),
                },
                material: part.material.id(),
                type_id: part.material.type_id(),
                casts_shadows: slot.lod.levels[part.level as usize].casts_shadows,
                dithers: fades,
                attached,
                list_start: part.level * entries + gpu_slot.start,
                list_len: gpu_slot.count,
            });
        }
    }

    // Upload.
    let mut gpu = ScatterGpu {
        scatter: scatter.clone(),
        attached: extracted.attached.clone(),
        world_from_scatter,
        records: StorageBuffer::default(),
        records_group: None,
        copies: StorageBuffer::default(),
        slots: StorageBuffer::default(),
        ranges: StorageBuffer::default(),
        part_lists: StorageBuffer::default(),
        template_args: None,
        parts,
        copy_count,
        list_count: slots.len() as u32 * levels,
        levels,
        hosts: StorageBuffer::default(),
        rows: StorageBuffer::default(),
        host_count,
        row_threads,
        attached_count,
        layers: extracted.layers.clone(),
    };
    if copy_count == 0 || gpu.parts.is_empty() {
        return gpu;
    }
    gpu.records.set_label(Some("procgena_instanced_records"));
    gpu.copies.set_label(Some("procgena_instanced_copies"));
    gpu.slots.set_label(Some("procgena_instanced_slots"));
    gpu.ranges.set_label(Some("procgena_instanced_ranges"));
    gpu.part_lists
        .set_label(Some("procgena_instanced_part_lists"));
    gpu.records.set(records);
    gpu.copies.set(copies);
    gpu.slots.set(gpu_slots);
    gpu.ranges.set(if ranges.is_empty() {
        vec![Vec4::ZERO]
    } else {
        ranges
    });
    gpu.part_lists.set(part_lists);
    gpu.records.write_buffer(device, queue);
    gpu.copies.write_buffer(device, queue);
    gpu.slots.write_buffer(device, queue);
    gpu.ranges.write_buffer(device, queue);
    gpu.part_lists.write_buffer(device, queue);
    if host_count > 0 {
        gpu.hosts.set_label(Some("procgena_instanced_hosts"));
        gpu.rows.set_label(Some("procgena_instanced_rows"));
        gpu.hosts.set(hosts);
        gpu.rows.set(gpu_rows);
        gpu.hosts.write_buffer(device, queue);
        gpu.rows.write_buffer(device, queue);
    }
    gpu.template_args = Some(device.create_buffer(&BufferDescriptor {
        label: Some("procgena_instanced_template_args"),
        size: gpu.parts.len() as u64 * DRAW_ARGS_SIZE,
        usage: BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    }));
    gpu
}

/// Which of `slot_count` slots rows draw: those any row of any table names.
fn attached_slots(slot_count: usize, tables: &[Arc<[AttachedRow]>]) -> Vec<bool> {
    let mut attached = vec![false; slot_count];
    for row in tables
        .iter()
        .take(slot_count)
        .flat_map(|table| table.iter())
    {
        if let Some(flag) = attached.get_mut(row.slot as usize) {
            *flag = true;
        }
    }
    attached
}

/// What the host stage needs to know about one host slot's rows, in the
/// host's own coordinates.
#[derive(Clone, Debug)]
struct TableInfo {
    first_row: u32,
    row_count: u32,
    /// How far the farthest row origin is from the host's origin.
    radius: f32,
    /// A sphere around every row's bounds.
    sphere: (Vec3, f32),
    /// The smallest and largest row size factors.
    min_factor: f32,
    max_factor: f32,
    /// The farthest a row is drawn, in reference metres times its factor.
    reach: f32,
    /// A slot whose levels every row's slot shares, or [`NO_SLOT`].
    whole_slot: u32,
}

/// Every host slot's rows, packed one table after another, and each host
/// slot's summary (`None` for slots that host no rows). Skips tables of
/// slots that rows draw, and rows whose slot is out of range or not drawn.
fn build_tables(
    slots: &[InstancedSlot],
    gpu_slots: &[GpuSlot],
    ranges: &[Vec4],
    tables: &[Arc<[AttachedRow]>],
    is_attached: &[bool],
) -> (Vec<GpuRow>, Vec<Option<TableInfo>>) {
    let slot_ranges = |slot: usize| {
        let gpu = &gpu_slots[slot];
        &ranges[gpu.first_range as usize..][..gpu.level_count as usize]
    };
    let mut rows = Vec::new();
    let mut infos = vec![None; slots.len()];
    for (host_slot, table) in tables.iter().enumerate().take(slots.len()) {
        if is_attached[host_slot] {
            continue;
        }

        // The table's rows, and the bounds and factors they span.
        let first_row = rows.len();
        let mut info = TableInfo {
            first_row: first_row as u32,
            row_count: 0,
            radius: 0.0,
            sphere: (Vec3::ZERO, 0.0),
            min_factor: f32::MAX,
            max_factor: 0.0,
            reach: 0.0,
            whole_slot: NO_SLOT,
        };
        let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        let mut shared: Option<&[Vec4]> = None;
        let mut all_share = true;
        for row in table.iter() {
            let index = row.slot as usize;
            let Some(slot) = slots.get(index) else {
                continue;
            };
            if gpu_slots[index].level_count == 0 {
                continue;
            }
            let levels = slot_ranges(index);
            let factor = row.size / slot.lod.reference_size;
            let (center, radius) = slot.bounds;
            let center = row.position + row.rotation * (center * row.scale);
            let radius = radius * row.scale;
            rows.push(GpuRow {
                place: row.position.extend(row.scale),
                turn: Vec4::from(row.rotation),
                wind: row.wind.extend(factor),
                sphere: center.extend(radius),
                slot: row.slot,
                host_levels: row.host_levels,
            });
            low = low.min(center - radius);
            high = high.max(center + radius);
            info.radius = info.radius.max(row.position.length());
            info.min_factor = info.min_factor.min(factor);
            info.max_factor = info.max_factor.max(factor);
            let last = levels[levels.len() - 1];
            info.reach = info.reach.max((last.w * factor).min(f32::MAX));
            match shared {
                None => {
                    shared = Some(levels);
                    info.whole_slot = row.slot;
                }
                Some(first) => all_share &= first == levels,
            }
        }
        info.row_count = (rows.len() - first_row) as u32;
        if info.row_count == 0 {
            continue;
        }

        // One sphere around them all.
        let center = (low + high) * 0.5;
        let radius = rows[first_row..]
            .iter()
            .map(|row| (row.sphere.truncate() - center).length() + row.sphere.w)
            .fold(0.0, f32::max);
        info.sphere = (center, radius);
        if !all_share {
            info.whole_slot = NO_SLOT;
        }
        infos[host_slot] = Some(info);
    }
    (rows, infos)
}

/// One host copy's culling input: `copy`, placed at `world`, whose slot's
/// rows are summed up in `info`. `origin` is the copy's `GpuCopy::origin`,
/// `record` its mesh record, and `first_thread` its first row's thread.
fn host(
    world: &Affine3A,
    copy: &super::InstancedCopy,
    origin: Vec4,
    record: u32,
    info: &TableInfo,
    first_thread: u32,
) -> GpuHost {
    let (m, t) = (world.matrix3, world.translation);
    let row = |i: usize| Vec4::new(m.x_axis[i], m.y_axis[i], m.z_axis[i], t[i]);
    let world_scale = largest_scale(world);
    let (center, radius) = info.sphere;
    GpuHost {
        world: [row(0), row(1), row(2)],
        sphere: world
            .transform_point3a(Vec3A::from(center))
            .extend(radius * world_scale),
        origin,
        rows: Vec4::new(
            info.radius * world_scale,
            largest_scale(&copy.transform),
            info.min_factor,
            info.max_factor,
        ),
        slot: copy.slot,
        record,
        first_row: info.first_row,
        row_count: info.row_count,
        first_thread,
        whole_slot: info.whole_slot,
        reach: info.reach,
        world_scale,
    }
}

/// A copy's mesh record: its world transform, which never changes between
/// frames, its wind phase as the record's tag, and Bevy's flags for a
/// shadow-receiving mesh without a visibility range.
fn copy_record(
    world_from_local: Affine3A,
    wind_phase: f32,
    metadata_index: Option<u32>,
) -> MeshUniform {
    let mut flags = MeshFlags::SHADOW_RECEIVER.bits() | u32::from(u16::MAX);
    if world_from_local.matrix3.determinant().is_sign_positive() {
        flags |= MeshFlags::SIGN_DETERMINANT_MODEL_3X3.bits();
    }
    let transform = Affine3::from(world_from_local);
    MeshUniform::new(
        &MeshTransforms {
            world_from_local: transform,
            previous_world_from_local: transform,
            flags,
        },
        0,
        MaterialBindGroupSlot(0),
        None,
        None,
        None,
        Some(wind_phase.to_bits()),
        metadata_index,
    )
}

/// The largest factor `transform` stretches any direction by.
fn largest_scale(transform: &Affine3A) -> f32 {
    let m = transform.matrix3;
    m.x_axis
        .length()
        .max(m.y_axis.length())
        .max(m.z_axis.length())
}

/// Writes the template draw of every part whose mesh is a Bevy mesh, from
/// where Bevy's mesh allocator keeps it this frame. External parts are
/// copied on the GPU by [`run_cull`].
fn write_template_args(
    gpu: &ScatterGpu,
    allocator: &MeshAllocator,
    meshes: &RenderAssets<RenderMesh>,
    queue: &RenderQueue,
) {
    let Some(buffer) = &gpu.template_args else {
        return;
    };
    let mut draws = vec![[0u32; 5]; gpu.parts.len()];
    for (part, args) in gpu.parts.iter().zip(&mut draws) {
        let MeshRef::Asset(id) = part.mesh else {
            continue;
        };
        let (Some(mesh), Some(vertices), Some(indices)) = (
            meshes.get(id),
            allocator.mesh_vertex_slice(&id),
            allocator.mesh_index_slice(&id),
        ) else {
            continue;
        };
        let RenderMeshBufferInfo::Indexed { count, .. } = mesh.buffer_info else {
            continue;
        };
        *args = [count, 0, indices.range.start, vertices.range.start, 0];
    }
    let bytes: Vec<u8> = draws
        .iter()
        .flatten()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    queue.write_buffer(buffer, 0, &bytes);
}

// --------------------------------------------------------------------------
// Per-view culling state

/// One view's culling settings for one Scatter. Mirrors `CullView` in
/// `instanced_cull.wesl`.
#[derive(Debug, Clone, Copy, Default, ShaderType)]
struct CullUniform {
    planes: [Vec4; 6],
    lod_origin: Vec4,
    quality_scale: f32,
    copy_count: u32,
    list_count: u32,
    part_count: u32,
    levels: u32,
    shadow: u32,
    skip_near: u32,
    host_count: u32,
    row_threads: u32,
    attached_count: u32,
}

/// One view's culling of one Scatter, and its pipelines this frame.
#[derive(Default)]
struct ViewScatter {
    uniform: UniformBuffer<CullUniform>,
    /// Holds what the cull needs; `None` until the view is first culled.
    buffers: Option<ViewBuffers>,
    cull_group: Option<BindGroup>,
    /// The attached culling's bind group, for Scatters with host copies.
    attach_group: Option<BindGroup>,
    /// Updated for this frame by `prepare_views`.
    seen: bool,
    /// The pipeline of each of the Scatter's parts in each pass, or `None`
    /// when the part isn't drawn in that pass or its pipeline isn't ready.
    pipelines: [Vec<Option<CachedRenderPipelineId>>; pass::COUNT],
    /// Whether each part is in Bevy's alpha-mask phases rather than its
    /// opaque ones.
    masked: Vec<bool>,
}

/// A view's culling output for one Scatter.
struct ViewBuffers {
    /// One indexed indirect draw per part.
    args: Buffer,
    counts: Buffer,
    /// The copies each part draws, as instance-rate vertex data.
    visible: Buffer,
    /// What the host stage decided for each host copy.
    host_state: Buffer,
    /// The attached copies each attached part draws, as instance-rate
    /// vertex data.
    attached_visible: Buffer,
    /// What the buffers have room for.
    size: ViewSize,
}

/// What a view's buffers must have room for.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ViewSize {
    parts: u32,
    lists: u32,
    entries: u32,
    hosts: u32,
    attached_entries: u32,
}

impl ViewSize {
    /// Whether buffers of this size hold everything `need` asks for.
    fn holds(&self, need: &ViewSize) -> bool {
        self.parts >= need.parts
            && self.lists >= need.lists
            && self.entries >= need.entries
            && self.hosts >= need.hosts
            && self.attached_entries >= need.attached_entries
    }
}

impl ViewBuffers {
    fn new(device: &RenderDevice, size: ViewSize) -> Self {
        let ViewSize {
            parts,
            lists,
            entries,
            hosts,
            attached_entries,
        } = size;
        let buffer = |label: &'static str, size: u64, usage: BufferUsages| {
            device.create_buffer(&BufferDescriptor {
                label: Some(label),
                size: size.max(16),
                usage,
                mapped_at_creation: false,
            })
        };
        Self {
            args: buffer(
                "procgena_instanced_view_args",
                u64::from(parts) * DRAW_ARGS_SIZE,
                BufferUsages::STORAGE
                    | BufferUsages::INDIRECT
                    | BufferUsages::COPY_DST
                    | BufferUsages::COPY_SRC,
            ),
            counts: buffer(
                "procgena_instanced_view_counts",
                u64::from(lists) * 4,
                BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            ),
            visible: buffer(
                "procgena_instanced_view_visible",
                u64::from(entries) * INSTANCE_SIZE,
                BufferUsages::STORAGE | BufferUsages::VERTEX | BufferUsages::COPY_SRC,
            ),
            host_state: buffer(
                "procgena_instanced_view_host_state",
                u64::from(hosts) * 4,
                BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            ),
            attached_visible: buffer(
                "procgena_instanced_view_attached",
                u64::from(attached_entries) * ATTACHED_SIZE,
                BufferUsages::STORAGE | BufferUsages::VERTEX | BufferUsages::COPY_SRC,
            ),
            size,
        }
    }
}

/// Every view's state, by view and Scatter.
#[derive(Resource, Default)]
struct ScatterViews(HashMap<(RetainedViewEntity, MainEntity), ViewScatter>);

/// How one view culls and picks detail levels.
#[derive(Clone, Copy)]
struct CullSpec {
    clip_from_world: Mat4,
    /// Where detail levels are measured from: the camera.
    lod_origin: Vec3,
    shadow: bool,
    /// Keep copies between a directional light and its shadow cascade.
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
/// shadow view, for every Scatter it draws. Drops state for views and
/// Scatters that are gone.
#[expect(clippy::type_complexity, reason = "Bevy system parameters")]
fn prepare_views(
    mut views: ResMut<ScatterViews>,
    scatters: Res<Scatters>,
    extracted: Res<ExtractedScatters>,
    cameras: Query<
        (
            &ExtractedView,
            Option<&ViewLightEntities>,
            Option<&RenderLayers>,
        ),
        With<Camera3d>,
    >,
    light_views: Query<(&ExtractedView, &LightEntity)>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    for view in views.0.values_mut() {
        view.seen = false;
    }
    let mut done = HashSet::new();
    let mut update =
        |retained: RetainedViewEntity, spec: CullSpec, layers: Option<&RenderLayers>| {
            // A light view is culled once, for the first camera that reaches it.
            if !done.insert(retained) {
                return;
            }
            for (entity, gpu) in &scatters.0 {
                if gpu.template_args.is_none()
                    || layers.is_some_and(|layers| !layers.intersects(&gpu.layers))
                {
                    continue;
                }
                let state = views.0.entry((retained, *entity)).or_default();
                update_view(state, gpu, spec, extracted.quality_scale, &device, &queue);
            }
        };

    // Each camera, and the directional shadow cascades Bevy lists for it.
    let mut first_camera = None;
    for (camera, lights, layers) in &cameras {
        let origin = camera.world_from_view.translation();
        first_camera.get_or_insert(origin);
        let default_layers = RenderLayers::default();
        update(
            camera.retained_view_entity,
            CullSpec::new(camera, origin, false, false),
            Some(layers.unwrap_or(&default_layers)),
        );
        for &light in lights.map_or(&[][..], |l| l.lights.as_slice()) {
            if let Ok((view, kind)) = light_views.get(light) {
                let directional = matches!(kind, LightEntity::Directional { .. });
                update(
                    view.retained_view_entity,
                    CullSpec::new(view, origin, true, directional),
                    None,
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
                None,
            );
        }
    }
    views.0.retain(|_, view| view.seen);
}

/// Makes sure `state` has room for `gpu`, and writes its culling settings.
fn update_view(
    state: &mut ViewScatter,
    gpu: &ScatterGpu,
    spec: CullSpec,
    quality_scale: f32,
    device: &RenderDevice,
    queue: &RenderQueue,
) {
    let size = ViewSize {
        parts: gpu.parts.len() as u32,
        lists: gpu.list_count,
        entries: gpu.copy_count * gpu.levels,
        hosts: gpu.host_count,
        attached_entries: gpu.attached_count * gpu.levels,
    };
    let fits = state.buffers.as_ref().is_some_and(|b| b.size.holds(&size));
    if !fits {
        state.buffers = Some(ViewBuffers::new(device, size));
    }
    state.uniform.set(CullUniform {
        planes: frustum_planes(&spec.clip_from_world),
        lod_origin: spec.lod_origin.extend(1.0),
        quality_scale,
        copy_count: gpu.copy_count,
        list_count: size.lists,
        part_count: size.parts,
        levels: gpu.levels,
        shadow: u32::from(spec.shadow),
        skip_near: u32::from(spec.skip_near),
        host_count: gpu.host_count,
        row_threads: gpu.row_threads,
        attached_count: gpu.attached_count,
    });
    state.uniform.write_buffer(device, queue);
    state.seen = true;
}

/// Bind group layout of the culling passes.
#[derive(Resource, Clone)]
struct CullLayout(BindGroupLayoutDescriptor);

impl CullLayout {
    fn new() -> Self {
        // Binding numbers match `instanced_cull.wesl`.
        Self(BindGroupLayoutDescriptor::new(
            "procgena_instanced_cull",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::COMPUTE,
                (
                    uniform_buffer::<CullUniform>(false),
                    storage_buffer_read_only_sized(false, None),
                    storage_buffer_read_only_sized(false, None),
                    storage_buffer_read_only_sized(false, None),
                    storage_buffer_read_only_sized(false, None),
                    storage_buffer_sized(false, None),
                    storage_buffer_sized(false, None),
                    storage_buffer_sized(false, None),
                ),
            ),
        ))
    }
}

/// Bind group layout of the attached culling passes.
#[derive(Resource, Clone)]
struct AttachLayout(BindGroupLayoutDescriptor);

impl AttachLayout {
    fn new() -> Self {
        // Binding numbers match `instanced_attach.wesl`.
        Self(BindGroupLayoutDescriptor::new(
            "procgena_instanced_attach",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::COMPUTE,
                (
                    uniform_buffer::<CullUniform>(false),
                    storage_buffer_read_only_sized(false, None),
                    storage_buffer_read_only_sized(false, None),
                    storage_buffer_read_only_sized(false, None),
                    storage_buffer_read_only_sized(false, None),
                    storage_buffer_sized(false, None),
                    storage_buffer_sized(false, None),
                    storage_buffer_sized(false, None),
                ),
            ),
        ))
    }
}

/// Builds each Scatter's mesh bind group and each view's culling bind groups.
#[expect(clippy::too_many_arguments, reason = "one Bevy system")]
fn prepare_bind_groups(
    mut scatters: ResMut<Scatters>,
    mut views: ResMut<ScatterViews>,
    layout: Res<CullLayout>,
    attach_layout: Res<AttachLayout>,
    mesh_pipeline: Res<MeshPipeline>,
    pipeline_cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    fallback_buffer: Option<Res<MeshMetadataFallbackBuffer>>,
) {
    for gpu in scatters.0.values_mut() {
        gpu.records_group = match (gpu.records.binding(), fallback_buffer.as_ref()) {
            (Some(records), Some(fallback)) => Some(mesh_pipeline.mesh_layouts.model_only(
                &device,
                &pipeline_cache,
                &records,
                &fallback.buffer,
            )),
            _ => None,
        };
    }
    for ((_, entity), view) in &mut views.0 {
        view.cull_group = None;
        view.attach_group = None;
        let (Some(gpu), Some(buffers)) = (scatters.0.get(entity), view.buffers.as_ref()) else {
            continue;
        };

        // The attached culling's inputs, for Scatters with host copies.
        if let (Some(uniform), Some(slots), Some(ranges), Some(hosts), Some(rows)) = (
            view.uniform.binding(),
            gpu.slots.binding(),
            gpu.ranges.binding(),
            gpu.hosts.binding(),
            gpu.rows.binding(),
        ) {
            view.attach_group = Some(device.create_bind_group(
                "procgena_instanced_attach",
                &pipeline_cache.get_bind_group_layout(&attach_layout.0),
                &BindGroupEntries::sequential((
                    uniform,
                    slots,
                    ranges,
                    hosts,
                    rows,
                    buffers.host_state.as_entire_binding(),
                    buffers.counts.as_entire_binding(),
                    buffers.attached_visible.as_entire_binding(),
                )),
            ));
        }

        let (Some(uniform), Some(copies), Some(slots), Some(ranges), Some(part_lists)) = (
            view.uniform.binding(),
            gpu.copies.binding(),
            gpu.slots.binding(),
            gpu.ranges.binding(),
            gpu.part_lists.binding(),
        ) else {
            continue;
        };
        view.cull_group = Some(device.create_bind_group(
            "procgena_instanced_cull",
            &pipeline_cache.get_bind_group_layout(&layout.0),
            &BindGroupEntries::sequential((
                uniform,
                copies,
                slots,
                ranges,
                part_lists,
                buffers.counts.as_entire_binding(),
                buffers.args.as_entire_binding(),
                buffers.visible.as_entire_binding(),
            )),
        ));
    }
}

/// The culling compute pipelines.
#[derive(Resource, Debug)]
struct CullPipelines {
    reset_counts: CachedComputePipelineId,
    cull: CachedComputePipelineId,
    write_args: CachedComputePipelineId,
    cull_hosts: CachedComputePipelineId,
    cull_rows: CachedComputePipelineId,
}

fn init_cull_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    layout: Res<CullLayout>,
    attach_layout: Res<AttachLayout>,
    shaders: Res<InstancedShaders>,
) {
    let queue =
        |entry: &'static str, layout: &BindGroupLayoutDescriptor, shader: &Handle<Shader>| {
            pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
                label: Some(format!("procgena_instanced_{entry}").into()),
                layout: vec![layout.clone()],
                shader: shader.clone(),
                entry_point: Some(entry.into()),
                ..Default::default()
            })
        };
    commands.insert_resource(CullPipelines {
        reset_counts: queue("reset_counts", &layout.0, &shaders.cull),
        cull: queue("cull", &layout.0, &shaders.cull),
        write_args: queue("write_args", &layout.0, &shaders.cull),
        cull_hosts: queue("cull_hosts", &attach_layout.0, &shaders.attach),
        cull_rows: queue("cull_rows", &attach_layout.0, &shaders.attach),
    });
}

/// Records every view's culling: copies external parts' draws into each
/// Scatter's template, the template into the view's draws, then counts and
/// lists the copies each part draws.
fn run_cull(
    mut ctx: RenderContext,
    pipelines: Option<Res<CullPipelines>>,
    pipeline_cache: Res<PipelineCache>,
    scatters: Res<Scatters>,
    views: Res<ScatterViews>,
    external: Res<ExternalMeshes>,
) {
    let Some(pipelines) = pipelines else {
        return;
    };
    let (Some(reset_counts), Some(cull), Some(write_args)) = (
        pipeline_cache.get_compute_pipeline(pipelines.reset_counts),
        pipeline_cache.get_compute_pipeline(pipelines.cull),
        pipeline_cache.get_compute_pipeline(pipelines.write_args),
    ) else {
        return;
    };
    let attach = pipeline_cache
        .get_compute_pipeline(pipelines.cull_hosts)
        .zip(pipeline_cache.get_compute_pipeline(pipelines.cull_rows));
    let encoder = ctx.command_encoder();

    // External parts' draws, as their owners wrote them this frame.
    for gpu in scatters.0.values() {
        let Some(template) = &gpu.template_args else {
            continue;
        };
        for (index, part) in gpu.parts.iter().enumerate() {
            if let MeshRef::External(id) = part.mesh
                && let Some(mesh) = external.0.get(&id)
            {
                encoder.copy_buffer_to_buffer(
                    &mesh.args,
                    mesh.args_offset,
                    template,
                    index as u64 * DRAW_ARGS_SIZE,
                    DRAW_ARGS_SIZE,
                );
            }
        }
    }

    // Each view's lists and draws.
    for ((_, entity), view) in &views.0 {
        let (Some(gpu), Some(group), Some(buffers)) = (
            scatters.0.get(entity),
            view.cull_group.as_ref(),
            view.buffers.as_ref(),
        ) else {
            continue;
        };
        let Some(template) = &gpu.template_args else {
            continue;
        };
        let parts = gpu.parts.len() as u32;
        encoder.copy_buffer_to_buffer(
            template,
            0,
            &buffers.args,
            0,
            u64::from(parts) * DRAW_ARGS_SIZE,
        );
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("procgena_instanced_cull"),
            ..Default::default()
        });
        pass.set_bind_group(0, group, &[]);
        pass.set_pipeline(reset_counts);
        pass.dispatch_workgroups(gpu.list_count.div_ceil(WORKGROUP_SIZE), 1, 1);
        pass.set_pipeline(cull);
        pass.dispatch_workgroups(gpu.copy_count.div_ceil(WORKGROUP_SIZE), 1, 1);

        // Attached copies: each host copy, then each of its rows.
        if let (Some((cull_hosts, cull_rows)), Some(attach_group)) =
            (attach, view.attach_group.as_ref())
        {
            pass.set_bind_group(0, attach_group, &[]);
            pass.set_pipeline(cull_hosts);
            pass.dispatch_workgroups(gpu.host_count.div_ceil(WORKGROUP_SIZE), 1, 1);
            pass.set_pipeline(cull_rows);
            let (x, y) = workgroups_2d(gpu.row_threads.div_ceil(WORKGROUP_SIZE));
            pass.dispatch_workgroups(x, y, 1);
            pass.set_bind_group(0, group, &[]);
        }
        pass.set_pipeline(write_args);
        pass.dispatch_workgroups(parts.div_ceil(WORKGROUP_SIZE), 1, 1);
    }
}

/// Most workgroups one dimension of a dispatch may have.
const MAX_WORKGROUPS: u32 = 65_535;

/// Splits `groups` workgroups over two dispatch dimensions, as `cull_rows`
/// in `instanced_attach.wesl` reads them: at least `groups` in all.
fn workgroups_2d(groups: u32) -> (u32, u32) {
    if groups <= MAX_WORKGROUPS {
        (groups, 1)
    } else {
        (MAX_WORKGROUPS, groups.div_ceil(MAX_WORKGROUPS))
    }
}

// --------------------------------------------------------------------------
// Specialization

/// A pipeline to look up: one part of one Scatter in one pass of one view.
struct Work {
    view: RetainedViewEntity,
    scatter: MainEntity,
    pass: u8,
    part: usize,
    key: MeshPipelineKey,
}

/// A view's pipeline keys and the passes it draws.
struct SpecializeView {
    retained: RetainedViewEntity,
    /// The forward pass key, or the light's key for a shadow view.
    key: MeshPipelineKey,
    /// The prepass key, when the camera has a prepass or a G-buffer pass.
    prepass_key: Option<MeshPipelineKey>,
    /// Whether the camera has a forward prepass.
    has_prepass: bool,
    /// Whether the camera draws deferred materials into a G-buffer.
    deferred: bool,
    /// Whether this is a shadow view.
    shadow: bool,
    /// A camera's render layers; `None` for a shadow view, which draws
    /// every Scatter.
    layers: Option<RenderLayers>,
}

/// Pipelines already looked up, by pass, mesh key, mesh, material, and
/// whether the pipeline draws attached copies.
#[derive(Resource, Default)]
struct PipelineMemo(
    HashMap<(u8, MeshPipelineKey, MeshRef, UntypedAssetId, bool), CachedRenderPipelineId>,
);

/// Works out, for every view and Scatter, which passes each part is drawn
/// in, and the pipeline for each.
///
/// The pipelines come from each material's own pipeline builders, with its
/// key switched to the instanced variant by the function registered with
/// [`InstancedMaterialPlugin`](super::InstancedMaterialPlugin).
fn specialize_scatters(world: &mut World) {
    let views = specialize_views(world);
    let work = collect_work(world, &views);
    {
        let scatters = world.resource::<Scatters>();
        let counts: HashMap<MainEntity, usize> = scatters
            .0
            .iter()
            .map(|(e, g)| (*e, g.parts.len()))
            .collect();
        let mut views = world.resource_mut::<ScatterViews>();
        for ((_, entity), view) in &mut views.0 {
            let count = counts.get(entity).copied().unwrap_or(0);
            for pipelines in &mut view.pipelines {
                pipelines.clear();
                pipelines.resize(count, None);
            }
        }
    }

    for item in work {
        let Some(id) = specialize(world, &item) else {
            continue;
        };
        let mut views = world.resource_mut::<ScatterViews>();
        let Some(view) = views.0.get_mut(&(item.view, item.scatter)) else {
            continue;
        };
        if let Some(slot) = view.pipelines[item.pass as usize].get_mut(item.part) {
            *slot = Some(id);
        }
    }

    // Record which phases each part belongs in.
    world.resource_scope(|world, mut views: Mut<ScatterViews>| {
        let scatters = world.resource::<Scatters>();
        let materials = world.resource::<ErasedRenderAssets<PreparedMaterial>>();
        for ((_, entity), view) in &mut views.0 {
            let Some(gpu) = scatters.0.get(entity) else {
                continue;
            };
            view.masked = gpu
                .parts
                .iter()
                .map(|part| {
                    materials
                        .get(part.material)
                        .is_some_and(|m| matches!(m.properties.alpha_mode, AlphaMode::Mask(_)))
                })
                .collect();
        }
    });
}

/// Every camera and shadow view that has pipeline keys this frame.
fn specialize_views(world: &mut World) -> Vec<SpecializeView> {
    let mut cameras = world.query_filtered::<
        (&ExtractedView, Has<DeferredPrepass>, Option<&RenderLayers>),
        With<Camera3d>,
    >();
    let mut lights = world.query_filtered::<&ExtractedView, With<LightEntity>>();
    let view_keys = world.resource::<ViewKeyCache>();
    let prepass_keys = world.resource::<ViewKeyPrepassCache>();
    let light_keys = world.resource::<LightKeyCache>();
    let prepass_phases = world.resource::<ViewBinnedRenderPhases<Opaque3dPrepass>>();
    let deferred_phases = world.resource::<ViewBinnedRenderPhases<Opaque3dDeferred>>();

    let mut views = Vec::new();
    for (view, deferred_camera, layers) in cameras.iter(world) {
        let retained = view.retained_view_entity;
        let Some(key) = view_keys.get(&retained).copied() else {
            continue;
        };
        let has_prepass = prepass_phases.contains_key(&retained);
        let deferred = deferred_camera && deferred_phases.contains_key(&retained);
        views.push(SpecializeView {
            retained,
            key,
            prepass_key: prepass_keys
                .get(&retained)
                .copied()
                .filter(|_| has_prepass || deferred),
            has_prepass,
            deferred,
            shadow: false,
            layers: Some(layers.cloned().unwrap_or_default()),
        });
    }
    for view in lights.iter(world) {
        let retained = view.retained_view_entity;
        let Some(key) = light_keys.get(&retained).copied() else {
            continue;
        };
        views.push(SpecializeView {
            retained,
            key,
            prepass_key: None,
            has_prepass: false,
            deferred: false,
            shadow: true,
            layers: None,
        });
    }
    views
}

/// The vertex layout and pipeline key bits of a part's mesh, if it is ready.
fn mesh_layout(
    mesh: MeshRef,
    meshes: &RenderAssets<RenderMesh>,
    external: &ExternalMeshes,
) -> Option<(MeshVertexBufferLayoutRef, MeshPipelineKey)> {
    match mesh {
        MeshRef::Asset(id) => {
            let mesh = meshes.get(id)?;
            Some((
                mesh.layout.clone(),
                MeshPipelineKey::from_bits_retain(mesh.key_bits.bits()),
            ))
        }
        MeshRef::External(id) => Some((
            external.0.get(&id)?.layout.clone(),
            MeshPipelineKey::from_primitive_topology_and_strip_index(
                PrimitiveTopology::TriangleList,
                None,
            ),
        )),
    }
}

/// Lists the pipelines every view needs for every Scatter: one per part and
/// pass, skipping parts whose mesh or material isn't ready.
fn collect_work(world: &World, views: &[SpecializeView]) -> Vec<Work> {
    let scatters = world.resource::<Scatters>();
    let scatter_views = world.resource::<ScatterViews>();
    let meshes = world.resource::<RenderAssets<RenderMesh>>();
    let external = world.resource::<ExternalMeshes>();
    let materials = world.resource::<ErasedRenderAssets<PreparedMaterial>>();
    let mut work = Vec::new();
    for view in views {
        let msaa = Msaa::from_samples(view.key.msaa_samples());
        for (entity, gpu) in &scatters.0 {
            if !scatter_views.0.contains_key(&(view.retained, *entity)) {
                continue;
            }
            if view
                .layers
                .as_ref()
                .is_some_and(|layers| !layers.intersects(&gpu.layers))
            {
                continue;
            }
            for (index, part) in gpu.parts.iter().enumerate() {
                let (Some((_, mesh_bits)), Some(material)) = (
                    mesh_layout(part.mesh, meshes, external),
                    materials.get(part.material),
                ) else {
                    continue;
                };
                let properties = &material.properties;
                let alpha = alpha_mode_pipeline_key(properties.alpha_mode, &msaa);
                let dither = if part.dithers {
                    MeshPipelineKey::VISIBILITY_RANGE_DITHER
                } else {
                    MeshPipelineKey::NONE
                };
                let mut add = |pass: u8, key: MeshPipelineKey| {
                    work.push(Work {
                        view: view.retained,
                        scatter: *entity,
                        pass,
                        part: index,
                        key: key | mesh_bits,
                    });
                };

                // A shadow view draws one level per copy, undithered.
                if view.shadow {
                    if properties.shadows_enabled && part.casts_shadows {
                        let discard = match properties.alpha_mode {
                            AlphaMode::Opaque => MeshPipelineKey::NONE,
                            _ => MeshPipelineKey::MAY_DISCARD,
                        };
                        add(
                            pass::SHADOW,
                            view.key | discard | MeshPipelineKey::PREPASS_READS_MATERIAL,
                        );
                    }
                    continue;
                }

                // A deferred camera draws deferred materials into its
                // G-buffer only; everything else is drawn forward, and into
                // the camera's prepass if it has one.
                let read_material = MeshPipelineKey::PREPASS_READS_MATERIAL;
                if view.deferred && properties.render_method == OpaqueRendererMethod::Deferred {
                    if let Some(prepass) = view.prepass_key {
                        let deferred = MeshPipelineKey::DEFERRED_PREPASS;
                        add(
                            pass::DEFERRED,
                            prepass | alpha | deferred | read_material | dither,
                        );
                    }
                    continue;
                }
                let bits: MeshPipelineKey = properties.mesh_pipeline_key_bits.downcast();
                add(pass::MAIN, view.key | bits | alpha | dither);
                if let (true, true, Some(prepass)) = (
                    view.has_prepass,
                    properties.prepass_enabled,
                    view.prepass_key,
                ) {
                    add(pass::PREPASS, prepass | alpha | read_material | dither);
                }
            }
        }
    }
    work
}

/// The pipeline for one work item, from the memo or the material's pipeline
/// builder, or `None` if it can't be built yet.
fn specialize(world: &mut World, item: &Work) -> Option<CachedRenderPipelineId> {
    let scatters = world.resource::<Scatters>();
    let part = scatters.0.get(&item.scatter)?.parts.get(item.part)?;
    let (mesh, material, type_id, attached) =
        (part.mesh, part.material, part.type_id, part.attached);
    let memo_key = (item.pass, item.key, mesh, material, attached);
    if let Some(id) = world.resource::<PipelineMemo>().0.get(&memo_key) {
        return Some(*id);
    }
    let (layout, _) = mesh_layout(
        mesh,
        world.resource::<RenderAssets<RenderMesh>>(),
        world.resource::<ExternalMeshes>(),
    )?;
    let properties = world
        .resource::<ErasedRenderAssets<PreparedMaterial>>()
        .get(material)?
        .properties
        .clone();
    let materials = world.resource::<InstancedMaterials>();
    let registered = if attached {
        &materials.attached
    } else {
        &materials.copies
    };
    let Some(instanced) = registered.get(&type_id).cloned() else {
        if attached {
            error!(
                "procgena_core_bevy: an attached part's material type was not registered with InstancedMaterialPlugin::attached; it is not drawn"
            );
        } else {
            error!(
                "procgena_core_bevy: a part's material type was not registered with InstancedMaterialPlugin; it is not drawn"
            );
        }
        return None;
    };

    // The material's own key, switched to its instanced variant.
    let key = ErasedMaterialPipelineKey {
        mesh_key: ErasedMeshPipelineKey::new(item.key),
        material_key: instanced(&properties.material_key),
        type_id,
    };
    let result = if item.pass == pass::MAIN {
        (properties.base_specialize?)(world, key, &layout, &properties)
    } else {
        (properties.prepass_specialize?)(world, &key, &layout, &properties)
    };
    match result {
        Ok(id) => {
            world.resource_mut::<PipelineMemo>().0.insert(memo_key, id);
            Some(id)
        }
        Err(error) => {
            error!("procgena_core_bevy: cannot build an instanced pipeline: {error}");
            None
        }
    }
}

// --------------------------------------------------------------------------
// Queueing

/// The phases [`queue_scatters`] fills, and their draw functions.
#[derive(SystemParam)]
struct Phases<'w> {
    opaque: ResMut<'w, ViewBinnedRenderPhases<Opaque3d>>,
    mask: ResMut<'w, ViewBinnedRenderPhases<AlphaMask3d>>,
    opaque_prepass: ResMut<'w, ViewBinnedRenderPhases<Opaque3dPrepass>>,
    mask_prepass: ResMut<'w, ViewBinnedRenderPhases<AlphaMask3dPrepass>>,
    opaque_deferred: ResMut<'w, ViewBinnedRenderPhases<Opaque3dDeferred>>,
    mask_deferred: ResMut<'w, ViewBinnedRenderPhases<AlphaMask3dDeferred>>,
    shadow: ResMut<'w, ViewBinnedRenderPhases<Shadow>>,
    opaque_draws: Res<'w, DrawFunctions<Opaque3d>>,
    mask_draws: Res<'w, DrawFunctions<AlphaMask3d>>,
    opaque_prepass_draws: Res<'w, DrawFunctions<Opaque3dPrepass>>,
    mask_prepass_draws: Res<'w, DrawFunctions<AlphaMask3dPrepass>>,
    opaque_deferred_draws: Res<'w, DrawFunctions<Opaque3dDeferred>>,
    mask_deferred_draws: Res<'w, DrawFunctions<AlphaMask3dDeferred>>,
    shadow_draws: Res<'w, DrawFunctions<Shadow>>,
}

/// The first pipeline among `view`'s parts in `pass` whose class is
/// `masked`, if it draws any.
fn first_pipeline(
    view: &ViewScatter,
    pass: u8,
    masked: Option<bool>,
) -> Option<CachedRenderPipelineId> {
    view.pipelines[pass as usize]
        .iter()
        .zip(&view.masked)
        .find(|(id, m)| id.is_some() && masked.is_none_or(|want| **m == want))
        .and_then(|(id, _)| *id)
}

/// Puts each Scatter into every phase of every view that draws some of it,
/// and takes it out of the rest.
fn queue_scatters(
    mut phases: Phases,
    extracted: Res<ExtractedScatters>,
    views: Res<ScatterViews>,
    cameras: Query<&ExtractedView, With<Camera3d>>,
    lights: Query<&ExtractedView, With<LightEntity>>,
    mut queued: Local<HashSet<MainEntity>>,
) {
    // This frame's Scatters, and last frame's, whose items must go.
    let scatters: HashSet<MainEntity> = extracted
        .scatters
        .keys()
        .copied()
        .chain(queued.iter().copied())
        .collect();
    *queued = extracted.scatters.keys().copied().collect();

    let main_opaque = phases
        .opaque_draws
        .read()
        .id::<DrawMain<{ class::OPAQUE }>>();
    let main_mask = phases.mask_draws.read().id::<DrawMain<{ class::MASK }>>();
    let prepass_opaque = phases
        .opaque_prepass_draws
        .read()
        .id::<DrawPrepass<{ pass::PREPASS }, { class::OPAQUE }>>();
    let prepass_mask = phases
        .mask_prepass_draws
        .read()
        .id::<DrawPrepass<{ pass::PREPASS }, { class::MASK }>>();
    let deferred_opaque = phases
        .opaque_deferred_draws
        .read()
        .id::<DrawPrepass<{ pass::DEFERRED }, { class::OPAQUE }>>();
    let deferred_mask = phases
        .mask_deferred_draws
        .read()
        .id::<DrawPrepass<{ pass::DEFERRED }, { class::MASK }>>();
    let shadow = phases
        .shadow_draws
        .read()
        .id::<DrawPrepass<{ pass::SHADOW }, { class::ALL }>>();
    let no_view = ViewScatter::default();

    for view in &cameras {
        let retained = view.retained_view_entity;
        for scatter in &scatters {
            let item = (Entity::PLACEHOLDER, *scatter);
            let state = views.0.get(&(retained, *scatter)).unwrap_or(&no_view);
            if let Some(phase) = phases.opaque.get_mut(&retained) {
                phase.remove(*scatter);
                if let Some(pipeline) = first_pipeline(state, pass::MAIN, Some(false)) {
                    add_opaque(phase, pipeline, main_opaque, item);
                }
            }
            let target = (state, retained, item);
            requeue(&mut phases.mask, target, pass::MAIN, true, main_mask);
            requeue(
                &mut phases.opaque_prepass,
                target,
                pass::PREPASS,
                false,
                prepass_opaque,
            );
            requeue(
                &mut phases.mask_prepass,
                target,
                pass::PREPASS,
                true,
                prepass_mask,
            );
            requeue(
                &mut phases.opaque_deferred,
                target,
                pass::DEFERRED,
                false,
                deferred_opaque,
            );
            requeue(
                &mut phases.mask_deferred,
                target,
                pass::DEFERRED,
                true,
                deferred_mask,
            );
        }
    }

    for view in &lights {
        let retained = view.retained_view_entity;
        let Some(phase) = phases.shadow.get_mut(&retained) else {
            continue;
        };
        for scatter in &scatters {
            phase.remove(*scatter);
            let Some(state) = views.0.get(&(retained, *scatter)) else {
                continue;
            };
            if let Some(pipeline) = first_pipeline(state, pass::SHADOW, None) {
                phase.add(
                    ShadowBatchSetKey {
                        pipeline,
                        draw_function: shadow,
                        material_bind_group_index: None,
                        slabs: MeshSlabs::default(),
                    },
                    ShadowBinKey {
                        asset_id: AssetId::<Mesh>::default().untyped(),
                    },
                    (Entity::PLACEHOLDER, *scatter),
                    InputUniformIndex::default(),
                    BinnedRenderPhaseType::NonMesh,
                );
            }
        }
    }
}

/// Adds a Scatter item to an opaque main phase.
fn add_opaque(
    phase: &mut BinnedRenderPhase<Opaque3d>,
    pipeline: CachedRenderPipelineId,
    draw_function: DrawFunctionId,
    item: (Entity, MainEntity),
) {
    phase.add(
        Opaque3dBatchSetKey {
            draw_function,
            pipeline,
            material_bind_group_index: None,
            lightmap_slab: None,
            slabs: MeshSlabs::default(),
        },
        Opaque3dBinKey {
            asset_id: AssetId::<Mesh>::default().untyped(),
        },
        item,
        InputUniformIndex::default(),
        BinnedRenderPhaseType::NonMesh,
    );
}

/// Takes a Scatter out of one of a camera's alpha-mask, prepass or deferred
/// phases, and puts it back if the camera draws parts of it there.
fn requeue<P>(
    phases: &mut ViewBinnedRenderPhases<P>,
    (state, view, item): (&ViewScatter, RetainedViewEntity, (Entity, MainEntity)),
    pass: u8,
    masked: bool,
    draw_function: DrawFunctionId,
) where
    P: BinnedPhaseItem<
            BatchSetKey = OpaqueNoLightmap3dBatchSetKey,
            BinKey = OpaqueNoLightmap3dBinKey,
        >,
{
    let Some(phase) = phases.get_mut(&view) else {
        return;
    };
    phase.remove(item.1);
    if let Some(pipeline) = first_pipeline(state, pass, Some(masked)) {
        phase.add(
            OpaqueNoLightmap3dBatchSetKey {
                pipeline,
                draw_function,
                material_bind_group_index: None,
                slabs: MeshSlabs::default(),
            },
            OpaqueNoLightmap3dBinKey {
                asset_id: AssetId::<Mesh>::default().untyped(),
            },
            item,
            InputUniformIndex::default(),
            BinnedRenderPhaseType::NonMesh,
        );
    }
}

// --------------------------------------------------------------------------
// Drawing

/// Draws a Scatter's parts of one class in one pass: for each part with a
/// pipeline, binds its pipeline, mesh and material, and draws its indirect
/// draw, whose instance count the cull wrote.
struct DrawParts<const PASS: u8, const CLASS: u8>;

impl<P: PhaseItem, const PASS: u8, const CLASS: u8> RenderCommand<P> for DrawParts<PASS, CLASS> {
    type Param = (
        SRes<Scatters>,
        SRes<ScatterViews>,
        SRes<PipelineCache>,
        SRes<MeshAllocator>,
        SRes<ExternalMeshes>,
        SRes<RenderAssets<RenderMesh>>,
        SRes<ErasedRenderAssets<PreparedMaterial>>,
        SRes<MaterialBindGroupAllocators>,
    );
    type ViewQuery = Read<ExtractedView>;
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _entity: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (scatters, views, pipelines, allocator, external, meshes, materials, allocators): SystemParamItem<
            'w,
            '_,
            Self::Param,
        >,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let scatters = scatters.into_inner();
        let views = views.into_inner();
        let pipelines = pipelines.into_inner();
        let allocator = allocator.into_inner();
        let external = external.into_inner();
        let meshes = meshes.into_inner();
        let materials = materials.into_inner();
        let allocators = allocators.into_inner();
        let entity = item.main_entity();
        let (Some(gpu), Some(state)) = (
            scatters.0.get(&entity),
            views.0.get(&(view.retained_view_entity, entity)),
        ) else {
            return RenderCommandResult::Skip;
        };
        let (Some(records), Some(buffers), true) = (
            gpu.records_group.as_ref(),
            state.buffers.as_ref(),
            state.cull_group.is_some(),
        ) else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(2, records, &[]);

        for (index, part) in gpu.parts.iter().enumerate() {
            let masked = state.masked.get(index).copied().unwrap_or(false);
            if (CLASS == class::OPAQUE && masked) || (CLASS == class::MASK && !masked) {
                continue;
            }
            let Some(Some(pipeline)) = state.pipelines[PASS as usize].get(index) else {
                continue;
            };
            let Some(pipeline) = pipelines.get_render_pipeline(*pipeline) else {
                continue;
            };
            let Some(bind_group) = materials
                .get(part.material)
                .zip(allocators.get(&part.type_id))
                .and_then(|(m, allocator)| allocator.get(m.binding.group))
                .and_then(|group| group.bind_group())
            else {
                continue;
            };

            // The part's vertices and indices.
            match part.mesh {
                MeshRef::Asset(id) => {
                    let (Some(mesh), Some(vertices), Some(indices)) = (
                        meshes.get(id),
                        allocator.mesh_vertex_slice(&id),
                        allocator.mesh_index_slice(&id),
                    ) else {
                        continue;
                    };
                    let RenderMeshBufferInfo::Indexed { index_format, .. } = mesh.buffer_info
                    else {
                        continue;
                    };
                    pass.set_vertex_buffer(0, vertices.buffer.slice(..));
                    pass.set_index_buffer(indices.buffer.slice(..), index_format);
                }
                MeshRef::External(id) => {
                    let Some(mesh) = external.0.get(&id) else {
                        continue;
                    };
                    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                    pass.set_index_buffer(mesh.indices.slice(..), mesh.index_format);
                }
            }

            pass.set_render_pipeline(pipeline);
            pass.set_bind_group(3, bind_group, &[]);
            let (entries, stride) = if part.attached {
                (&buffers.attached_visible, ATTACHED_SIZE)
            } else {
                (&buffers.visible, INSTANCE_SIZE)
            };
            let start = u64::from(part.list_start) * stride;
            let end = start + u64::from(part.list_len) * stride;
            pass.set_vertex_buffer(1, entries.slice(start..end));
            pass.draw_indexed_indirect(&buffers.args, index as u64 * DRAW_ARGS_SIZE);
        }
        RenderCommandResult::Success
    }
}

/// Draws parts in Bevy's forward opaque or alpha-mask phase.
type DrawMain<const CLASS: u8> = (
    SetMeshViewBindGroup<0>,
    SetMeshViewBindingArrayBindGroup<1>,
    DrawParts<{ pass::MAIN }, CLASS>,
);

/// Draws parts in a prepass, deferred or shadow phase.
type DrawPrepass<const PASS: u8, const CLASS: u8> = (
    SetPrepassViewBindGroup<0>,
    SetPrepassViewEmptyBindGroup<1>,
    DrawParts<PASS, CLASS>,
);
