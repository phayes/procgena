//! Each pool's GPU resources: its generated shapes, its rocks' transforms, its
//! material and the bind group every pooled-rock draw uses.
//!
//! A pool's shapes are generated with `procgena_stone`'s GPU generator on Bevy's own
//! device. Every mesh reserves room in advance; when a mesh needs more, the
//! generator draws nothing for it and reports the room it needs, which this
//! module reads back without blocking and uses to generate the pool again.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use bevy::asset::AssetId;
use bevy::ecs::prelude::*;
use bevy::log::warn;
use bevy::math::Vec4;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, Buffer, BufferDescriptor, BufferUsages, CommandEncoderDescriptor,
    MapMode, PipelineCache, ShaderType, StorageBuffer, UniformBuffer,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::texture::{FallbackImage, GpuImage};
use procgena_stone::gpu::{
    GPU_DRAW_ARGS_SIZE, GPU_STATUS_SIZE, GPU_VERTEX_SIZE, GpuError, GpuStoneGenerator,
    GpuStoneRequest, GpuStoneStatus,
};

use super::{ExtractedPools, PoolLayouts};
use crate::pool::{PoolDefinition, PoolInstance, StonePool};
use crate::spawn::visibility_ranges;

/// One pooled rock as the shaders read it. Mirrors `PoolInstance` in `pool_types.wesl`.
#[derive(Debug, Clone, Copy, ShaderType)]
pub(crate) struct GpuInstance {
    rows: [Vec4; 3],
    variant: u32,
}

impl From<&PoolInstance> for GpuInstance {
    fn from(instance: &PoolInstance) -> Self {
        let m = instance.world_from_local.matrix3;
        let t = instance.world_from_local.translation;
        Self {
            rows: [
                Vec4::new(m.x_axis.x, m.y_axis.x, m.z_axis.x, t.x),
                Vec4::new(m.x_axis.y, m.y_axis.y, m.z_axis.y, t.y),
                Vec4::new(m.x_axis.z, m.y_axis.z, m.z_axis.z, t.z),
            ],
            variant: instance.variant,
        }
    }
}

/// A pool's surface. Mirrors `PoolMaterial` in `pool_draw.wesl`.
#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub(crate) struct PoolMaterialUniform {
    base_color: Vec4,
    perceptual_roughness: f32,
    metallic: f32,
    scale: f32,
    texture_size: f32,
    textured: u32,
}

/// Where the status read-back is: waiting for the GPU, or holding the bytes.
const READBACK_MAPPING: u8 = 0;
const READBACK_READY: u8 = 1;

/// Times a pool is generated again with more room before it gives up.
const MAX_ATTEMPTS: u32 = 4;

/// Conservative unscaled radius enclosing any placed rock before its GPU
/// result is available. Keeps visibility checks from hiding rocks prematurely.
const UNPLACED_RADIUS: f32 = 2.0 * 1.732_050_8;

/// Every pool's GPU resources, and the generator they share.
#[derive(Resource, Default)]
pub(crate) struct RenderPools {
    /// Created when the first pool needs generating.
    generator: Option<GpuStoneGenerator>,
    pub pools: HashMap<AssetId<StonePool>, RenderPool>,
    /// The [`crate::pool::PoolInstances::version`] last uploaded.
    instance_version: Option<u64>,
}

