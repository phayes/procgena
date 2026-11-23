//! Generate clumps with wgpu (requires the `gpu` feature).
//!
//! [`GpuClumpGenerator`] records compute work that builds a batch of clump
//! meshes on top of input meshes that are already on the GPU, such as
//! `procgena_stone`'s GPU rocks. The results stay on the GPU, ready for
//! indirect drawing. For the same input and settings, a GPU clump describes
//! the same surface as [`Clump::new`](crate::Clump::new) on the CPU, but not
//! necessarily the same buffers: GPU rounding can change a few vertices, and
//! a decision that falls right at a threshold can go the other way.
//!
//! The GPU builds scattered clumps ([`Placement::Scatter`]) and layers
//! ([`Placement::Everywhere`]), open or closed, in strict mode too. Strict
//! mode on the GPU has a fixed number of passes: a layer that still crosses
//! itself after them reports it in [`GpuClumpStatus::crossings`].
//!
//! Each request reserves room for its output in advance. A request that
//! needs more draws nothing, and its [`GpuClumpStatus`] reports the counts
//! needed, so you can retry with [`GpuClumpRequest::grow_to_fit`].
//!
//! [`Placement::Scatter`]: crate::Placement::Scatter
//! [`Placement::Everywhere`]: crate::Placement::Everywhere

mod batch;
mod params;
mod scan;
mod sort;
mod stages;

use std::fmt;

use crate::blob::{BOTTOM_GAP, MIN_RINGS, MIN_SEGMENTS};
use crate::options::{ClumpOptions, OptionsError, Placement};
use batch::{OVERFLOW_INDICES, OVERFLOW_INPUT, OVERFLOW_VERTICES};
use stages::{
    BatchResources, DRAW_ARGS_SIZE, OUT_VERTEX_SIZE, Pipelines, STATUS_SIZE, StageRequest,
};

#[cfg(test)]
mod tests;

/// Byte sizes of the generator's packed structs, for layout tests.
#[cfg(test)]
pub(crate) mod layout_sizes {
    pub(crate) const REQUEST: usize = super::batch::REQUEST_SIZE;
    pub(crate) const BATCH: usize = super::batch::BATCH_SIZE;
}

/// Where the position and normal of each vertex are in an input vertex
/// buffer. All values are in bytes and must be multiples of 4; positions
/// and normals are three `f32` values each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuVertexFormat {
    pub stride: u32,
    pub position_offset: u32,
    pub normal_offset: u32,
}

impl GpuVertexFormat {
    /// Position then normal, 24 bytes per vertex: the layout of
    /// `procgena_stone`'s GPU meshes.
    pub const POSITION_NORMAL: Self = Self {
        stride: 24,
        position_offset: 0,
        normal_offset: 12,
    };
}

/// An input mesh that is already on the GPU.
///
/// Every buffer must have been created with `STORAGE` usage. The mesh is
/// drawn by one `DrawIndexedIndirectArgs` entry: its index count, first
/// index and base vertex say which triangles to read, and those counts are
/// only read on the GPU.
#[derive(Debug, Clone, Copy)]
pub struct GpuSurfaceInput<'a> {
    /// Vertices laid out as `format` says.
    pub vertices: &'a wgpu::Buffer,
    /// `u32` triangle indices, counter-clockwise seen from outside.
    pub indices: &'a wgpu::Buffer,
    /// Holds the `DrawIndexedIndirectArgs` entry at `draw_args_offset`
    /// bytes, which must be a multiple of 4.
    pub draw_args: &'a wgpu::Buffer,
    pub draw_args_offset: u64,
    pub format: GpuVertexFormat,
    /// Places the input in clump space: metres, with +Y up. Normals are
    /// turned to match.
    pub transform: glam::Affine3A,
    /// The most vertices (counted from the base vertex) and triangles the
    /// input can have. Space is reserved for this many; an input with more
    /// is reported as an overflow.
    pub vertex_capacity: u32,
    pub triangle_capacity: u32,
    /// Whether the input encloses a solid, if known. `None` measures it on
    /// the GPU: closed when every edge, with welded positions, is shared by
    /// exactly two triangles.
    pub closed: Option<bool>,
}

