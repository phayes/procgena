//! Generate grounded rock meshes with wgpu (requires the `gpu` feature).
//!
//! [`GpuStoneGenerator`] records compute work that writes a batch of rock
//! meshes into GPU buffers that stay on the GPU, ready for indirect drawing.
//! For the same settings, a GPU mesh describes the same surface as
//! [`mesh_field`](crate::mesh_field) on the CPU, placed by
//! [`StonePlacement`](crate::StonePlacement), but not necessarily the same
//! buffers: GPUs round differently (many drivers use fast math), so a mesh can
//! have a few more or fewer vertices, a different vertex order, and positions
//! that differ slightly. Do not size capacities exactly from CPU counts. A rock
//! whose resting sides are nearly equally stable can settle onto a different
//! side on the GPU than on the CPU.
//!
//! GPU meshes arrive placed approximately at ground level (`y = 0`), like
//! [`crate::Stone`] meshes, but in unscaled stone-space units. Apply
//! [`GpuMeshRange::scale`] when drawing. Settling optionally turns the rock
//! onto an estimated resting side; placement always lifts it to ground level.
//! Requests that differ only in mesh settings, material, or scale share one
//! placement, so a rock's detail levels line up. Each mesh's
//! [`GpuStoneStatus`] reports its suggested embed depth and bounding radius.
//! To bury a rock, lower it by `embed_depth * scale` metres.
//!
//! Each mesh needs room reserved in advance. If a mesh needs more room
//! than its request reserved, it draws nothing and its [`GpuStoneStatus`] reports
//! the counts needed, so you can retry with [`GpuStoneRequest::grow_to_fit`].
//!
//! ```no_run
//! # fn run(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<(), procgena_stone::gpu::GpuError> {
//! use procgena_stone::StoneOptions;
//! use procgena_stone::gpu::{GpuStoneGenerator, GpuStoneRequest};
//!
//! let generator = GpuStoneGenerator::new(device);
//! let requests: Vec<_> = (0..4)
//!     .map(|seed| GpuStoneRequest::new(StoneOptions { default_seed: seed, ..StoneOptions::default() }, 96))
//!     .collect();
//! let mut encoder = device.create_command_encoder(&Default::default());
//! let batch = generator.encode(device, &mut encoder, &requests)?;
//! queue.submit([encoder.finish()]);
//! // Draw mesh `i` with `draw_indexed_indirect(&batch.draw_args, batch.ranges[i].draw_args_offset)`,
//! // binding `batch.vertices` and `batch.indices` (32-bit indices).
//! # Ok(())
//! # }
//! ```

use std::borrow::Cow;
use std::fmt;
use std::num::NonZeroU64;

use wgpu::util::{BufferInitDescriptor, DeviceExt};

use crate::field::StoneField;
use crate::options::{
    MaterialOptions, MeshOptions, Mesher, OptionsError, StoneOptions, check_resolution,
};
use crate::placement::{
    ANALYSIS_RESOLUTION, DIRECTION_COUNT, REFINE_CANDIDATES, REFINE_ROUNDS, profile_bins,
};
use crate::shaders::{
    COMPILED_WGSL, DRAW_ARGS_SIZE, MAX_PROFILE_BINS, OVERFLOW_CROSSINGS, OVERFLOW_INDICES,
    OVERFLOW_VERTICES, REQUEST_SIZE, RequestSlots, SCAN_BLOCK, STATUS_SIZE, VERTEX_SIZE,
    WORKGROUP_SIZE, pack_request, packed_tri_table, place_scratch_words,
};

/// Highest mesh resolution the GPU generator accepts.
///
/// Higher resolutions need scratch buffers larger than the 128 MiB storage
/// buffer limit that every WebGPU device supports. Use the CPU generator for
/// resolutions up to [`crate::MAX_RESOLUTION`].
pub const MAX_GPU_RESOLUTION: u32 = 254;

/// Byte size of one vertex in [`GpuStoneBatch::vertices`]: position then
/// outward unit normal, each three `f32` values.
pub const GPU_VERTEX_SIZE: u64 = VERTEX_SIZE;

/// Byte size of one draw entry in [`GpuStoneBatch::draw_args`], laid out as
/// wgpu's `DrawIndexedIndirectArgs`.
pub const GPU_DRAW_ARGS_SIZE: u64 = DRAW_ARGS_SIZE;

/// Byte size of one entry in [`GpuStoneBatch::status`]; see [`GpuStoneStatus::decode`].
pub const GPU_STATUS_SIZE: u64 = STATUS_SIZE;

/// Returns `(vertex_capacity, index_capacity)` that typical rocks at
/// `resolution` cells per axis fit within, for either meshing method.
///
/// The estimate allows for a plain sphere filling most of the cube. Unusually
/// detailed settings, such as a very high `displacement_spread`, can need more;
/// check [`GpuStoneStatus`] and retry when a mesh overflows.
pub fn estimate_capacity(resolution: u32) -> (u32, u32) {
    let r = resolution as u64;
    let vertices = (9 * r * r).div_ceil(2);
    let indices = 6 * vertices;
    (
        vertices.min(u32::MAX as u64) as u32,
        indices.min(u32::MAX as u64) as u32,
    )
}