/// One pool's generated shapes, rocks, material and bind group.
pub(crate) struct RenderPool {
    /// The [`PoolDefinition::version`] these resources were made for.
    version: u64,
    definition: PoolDefinition,
    /// One request per mesh, variant-major: mesh `v * lod_count + l`.
    requests: Vec<GpuStoneRequest>,
    /// The generated meshes; empty until generated.
    pub batches: Vec<PoolBatch>,
    needs_generation: bool,
    attempts: u32,
    /// Generation failed for good; the pool draws nothing.
    failed: bool,
    readback: Option<StatusReadback>,
    pub lod_count: u32,
    pub variant_count: u32,
    /// Camera-distance limits of each detail level, as Bevy's visibility
    /// ranges (`x..y` fade in, `z..w` fade out), in world units.
    pub lod_ranges: Vec<Vec4>,
    /// Radius enclosing every variant about its origin, including the pool's
    /// option scale but excluding each entity's transform scale.
    pub radius: f32,
    /// The rocks, sorted by variant; `None` when the pool has no rocks.
    pub instances: Option<StorageBuffer<Vec<GpuInstance>>>,
    /// Index in `instances` of each variant's first rock.
    pub variant_start: StorageBuffer<Vec<u32>>,
    /// Number of rocks of each variant.
    pub variant_counts: Vec<u32>,
    pub instance_count: u32,
    material: UniformBuffer<PoolMaterialUniform>,
    /// Group 2 of every pooled-rock draw.
    pub draw_group: Option<BindGroup>,
}

/// Some of a pool's meshes, generated together into shared buffers.
pub(crate) struct PoolBatch {
    pub vertices: Buffer,
    pub indices: Buffer,
    /// One indexed indirect draw per mesh, with an instance count of one.
    pub draw_args: Buffer,
    status: Buffer,
    /// The pool mesh index of this batch's first mesh.
    pub first_mesh: u32,
    pub mesh_count: u32,
}

/// A copy of a pool's generation status on its way to the CPU.
struct StatusReadback {
    state: Arc<AtomicU8>,
    bytes: Arc<Mutex<Vec<u8>>>,
}

impl RenderPool {
    fn new(definition: &PoolDefinition) -> Self {
        let pool = &definition.pool;
        let levels = pool.options.mesh.detail.levels();
        let lod_count = levels.len() as u32;
        let requests = (0..pool.variants)
            .flat_map(|variant| {
                let options = pool.variant_options(variant);
                levels
                    .iter()
                    .map(move |level| GpuStoneRequest::new(options.clone(), level.resolution))
            })
            .collect();
        let scale = pool.options.scale;
        let lod_ranges = visibility_ranges(levels.iter().copied(), scale)
            .into_iter()
            .map(|range| match range {
                Some(range) => Vec4::new(
                    range.start_margin.start,
                    range.start_margin.end,
                    range.end_margin.start.min(f32::MAX),
                    range.end_margin.end.min(f32::MAX),
                ),
                None => Vec4::new(0.0, 0.0, f32::MAX, f32::MAX),
            })
            .collect();
        Self {
            version: definition.version,
            definition: definition.clone(),
            requests,
            batches: Vec::new(),
            needs_generation: true,
            attempts: 0,
            failed: false,
            readback: None,
            lod_count,
            variant_count: pool.variants,
            lod_ranges,
            radius: scale * UNPLACED_RADIUS,
            instances: None,
            variant_start: StorageBuffer::from(vec![0; pool.variants as usize]),
            variant_counts: vec![0; pool.variants as usize],
            instance_count: 0,
            material: UniformBuffer::default(),
            draw_group: None,
        }
    }

    /// Number of meshes: one per variant and detail level.
    pub fn mesh_count(&self) -> u32 {
        self.variant_count * self.lod_count
    }

    /// Times the pool has been generated.
    #[cfg(test)]
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    /// Returns whether generation has run with no pending retry or status readback.
    /// This does not distinguish a successful readback from an empty failed readback.
    #[cfg(test)]
    pub fn status_read(&self) -> bool {
        self.attempts > 0 && !self.needs_generation && self.readback.is_none()
    }

    /// The seed of the pool these resources were made for.
    #[cfg(test)]
    pub fn seed(&self) -> u32 {
        self.definition.pool.options.seed
    }

    /// Generates the pool again with room for only a few vertices and indices
    /// per mesh, so every mesh overflows.
    #[cfg(test)]
    pub fn regenerate_too_small(&mut self) {
        for request in &mut self.requests {
            request.vertex_capacity = 16;
            request.index_capacity = 48;
        }
        self.needs_generation = true;
    }