/// Most clumps one scattered request may ask for
/// ([`PlacementOptions::count`](crate::PlacementOptions::count)).
pub const MAX_GPU_SCATTER_COUNT: u32 = 4096;

/// Byte size of one vertex in [`GpuClumpBatch::vertices`]: position, then
/// unit normal (three `f32` each), then fade (one `f32`; see
/// [`ClumpMesh::fade`](crate::ClumpMesh::fade)).
pub const GPU_VERTEX_SIZE: u64 = OUT_VERTEX_SIZE;

/// Byte size of one entry in [`GpuClumpBatch::draw_args`], laid out as
/// wgpu's `DrawIndexedIndirectArgs`.
pub const GPU_DRAW_ARGS_SIZE: u64 = DRAW_ARGS_SIZE;

/// Byte size of one entry in [`GpuClumpBatch::status`]; see
/// [`GpuClumpStatus::decode`].
pub const GPU_STATUS_SIZE: u64 = STATUS_SIZE;

/// Returns `(vertex_capacity, index_capacity)` that a clump with `options`
/// typically fits within.
///
/// For a scattered clump the estimate covers the requested number of the
/// largest blobs the settings allow. For a layer, pass the area of input
/// the clump may cover, in square metres; it is ignored for scattered
/// clumps. Unusual inputs can need more: check [`GpuClumpStatus`] and retry.
pub fn estimate_capacity(options: &ClumpOptions, covered_area: f32) -> (u32, u32) {
    let scale = options.scale;
    let edge = (options.mesh.edge_length * scale).max(f32::MIN_POSITIVE);
    let (vertices, triangles) = match options.placement.mode {
        Placement::Scatter => {
            // The biggest blob size variation allows.
            let grow = 1.0 + options.placement.size_variation;
            let radius = options.placement.radius * scale * grow;
            let height = options.shape.thickness * scale * grow;
            let depth = options.shape.sink * scale * grow + BOTTOM_GAP * height;
            let segments = (std::f32::consts::TAU * radius / edge)
                .ceil()
                .max(MIN_SEGMENTS as f32);
            let meridian = 0.5 * std::f32::consts::PI * (radius + 0.5 * (height + depth));
            let rings = (meridian / edge).ceil().max(MIN_RINGS as f32);
            let count = options.placement.count as f32;
            (
                count * (2.0 + (rings - 1.0) * segments),
                count * 2.0 * segments * (rings - 1.0),
            )
        }
        Placement::Everywhere => {
            // Refined edges fall between half the target edge length and
            // all of it, which packs about 4 triangles per square edge
            // length; with an underside and walls when closed.
            let top = 5.0 * covered_area.max(0.0) / (edge * edge) + 64.0;
            let factor = if options.mesh.closed { 2.5 } else { 1.0 };
            (factor * top * 0.6, factor * top)
        }
    };
    let clamp = |x: f32| x.ceil().clamp(0.0, u32::MAX as f32) as u32;
    (clamp(vertices), clamp(triangles * 3.0))
}

/// One clump to generate on the GPU.
#[derive(Debug, Clone)]
pub struct GpuClumpRequest<'a> {
    /// The input mesh the clump grows on.
    pub surface: GpuSurfaceInput<'a>,
    /// The clump's settings.
    pub options: ClumpOptions,
    /// Vertices and indices reserved for this clump in the batch's output.
    /// While it is being built, the clump also needs this much room before
    /// buried parts are dropped, so size these for that.
    pub vertex_capacity: u32,
    pub index_capacity: u32,
}