/// One rock mesh to generate on the GPU.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuStoneRequest {
    /// Rock settings, including the meshing method in [`StoneOptions::mesh`].
    /// [`MeshOptions::game_level`] is validated but not used; `resolution` sets the mesh detail.
    pub options: StoneOptions,
    /// Grid cells per axis for this mesh, from [`crate::MIN_RESOLUTION`]
    /// through [`MAX_GPU_RESOLUTION`]. To build the game-level or lower mesh,
    /// pass that detail's [`StoneDetail::resolution`](crate::StoneDetail::resolution)
    /// (see [`StoneDetail::lod`](crate::StoneDetail::lod)).
    pub resolution: u32,
    /// Vertices reserved for this mesh in the batch's vertex buffer.
    pub vertex_capacity: u32,
    /// Indices reserved for this mesh in the batch's index buffer.
    pub index_capacity: u32,
}

impl GpuStoneRequest {
    /// Creates a request for a mesh at `resolution` cells per axis, with
    /// capacities from [`estimate_capacity`].
    pub fn new(options: StoneOptions, resolution: u32) -> Self {
        let (vertex_capacity, index_capacity) = estimate_capacity(resolution);
        Self {
            options,
            resolution,
            vertex_capacity,
            index_capacity,
        }
    }

    /// Raises the capacities to at least the counts `status` reports, so a
    /// retry of an overflowed mesh fits. Capacities never shrink.
    pub fn grow_to_fit(&mut self, status: &GpuStoneStatus) {
        self.vertex_capacity = self.vertex_capacity.max(status.vertex_count);
        self.index_capacity = self.index_capacity.max(status.index_count);
    }
}

/// Where one generated mesh lives in a [`GpuStoneBatch`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpuMeshRange {
    /// First vertex of this mesh in [`GpuStoneBatch::vertices`]. Indices are
    /// relative to it; the draw arguments already use it as the base vertex.
    pub vertex_base: u32,
    /// Vertices reserved for this mesh.
    pub vertex_capacity: u32,
    /// First index of this mesh in [`GpuStoneBatch::indices`].
    pub index_base: u32,
    /// Indices reserved for this mesh.
    pub index_capacity: u32,
    /// Byte offset of this mesh's entry in [`GpuStoneBatch::draw_args`].
    pub draw_args_offset: u64,
    /// Byte offset of this mesh's entry in [`GpuStoneBatch::status`].
    pub status_offset: u64,
    /// Multiplier from unscaled mesh coordinates to metres, equal to
    /// [`StoneOptions::scale`]. Apply when drawing and when converting embed
    /// depth or bounding diameter to metres. The placed mesh can extend
    /// beyond the original unit cube; see
    /// [`GpuStoneStatus::bounding_diameter`].
    pub scale: f32,
    /// Meshing method used.
    pub mesher: Mesher,
    /// Grid cells per axis used.
    pub resolution: u32,
}

/// GPU buffers holding a batch of generated meshes, filled once the recorded
/// commands run.
///
/// Every buffer also allows `COPY_SRC`, so it can be copied or read back.
#[derive(Debug)]
pub struct GpuStoneBatch {
    /// Vertices of every mesh ([`GPU_VERTEX_SIZE`] bytes each). Usable as a
    /// vertex buffer and as a storage buffer.
    pub vertices: wgpu::Buffer,
    /// 32-bit triangle indices of every mesh, with outward-facing
    /// counter-clockwise winding. Usable as an index buffer and as a storage buffer.
    pub indices: wgpu::Buffer,
    /// One indexed indirect draw per mesh ([`GPU_DRAW_ARGS_SIZE`] bytes each),
    /// with an instance count of one. A mesh that overflowed draws no indices.
    /// Usable for indirect draws and as a storage buffer.
    pub draw_args: wgpu::Buffer,
    /// One status entry per mesh ([`GPU_STATUS_SIZE`] bytes each). Copy it to
    /// a mappable buffer and decode it with [`GpuStoneStatus::decode`].
    pub status: wgpu::Buffer,
    /// Location of each mesh, in request order.
    pub ranges: Vec<GpuMeshRange>,
}

/// A generated mesh's required capacity, suggested burial depth, and bounding diameter.
/// Read these values from [`GpuStoneBatch::status`] with [`Self::decode`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GpuStoneStatus {
    /// The mesh needed more vertices than its request reserved.
    pub vertex_overflow: bool,
    /// The mesh needed more indices than its request reserved.
    pub index_overflow: bool,
    /// Vertices the complete mesh has.
    pub vertex_count: u32,
    /// Indices the complete mesh has.
    pub index_count: u32,
    /// Suggested burial depth in unscaled units. Multiply by
    /// [`GpuMeshRange::scale`] and subtract from the rock's world y position
    /// to apply it; the generated mesh is not buried automatically. See
    /// [`StonePlacement::embed_depth`](crate::StonePlacement::embed_depth).
    pub embed_depth: f32,
    /// Diagonal of the placed mesh's axis-aligned bounding box, in
    /// unscaled units: the diameter of a sphere holding the mesh, the same
    /// measure as [`Stone::bounding_diameter`](crate::Stone::bounding_diameter).
    /// Multiply by [`GpuMeshRange::scale`] for metres. Zero for a mesh that
    /// did not fit or has no vertices.
    pub bounding_diameter: f32,
}