    /// Records generation of every mesh and submits it, with a copy of the
    /// status for the read-back.
    fn generate(
        &mut self,
        generator: &GpuStoneGenerator,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) {
        self.needs_generation = false;
        self.attempts += 1;
        self.batches.clear();
        let wgpu_device = device.wgpu_device();
        let mut encoder = wgpu_device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("ze_stone_pool_generate"),
        });

        // Generate the meshes in batches whose buffers fit the device.
        let limits = device.limits();
        let limit = limits
            .max_storage_buffer_binding_size
            .min(limits.max_buffer_size);
        let lods = self.lod_count as usize;
        for variants in plan_batches(&self.requests, lods, limit) {
            let first_mesh = variants.start * lods;
            let requests = &self.requests[first_mesh..variants.end * lods];
            match generator.encode(wgpu_device, &mut encoder, requests) {
                Ok(batch) => self.batches.push(PoolBatch {
                    vertices: Buffer::from(batch.vertices),
                    indices: Buffer::from(batch.indices),
                    draw_args: Buffer::from(batch.draw_args),
                    status: Buffer::from(batch.status),
                    first_mesh: first_mesh as u32,
                    mesh_count: requests.len() as u32,
                }),
                Err(error) => {
                    self.fail(&error);
                    return;
                }
            }
        }

        // Copy every batch's status into one buffer the CPU can read.
        let status_bytes = u64::from(self.mesh_count()) * GPU_STATUS_SIZE;
        let readback = device.create_buffer(&BufferDescriptor {
            label: Some("ze_stone_pool_status_readback"),
            size: status_bytes,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        for batch in &self.batches {
            encoder.copy_buffer_to_buffer(
                &batch.status,
                0,
                &readback,
                u64::from(batch.first_mesh) * GPU_STATUS_SIZE,
                u64::from(batch.mesh_count) * GPU_STATUS_SIZE,
            );
        }
        queue.submit([encoder.finish()]);

        // Map the copy once the GPU has finished; the callback stores the bytes.
        let state = Arc::new(AtomicU8::new(READBACK_MAPPING));
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let (buffer, callback_state, callback_bytes) =
            (readback.clone(), state.clone(), bytes.clone());
        readback.slice(..).map_async(MapMode::Read, move |result| {
            if result.is_ok() {
                if let (Ok(view), Ok(mut out)) =
                    (buffer.slice(..).get_mapped_range(), callback_bytes.lock())
                {
                    out.extend_from_slice(&view);
                }
                buffer.unmap();
            }
            callback_state.store(READBACK_READY, Ordering::Release);
        });
        self.readback = Some(StatusReadback { state, bytes });
    }

    /// Stops drawing the pool after an error that more room cannot fix.
    fn fail(&mut self, error: &GpuError) {
        warn!("procgena_stone_bevy: a stone pool cannot be generated and is not drawn: {error}");
        self.failed = true;
        self.batches.clear();
    }

    /// Processes available GPU status without waiting. Updates the enclosing
    /// radius used for visibility checks and schedules generation with more
    /// capacity after overflow, unless the retry limit has been reached.
    fn check_readback(&mut self) {
        let Some(readback) = &self.readback else {
            return;
        };
        if readback.state.load(Ordering::Acquire) != READBACK_READY {
            return;
        }
        let bytes = readback.bytes.lock().map(|b| b.clone()).unwrap_or_default();
        self.readback = None;
        if bytes.is_empty() {
            warn!("procgena_stone_bevy: a stone pool's generation status could not be read");
            return;
        }
        let statuses = GpuStoneStatus::decode(&bytes);
        let largest = statuses
            .iter()
            .map(|status| status.bounding_radius)
            .fold(0.0, f32::max);
        if largest > 0.0 {
            self.radius = self.definition.pool.options.scale * largest;
        }
        let mut overflowed = false;
        for (request, status) in self.requests.iter_mut().zip(&statuses) {
            if !status.fits() {
                overflowed = true;
                request.grow_to_fit(status);
            }
        }
        if !overflowed {
            return;
        }
        if self.attempts >= MAX_ATTEMPTS {
            warn!(
                "procgena_stone_bevy: a stone pool still needs more room after {} attempts; some of its shapes are not drawn",
                self.attempts
            );
            return;
        }
        self.needs_generation = true;
    }

    /// Uploads this pool's rocks, grouped by variant.
    fn upload_instances(
        &mut self,
        rocks: &[PoolInstance],
        device: &RenderDevice,
        queue: &RenderQueue,
    ) {
        let mut counts = vec![0u32; self.variant_count as usize];
        for rock in rocks {
            counts[rock.variant as usize] += 1;
        }
        let starts: Vec<u32> = counts
            .iter()
            .scan(0, |start, &count| {
                let this = *start;
                *start += count;
                Some(this)
            })
            .collect();
        self.variant_start.set(starts);
        self.variant_start.write_buffer(device, queue);
        self.variant_counts = counts;
        self.instance_count = rocks.len() as u32;
        self.instances = (!rocks.is_empty()).then(|| {
            let mut buffer =
                StorageBuffer::from(rocks.iter().map(GpuInstance::from).collect::<Vec<_>>());
            buffer.write_buffer(device, queue);
            buffer
        });
    }
}