impl<'a> GpuClumpRequest<'a> {
    /// Creates a request with capacities from [`estimate_capacity`] for a
    /// clump covering `covered_area` square metres (ignored for scattered
    /// clumps).
    pub fn new(surface: GpuSurfaceInput<'a>, options: ClumpOptions, covered_area: f32) -> Self {
        let (vertex_capacity, index_capacity) = estimate_capacity(&options, covered_area);
        Self {
            surface,
            options,
            vertex_capacity,
            index_capacity,
        }
    }

    /// Raises the output capacities to at least what `status` reports was
    /// needed, so a retry of a clump that overflowed fits. Capacities never
    /// shrink. An input overflow ([`GpuClumpStatus::input_overflow`]) needs
    /// larger [`GpuSurfaceInput`] capacities instead.
    pub fn grow_to_fit(&mut self, status: &GpuClumpStatus) {
        self.vertex_capacity = self.vertex_capacity.max(status.work_vertex_count);
        self.index_capacity = self.index_capacity.max(status.work_index_count);
    }
}

/// Where one request's clump lives in a [`GpuClumpBatch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuClumpRange {
    /// First vertex of this clump in [`GpuClumpBatch::vertices`]. Indices
    /// are relative to it; the draw arguments already use it as the base
    /// vertex.
    pub vertex_base: u32,
    /// Vertices reserved for this clump.
    pub vertex_capacity: u32,
    /// First index of this clump in [`GpuClumpBatch::indices`].
    pub index_base: u32,
    /// Indices reserved for this clump: the request's capacity rounded down
    /// to whole triangles.
    pub index_capacity: u32,
    /// Byte offset of this clump's entry in [`GpuClumpBatch::draw_args`].
    pub draw_args_offset: u64,
    /// Byte offset of this clump's entry in [`GpuClumpBatch::status`].
    pub status_offset: u64,
}

/// GPU buffers holding a batch of generated clumps, filled once the
/// recorded commands run. Every buffer also allows `COPY_SRC` and `STORAGE`.
#[derive(Debug)]
pub struct GpuClumpBatch {
    /// Vertices of every clump ([`GPU_VERTEX_SIZE`] bytes each). Usable as a
    /// vertex buffer.
    pub vertices: wgpu::Buffer,
    /// 32-bit triangle indices of every clump, counter-clockwise seen from
    /// outside. Usable as an index buffer.
    pub indices: wgpu::Buffer,
    /// One indexed indirect draw per clump ([`GPU_DRAW_ARGS_SIZE`] bytes
    /// each), with an instance count of one. A clump that overflowed draws
    /// nothing.
    pub draw_args: wgpu::Buffer,
    /// One status entry per clump ([`GPU_STATUS_SIZE`] bytes each). Copy it
    /// to a mappable buffer and decode it with [`GpuClumpStatus::decode`].
    pub status: wgpu::Buffer,
    /// Location of each clump, in request order.
    pub ranges: Vec<GpuClumpRange>,
}

/// What building one clump found, read from [`GpuClumpBatch::status`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GpuClumpStatus {
    /// The input had more triangles, or used more vertices, than its
    /// [`GpuSurfaceInput`] capacities. The clump is not usable.
    pub input_overflow: bool,
    /// The clump needed more vertices, or indices, than its request
    /// reserved. It draws nothing.
    pub vertex_overflow: bool,
    pub index_overflow: bool,
    /// Vertices and indices of the finished clump.
    pub vertex_count: u32,
    pub index_count: u32,
    /// Vertices and indices the clump needed while being built: what its
    /// request must reserve. For a layer clump that ran out of room part
    /// way through, this is only what it needed when it stopped, so growing
    /// to fit may take more than one retry.
    pub work_vertex_count: u32,
    pub work_index_count: u32,
    /// Separate clumps placed, for scattered clumps.
    pub clumps: u32,
    /// Triangles and vertices read from the input.
    pub input_triangles: u32,
    pub input_vertices: u32,
    /// Distinct input positions: vertices at the same position count once.
    pub input_positions: u32,
    /// Whether the input was treated as closed.
    pub input_closed: bool,
    /// Layer clumps: triangles left out because they stayed turned over,
    /// and vertices lowered to fit a gap or the input's thickness, as in
    /// [`BuildStats`](crate::BuildStats).
    pub folded: u32,
    pub thinned: u32,
    /// Layer clumps: the slope limit ran out of passes before it settled,
    /// so some edges may be steeper than `shape.max_slope`.
    pub slope_unsettled: bool,
    /// Layer clumps: simplifying the surface ran out of selection rounds in
    /// some pass, so it may have merged less than the CPU would.
    pub simplify_rounds_capped: bool,
    /// Strict layer clumps: triangles that still pass through another,
    /// because strict mode ran out of passes; 0 otherwise, and always 0 for
    /// other clumps.
    pub crossings: u32,
}