impl GpuStoneStatus {
    /// Returns `true` when the mesh fitted and was drawn.
    pub fn fits(&self) -> bool {
        !self.vertex_overflow && !self.index_overflow
    }

    /// Decodes the bytes of a [`GpuStoneBatch::status`] buffer, one entry per
    /// [`GPU_STATUS_SIZE`] bytes in request order. Trailing bytes are ignored.
    pub fn decode(bytes: &[u8]) -> Vec<Self> {
        bytes
            .as_chunks::<{ STATUS_SIZE as usize }>()
            .0
            .iter()
            .map(|entry| {
                let word = |i: usize| u32::from_le_bytes(entry.as_chunks::<4>().0[i]);
                let float = |i: usize| f32::from_bits(word(i));
                let overflow = word(0);
                // The padded rotation occupies words 4–15; offset is 16–18,
                // depth is 19 and the bounding diameter 20.
                Self {
                    vertex_overflow: overflow & OVERFLOW_VERTICES != 0,
                    index_overflow: overflow & (OVERFLOW_INDICES | OVERFLOW_CROSSINGS) != 0,
                    vertex_count: word(1),
                    index_count: word(2),
                    embed_depth: float(19),
                    bounding_diameter: float(20),
                }
            })
            .collect()
    }
}

/// A request that the GPU generator cannot run.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum GpuError {
    /// A request's settings failed [`StoneOptions::validate`].
    Options {
        /// Position of the request in the batch.
        request: usize,
        /// The invalid setting.
        error: OptionsError,
    },
    /// A request's resolution is above [`MAX_GPU_RESOLUTION`].
    ResolutionTooHigh {
        /// Position of the request in the batch.
        request: usize,
        /// The rejected resolution.
        resolution: u32,
    },
    /// The batch needs a buffer larger than the device allows. Split the batch
    /// or lower the capacities.
    BufferTooLarge {
        /// Which buffer, for example `"vertices"`.
        buffer: &'static str,
        /// Bytes needed.
        size: u64,
        /// Largest size the device allows for it.
        limit: u64,
    },
}

impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Options { request, error } => write!(f, "request {request}: {error}"),
            Self::ResolutionTooHigh {
                request,
                resolution,
            } => write!(
                f,
                "request {request}: resolution is {resolution}, but the GPU generator allows at most {MAX_GPU_RESOLUTION}"
            ),
            Self::BufferTooLarge {
                buffer,
                size,
                limit,
            } => write!(
                f,
                "the {buffer} buffer needs {size} bytes, but the device allows {limit}"
            ),
        }
    }
}

