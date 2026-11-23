//! Every pass of the generator over a batch, and the buffers they share:
//! reading and welding the inputs, building their BVHs, measuring, placing
//! and meshing the clumps, and writing the output.

use std::num::NonZeroU64;

use wgpu::util::{BufferInitDescriptor, DeviceExt};

use super::GpuSurfaceInput;
use super::batch::{
    BATCH_SIZE, BatchPlan, COUNTER_STRIDE, MAX_BLUR_PASSES, MAX_MIS_ROUNDS, MAX_REPAIR_PASSES,
    MAX_SLOPE_PASSES, MAX_SWEEPS, REFINE_PASSES, REGION_RAYS, REGION_WORK_KEPT, REGION_WORK_USED,
    RequestShape, pack_rays, pack_request,
};
use super::params::{ParamSlots, params_and_arena};
use super::scan::{SCAN_PARAMS_SIZE, ScanLevel, Scanner, dispatch_groups};
use super::sort::{SORT_PARAMS_SIZE, SortPlan, Sorter};
use crate::options::ClumpOptions;
use crate::shaders::COMPILED_WGSL;

/// Work items per workgroup of the per-item passes (`WORKGROUP_SIZE` in
/// `batch.wesl`).
pub(crate) const WORKGROUP_SIZE: u32 = 64;

/// Byte sizes of `IngestParams` in `ingest.wesl` and `BvhPass` in `bvh.wesl`.
const INGEST_PARAMS_SIZE: u64 = 16;
const BVH_PASS_SIZE: u64 = 16;

/// Byte size of one output vertex: position, normal and fade.
pub(crate) const OUT_VERTEX_SIZE: u64 = 28;
/// Byte sizes of one request's draw arguments and status.
pub(crate) const DRAW_ARGS_SIZE: u64 = 20;
pub(crate) const STATUS_SIZE: u64 = 64;

/// Returns a compute-visible layout entry for `ty` at `binding`.
fn entry(binding: u32, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty,
        count: None,
    }
}

/// Returns a storage buffer binding type.
fn storage(read_only: bool) -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Storage { read_only },
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

/// Returns a uniform buffer binding type of at least `size` bytes.
fn uniform(size: u64, has_dynamic_offset: bool) -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Uniform,
        has_dynamic_offset,
        min_binding_size: NonZeroU64::new(size),
    }
}

/// Creates one pipeline per entry point of compiled module `module`, with
/// bind group `layouts`.
fn pipelines<const N: usize>(
    device: &wgpu::Device,
    module: &str,
    layouts: &[&wgpu::BindGroupLayout],
    entries: [&str; N],
) -> [wgpu::ComputePipeline; N] {
    let source = COMPILED_WGSL
        .iter()
        .find(|(name, _)| *name == module)
        .map(|(_, source)| *source)
        .unwrap_or_else(|| panic!("{module} module is compiled"));
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(module),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layouts: Vec<Option<&wgpu::BindGroupLayout>> = layouts.iter().map(|&l| Some(l)).collect();
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(module),
        bind_group_layouts: &layouts,
        immediate_size: 0,
    });
    entries.map(|entry| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: Some(&layout),
            module: &shader,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    })
}

/// Every pipeline of the generator, created once per device.
pub(crate) struct Pipelines {
    pub(crate) scanner: Scanner,
    pub(crate) sorter: Sorter,
    /// Group 0 of every generator pass: requests, batch, arena, counters.
    pub(crate) batch_layout: wgpu::BindGroupLayout,
    /// Group 1 of the ingest passes: one request's input buffers.
    ingest_layout: wgpu::BindGroupLayout,
    /// Group 1 of the BVH refit: the pass number, at a dynamic offset.
    pass_layout: wgpu::BindGroupLayout,
    /// Group 1 of the output passes: the batch's output buffers.
    output_layout: wgpu::BindGroupLayout,
    ingest: [wgpu::ComputePipeline; 2],
    weld: [wgpu::ComputePipeline; 11],
    bvh: [wgpu::ComputePipeline; 6],
    measure: [wgpu::ComputePipeline; 2],
    scatter: [wgpu::ComputePipeline; 13],
    output: [wgpu::ComputePipeline; 3],
    patch: [wgpu::ComputePipeline; 7],
    simplify: [wgpu::ComputePipeline; 20],
    envelope: [wgpu::ComputePipeline; 7],
    refine: [wgpu::ComputePipeline; 13],
    shell: [wgpu::ComputePipeline; 18],
    close: [wgpu::ComputePipeline; 16],
}