/// Splits a pool's meshes into batches of whole variants, as ranges of
/// variants, so each batch's vertex and index buffers fit within `limit`
/// bytes. `requests` holds `lods` requests per variant. A variant too large for
/// the limit gets a batch of its own, which the generator then rejects.
fn plan_batches(requests: &[GpuStoneRequest], lods: usize, limit: u64) -> Vec<Range<usize>> {
    let variants = requests.len() / lods;
    let (mut batches, mut start) = (Vec::new(), 0);
    let (mut vertex_bytes, mut index_bytes) = (0u64, 0u64);
    for variant in 0..variants {
        let meshes = &requests[variant * lods..(variant + 1) * lods];
        let v: u64 = meshes
            .iter()
            .map(|r| u64::from(r.vertex_capacity))
            .sum::<u64>()
            * GPU_VERTEX_SIZE;
        let i: u64 = meshes
            .iter()
            .map(|r| u64::from(r.index_capacity))
            .sum::<u64>()
            * 4;
        // Start a new batch when this variant would overflow the current one.
        if variant > start && (vertex_bytes + v > limit || index_bytes + i > limit) {
            batches.push(start..variant);
            (start, vertex_bytes, index_bytes) = (variant, 0, 0);
        }
        vertex_bytes += v;
        index_bytes += i;
    }
    if variants > start {
        batches.push(start..variants);
    }
    batches
}

/// Creates, regenerates and removes pools as their definitions change, reads
/// back generation status, and uploads changed rocks.
pub(crate) fn prepare_pools(
    mut render_pools: ResMut<RenderPools>,
    extracted: Res<ExtractedPools>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    let RenderPools {
        generator,
        pools,
        instance_version,
    } = &mut *render_pools;

    // Follow the main world's pools.
    let definitions = &extracted.definitions.pools;
    pools.retain(|id, _| definitions.contains_key(id));
    let mut new_pools = false;
    for (id, definition) in definitions {
        let current = pools
            .get(id)
            .is_some_and(|pool| pool.version == definition.version);
        if !current {
            pools.insert(*id, RenderPool::new(definition));
            new_pools = true;
        }
    }

    // Generate pools that are new or need more room.
    for pool in pools.values_mut() {
        pool.check_readback();
        if pool.needs_generation && !pool.failed {
            let generator =
                generator.get_or_insert_with(|| GpuStoneGenerator::new(device.wgpu_device()));
            pool.generate(generator, &device, &queue);
        }
    }

    // Upload rocks when they or the pools changed.
    let version = extracted.instances.version;
    if new_pools || *instance_version != Some(version) {
        *instance_version = Some(version);
        for (id, pool) in pools.iter_mut() {
            let rocks = extracted
                .instances
                .pools
                .get(id)
                .map_or(&[][..], Vec::as_slice);
            pool.upload_instances(rocks, &device, &queue);
        }
    }
}