impl std::error::Error for GpuError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Options { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// Compute pipelines that generate rock meshes on one wgpu device.
///
/// Create one per device and reuse it for every batch.
#[derive(Debug)]
pub struct GpuStoneGenerator {
    layouts: Layouts,
    pipelines: Pipelines,
    tri_table: wgpu::Buffer,
}

#[derive(Debug)]
struct Layouts {
    generate: wgpu::BindGroupLayout,
    mc: wgpu::BindGroupLayout,
    dc: wgpu::BindGroupLayout,
    scan: wgpu::BindGroupLayout,
    place: wgpu::BindGroupLayout,
}

#[derive(Debug)]
struct Pipelines {
    sample_field: wgpu::ComputePipeline,
    count_mc: wgpu::ComputePipeline,
    emit_mc_vertices: wgpu::ComputePipeline,
    emit_mc_indices: wgpu::ComputePipeline,
    finish_mc: wgpu::ComputePipeline,
    measure_mc: wgpu::ComputePipeline,
    count_dc: wgpu::ComputePipeline,
    emit_dc_crossings: wgpu::ComputePipeline,
    emit_dc_vertices: wgpu::ComputePipeline,
    emit_dc_indices: wgpu::ComputePipeline,
    finish_dc: wgpu::ComputePipeline,
    measure_dc: wgpu::ComputePipeline,
    scan_blocks: wgpu::ComputePipeline,
    add_block_offsets: wgpu::ComputePipeline,
    place_com_slices: wgpu::ComputePipeline,
    place_com: wgpu::ComputePipeline,
    place_heights: wgpu::ComputePipeline,
    place_descend: wgpu::ComputePipeline,
    place_refine_heights: wgpu::ComputePipeline,
    place_refine_pick: wgpu::ComputePipeline,
    place_finish: wgpu::ComputePipeline,
    place_profile_slices: wgpu::ComputePipeline,
    place_embed: wgpu::ComputePipeline,
}

/// Byte size of the `ScanParams` uniform slot bound for each scan level.
const SCAN_PARAMS_SIZE: u64 = 16;

impl GpuStoneGenerator {
    /// Compiles the generator's compute pipelines for `device`.
    ///
    /// The device's default WebGPU limits are enough.
    pub fn new(device: &wgpu::Device) -> Self {
        let layouts = Layouts::new(device);

        // Build each module's pipelines against its bind group layout.
        let module = |name: &str| {
            let (_, wgsl) = COMPILED_WGSL
                .iter()
                .find(|(module, _)| *module == name)
                .expect("every generator module is compiled");
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&format!("procgena_stone {name}")),
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(wgsl)),
            })
        };
        let pipeline_layout = |layout: &wgpu::BindGroupLayout| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("procgena_stone generator"),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            })
        };
        let pipeline = |module: &wgpu::ShaderModule, layout: &wgpu::PipelineLayout, entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(&format!("procgena_stone {entry}")),
                layout: Some(layout),
                module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let (generate, mc, dc, scan, place) = (
            module("generate"),
            module("mc"),
            module("dc"),
            module("scan"),
            module("place"),
        );
        let generate_layout = pipeline_layout(&layouts.generate);
        let mc_layout = pipeline_layout(&layouts.mc);
        let dc_layout = pipeline_layout(&layouts.dc);
        let scan_layout = pipeline_layout(&layouts.scan);
        let place_layout = pipeline_layout(&layouts.place);
        let pipelines = Pipelines {
            sample_field: pipeline(&generate, &generate_layout, "sample_field"),
            count_mc: pipeline(&mc, &mc_layout, "count_mc"),
            emit_mc_vertices: pipeline(&mc, &mc_layout, "emit_mc_vertices"),
            emit_mc_indices: pipeline(&mc, &mc_layout, "emit_mc_indices"),
            finish_mc: pipeline(&mc, &mc_layout, "finish_mc"),
            measure_mc: pipeline(&mc, &mc_layout, "measure_mc"),
            count_dc: pipeline(&dc, &dc_layout, "count_dc"),
            emit_dc_crossings: pipeline(&dc, &dc_layout, "emit_dc_crossings"),
            emit_dc_vertices: pipeline(&dc, &dc_layout, "emit_dc_vertices"),
            emit_dc_indices: pipeline(&dc, &dc_layout, "emit_dc_indices"),
            finish_dc: pipeline(&dc, &dc_layout, "finish_dc"),
            measure_dc: pipeline(&dc, &dc_layout, "measure_dc"),
            scan_blocks: pipeline(&scan, &scan_layout, "scan_blocks"),
            add_block_offsets: pipeline(&scan, &scan_layout, "add_block_offsets"),
            place_com_slices: pipeline(&place, &place_layout, "place_com_slices"),
            place_com: pipeline(&place, &place_layout, "place_com"),
            place_heights: pipeline(&place, &place_layout, "place_heights"),
            place_descend: pipeline(&place, &place_layout, "place_descend"),
            place_refine_heights: pipeline(&place, &place_layout, "place_refine_heights"),
            place_refine_pick: pipeline(&place, &place_layout, "place_refine_pick"),
            place_finish: pipeline(&place, &place_layout, "place_finish"),
            place_profile_slices: pipeline(&place, &place_layout, "place_profile_slices"),
            place_embed: pipeline(&place, &place_layout, "place_embed"),
        };

        let tri_table: Vec<u8> = packed_tri_table()
            .into_iter()
            .flatten()
            .flat_map(u32::to_le_bytes)
            .collect();
        let tri_table = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("procgena_stone tri_table"),
            contents: &tri_table,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        Self {
            layouts,
            pipelines,
            tri_table,
        }
    }

    /// Records the compute passes that generate every request into `encoder`,
    /// and returns the output buffers they write.
    ///
    /// The buffers hold the meshes once `encoder` has been submitted and has
    /// run. `device` must be the device this generator was created for. An
    /// empty `requests` slice records nothing and returns empty ranges.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError::Options`] or [`GpuError::ResolutionTooHigh`] for the
    /// first request that cannot run, and [`GpuError::BufferTooLarge`] when the
    /// batch exceeds the device's buffer limits. Nothing is recorded on error.
    pub fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        requests: &[GpuStoneRequest],
    ) -> Result<GpuStoneBatch, GpuError> {
        Ok(self.encode_with_scratch(device, encoder, requests)?.0)
    }

    /// [`Self::encode`], also returning the scratch buffers for inspection.
    fn encode_with_scratch(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        requests: &[GpuStoneRequest],
    ) -> Result<(GpuStoneBatch, Scratch), GpuError> {
        let limits = device.limits();
        let plan = BatchPlan::new(requests, &BufferLimits::from(&limits))?;
        let uniform_alignment = limits.min_uniform_buffer_offset_alignment as u64;

        // Create the outputs, the per-request uniforms, and the shared scratch buffers.
        let output = |label: &str, size: u64, usage: wgpu::BufferUsages| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: usage | wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        };
        let vertices = output(
            "procgena_stone vertices",
            plan.sizes.vertices,
            wgpu::BufferUsages::VERTEX,
        );
        let indices = output(
            "procgena_stone indices",
            plan.sizes.indices,
            wgpu::BufferUsages::INDEX,
        );
        let draw_args = output(
            "procgena_stone draw_args",
            plan.sizes.draw_args,
            wgpu::BufferUsages::INDIRECT,
        );
        let status = output(
            "procgena_stone status",
            plan.sizes.status,
            wgpu::BufferUsages::empty(),
        );
        let scratch = Scratch {
            samples: output(
                "procgena_stone samples",
                plan.sizes.samples,
                wgpu::BufferUsages::empty(),
            ),
            prefix: output(
                "procgena_stone prefix",
                plan.sizes.prefix,
                wgpu::BufferUsages::empty(),
            ),
            crossings: output(
                "procgena_stone crossings",
                plan.sizes.crossings,
                wgpu::BufferUsages::empty(),
            ),
            place: output(
                "procgena_stone place",
                plan.sizes.place,
                wgpu::BufferUsages::empty(),
            ),
        };

        // Store analysis settings before mesh settings in the uniform buffer.
        let request_stride = REQUEST_SIZE.next_multiple_of(uniform_alignment as usize);
        let uniforms: Vec<&[u8]> = plan
            .analyses
            .iter()
            .map(|a| a.uniform.as_slice())
            .chain(plan.requests.iter().map(|r| r.uniform.as_slice()))
            .collect();
        let mut request_bytes = vec![0; request_stride * uniforms.len().max(1)];
        for (slot, uniform) in request_bytes
            .chunks_exact_mut(request_stride)
            .zip(&uniforms)
        {
            slot[..REQUEST_SIZE].copy_from_slice(uniform);
        }
        let request_buffer = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("procgena_stone requests"),
            contents: &request_bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let scan_counts: Vec<u32> = plan
            .analyses
            .iter()
            .map(|_| ANALYSIS_SAMPLES + 1)
            .chain(plan.requests.iter().map(|r| r.samples + 1))
            .collect();
        let scan = ScanResources::new(
            device,
            &self.layouts.scan,
            &scratch.prefix,
            plan.max_samples + 1,
            &scan_counts,
            uniform_alignment,
        );

        // Bind every buffer each module uses; the request uniform moves per request.
        let request_binding = wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer: &request_buffer,
            offset: 0,
            size: NonZeroU64::new(REQUEST_SIZE as u64),
        });
        fn entry(binding: u32, resource: wgpu::BindingResource<'_>) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry { binding, resource }
        }
        let generate_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("procgena_stone generate"),
            layout: &self.layouts.generate,
            entries: &[
                entry(0, request_binding.clone()),
                entry(2, scratch.samples.as_entire_binding()),
            ],
        });
        let mc_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("procgena_stone mc"),
            layout: &self.layouts.mc,
            entries: &[
                entry(0, request_binding.clone()),
                entry(1, self.tri_table.as_entire_binding()),
                entry(2, scratch.samples.as_entire_binding()),
                entry(3, scratch.prefix.as_entire_binding()),
                entry(5, vertices.as_entire_binding()),
                entry(6, indices.as_entire_binding()),
                entry(7, draw_args.as_entire_binding()),
                entry(8, status.as_entire_binding()),
            ],
        });
        let place_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("procgena_stone place"),
            layout: &self.layouts.place,
            entries: &[
                entry(0, request_binding.clone()),
                entry(2, scratch.samples.as_entire_binding()),
                entry(3, scratch.prefix.as_entire_binding()),
                entry(4, scratch.crossings.as_entire_binding()),
                entry(8, status.as_entire_binding()),
                entry(9, scratch.place.as_entire_binding()),
            ],
        });
        let dc_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("procgena_stone dc"),
            layout: &self.layouts.dc,
            entries: &[
                entry(0, request_binding),
                entry(2, scratch.samples.as_entire_binding()),
                entry(3, scratch.prefix.as_entire_binding()),
                entry(4, scratch.crossings.as_entire_binding()),
                entry(5, vertices.as_entire_binding()),
                entry(6, indices.as_entire_binding()),
                entry(7, draw_args.as_entire_binding()),
                entry(8, status.as_entire_binding()),
            ],
        });

        // Run each request's passes in order; requests share the scratch buffers.
        if !plan.requests.is_empty() {
            let max_groups = limits.max_compute_workgroups_per_dimension;
            let p = &self.pipelines;
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("procgena_stone generate"),
                timestamp_writes: None,
            });

            // Estimate each shape's ground placement before generating any of its meshes.
            for a in 0..plan.analyses.len() {
                let offset = [(a * request_stride) as u32];
                let n = ANALYSIS_SAMPLES;
                let slices = ANALYSIS_RESOLUTION + 1;
                pass.set_pipeline(&p.sample_field);
                pass.set_bind_group(0, &generate_group, &offset);
                dispatch_items(&mut pass, n, max_groups);
                pass.set_pipeline(&p.count_dc);
                pass.set_bind_group(0, &dc_group, &offset);
                dispatch_items(&mut pass, n + 1, max_groups);
                scan.record(
                    &mut pass,
                    &p.scan_blocks,
                    &p.add_block_offsets,
                    a,
                    max_groups,
                );
                pass.set_bind_group(0, &dc_group, &offset);
                pass.set_pipeline(&p.emit_dc_crossings);
                dispatch_items(&mut pass, n, max_groups);

                pass.set_bind_group(0, &place_group, &offset);
                let mut run = |pipeline: &wgpu::ComputePipeline, items: u32| {
                    pass.set_pipeline(pipeline);
                    dispatch_items(&mut pass, items, max_groups);
                };
                run(&p.place_com_slices, slices);
                run(&p.place_com, 1);
                run(&p.place_heights, DIRECTION_COUNT);
                run(&p.place_descend, 1);
                for _ in 0..REFINE_ROUNDS {
                    run(&p.place_refine_heights, REFINE_CANDIDATES);
                    run(&p.place_refine_pick, 1);
                }
                run(&p.place_finish, 1);
                run(&p.place_profile_slices, slices);
                run(&p.place_embed, 1);
            }

            // Generate each mesh and apply the shared ground rotation and offset.
            for (i, request) in plan.requests.iter().enumerate() {
                let uniform = plan.analyses.len() + i;
                let offset = [(uniform * request_stride) as u32];
                let n = request.samples;

                pass.set_pipeline(&p.sample_field);
                pass.set_bind_group(0, &generate_group, &offset);
                dispatch_items(&mut pass, n, max_groups);

                let (group, count, emits, finish, measure): (_, _, &[_], _, _) =
                    match request.mesher {
                        Mesher::MarchingCubes => (
                            &mc_group,
                            &p.count_mc,
                            &[&p.emit_mc_vertices, &p.emit_mc_indices],
                            &p.finish_mc,
                            &p.measure_mc,
                        ),
                        Mesher::DualContouring => (
                            &dc_group,
                            &p.count_dc,
                            &[
                                &p.emit_dc_crossings,
                                &p.emit_dc_vertices,
                                &p.emit_dc_indices,
                            ],
                            &p.finish_dc,
                            &p.measure_dc,
                        ),
                    };
                pass.set_pipeline(count);
                pass.set_bind_group(0, group, &offset);
                dispatch_items(&mut pass, n + 1, max_groups);

                scan.record(
                    &mut pass,
                    &p.scan_blocks,
                    &p.add_block_offsets,
                    uniform,
                    max_groups,
                );

                pass.set_bind_group(0, group, &offset);
                for emit in emits {
                    pass.set_pipeline(emit);
                    dispatch_items(&mut pass, n, max_groups);
                }
                pass.set_pipeline(finish);
                pass.dispatch_workgroups(1, 1, 1);
                pass.set_pipeline(measure);
                pass.dispatch_workgroups(1, 1, 1);
            }
        }

        let batch = GpuStoneBatch {
            vertices,
            indices,
            draw_args,
            status,
            ranges: plan.requests.iter().map(|r| r.range).collect(),
        };
        Ok((batch, scratch))
    }
}