impl Pipelines {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let batch_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("procgena_clump batch"),
            entries: &[
                entry(0, storage(true)),
                entry(1, uniform(BATCH_SIZE as u64, false)),
                entry(2, storage(false)),
                entry(3, storage(false)),
                entry(4, storage(false)),
            ],
        });
        let ingest_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("procgena_clump ingest"),
            entries: &[
                entry(0, storage(true)),
                entry(1, storage(true)),
                entry(2, storage(true)),
                entry(3, uniform(INGEST_PARAMS_SIZE, false)),
            ],
        });
        let pass_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("procgena_clump pass"),
            entries: &[entry(0, uniform(BVH_PASS_SIZE, true))],
        });
        let output_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("procgena_clump output"),
            entries: &[0, 1, 2, 3].map(|b| entry(b, storage(false))),
        });
        let batch = &[&batch_layout];
        Self {
            scanner: Scanner::new(device),
            sorter: Sorter::new(device),
            ingest: pipelines(
                device,
                "ingest",
                &[&batch_layout, &ingest_layout],
                ["ingest_triangles", "ingest_vertices"],
            ),
            weld: pipelines(
                device,
                "weld",
                batch,
                [
                    "weld_keys",
                    "weld_list",
                    "weld_starts",
                    "weld_heads",
                    "weld_ids",
                    "weld_counts",
                    "edge_keys",
                    "edge_list",
                    "edge_starts",
                    "edge_heads",
                    "edge_runs",
                ],
            ),
            bvh: pipelines(
                device,
                "bvh",
                &[&batch_layout, &pass_layout],
                [
                    "bvh_init",
                    "bvh_bounds",
                    "bvh_keys",
                    "bvh_list",
                    "bvh_build",
                    "bvh_refit",
                ],
            ),
            measure: pipelines(device, "measure", batch, ["measure_bounds", "measure_init"]),
            scatter: pipelines(
                device,
                "scatter",
                batch,
                [
                    "scatter_weights",
                    "scatter_attempts",
                    "scatter_accept",
                    "blob_counts",
                    "blob_totals",
                    "blob_vertices",
                    "blob_triangles",
                    "blob_normals",
                    "strict_bounds",
                    "strict_cross",
                    "strict_decide",
                    "strict_drop_vertices",
                    "strict_drop_triangles",
                ],
            ),
            simplify: pipelines(
                device,
                "simplify",
                &[&batch_layout, &pass_layout],
                [
                    "simplify_init_points",
                    "simplify_init_tris",
                    "around_count",
                    "around_copy",
                    "around_fill",
                    "edge_uses",
                    "point_flags",
                    "candidates",
                    "mis_reset",
                    "mis_priority",
                    "mis_slot",
                    "mis_accept",
                    "mis_reject",
                    "apply_merges",
                    "sweep_end",
                    "coarse_point_flags",
                    "coarse_tri_flags",
                    "coarse_points",
                    "coarse_tris",
                    "coarse_counts",
                ],
            ),
            patch: pipelines(
                device,
                "patch",
                batch,
                [
                    "patch_reset",
                    "facing_keep",
                    "point_owner",
                    "corner_owner",
                    "patch_points",
                    "patch_tris",
                    "patch_counts",
                ],
            ),
            envelope: pipelines(
                device,
                "envelope",
                batch,
                [
                    "envelope_reset",
                    "envelope_fine",
                    "envelope_store",
                    "measure_points",
                    "cover_flags",
                    "cover_tris",
                    "cover_counts",
                ],
            ),
            refine: pipelines(
                device,
                "refine",
                &[&batch_layout, &pass_layout],
                [
                    "sheet_clear",
                    "sheet_flag_clear",
                    "sheet_flag_points",
                    "sheet_points",
                    "sheet_tris",
                    "sheet_counts",
                    "refine_marks",
                    "sheet_around_copy",
                    "sheet_around_fill",
                    "refine_owner",
                    "refine_points",
                    "refine_tris",
                    "refine_counts",
                ],
            ),
            shell: pipelines(
                device,
                "shell",
                &[&batch_layout, &pass_layout],
                [
                    "shell_neighbour_count",
                    "shell_neighbour_fill",
                    "shell_edges",
                    "shell_edge_sum",
                    "shell_setup",
                    "shell_blur_init",
                    "shell_blur",
                    "shell_tops",
                    "shell_slope",
                    "shell_slope_end",
                    "shell_sink",
                    "shell_keep",
                    "shell_fold_mark",
                    "shell_fold_end",
                    "shell_fold_lower",
                    "shell_out_vertices",
                    "shell_out_tris",
                    "shell_normals",
                ],
            ),
            close: pipelines(
                device,
                "close",
                &[&batch_layout, &pass_layout],
                [
                    "close_depth",
                    "close_border",
                    "close_kept",
                    "close_fans",
                    "close_split",
                    "close_first",
                    "close_below",
                    "close_walls",
                    "close_size",
                    "close_out_vertices",
                    "close_out_tris",
                    "close_out_walls",
                    "close_around_count",
                    "close_around_copy",
                    "close_around_fill",
                    "close_normals",
                ],
            ),
            output: pipelines(
                device,
                "output",
                &[&batch_layout, &output_layout],
                ["output_vertices", "output_indices", "output_finish"],
            ),
            batch_layout,
            ingest_layout,
            pass_layout,
            output_layout,
        }
    }
}

/// One request of a batch: its input, settings and output capacities.
pub(crate) struct StageRequest<'a> {
    pub(crate) input: GpuSurfaceInput<'a>,
    pub(crate) options: &'a ClumpOptions,
    /// Output vertices and indices reserved for the clump mesh.
    pub(crate) out_vertices: u32,
    pub(crate) out_indices: u32,
}

/// Byte offsets of each scan level, sort pass and refit pass in the params
/// buffer.
struct ParamOffsets {
    weld_flags: Vec<u32>,
    weld_starts: Vec<u32>,
    weld_rank: Vec<u32>,
    weld_sort: Vec<u32>,
    weld_sort_scan: Vec<u32>,
    edge_flags: Vec<u32>,
    edge_starts: Vec<u32>,
    edge_sort: Vec<u32>,
    edge_sort_scan: Vec<u32>,
    triangle_flags: Vec<u32>,
    triangle_sort: Vec<u32>,
    triangle_sort_scan: Vec<u32>,
    coarse_flags: Vec<u32>,
    coarse_sort: Vec<u32>,
    coarse_sort_scan: Vec<u32>,
    /// One `BvhPass` per refit pass number (0 for the other build passes),
    /// for the input's BVH and for the simplified surface's.
    input_bvh_passes: Vec<u32>,
    coarse_bvh_passes: Vec<u32>,
    corner_owner: Vec<u32>,
    around_start: Vec<u32>,
    coarse_point: Vec<u32>,
    coarse_tri: Vec<u32>,
    cover: Vec<u32>,
    sheet_point: Vec<u32>,
    sheet_around: Vec<u32>,
    refine_children: Vec<u32>,
    refine_creates: Vec<u32>,
    shell_neighbour: Vec<u32>,
    close_border: Vec<u32>,
    close_copies: Vec<u32>,
    close_first: Vec<u32>,
    close_kept: Vec<u32>,
    close_walls: Vec<u32>,
    close_around: Vec<u32>,
    /// One simplify `Step` per sweep and selection round, at
    /// `sweep * MAX_MIS_ROUNDS + round`.
    steps: Vec<u32>,
    /// One block per pass number, from 0, whose first word is the number:
    /// for refine and the shell's repeated passes.
    numbers: Vec<u32>,
    facing_keep: Vec<u32>,
    blob_vertices: Vec<u32>,
    blob_triangles: Vec<u32>,
    work_used: Vec<u32>,
    work_kept: Vec<u32>,
}

/// The batch's output buffers, filled once the recorded passes run.
pub(crate) struct Outputs {
    pub(crate) vertices: wgpu::Buffer,
    pub(crate) indices: wgpu::Buffer,
    pub(crate) draw_args: wgpu::Buffer,
    pub(crate) status: wgpu::Buffer,
    group: wgpu::BindGroup,
}