impl GpuClumpStatus {
    /// Decodes one entry of [`GpuClumpBatch::status`]. `bytes` must hold
    /// [`GPU_STATUS_SIZE`] bytes.
    pub fn decode(bytes: &[u8]) -> Self {
        let word = |k: usize| u32::from_le_bytes(bytes[4 * k..4 * k + 4].try_into().unwrap());
        let overflow = word(0);
        Self {
            input_overflow: overflow & OVERFLOW_INPUT != 0,
            vertex_overflow: overflow & OVERFLOW_VERTICES != 0,
            index_overflow: overflow & OVERFLOW_INDICES != 0,
            vertex_count: word(1),
            index_count: word(2),
            work_vertex_count: word(3),
            work_index_count: word(4),
            clumps: word(5),
            input_triangles: word(6),
            input_vertices: word(7),
            input_positions: word(8),
            input_closed: word(9) != 0,
            folded: word(10),
            thinned: word(11),
            slope_unsettled: word(12) != 0,
            simplify_rounds_capped: word(13) != 0,
            crossings: word(14),
        }
    }
}

/// Why a batch couldn't be recorded.
#[derive(Debug, Clone, PartialEq)]
pub enum GpuError {
    /// Request `index`'s settings are out of range.
    InvalidOptions { index: usize, error: OptionsError },
    /// Request `index`'s input can't be read: `what` says why.
    InvalidInput { index: usize, what: &'static str },
    /// Request `index` asks for something the GPU generator can't do yet.
    Unsupported { index: usize, what: &'static str },
    /// A buffer the batch needs is larger than the device allows. Split the
    /// batch, or lower its capacities.
    BufferTooLarge {
        buffer: &'static str,
        size: u64,
        limit: u64,
    },
}

impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOptions { index, error } => write!(f, "request {index}: {error}"),
            Self::InvalidInput { index, what } => write!(f, "request {index}: {what}"),
            Self::Unsupported { index, what } => {
                write!(
                    f,
                    "request {index}: the GPU generator doesn't support {what} yet"
                )
            }
            Self::BufferTooLarge {
                buffer,
                size,
                limit,
            } => {
                write!(
                    f,
                    "the batch's {buffer} buffer needs {size} bytes, more than the device's {limit}"
                )
            }
        }
    }
}

impl std::error::Error for GpuError {}

/// Records batches of clump generation on one device.
pub struct GpuClumpGenerator {
    pipelines: Pipelines,
}