impl Layouts {
    fn new(device: &wgpu::Device) -> Self {
        let buffer = |binding: u32, ty: wgpu::BufferBindingType, dynamic: bool, size: u64| {
            wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty,
                    has_dynamic_offset: dynamic,
                    min_binding_size: NonZeroU64::new(size),
                },
                count: None,
            }
        };
        let storage = |binding: u32| {
            buffer(
                binding,
                wgpu::BufferBindingType::Storage { read_only: false },
                false,
                0,
            )
        };
        let uniform = wgpu::BufferBindingType::Uniform;
        let request = buffer(0, uniform, true, REQUEST_SIZE as u64);
        let layout = |label: &str, entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries,
            })
        };
        Self {
            generate: layout("procgena_stone generate", &[request, storage(2)]),
            mc: layout(
                "procgena_stone mc",
                &[
                    request,
                    buffer(1, uniform, false, 16 * 256),
                    storage(2),
                    storage(3),
                    storage(5),
                    storage(6),
                    storage(7),
                    storage(8),
                ],
            ),
            dc: layout(
                "procgena_stone dc",
                &[
                    request,
                    storage(2),
                    storage(3),
                    storage(4),
                    storage(5),
                    storage(6),
                    storage(7),
                    storage(8),
                ],
            ),
            scan: layout(
                "procgena_stone scan",
                &[
                    buffer(0, uniform, true, SCAN_PARAMS_SIZE),
                    storage(1),
                    storage(2),
                ],
            ),
            place: layout(
                "procgena_stone place",
                &[
                    request,
                    storage(2),
                    storage(3),
                    storage(4),
                    storage(8),
                    storage(9),
                ],
            ),
        }
    }
}