/// The GPU buffers and recorded parameters of a batch.
pub(crate) struct BatchResources {
    pub(crate) plan: BatchPlan,
    /// The batch's scratch arrays; see `BatchPlan::regions`.
    pub(crate) arena: wgpu::Buffer,
    /// Per-request counts and flags; see `COUNTER_*`.
    pub(crate) counters: wgpu::Buffer,
    /// Bit flags many threads set at once; see `flags` in `batch.wesl`.
    pub(crate) flags: wgpu::Buffer,
    /// Every request's exposure ray directions, copied into the arena first.
    rays: Option<wgpu::Buffer>,
    /// Group 0 of every generator pass over this batch.
    pub(crate) batch_group: wgpu::BindGroup,
    ingest_groups: Vec<wgpu::BindGroup>,
    scan_group: wgpu::BindGroup,
    sort_group: wgpu::BindGroup,
    pass_group: wgpu::BindGroup,
    offsets: ParamOffsets,
}

/// Returns a byte size for `words` words that a storage buffer accepts.
pub(crate) fn storage_bytes(words: u64) -> u64 {
    (words * 4).next_multiple_of(16).max(16)
}

impl BatchResources {
    /// Lays out the batch of `requests` and creates its buffers. Options
    /// must be validated, and there must be at least one request.
    pub(crate) fn new(
        device: &wgpu::Device,
        pipelines: &Pipelines,
        requests: &[StageRequest],
    ) -> Self {
        let shapes: Vec<RequestShape> = requests
            .iter()
            .map(|r| RequestShape::new(&r.input, r.options, r.out_vertices, r.out_indices))
            .collect();
        let plan = BatchPlan::new(&shapes);

        // The batch-wide buffers.
        let request_bytes: Vec<u8> = requests
            .iter()
            .zip(&plan.slices)
            .flat_map(|(r, slices)| pack_request(&r.input, r.options, slices))
            .collect();
        let request_buffer = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("procgena_clump requests"),
            contents: &request_bytes,
            usage: wgpu::BufferUsages::STORAGE,
        });
        let batch_buffer = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("procgena_clump batch"),
            contents: &plan.batch_bytes(),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let arena = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("procgena_clump arena"),
            size: storage_bytes(plan.arena_words),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let counters = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("procgena_clump counters"),
            size: 4 * (COUNTER_STRIDE as u64) * requests.len() as u64,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let flags = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("procgena_clump flags"),
            size: storage_bytes(plan.flag_words),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let options: Vec<&ClumpOptions> = requests.iter().map(|r| r.options).collect();
        let ray_words = pack_rays(&options);
        let rays = (!ray_words.is_empty()).then(|| {
            device.create_buffer_init(&BufferInitDescriptor {
                label: Some("procgena_clump rays"),
                contents: &ray_words
                    .iter()
                    .flat_map(|w| w.to_le_bytes())
                    .collect::<Vec<_>>(),
                usage: wgpu::BufferUsages::COPY_SRC,
            })
        });
        let batch_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("procgena_clump batch"),
            layout: &pipelines.batch_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: request_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: batch_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: arena.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: counters.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: flags.as_entire_binding(),
                },
            ],
        });

        // Every scan level, sort pass, refit pass and ingest request in one
        // params buffer.
        let mut slots = ParamSlots::new(device.limits().min_uniform_buffer_offset_alignment);
        let scan_slots = |slots: &mut ParamSlots, levels: &[ScanLevel]| -> Vec<u32> {
            levels.iter().map(|l| slots.push(&l.params())).collect()
        };
        let sort_slots = |slots: &mut ParamSlots, plan: &SortPlan| -> Vec<u32> {
            plan.passes
                .iter()
                .map(|p| slots.push(&p.params()))
                .collect()
        };
        let number = |slots: &mut ParamSlots, n: u32| {
            let mut block = [0u8; 16];
            block[..4].copy_from_slice(&n.to_le_bytes());
            slots.push(&block)
        };
        // `BvhPass` blocks: pass numbers 0 (build) through the last refit.
        let bvh_passes = |slots: &mut ParamSlots, mesh: u32| -> Vec<u32> {
            (0..=plan.refit_passes)
                .map(|n| {
                    let mut block = [0u8; 16];
                    block[..4].copy_from_slice(&n.to_le_bytes());
                    block[4..8].copy_from_slice(&mesh.to_le_bytes());
                    slots.push(&block)
                })
                .collect()
        };
        let offsets = ParamOffsets {
            weld_flags: scan_slots(&mut slots, &plan.weld_flags_scan),
            weld_starts: scan_slots(&mut slots, &plan.weld_starts_scan),
            weld_rank: scan_slots(&mut slots, &plan.weld_rank_scan),
            weld_sort: sort_slots(&mut slots, &plan.weld_sort),
            weld_sort_scan: scan_slots(&mut slots, &plan.weld_sort.scan),
            edge_flags: scan_slots(&mut slots, &plan.edge_flags_scan),
            edge_starts: scan_slots(&mut slots, &plan.edge_starts_scan),
            edge_sort: sort_slots(&mut slots, &plan.edge_sort),
            edge_sort_scan: scan_slots(&mut slots, &plan.edge_sort.scan),
            triangle_flags: scan_slots(&mut slots, &plan.triangle_flags_scan),
            triangle_sort: sort_slots(&mut slots, &plan.triangle_sort),
            triangle_sort_scan: scan_slots(&mut slots, &plan.triangle_sort.scan),
            coarse_flags: scan_slots(&mut slots, &plan.coarse_flags_scan),
            coarse_sort: sort_slots(&mut slots, &plan.coarse_sort),
            coarse_sort_scan: scan_slots(&mut slots, &plan.coarse_sort.scan),
            input_bvh_passes: bvh_passes(&mut slots, 0),
            coarse_bvh_passes: bvh_passes(&mut slots, 1),
            corner_owner: scan_slots(&mut slots, &plan.corner_owner_scan),
            around_start: scan_slots(&mut slots, &plan.around_start_scan),
            coarse_point: scan_slots(&mut slots, &plan.coarse_point_scan),
            coarse_tri: scan_slots(&mut slots, &plan.coarse_tri_scan),
            cover: scan_slots(&mut slots, &plan.cover_scan),
            sheet_point: scan_slots(&mut slots, &plan.sheet_point_scan),
            sheet_around: scan_slots(&mut slots, &plan.sheet_around_scan),
            refine_children: scan_slots(&mut slots, &plan.refine_children_scan),
            refine_creates: scan_slots(&mut slots, &plan.refine_creates_scan),
            shell_neighbour: scan_slots(&mut slots, &plan.shell_neighbour_scan),
            close_border: scan_slots(&mut slots, &plan.close_border_scan),
            close_copies: scan_slots(&mut slots, &plan.close_copies_scan),
            close_first: scan_slots(&mut slots, &plan.close_first_scan),
            close_kept: scan_slots(&mut slots, &plan.close_kept_scan),
            close_walls: scan_slots(&mut slots, &plan.close_walls_scan),
            close_around: scan_slots(&mut slots, &plan.close_around_scan),
            numbers: if plan.has_layers {
                let last = REFINE_PASSES
                    .max(MAX_BLUR_PASSES)
                    .max(MAX_SLOPE_PASSES)
                    .max(MAX_REPAIR_PASSES);
                (0..=last).map(|n| number(&mut slots, n)).collect()
            } else {
                Vec::new()
            },
            steps: if plan.has_layers {
                (0..MAX_SWEEPS * MAX_MIS_ROUNDS)
                    .map(|i| {
                        let mut block = [0u8; 16];
                        block[..4].copy_from_slice(&(i / MAX_MIS_ROUNDS).to_le_bytes());
                        block[4..8].copy_from_slice(&(i % MAX_MIS_ROUNDS).to_le_bytes());
                        slots.push(&block)
                    })
                    .collect()
            } else {
                Vec::new()
            },
            facing_keep: scan_slots(&mut slots, &plan.facing_keep_scan),
            blob_vertices: scan_slots(&mut slots, &plan.blob_vertices_scan),
            blob_triangles: scan_slots(&mut slots, &plan.blob_triangles_scan),
            work_used: scan_slots(&mut slots, &plan.work_used_scan),
            work_kept: scan_slots(&mut slots, &plan.work_kept_scan),
        };
        let ingest_offsets: Vec<u32> = (0..requests.len() as u32)
            .map(|r| number(&mut slots, r))
            .collect();
        let params = slots.finish(device);

        // Bind groups: the scan, sort and pass windows, and each request's input.
        let scan_group = params_and_arena(
            device,
            &pipelines.scanner.layout,
            &params,
            SCAN_PARAMS_SIZE,
            &arena,
        );
        let sort_group = params_and_arena(
            device,
            &pipelines.sorter.layout,
            &params,
            SORT_PARAMS_SIZE,
            &arena,
        );
        let window = |offset: u64, size: u64| {
            wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &params,
                offset,
                size: NonZeroU64::new(size),
            })
        };
        let pass_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("procgena_clump pass"),
            layout: &pipelines.pass_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: window(0, BVH_PASS_SIZE),
            }],
        });
        let ingest_groups = requests
            .iter()
            .zip(ingest_offsets)
            .map(|(r, offset)| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("procgena_clump ingest"),
                    layout: &pipelines.ingest_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: r.input.vertices.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: r.input.indices.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: r.input.draw_args.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: window(offset as u64, INGEST_PARAMS_SIZE),
                        },
                    ],
                })
            })
            .collect();

        Self {
            plan,
            arena,
            counters,
            flags,
            rays,
            batch_group,
            ingest_groups,
            scan_group,
            sort_group,
            pass_group,
            offsets,
        }
    }

    /// Creates the batch's output buffers: room for every request's output
    /// slices, usable for drawing and as storage, and readable by copy.
    pub(crate) fn outputs(&self, device: &wgpu::Device, pipelines: &Pipelines) -> Outputs {
        let plan = &self.plan;
        let buffer = |label: &str, size: u64, usage: wgpu::BufferUsages| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size.next_multiple_of(16).max(16),
                usage: usage | wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        };
        let requests = plan.slices.len() as u64;
        let vertices = buffer(
            "procgena_clump vertices",
            OUT_VERTEX_SIZE * plan.total_work_vertices as u64,
            wgpu::BufferUsages::VERTEX,
        );
        let indices = buffer(
            "procgena_clump indices",
            12 * plan.total_work_triangles as u64,
            wgpu::BufferUsages::INDEX,
        );
        let draw_args = buffer(
            "procgena_clump draw args",
            DRAW_ARGS_SIZE * requests,
            wgpu::BufferUsages::INDIRECT,
        );
        let status = buffer(
            "procgena_clump status",
            STATUS_SIZE * requests,
            wgpu::BufferUsages::empty(),
        );
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("procgena_clump output"),
            layout: &pipelines.output_layout,
            entries: &[&vertices, &indices, &draw_args, &status]
                .iter()
                .enumerate()
                .map(|(binding, b)| wgpu::BindGroupEntry {
                    binding: binding as u32,
                    resource: b.as_entire_binding(),
                })
                .collect::<Vec<_>>(),
        });
        Outputs {
            vertices,
            indices,
            draw_args,
            status,
            group,
        }
    }

    /// Records every pass of the batch into `encoder`, writing `outputs`.
    pub(crate) fn record(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipelines: &Pipelines,
        outputs: &Outputs,
        max_groups: u32,
    ) {
        self.record_prepare(encoder, pipelines, max_groups);
        let plan = &self.plan;
        if !plan.has_scatter {
            // Nothing else writes these before the output passes read them.
            for (region, words) in [
                (REGION_WORK_USED, plan.total_work_vertices + 1),
                (REGION_WORK_KEPT, plan.total_work_triangles + 1),
            ] {
                let at = 4 * plan.regions[region] as u64;
                encoder.clear_buffer(&self.arena, at, Some(4 * words as u64));
            }
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("procgena_clump clumps"),
            timestamp_writes: None,
        });
        let o = &self.offsets;
        let (w, wt) = (plan.total_work_vertices, plan.total_work_triangles);
        let requests = plan.slices.len() as u32;

        // Measure each input, then build layers and scattered clumps.
        let [measure_bounds, measure_init] = &pipelines.measure;
        self.items(&mut pass, measure_bounds, plan.total_vertices, max_groups);
        self.items(&mut pass, measure_init, requests, max_groups);
        if plan.has_layers {
            self.record_layers(&mut pass, pipelines, max_groups);
        }
        if plan.has_scatter {
            self.record_scatter(&mut pass, pipelines, max_groups);
        }
        if plan.has_layers {
            self.record_layer_output(&mut pass, pipelines, max_groups);
        }
        self.scan(
            &mut pass,
            pipelines,
            &plan.work_used_scan,
            &o.work_used,
            max_groups,
        );
        self.scan(
            &mut pass,
            pipelines,
            &plan.work_kept_scan,
            &o.work_kept,
            max_groups,
        );
        if plan.has_scatter {
            let normals = &pipelines.scatter[7];
            self.items(&mut pass, normals, w, max_groups);
        }
        if plan.has_layers {
            let normals = &pipelines.shell[17];
            self.numbered(&mut pass, normals, w, 0, max_groups);
        }
        if plan.has_closed_layers {
            let normals = &pipelines.close[15];
            self.numbered(&mut pass, normals, w, 0, max_groups);
        }

        // Write each request's mesh, draw arguments and status.
        let [out_vertices, out_indices, finish] = &pipelines.output;
        for (pipeline, count) in [(out_vertices, w), (out_indices, wt), (finish, requests)] {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.batch_group, &[]);
            pass.set_bind_group(1, &outputs.group, &[]);
            dispatch_groups(&mut pass, count.div_ceil(WORKGROUP_SIZE), max_groups);
        }
    }

    /// Records the layer passes: the working surface from the facing input
    /// triangles, then its simplification.
    fn record_layers(
        &self,
        pass: &mut wgpu::ComputePass<'_>,
        pipelines: &Pipelines,
        max_groups: u32,
    ) {
        let plan = &self.plan;
        let o = &self.offsets;
        let (v, t) = (plan.total_vertices, plan.total_triangles);
        let e = 3 * t;
        let requests = plan.slices.len() as u32;

        // The working surface.
        let [
            reset,
            facing_keep,
            point_owner,
            corner_owner,
            points,
            tris,
            patch_counts,
        ] = &pipelines.patch;
        self.items(pass, reset, v, max_groups);
        self.items(pass, facing_keep, t + 1, max_groups);
        self.items(pass, point_owner, t, max_groups);
        self.items(pass, corner_owner, e + 1, max_groups);
        self.scan(
            pass,
            pipelines,
            &plan.corner_owner_scan,
            &o.corner_owner,
            max_groups,
        );
        self.scan(
            pass,
            pipelines,
            &plan.facing_keep_scan,
            &o.facing_keep,
            max_groups,
        );
        self.items(pass, points, e, max_groups);
        self.items(pass, tris, t, max_groups);
        self.items(pass, patch_counts, requests, max_groups);

        // Simplify it, sweep by sweep.
        let [
            init_points,
            init_tris,
            around_count,
            around_copy,
            around_fill,
            edge_uses,
            point_flags,
            candidates,
            mis_reset,
            mis_priority,
            mis_slot,
            mis_accept,
            mis_reject,
            apply_merges,
            sweep_end,
            coarse_point_flags,
            coarse_tri_flags,
            coarse_points,
            coarse_tris,
            coarse_counts,
        ] = &pipelines.simplify;
        let stepped = |pass: &mut wgpu::ComputePass<'_>,
                       pipeline: &wgpu::ComputePipeline,
                       count: u32,
                       sweep: u32,
                       round: u32| {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.batch_group, &[]);
            let slot = (sweep * MAX_MIS_ROUNDS + round) as usize;
            pass.set_bind_group(1, &self.pass_group, &[o.steps[slot]]);
            dispatch_groups(pass, count.div_ceil(WORKGROUP_SIZE), max_groups);
        };
        stepped(pass, init_points, v, 0, 0);
        stepped(pass, init_tris, t, 0, 0);
        for sweep in 0..MAX_SWEEPS {
            stepped(pass, around_count, t, sweep, 0);
            stepped(pass, around_copy, v + 1, sweep, 0);
            self.scan(
                pass,
                pipelines,
                &plan.around_start_scan,
                &o.around_start,
                max_groups,
            );
            stepped(pass, around_fill, t, sweep, 0);
            if sweep == 0 {
                stepped(pass, edge_uses, e, sweep, 0);
                stepped(pass, point_flags, v, sweep, 0);
            }
            stepped(pass, candidates, e, sweep, 0);
            for round in 0..MAX_MIS_ROUNDS {
                stepped(pass, mis_reset, v, sweep, round);
                stepped(pass, mis_priority, e, sweep, round);
                stepped(pass, mis_slot, e, sweep, round);
                stepped(pass, mis_accept, e, sweep, round);
                stepped(pass, mis_reject, e, sweep, round);
            }
            stepped(pass, apply_merges, e, sweep, 0);
            stepped(pass, sweep_end, requests, sweep, 0);
        }

        // Compact what survives.
        stepped(pass, coarse_point_flags, v + 1, 0, 0);
        stepped(pass, coarse_tri_flags, t + 1, 0, 0);
        self.scan(
            pass,
            pipelines,
            &plan.coarse_point_scan,
            &o.coarse_point,
            max_groups,
        );
        self.scan(
            pass,
            pipelines,
            &plan.coarse_tri_scan,
            &o.coarse_tri,
            max_groups,
        );
        stepped(pass, coarse_points, v, 0, 0);
        stepped(pass, coarse_tris, t, 0, 0);
        stepped(pass, coarse_counts, requests, 0, 0);

        // A BVH over the simplified surface, for its envelope.
        self.record_bvh(
            pass,
            pipelines,
            BvhBuild {
                flags_scan: &plan.coarse_flags_scan,
                flags_offsets: &o.coarse_flags,
                sort: &plan.coarse_sort,
                sort_offsets: &o.coarse_sort,
                sort_scan_offsets: &o.coarse_sort_scan,
                passes: &o.coarse_bvh_passes,
            },
            max_groups,
        );

        // Rise and dip from the surface before simplifying, then each
        // point's measurements, then only the triangles a clump may cover.
        let [
            envelope_reset,
            envelope_fine,
            envelope_store,
            measure_points,
            cover_flags,
            cover_tris,
            cover_counts,
        ] = &pipelines.envelope;
        self.items(pass, envelope_reset, v, max_groups);
        self.items(pass, envelope_fine, v, max_groups);
        self.items(pass, envelope_store, v, max_groups);
        self.items(pass, measure_points, v, max_groups);
        self.items(pass, cover_flags, t + 1, max_groups);
        self.scan(pass, pipelines, &plan.cover_scan, &o.cover, max_groups);
        self.items(pass, cover_tris, t, max_groups);
        self.items(pass, cover_counts, requests, max_groups);

        self.record_refine(pass, pipelines, max_groups);
        self.record_shell(pass, pipelines, max_groups);
    }

    /// Records building each layer's sheet from its covered triangles, then
    /// the refine passes.
    fn record_refine(
        &self,
        pass: &mut wgpu::ComputePass<'_>,
        pipelines: &Pipelines,
        max_groups: u32,
    ) {
        let plan = &self.plan;
        let o = &self.offsets;
        let (v, t) = (plan.total_vertices, plan.total_triangles);
        let (w, wt) = (plan.total_work_vertices, plan.total_work_triangles);
        let requests = plan.slices.len() as u32;
        let [
            sheet_clear,
            sheet_flag_clear,
            sheet_flag_points,
            sheet_points,
            sheet_tris,
            sheet_counts,
            marks,
            around_copy,
            around_fill,
            owner,
            refine_points,
            refine_tris,
            refine_counts,
        ] = &pipelines.refine;
        let numbered = |pass: &mut wgpu::ComputePass<'_>,
                        pipeline: &wgpu::ComputePipeline,
                        count: u32,
                        number: u32| {
            self.numbered(pass, pipeline, count, number, max_groups);
        };

        // The sheet: covered triangles and the points they use.
        numbered(pass, sheet_clear, w, 0);
        numbered(pass, sheet_flag_clear, v + 1, 0);
        numbered(pass, sheet_flag_points, t, 0);
        self.scan(
            pass,
            pipelines,
            &plan.sheet_point_scan,
            &o.sheet_point,
            max_groups,
        );
        numbered(pass, sheet_points, v, 0);
        numbered(pass, sheet_tris, t, 0);
        numbered(pass, sheet_counts, requests, 0);

        // Split long edges, pass by pass.
        for number in 0..REFINE_PASSES {
            numbered(pass, marks, wt + 1, number);
            numbered(pass, around_copy, w + 1, number);
            self.scan(
                pass,
                pipelines,
                &plan.sheet_around_scan,
                &o.sheet_around,
                max_groups,
            );
            numbered(pass, around_fill, wt, number);
            numbered(pass, owner, 3 * wt + 1, number);
            self.scan(
                pass,
                pipelines,
                &plan.refine_creates_scan,
                &o.refine_creates,
                max_groups,
            );
            self.scan(
                pass,
                pipelines,
                &plan.refine_children_scan,
                &o.refine_children,
                max_groups,
            );
            numbered(pass, refine_points, 3 * wt, number);
            numbered(pass, refine_tris, wt, number);
            numbered(pass, refine_counts, requests, number);
        }

        // List the triangles around each point of the finished sheet, in
        // buffer A. Pass number `REFINE_PASSES` reads A; its marks and
        // child counts are left unused.
        numbered(pass, marks, wt + 1, REFINE_PASSES);
        numbered(pass, around_copy, w + 1, REFINE_PASSES);
        self.scan(
            pass,
            pipelines,
            &plan.sheet_around_scan,
            &o.sheet_around,
            max_groups,
        );
        numbered(pass, around_fill, wt, REFINE_PASSES);
    }

    /// Records pushing each open layer's sheet out into a clump, up to
    /// writing its working mesh, which [`record_layer_output`] does.
    ///
    /// [`record_layer_output`]: Self::record_layer_output
    fn record_shell(
        &self,
        pass: &mut wgpu::ComputePass<'_>,
        pipelines: &Pipelines,
        max_groups: u32,
    ) {
        let plan = &self.plan;
        let (w, wt) = (plan.total_work_vertices, plan.total_work_triangles);
        let requests = plan.slices.len() as u32;
        let [
            neighbour_count,
            neighbour_fill,
            edges,
            edge_sum,
            setup,
            blur_init,
            blur,
            tops,
            slope,
            slope_end,
            sink,
            keep,
            fold_mark,
            fold_end,
            fold_lower,
            ..,
        ] = &pipelines.shell;
        let numbered = |pass: &mut wgpu::ComputePass<'_>,
                        pipeline: &wgpu::ComputePipeline,
                        count: u32,
                        number: u32| {
            self.numbered(pass, pipeline, count, number, max_groups);
        };

        // Each sheet point's neighbours, listed once for blur and slope.
        numbered(pass, neighbour_count, w + 1, 0);
        self.scan(
            pass,
            pipelines,
            &plan.shell_neighbour_scan,
            &self.offsets.shell_neighbour,
            max_groups,
        );
        numbered(pass, neighbour_fill, w, 0);

        // Each layer's edge lengths summed in pairs, a level at a time,
        // until the stride covers the largest layer's edge slots.
        numbered(pass, edges, 3 * wt, 0);
        let most_edges = plan
            .slices
            .iter()
            .map(|s| 3 * (s.out_index_capacity / 3))
            .max()
            .unwrap_or(0);
        for level in 0..most_edges.next_power_of_two().trailing_zeros() {
            numbered(pass, edge_sum, 3 * wt, level);
        }

        // Push directions: normals blurred over about the clump's height.
        numbered(pass, setup, requests, 0);
        numbered(pass, blur_init, w, 0);
        for number in 0..MAX_BLUR_PASSES {
            numbered(pass, blur, w, number);
        }

        // Heights, then the slope limit, then sinking what is buried.
        numbered(pass, tops, w, 0);
        for number in 0..MAX_SLOPE_PASSES {
            numbered(pass, slope, w, number);
            numbered(pass, slope_end, requests, number);
        }
        numbered(pass, sink, w, 0);
        numbered(pass, keep, wt, 0);
        let [
            close_depth,
            close_border,
            close_kept,
            close_fans,
            close_split,
            close_first,
            close_below,
            close_walls,
            close_size,
            ..,
        ] = &pipelines.close;
        if plan.has_closed_layers {
            numbered(pass, close_depth, w, 0);
        }

        // Lower the clump around triangles that turned over.
        for number in 0..=MAX_REPAIR_PASSES {
            numbered(pass, fold_mark, wt, number);
            numbered(pass, fold_end, requests, number);
            numbered(pass, fold_lower, w, number);
        }
        if !plan.has_closed_layers {
            return;
        }

        // Closed layers: find the border edges and pinch points of the
        // kept top triangles, number the copies, underside vertices and
        // walls, and check the result fits.
        let o = &self.offsets;
        let scan = |pass: &mut wgpu::ComputePass<'_>, levels: &[ScanLevel], offsets: &[u32]| {
            self.scan(pass, pipelines, levels, offsets, max_groups);
        };
        numbered(pass, close_border, 3 * wt + 1, 0);
        numbered(pass, close_kept, wt + 1, 0);
        scan(pass, &plan.close_border_scan, &o.close_border);
        scan(pass, &plan.close_kept_scan, &o.close_kept);
        numbered(pass, close_fans, w + 1, 0);
        scan(pass, &plan.close_copies_scan, &o.close_copies);
        numbered(pass, close_split, 3 * wt, 0);
        numbered(pass, close_first, 3 * wt + 1, 0);
        scan(pass, &plan.close_first_scan, &o.close_first);
        numbered(pass, close_below, 3 * wt, 0);
        numbered(pass, close_walls, w + 1, 0);
        scan(pass, &plan.close_walls_scan, &o.close_walls);
        numbered(pass, close_size, requests, 0);
    }

    /// Records writing each layer's kept triangles and their vertices to the
    /// working mesh. Runs after the scatter passes, which clear it.
    fn record_layer_output(
        &self,
        pass: &mut wgpu::ComputePass<'_>,
        pipelines: &Pipelines,
        max_groups: u32,
    ) {
        let (w, wt) = (
            self.plan.total_work_vertices,
            self.plan.total_work_triangles,
        );
        let [.., out_vertices, out_tris, _] = &pipelines.shell;
        self.numbered(pass, out_vertices, w, 0, max_groups);
        self.numbered(pass, out_tris, wt, 0, max_groups);
        if !self.plan.has_closed_layers {
            return;
        }

        // Closed layers: their copies, underside and walls, then the
        // triangles around each vertex, for its normal.
        let [
            ..,
            close_out_vertices,
            close_out_tris,
            close_out_walls,
            around_count,
            around_copy,
            around_fill,
            _,
        ] = &pipelines.close;
        self.numbered(pass, close_out_vertices, w, 0, max_groups);
        self.numbered(pass, close_out_tris, wt, 0, max_groups);
        self.numbered(pass, close_out_walls, 3 * wt, 0, max_groups);
        self.numbered(pass, around_count, wt, 0, max_groups);
        self.numbered(pass, around_copy, w + 1, 0, max_groups);
        self.scan(
            pass,
            pipelines,
            &self.plan.close_around_scan,
            &self.offsets.close_around,
            max_groups,
        );
        self.numbered(pass, around_fill, wt, 0, max_groups);
    }

    /// Records one dispatch of `pipeline` over `count` items with pass
    /// number `number` bound as group 1.
    fn numbered(
        &self,
        pass: &mut wgpu::ComputePass<'_>,
        pipeline: &wgpu::ComputePipeline,
        count: u32,
        number: u32,
        max_groups: u32,
    ) {
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.batch_group, &[]);
        pass.set_bind_group(
            1,
            &self.pass_group,
            &[self.offsets.numbers[number as usize]],
        );
        dispatch_groups(pass, count.div_ceil(WORKGROUP_SIZE), max_groups);
    }

    /// Records the scatter passes: placing the clumps and meshing them,
    /// with strict mode's checks.
    fn record_scatter(
        &self,
        pass: &mut wgpu::ComputePass<'_>,
        pipelines: &Pipelines,
        max_groups: u32,
    ) {
        let plan = &self.plan;
        let o = &self.offsets;
        let (s, w, wt) = (
            plan.total_seeds,
            plan.total_work_vertices,
            plan.total_work_triangles,
        );
        let requests = plan.slices.len() as u32;
        let [
            weights,
            attempts,
            accept,
            counts,
            totals,
            vertices,
            triangles,
            _normals,
            strict_bounds,
            strict_cross,
            strict_decide,
            strict_drop_vertices,
            strict_drop_triangles,
        ] = &pipelines.scatter;
        self.items(pass, weights, requests, max_groups);
        self.items(pass, attempts, plan.total_attempts, max_groups);
        self.items(pass, accept, requests, max_groups);
        self.items(pass, counts, s + 1, max_groups);
        self.scan(
            pass,
            pipelines,
            &plan.blob_vertices_scan,
            &o.blob_vertices,
            max_groups,
        );
        self.scan(
            pass,
            pipelines,
            &plan.blob_triangles_scan,
            &o.blob_triangles,
            max_groups,
        );
        self.items(pass, totals, requests, max_groups);
        self.items(pass, vertices, w + 1, max_groups);
        self.items(pass, triangles, wt + 1, max_groups);

        // Strict scatter: leave out blobs that cross themselves or a blob
        // kept before them.
        self.items(pass, strict_bounds, s, max_groups);
        self.items(pass, strict_cross, wt, max_groups);
        self.items(pass, strict_decide, requests, max_groups);
        self.items(pass, strict_drop_vertices, w, max_groups);
        self.items(pass, strict_drop_triangles, wt, max_groups);
    }

    /// Dispatches `pipeline` over `count` items with the batch bound.
    fn items(
        &self,
        pass: &mut wgpu::ComputePass<'_>,
        pipeline: &wgpu::ComputePipeline,
        count: u32,
        max_groups: u32,
    ) {
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.batch_group, &[]);
        dispatch_groups(pass, count.div_ceil(WORKGROUP_SIZE), max_groups);
    }

    /// Records a planned scan whose levels' params sit at `offsets`.
    fn scan(
        &self,
        pass: &mut wgpu::ComputePass<'_>,
        pipelines: &Pipelines,
        levels: &[ScanLevel],
        offsets: &[u32],
        max_groups: u32,
    ) {
        pipelines
            .scanner
            .record(pass, &self.scan_group, levels, offsets, max_groups);
    }

    /// Records a planned sort whose passes and digit-count scan levels sit
    /// at `passes` and `scans`.
    fn sort(
        &self,
        pass: &mut wgpu::ComputePass<'_>,
        pipelines: &Pipelines,
        plan: &SortPlan,
        passes: &[u32],
        scans: &[u32],
        max_groups: u32,
    ) {
        pipelines.sorter.record(
            pass,
            &pipelines.scanner,
            &self.sort_group,
            &self.scan_group,
            plan,
            passes,
            scans,
            max_groups,
        );
    }

    /// Records the passes that read, weld and measure the inputs and build
    /// their BVHs. Clears the counters first, so `encoder` must not be
    /// inside a pass.
    pub(crate) fn record_prepare(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipelines: &Pipelines,
        max_groups: u32,
    ) {
        encoder.clear_buffer(&self.counters, 0, None);
        encoder.clear_buffer(&self.flags, 0, None);
        if let Some(rays) = &self.rays {
            let at = 4 * self.plan.regions[REGION_RAYS] as u64;
            encoder.copy_buffer_to_buffer(rays, 0, &self.arena, at, rays.size());
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("procgena_clump prepare"),
            timestamp_writes: None,
        });
        let plan = &self.plan;
        let (v, e) = (plan.total_vertices, 3 * plan.total_triangles);
        let o = &self.offsets;

        // Ingest, one request at a time with its own input bound.
        for (slices, group) in plan.slices.iter().zip(&self.ingest_groups) {
            for (pipeline, count) in pipelines
                .ingest
                .iter()
                .zip([slices.triangle_capacity, slices.vertex_capacity])
            {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &self.batch_group, &[]);
                pass.set_bind_group(1, group, &[]);
                dispatch_groups(&mut pass, count.div_ceil(WORKGROUP_SIZE), max_groups);
            }
        }

        // Weld the whole batch, then count each request's open edges.
        let [
            keys,
            list,
            starts,
            heads,
            ids,
            counts,
            edge_keys,
            edge_list,
            edge_starts,
            edge_heads,
            edge_runs,
        ] = &pipelines.weld;
        self.items(&mut pass, keys, v + 1, max_groups);
        self.scan(
            &mut pass,
            pipelines,
            &plan.weld_flags_scan,
            &o.weld_flags,
            max_groups,
        );
        self.items(&mut pass, list, v, max_groups);
        self.sort(
            &mut pass,
            pipelines,
            &plan.weld_sort,
            &o.weld_sort,
            &o.weld_sort_scan,
            max_groups,
        );
        self.items(&mut pass, starts, v + 1, max_groups);
        self.scan(
            &mut pass,
            pipelines,
            &plan.weld_starts_scan,
            &o.weld_starts,
            max_groups,
        );
        self.items(&mut pass, heads, v, max_groups);
        self.scan(
            &mut pass,
            pipelines,
            &plan.weld_rank_scan,
            &o.weld_rank,
            max_groups,
        );
        self.items(&mut pass, ids, v, max_groups);
        self.items(&mut pass, counts, plan.slices.len() as u32, max_groups);
        self.items(&mut pass, edge_keys, e + 1, max_groups);
        self.scan(
            &mut pass,
            pipelines,
            &plan.edge_flags_scan,
            &o.edge_flags,
            max_groups,
        );
        self.items(&mut pass, edge_list, e, max_groups);
        self.sort(
            &mut pass,
            pipelines,
            &plan.edge_sort,
            &o.edge_sort,
            &o.edge_sort_scan,
            max_groups,
        );
        self.items(&mut pass, edge_starts, e + 1, max_groups);
        self.scan(
            &mut pass,
            pipelines,
            &plan.edge_starts_scan,
            &o.edge_starts,
            max_groups,
        );
        self.items(&mut pass, edge_heads, e, max_groups);
        self.items(&mut pass, edge_runs, e, max_groups);

        // Build each request's input BVH.
        self.record_bvh(
            &mut pass,
            pipelines,
            BvhBuild {
                flags_scan: &plan.triangle_flags_scan,
                flags_offsets: &o.triangle_flags,
                sort: &plan.triangle_sort,
                sort_offsets: &o.triangle_sort,
                sort_scan_offsets: &o.triangle_sort_scan,
                passes: &o.input_bvh_passes,
            },
            max_groups,
        );
    }

    /// Records building one mesh's BVHs: sort its triangles by Morton code,
    /// link the hierarchy, then fit node bounds one tree level per pass.
    fn record_bvh(
        &self,
        pass: &mut wgpu::ComputePass<'_>,
        pipelines: &Pipelines,
        build: BvhBuild,
        max_groups: u32,
    ) {
        let t = self.plan.total_triangles;
        let requests = self.plan.slices.len() as u32;
        let [init, bounds, keys, list, link, refit] = &pipelines.bvh;
        let numbered = |pass: &mut wgpu::ComputePass<'_>,
                        pipeline: &wgpu::ComputePipeline,
                        count: u32,
                        number: usize| {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.batch_group, &[]);
            pass.set_bind_group(1, &self.pass_group, &[build.passes[number]]);
            dispatch_groups(pass, count.div_ceil(WORKGROUP_SIZE), max_groups);
        };
        numbered(pass, init, requests, 0);
        numbered(pass, bounds, t, 0);
        numbered(pass, keys, t + 1, 0);
        self.scan(
            pass,
            pipelines,
            build.flags_scan,
            build.flags_offsets,
            max_groups,
        );
        numbered(pass, list, t, 0);
        self.sort(
            pass,
            pipelines,
            build.sort,
            build.sort_offsets,
            build.sort_scan_offsets,
            max_groups,
        );
        numbered(pass, link, t, 0);
        for number in 1..build.passes.len() {
            numbered(pass, refit, t, number);
        }
    }
}

/// What one BVH build needs: its flag scan and sort, and where their params
/// and the build's `BvhPass` blocks sit.
struct BvhBuild<'a> {
    flags_scan: &'a [ScanLevel],
    flags_offsets: &'a [u32],
    sort: &'a SortPlan,
    sort_offsets: &'a [u32],
    sort_scan_offsets: &'a [u32],
    passes: &'a [u32],
}