/// Writes each pool's material and builds the bind group its draws use. A
/// textured pool draws without textures until all three images have loaded.
pub(crate) fn prepare_pool_bind_groups(
    mut render_pools: ResMut<RenderPools>,
    images: Res<RenderAssets<GpuImage>>,
    fallback: Res<FallbackImage>,
    layouts: Res<PoolLayouts>,
    pipeline_cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    for pool in render_pools.pools.values_mut() {
        let Some(instances) = pool.instances.as_ref().and_then(StorageBuffer::binding) else {
            pool.draw_group = None;
            continue;
        };

        // Use the textures once all three are on the GPU.
        let textures = pool.definition.textures.as_ref().and_then(|t| {
            Some([
                images.get(&t.color)?,
                images.get(&t.normal)?,
                images.get(&t.orh)?,
            ])
        });
        let options = &pool.definition.pool.options;
        let [r, g, b] = options.material.color;
        pool.material.set(PoolMaterialUniform {
            base_color: Vec4::new(r, g, b, 1.0),
            perceptual_roughness: options.material.roughness,
            metallic: options.material.metallic,
            scale: options.scale,
            texture_size: options.material.texture_size,
            textured: u32::from(textures.is_some()),
        });
        pool.material.write_buffer(&device, &queue);
        let Some(material) = pool.material.binding() else {
            pool.draw_group = None;
            continue;
        };
        let [color, normal, orh] = textures.unwrap_or([&fallback.d2; 3]);
        pool.draw_group = Some(device.create_bind_group(
            "ze_stone_pool_draw",
            &pipeline_cache.get_bind_group_layout(&layouts.draw),
            &BindGroupEntries::sequential((
                instances,
                material,
                &color.texture_view,
                &color.sampler,
                &normal.texture_view,
                &normal.sampler,
                &orh.texture_view,
                &orh.sampler,
            )),
        ));
    }
}

/// Byte offset of mesh `mesh`'s draw in a view's draw arguments.
pub(crate) fn draw_args_offset(mesh: u32) -> u64 {
    u64::from(mesh) * GPU_DRAW_ARGS_SIZE
}

#[cfg(test)]
mod tests {
    use procgena_stone::StoneOptions;

    use super::*;

    /// `variants` variants of two meshes each, every mesh with room for
    /// `vertices` vertices and twice as many indices.
    fn requests(variants: usize, vertices: u32) -> Vec<GpuStoneRequest> {
        let request = GpuStoneRequest {
            options: StoneOptions::default(),
            resolution: 24,
            vertex_capacity: vertices,
            index_capacity: 2 * vertices,
        };
        vec![request; variants * 2]
    }

    #[test]
    fn batches_hold_whole_variants_within_the_limit() {
        // One variant: 2 × 100 vertices × 24 bytes = 4800 vertex bytes, 1600 index bytes.
        let requests = requests(5, 100);
        assert_eq!(plan_batches(&requests, 2, u64::MAX), vec![0..5]);
        assert_eq!(plan_batches(&requests, 2, 4800 * 2), vec![0..2, 2..4, 4..5]);
        assert_eq!(
            plan_batches(&requests, 2, 4800 * 2 + 4799),
            vec![0..2, 2..4, 4..5]
        );
        assert_eq!(plan_batches(&requests, 2, 4800 * 3), vec![0..3, 3..5]);
        // A variant larger than the limit still gets its own batch.
        assert_eq!(
            plan_batches(&requests, 2, 100),
            vec![0..1, 1..2, 2..3, 3..4, 4..5]
        );
    }
}