/// Scratch buffers shared by the requests of one batch.
struct Scratch {
    /// Field values of the last request's grid samples.
    samples: wgpu::Buffer,
    /// Counts, then their prefix sums, for the last request.
    prefix: wgpu::Buffer,
    /// Surface points from the last dual-contouring mesh or placement analysis.
    crossings: wgpu::Buffer,
    /// Temporary values left by the last ground-placement analysis.
    place: wgpu::Buffer,
}

/// Buffers and bind groups for exclusive prefix sums over a `vec2<u32>` buffer,
/// one sum per entry of `counts`, run one after another.
struct ScanResources {
    /// Bind group for each level: level 0 scans the data, level `k` scans the
    /// block totals of level `k - 1`.
    groups: Vec<wgpu::BindGroup>,
    /// Entries scanned at each level, per scan.
    levels: Vec<Vec<u32>>,
    params_stride: u64,
}

impl ScanResources {
    /// Prepares scans of the first `counts[i]` entries of `data`, which must
    /// hold at least `max_count` entries; every count must be at most
    /// `max_count` and at least one.
    fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        data: &wgpu::Buffer,
        max_count: u32,
        counts: &[u32],
        uniform_alignment: u64,
    ) -> Self {
        // One block-totals buffer per level of the largest scan.
        let max_levels = scan_levels(max_count);
        let sums: Vec<wgpu::Buffer> = max_levels
            .iter()
            .map(|&count| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("procgena_stone scan sums"),
                    size: 8 * count.div_ceil(SCAN_BLOCK) as u64,
                    usage: wgpu::BufferUsages::STORAGE,
                    mapped_at_creation: false,
                })
            })
            .collect();

        // One parameter slot per (scan, level), and a bind group per level.
        let params_stride = SCAN_PARAMS_SIZE.next_multiple_of(uniform_alignment);
        let levels: Vec<Vec<u32>> = counts.iter().map(|&c| scan_levels(c)).collect();
        let mut params = vec![0; (params_stride as usize) * max_levels.len() * counts.len().max(1)];
        for (i, scan) in levels.iter().enumerate() {
            for (level, &count) in scan.iter().enumerate() {
                let at = Self::slot(i, level, max_levels.len(), params_stride) as usize;
                params[at..at + 4].copy_from_slice(&count.to_le_bytes());
            }
        }
        let params = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("procgena_stone scan params"),
            contents: &params,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let groups = (0..max_levels.len())
            .map(|level| {
                let data = if level == 0 { data } else { &sums[level - 1] };
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("procgena_stone scan"),
                    layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &params,
                                offset: 0,
                                size: NonZeroU64::new(SCAN_PARAMS_SIZE),
                            }),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: data.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: sums[level].as_entire_binding(),
                        },
                    ],
                })
            })
            .collect();
        Self {
            groups,
            levels,
            params_stride,
        }
    }

    /// Byte offset of the parameter slot for `scan` at `level`.
    fn slot(scan: usize, level: usize, levels_per_scan: usize, stride: u64) -> u64 {
        (scan * levels_per_scan + level) as u64 * stride
    }

    /// Records scan number `scan`: block sums up the levels, then block offsets
    /// back down.
    fn record(
        &self,
        pass: &mut wgpu::ComputePass<'_>,
        scan_blocks: &wgpu::ComputePipeline,
        add_block_offsets: &wgpu::ComputePipeline,
        scan: usize,
        max_groups: u32,
    ) {
        let levels = &self.levels[scan];
        let offset =
            |level: usize| [Self::slot(scan, level, self.groups.len(), self.params_stride) as u32];
        pass.set_pipeline(scan_blocks);
        for (level, &count) in levels.iter().enumerate() {
            pass.set_bind_group(0, &self.groups[level], &offset(level));
            dispatch_groups(pass, count.div_ceil(SCAN_BLOCK), max_groups);
        }
        pass.set_pipeline(add_block_offsets);
        for (level, &count) in levels.iter().enumerate().rev().skip(1) {
            pass.set_bind_group(0, &self.groups[level], &offset(level));
            dispatch_groups(pass, count.div_ceil(SCAN_BLOCK), max_groups);
        }
    }
}