impl GpuClumpGenerator {
    /// Compiles the generator's pipelines for `device`.
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            pipelines: Pipelines::new(device),
        }
    }

    /// Records commands into `encoder` that build every clump in
    /// `requests`, and returns the buffers they fill. Submit the encoder
    /// before using the buffers.
    ///
    /// Returns an error if a request's settings are out of range, its input
    /// can't be read, it asks for something not supported yet, or the batch
    /// needs a buffer larger than the device allows.
    pub fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        requests: &[GpuClumpRequest],
    ) -> Result<GpuClumpBatch, GpuError> {
        for (index, request) in requests.iter().enumerate() {
            check_request(index, request)?;
        }
        if requests.is_empty() {
            let empty = |label: &str, usage: wgpu::BufferUsages| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size: 16,
                    usage: usage | wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                })
            };
            return Ok(GpuClumpBatch {
                vertices: empty("procgena_clump vertices", wgpu::BufferUsages::VERTEX),
                indices: empty("procgena_clump indices", wgpu::BufferUsages::INDEX),
                draw_args: empty("procgena_clump draw args", wgpu::BufferUsages::INDIRECT),
                status: empty("procgena_clump status", wgpu::BufferUsages::empty()),
                ranges: Vec::new(),
            });
        }

        // Lay out the batch, and check it fits the device.
        let stage_requests: Vec<StageRequest> = requests
            .iter()
            .map(|r| StageRequest {
                input: r.surface,
                options: &r.options,
                out_vertices: r.vertex_capacity,
                out_indices: r.index_capacity,
            })
            .collect();
        let shapes: Vec<_> = stage_requests
            .iter()
            .map(|r| batch::RequestShape::new(&r.input, r.options, r.out_vertices, r.out_indices))
            .collect();
        let plan = batch::BatchPlan::new(&shapes);
        let limits = device.limits();
        let limit = limits
            .max_storage_buffer_binding_size
            .min(limits.max_buffer_size);
        let words = |buffer: &'static str, words: u64| {
            let size = stages::storage_bytes(words);
            if size > limit {
                Err(GpuError::BufferTooLarge {
                    buffer,
                    size,
                    limit,
                })
            } else {
                Ok(())
            }
        };
        words("arena", plan.arena_words)?;
        words("vertices", 7 * plan.total_work_vertices as u64)?;
        words("indices", 3 * plan.total_work_triangles as u64)?;

        // Record every pass.
        let resources = BatchResources::new(device, &self.pipelines, &stage_requests);
        let outputs = resources.outputs(device, &self.pipelines);
        resources.record(
            encoder,
            &self.pipelines,
            &outputs,
            limits.max_compute_workgroups_per_dimension,
        );
        let ranges = resources
            .plan
            .slices
            .iter()
            .enumerate()
            .map(|(i, s)| GpuClumpRange {
                vertex_base: s.out_vertex_base,
                vertex_capacity: s.out_vertex_capacity,
                index_base: s.out_index_base,
                index_capacity: s.out_index_capacity,
                draw_args_offset: i as u64 * DRAW_ARGS_SIZE,
                status_offset: i as u64 * STATUS_SIZE,
            })
            .collect();
        Ok(GpuClumpBatch {
            vertices: outputs.vertices,
            indices: outputs.indices,
            draw_args: outputs.draw_args,
            status: outputs.status,
            ranges,
        })
    }
}

/// Checks that request `index` can be run.
fn check_request(index: usize, request: &GpuClumpRequest) -> Result<(), GpuError> {
    let options = &request.options;
    options
        .validate()
        .map_err(|error| GpuError::InvalidOptions { index, error })?;
    let unsupported = |what| Err(GpuError::Unsupported { index, what });
    if options.placement.count > MAX_GPU_SCATTER_COUNT {
        return unsupported("more than MAX_GPU_SCATTER_COUNT clumps");
    }
    let format = request.surface.format;
    let invalid = |what| Err(GpuError::InvalidInput { index, what });
    let aligned = [format.stride, format.position_offset, format.normal_offset]
        .iter()
        .all(|x| x.is_multiple_of(4));
    if !aligned {
        return invalid("the vertex format's stride and offsets must be multiples of 4");
    }
    if format.position_offset.saturating_add(12) > format.stride
        || format.normal_offset.saturating_add(12) > format.stride
    {
        return invalid("the vertex format's position and normal must fit in its stride");
    }
    if !request.surface.draw_args_offset.is_multiple_of(4) {
        return invalid("draw_args_offset must be a multiple of 4");
    }
    Ok(())
}
