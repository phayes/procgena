//! Each pool's generated shapes, listed for the shared instanced renderer.
//!
//! A pool's shapes are generated with `procgena_stone`'s GPU generator on Bevy's own
//! device. Every mesh reserves room in advance; when a mesh needs more, the
//! generator draws nothing for it and reports the room it needs, which this
//! module reads back without blocking and uses to generate the pool again.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use bevy::asset::AssetId;
use bevy::ecs::prelude::*;
use bevy::log::warn;
use bevy::render::render_resource::{
    Buffer, BufferDescriptor, BufferUsages, CommandEncoderDescriptor, IndexFormat, MapMode,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use procgena_core_bevy::instanced::{ExternalMesh, ExternalMeshId, ExternalMeshes};
use procgena_stone::gpu::{
    GPU_DRAW_ARGS_SIZE, GPU_STATUS_SIZE, GPU_VERTEX_SIZE, GpuError, GpuStoneGenerator,
    GpuStoneRequest, GpuStoneStatus,
};

use super::{ExtractedPools, PoolMeshLayout};
use crate::pool::{MeasuredPoolSizes, PoolDefinition, StonePool, external_mesh_id};

/// Where the status read-back is: waiting for the GPU, or holding the bytes.
const READBACK_MAPPING: u8 = 0;
const READBACK_READY: u8 = 1;

/// Times a pool is generated again with more room before it gives up.
const MAX_ATTEMPTS: u32 = 4;

/// Every pool's GPU resources, and the generator they share.
#[derive(Resource, Default)]
pub(crate) struct RenderPools {
    /// Created when the first pool needs generating.
    generator: Option<GpuStoneGenerator>,
    pub pools: HashMap<AssetId<StonePool>, RenderPool>,
    /// The external meshes listed last frame, to take out again.
    listed: HashSet<ExternalMeshId>,
}

/// One pool's generated shapes.
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
        let levels = pool.mesh_levels();
        let requests = (0..pool.variants)
            .flat_map(|variant| {
                let options = pool.variant_options(variant);
                levels.map(move |level| GpuStoneRequest::new(options.clone(), level.resolution))
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
        }
    }

    /// Number of meshes: one per variant and detail level.
    pub fn mesh_count(&self) -> u32 {
        self.requests.len() as u32
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
        self.definition.pool.options.default_seed
    }

    /// The [`PoolDefinition::version`] these resources were made for.
    #[cfg(test)]
    pub fn version(&self) -> u64 {
        self.version
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
            label: Some("procgena_stone_pool_generate"),
        });

        // Generate the meshes in batches whose buffers fit the device.
        let limits = device.limits();
        let limit = limits
            .max_storage_buffer_binding_size
            .min(limits.max_buffer_size);
        let lods = self.definition_levels();
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
            label: Some("procgena_stone_pool_status_readback"),
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

    /// Number of meshes per variant: the game-level and lower meshes.
    fn definition_levels(&self) -> usize {
        self.definition.pool.mesh_levels().len()
    }

    /// Stops drawing the pool after an error that more room cannot fix.
    fn fail(&mut self, error: &GpuError) {
        warn!("procgena_stone_bevy: a stone pool cannot be generated and is not drawn: {error}");
        self.failed = true;
        self.batches.clear();
    }

    /// Processes available GPU status without waiting, and schedules
    /// generation with more capacity after overflow, unless the retry limit
    /// has been reached.
    ///
    /// Returns each variant's measured size once its status is read: its
    /// game-level mesh's bounding diameter in metres, or `None` for a
    /// variant whose mesh did not fit or is empty.
    fn check_readback(&mut self) -> Option<Vec<Option<f32>>> {
        let readback = self.readback.as_ref()?;
        if readback.state.load(Ordering::Acquire) != READBACK_READY {
            return None;
        }
        let bytes = readback.bytes.lock().map(|b| b.clone()).unwrap_or_default();
        self.readback = None;
        if bytes.is_empty() {
            warn!("procgena_stone_bevy: a stone pool's generation status could not be read");
            return None;
        }
        let statuses = GpuStoneStatus::decode(&bytes);
        let sizes = self.measured_sizes(&statuses);
        self.grow_overflowed(&statuses);
        Some(sizes)
    }

    /// Each variant's game-level bounding diameter in metres, from the
    /// pool's statuses in request order, or `None` where it was not measured.
    fn measured_sizes(&self, statuses: &[GpuStoneStatus]) -> Vec<Option<f32>> {
        let lods = self.definition_levels();
        self.requests
            .chunks(lods)
            .zip(statuses.chunks(lods))
            .map(|(requests, statuses)| {
                let (request, status) = (&requests[0], statuses.first()?);
                let measured = status.fits() && status.bounding_diameter > 0.0;
                measured.then_some(status.bounding_diameter * request.options.scale)
            })
            .collect()
    }

    /// Schedules generation with more room for every mesh that overflowed,
    /// unless the retry limit has been reached.
    fn grow_overflowed(&mut self, statuses: &[GpuStoneStatus]) {
        let mut overflowed = false;
        for (request, status) in self.requests.iter_mut().zip(statuses) {
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
/// back generation status, and lists every generated mesh for the shared
/// renderer.
pub(crate) fn prepare_pools(
    mut render_pools: ResMut<RenderPools>,
    extracted: Res<ExtractedPools>,
    measured: Res<MeasuredPoolSizes>,
    layout: Option<Res<PoolMeshLayout>>,
    mut external: ResMut<ExternalMeshes>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    let RenderPools {
        generator,
        pools,
        listed,
    } = &mut *render_pools;

    // Follow the main world's pools.
    let definitions = &extracted.definitions.pools;
    pools.retain(|id, _| definitions.contains_key(id));
    for (id, definition) in definitions {
        let current = pools
            .get(id)
            .is_some_and(|pool| pool.version == definition.version);
        if !current {
            pools.insert(*id, RenderPool::new(definition));
        }
    }

    // Generate pools that are new or need more room.
    for (id, pool) in pools.iter_mut() {
        if let Some(sizes) = pool.check_readback() {
            measured.post(*id, pool.version, sizes);
        }
        if pool.needs_generation && !pool.failed {
            let generator =
                generator.get_or_insert_with(|| GpuStoneGenerator::new(device.wgpu_device()));
            pool.generate(generator, &device, &queue);
        }
    }

    // List every generated mesh, in place of last frame's.
    for id in listed.drain() {
        external.0.remove(&id);
    }
    let Some(layout) = layout else {
        return;
    };
    for pool in pools.values() {
        for batch in &pool.batches {
            for mesh in 0..batch.mesh_count {
                let id = external_mesh_id(pool.version, batch.first_mesh + mesh);
                external.0.insert(
                    id,
                    ExternalMesh {
                        vertices: batch.vertices.clone(),
                        indices: batch.indices.clone(),
                        index_format: IndexFormat::Uint32,
                        args: batch.draw_args.clone(),
                        args_offset: u64::from(mesh) * GPU_DRAW_ARGS_SIZE,
                        layout: layout.0.clone(),
                    },
                );
                listed.insert(id);
            }
        }
    }
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