/// Entries scanned at each level of a prefix sum over `count` entries: the
/// data, then each level's block totals, ending with a level of one block.
fn scan_levels(count: u32) -> Vec<u32> {
    let mut levels = vec![count];
    let mut count = count;
    while count > SCAN_BLOCK {
        count = count.div_ceil(SCAN_BLOCK);
        levels.push(count);
    }
    levels
}

/// Dispatches enough `WORKGROUP_SIZE`-wide workgroups to cover `items`.
fn dispatch_items(pass: &mut wgpu::ComputePass<'_>, items: u32, max_groups: u32) {
    dispatch_groups(pass, items.div_ceil(WORKGROUP_SIZE), max_groups);
}

/// Dispatches at least `groups` workgroups as rows along x, adding rows along y
/// when one row is not enough. The shaders skip workgroups past the end.
fn dispatch_groups(pass: &mut wgpu::ComputePass<'_>, groups: u32, max_groups: u32) {
    let x = groups.clamp(1, max_groups);
    pass.dispatch_workgroups(x, groups.div_ceil(x), 1);
}

/// Device buffer limits that a batch must fit within.
#[derive(Debug, Clone, Copy)]
struct BufferLimits {
    /// Largest storage buffer binding, also capped by the largest buffer.
    storage: u64,
}

impl From<&wgpu::Limits> for BufferLimits {
    fn from(limits: &wgpu::Limits) -> Self {
        Self {
            storage: limits
                .max_storage_buffer_binding_size
                .min(limits.max_buffer_size),
        }
    }
}

/// Validated requests with their output slots, and the buffer sizes they need.
#[derive(Debug)]
struct BatchPlan {
    requests: Vec<PlannedRequest>,
    /// Analyses shared by requests with identical shape and placement settings.
    analyses: Vec<PlannedAnalysis>,
    /// Sample capacity needed by the largest mesh or placement grid in the batch.
    max_samples: u32,
    sizes: BufferSizes,
}

/// Settings for one ground-placement analysis at [`ANALYSIS_RESOLUTION`].
/// Stores its result in the first matching mesh request's status entry.
#[derive(Debug)]
struct PlannedAnalysis {
    /// Packed `Request` uniform.
    uniform: Vec<u8>,
}

#[derive(Debug)]
struct PlannedRequest {
    mesher: Mesher,
    /// Grid samples, `(resolution + 1)³`.
    samples: u32,
    /// Packed `Request` uniform.
    uniform: Vec<u8>,
    range: GpuMeshRange,
}

/// Byte sizes of every buffer in a batch. Each is at least one entry, since
/// empty buffers cannot be bound.
#[derive(Debug)]
struct BufferSizes {
    vertices: u64,
    indices: u64,
    draw_args: u64,
    status: u64,
    samples: u64,
    prefix: u64,
    crossings: u64,
    place: u64,
}

impl BatchPlan {
    fn new(requests: &[GpuStoneRequest], limits: &BufferLimits) -> Result<Self, GpuError> {
        // Validate each request and give it consecutive output slots.
        let mut planned = Vec::with_capacity(requests.len());
        let mut analyses = Vec::new();
        let mut shapes: Vec<(StoneOptions, u32)> = Vec::new();
        let (mut vertex_total, mut index_total) = (0u64, 0u64);
        let mut max_samples = 0;
        let mut max_crossings = 0;
        for (i, request) in requests.iter().enumerate() {
            let options = &request.options;
            let field = StoneField::new(options)
                .map_err(|error| GpuError::Options { request: i, error })?;

            // Reuse placement when shape and placement settings match across requests.
            let shape = shape_key(options);
            let placement_index = match shapes.iter().position(|(key, _)| *key == shape) {
                Some(at) => shapes[at].1,
                None => {
                    let slot = i as u32;
                    let crossing_capacity = estimate_capacity(ANALYSIS_RESOLUTION).1 / 6;
                    let slots = RequestSlots {
                        request_index: slot,
                        crossing_capacity,
                        placement_index: slot,
                        ..RequestSlots::default()
                    };
                    analyses.push(PlannedAnalysis {
                        uniform: pack_request(
                            &field,
                            ANALYSIS_RESOLUTION,
                            &slots,
                            &options.placement,
                        ),
                    });
                    max_samples = max_samples.max(ANALYSIS_SAMPLES);
                    max_crossings = max_crossings.max(crossing_capacity as u64);
                    shapes.push((shape, slot));
                    slot
                }
            };

            let resolution = request.resolution;
            check_resolution("resolution", resolution)
                .map_err(|error| GpuError::Options { request: i, error })?;
            if resolution > MAX_GPU_RESOLUTION {
                return Err(GpuError::ResolutionTooHigh {
                    request: i,
                    resolution,
                });
            }
            let crossing_capacity = match options.mesh.mesher {
                Mesher::MarchingCubes => 0,
                Mesher::DualContouring => request.index_capacity / 6,
            };
            let slots = RequestSlots {
                request_index: i as u32,
                vertex_base: u32::try_from(vertex_total).unwrap_or(u32::MAX),
                vertex_capacity: request.vertex_capacity,
                index_base: u32::try_from(index_total).unwrap_or(u32::MAX),
                index_capacity: request.index_capacity,
                crossing_capacity,
                placement_index,
            };
            let samples = (resolution + 1).pow(3);
            max_samples = max_samples.max(samples);
            max_crossings = max_crossings.max(crossing_capacity as u64);
            planned.push(PlannedRequest {
                mesher: options.mesh.mesher,
                samples,
                uniform: pack_request(&field, resolution, &slots, &options.placement),
                range: GpuMeshRange {
                    vertex_base: slots.vertex_base,
                    vertex_capacity: slots.vertex_capacity,
                    index_base: slots.index_base,
                    index_capacity: slots.index_capacity,
                    draw_args_offset: i as u64 * DRAW_ARGS_SIZE,
                    status_offset: i as u64 * STATUS_SIZE,
                    scale: options.scale,
                    mesher: options.mesh.mesher,
                    resolution,
                },
            });
            vertex_total += request.vertex_capacity as u64;
            index_total += request.index_capacity as u64;
        }

        // Check every buffer against the device limits.
        let count = requests.len().max(1) as u64;
        let sizes = BufferSizes {
            vertices: vertex_total.max(1) * VERTEX_SIZE,
            indices: index_total.max(1) * 4,
            draw_args: count * DRAW_ARGS_SIZE,
            status: count * STATUS_SIZE,
            samples: (max_samples.max(1) as u64) * 4,
            prefix: (max_samples as u64 + 1) * 8,
            crossings: max_crossings.max(1) * VERTEX_SIZE,
            place: place_scratch_words(ANALYSIS_RESOLUTION) * 4,
        };
        for (buffer, size) in [
            ("vertices", sizes.vertices),
            ("indices", sizes.indices),
            ("draw_args", sizes.draw_args),
            ("status", sizes.status),
            ("samples", sizes.samples),
            ("prefix", sizes.prefix),
            ("crossings", sizes.crossings),
            ("place", sizes.place),
        ] {
            if size > limits.storage {
                return Err(GpuError::BufferTooLarge {
                    buffer,
                    size,
                    limit: limits.storage,
                });
            }
        }
        Ok(Self {
            requests: planned,
            analyses,
            max_samples,
            sizes,
        })
    }
}

/// Number of sample points in the fixed ground-placement grid, including its boundary.
const ANALYSIS_SAMPLES: u32 = (ANALYSIS_RESOLUTION + 1).pow(3);

// Reject an analysis resolution whose horizontal slices exceed shader storage.
const _: () = assert!(profile_bins(ANALYSIS_RESOLUTION) <= MAX_PROFILE_BINS);

/// Returns a comparison key retaining shape and placement settings while
/// ignoring mesh detail, material, and world scale.
fn shape_key(options: &StoneOptions) -> StoneOptions {
    StoneOptions {
        scale: 1.0,
        mesh: MeshOptions::default(),
        material: MaterialOptions::default(),
        ..options.clone()
    }
}

#[cfg(test)]
mod tests;
