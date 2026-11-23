//! GPU generator tests. Tests marked `#[ignore]` need a GPU; run them with
//! `cargo test --release -p procgena_clump --features gpu --lib -- --ignored gpu`.

use std::time::{Duration, Instant};

use futures_lite::future::block_on;
use wgpu::util::{BufferInitDescriptor, DeviceExt};

use glam::{Affine3A, Quat, Vec3};
use procgena_mesh::SurfaceMesh;

use super::batch::*;
use super::params::{ParamSlots, params_and_arena};
use super::scan::{SCAN_PARAMS_SIZE, Scanner, plan_scan, sums_words};
use super::sort::{SORT_PARAMS_SIZE, SortKey, Sorter, hist_words, plan_sort};
use super::stages::{BatchResources, Pipelines, StageRequest};
use super::{GpuSurfaceInput, GpuVertexFormat};
use crate::hash::Rng;
use crate::options::ClumpOptions;

/// Packs parameter blocks into one uniform buffer and returns the buffer
/// and each block's byte offset.
fn uniform_slots(device: &wgpu::Device, blocks: &[&[u8]]) -> (wgpu::Buffer, Vec<u32>) {
    let mut slots = ParamSlots::new(device.limits().min_uniform_buffer_offset_alignment);
    let offsets = blocks.iter().map(|b| slots.push(b)).collect();
    (slots.finish(device), offsets)
}

/// Creates an arena buffer holding `words`.
fn arena_buffer(device: &wgpu::Device, words: &[u32]) -> wgpu::Buffer {
    device.create_buffer_init(&BufferInitDescriptor {
        label: Some("arena"),
        contents: &words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>(),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    })
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_sort_is_a_stable_sort_by_every_key_word() {
    let (device, queue) = device();
    let scanner = Scanner::new(&device);
    let sorter = Sorter::new(&device);
    let max_groups = device.limits().max_compute_workgroups_per_dimension;
    let mut rng = Rng::new(17);

    for (capacity, count) in [(1, 1), (300, 300), (5_000, 4_321), (100_000, 99_000)] {
        // Regions: the count, two lists, two key words, then sort scratch.
        let count_offset = 3;
        let list_a = count_offset + 1;
        let list_b = list_a + capacity;
        let low = list_b + capacity;
        let high = low + capacity;
        let hist = high + capacity;
        let mut arena = vec![0xdead_beef; (hist + hist_words(capacity)) as usize];
        arena[count_offset as usize] = count;
        for i in 0..capacity {
            arena[(list_a + i) as usize] = i;
            // Many repeated keys, so stability matters.
            arena[(low + i) as usize] = (rng.next_f32() * 3000.0) as u32;
            arena[(high + i) as usize] = (rng.next_f32() * 4.0) as u32;
        }
        let keys = [
            SortKey {
                offset: low,
                bits: 12,
            },
            SortKey {
                offset: high,
                bits: 2,
            },
        ];
        let plan = plan_sort(capacity, count_offset, list_a, list_b, &keys, hist);

        // Parameter slots: every sort pass, then every scan level.
        let pass_params: Vec<_> = plan.passes.iter().map(|p| p.params()).collect();
        let scan_params: Vec<_> = plan.scan.iter().map(|l| l.params()).collect();
        let blocks: Vec<&[u8]> = pass_params
            .iter()
            .map(|p| p.as_slice())
            .chain(scan_params.iter().map(|p| p.as_slice()))
            .collect();
        let (params, offsets) = uniform_slots(&device, &blocks);
        let (pass_offsets, scan_offsets) = offsets.split_at(pass_params.len());
        let arena_gpu = arena_buffer(&device, &arena);
        let sort_group = params_and_arena(
            &device,
            &sorter.layout,
            &params,
            SORT_PARAMS_SIZE,
            &arena_gpu,
        );
        let scan_group = params_and_arena(
            &device,
            &scanner.layout,
            &params,
            SCAN_PARAMS_SIZE,
            &arena_gpu,
        );

        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            sorter.record(
                &mut pass,
                &scanner,
                &sort_group,
                &scan_group,
                &plan,
                pass_offsets,
                scan_offsets,
                max_groups,
            );
        }
        let out = &read_words(&device, &queue, encoder, &[&arena_gpu])[0];

        // The first `count` elements, stably sorted by (high, low).
        let mut expected: Vec<u32> = (0..count).collect();
        expected.sort_by_key(|&e| (arena[(high + e) as usize], arena[(low + e) as usize]));
        let start = plan.result_offset as usize;
        assert_eq!(
            &out[start..start + count as usize],
            expected.as_slice(),
            "capacity {capacity}"
        );
    }
}

/// Creates a device on the default adapter.
fn device() -> (wgpu::Device, wgpu::Queue) {
    device_with(wgpu::InstanceFlags::default())
}

/// Creates a device on the default adapter, from an instance with `flags`.
fn device_with(flags: wgpu::InstanceFlags) -> (wgpu::Device, wgpu::Queue) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        flags,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .expect("a GPU adapter is available");
    block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("device")
}

/// Copies each of `buffers` to a mappable buffer after the commands in
/// `encoder`, runs everything, and returns their contents as words.
fn read_words(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    mut encoder: wgpu::CommandEncoder,
    buffers: &[&wgpu::Buffer],
) -> Vec<Vec<u32>> {
    let staging: Vec<wgpu::Buffer> = buffers
        .iter()
        .map(|buffer| {
            let copy = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("test readback"),
                size: buffer.size(),
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            encoder.copy_buffer_to_buffer(buffer, 0, &copy, 0, buffer.size());
            copy
        })
        .collect();
    queue.submit([encoder.finish()]);
    for copy in &staging {
        copy.map_async(wgpu::MapMode::Read, .., |result| result.expect("map"));
    }
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    staging
        .iter()
        .map(|copy| {
            let bytes = copy.get_mapped_range(..).expect("mapped");
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|&w| u32::from_le_bytes(w))
                .collect()
        })
        .collect()
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_scan_matches_cpu_prefix_sum() {
    let (device, queue) = device();
    let scanner = Scanner::new(&device);
    let align = device.limits().min_uniform_buffer_offset_alignment as u64;
    let max_groups = device.limits().max_compute_workgroups_per_dimension;
    let mut rng = Rng::new(3);

    // (capacity, entries the caller uses); the rest of the capacity is junk.
    // 70,000 and 300,000 need three levels.
    for (capacity, used) in [
        (1, 1),
        (255, 255),
        (256, 256),
        (257, 200),
        (65_536, 65_536),
        (70_000, 69_999),
        (300_000, 123_457),
    ] {
        // The data region starts at an odd offset; the sums region follows it.
        let data_offset = 7;
        let sums_offset = data_offset + capacity + 3;
        let words = sums_offset + sums_words(capacity);
        let mut arena: Vec<u32> = (0..words).map(|_| 0xdead_beef).collect();
        let data: Vec<u32> = (0..capacity)
            .map(|i| {
                if i < used {
                    (rng.next_f32() * 1000.0) as u32
                } else {
                    u32::MAX - i
                }
            })
            .collect();
        arena[data_offset as usize..(data_offset + capacity) as usize].copy_from_slice(&data);

        // One parameter slot per level.
        let levels = plan_scan(capacity, data_offset, sums_offset);
        let stride = SCAN_PARAMS_SIZE.next_multiple_of(align);
        let mut params = vec![0u8; stride as usize * levels.len()];
        for (k, level) in levels.iter().enumerate() {
            let at = k * stride as usize;
            params[at..at + SCAN_PARAMS_SIZE as usize].copy_from_slice(&level.params());
        }
        let params = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("scan params"),
            contents: &params,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let arena_buffer = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("arena"),
            contents: &arena
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scan"),
            layout: &scanner.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &params,
                        offset: 0,
                        size: std::num::NonZeroU64::new(SCAN_PARAMS_SIZE),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: arena_buffer.as_entire_binding(),
                },
            ],
        });

        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            let offsets: Vec<u32> = (0..levels.len() as u32)
                .map(|k| k * stride as u32)
                .collect();
            scanner.record(&mut pass, &group, &levels, &offsets, max_groups);
        }
        let out = &read_words(&device, &queue, encoder, &[&arena_buffer])[0];

        // Every prefix up to and including the used count is exact; words
        // before the data region are untouched.
        let mut sum = 0u32;
        for i in 0..=used.min(capacity - 1) {
            let got = out[(data_offset + i) as usize];
            assert_eq!(got, sum, "capacity {capacity}, entry {i}");
            sum = sum.wrapping_add(data[i as usize]);
        }
        assert!(
            out[..data_offset as usize]
                .iter()
                .all(|&w| w == 0xdead_beef)
        );
    }
}

/// A surface mesh uploaded as GPU input, in the 24-byte position-normal
/// layout, drawn by one indirect-draw entry.
struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    draw_args: wgpu::Buffer,
    draw_args_offset: u64,
}

impl GpuMesh {
    /// Uploads `mesh` with `lead_vertices` unused vertices and `lead_indices`
    /// unused indices in front, so the draw starts at a non-zero base vertex
    /// and first index, and the draw entry one entry into its buffer.
    fn upload(
        device: &wgpu::Device,
        mesh: &SurfaceMesh,
        lead_vertices: u32,
        lead_indices: u32,
    ) -> Self {
        let mut vertices: Vec<u32> = vec![0x7fc0_0000; 6 * lead_vertices as usize];
        for (p, n) in mesh.positions.iter().zip(&mesh.normals) {
            vertices.extend(p.map(f32::to_bits));
            vertices.extend(n.map(f32::to_bits));
        }
        let mut indices: Vec<u32> = vec![u32::MAX; lead_indices as usize];
        indices.extend(&mesh.indices);
        let draw = [
            0xdead,
            0,
            0,
            0,
            0,
            mesh.indices.len() as u32,
            1,
            lead_indices,
            lead_vertices,
            0,
        ];
        let buffer = |words: &[u32]| {
            device.create_buffer_init(&BufferInitDescriptor {
                label: Some("test input"),
                contents: &words
                    .iter()
                    .flat_map(|w| w.to_le_bytes())
                    .collect::<Vec<_>>(),
                usage: wgpu::BufferUsages::STORAGE,
            })
        };
        Self {
            vertices: buffer(&vertices),
            indices: buffer(&indices),
            draw_args: buffer(&draw),
            draw_args_offset: 20,
        }
    }

    /// Returns this mesh as input placed by `transform`, with room for
    /// exactly `mesh`'s vertices and triangles.
    fn input(
        &self,
        mesh: &SurfaceMesh,
        transform: Affine3A,
        closed: Option<bool>,
    ) -> GpuSurfaceInput<'_> {
        GpuSurfaceInput {
            vertices: &self.vertices,
            indices: &self.indices,
            draw_args: &self.draw_args,
            draw_args_offset: self.draw_args_offset,
            format: GpuVertexFormat::POSITION_NORMAL,
            transform,
            vertex_capacity: mesh.positions.len() as u32,
            triangle_capacity: (mesh.indices.len() / 3) as u32,
            closed,
        }
    }
}

/// A closed 1 m square box `height` tall resting on y = 0, with shared
/// corners and smoothed normals.
fn sharp_box(height: f32) -> SurfaceMesh {
    let positions: Vec<[f32; 3]> = (0..8)
        .map(|i| {
            [
                (i & 1) as f32 - 0.5,
                ((i >> 1) & 1) as f32 * height,
                ((i >> 2) & 1) as f32 - 0.5,
            ]
        })
        .collect();
    let normals = positions
        .iter()
        .map(|&[x, y, z]| {
            Vec3::new(x, if y > 0.0 { 0.5 } else { -0.5 }, z)
                .normalize()
                .to_array()
        })
        .collect();
    let faces = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    let indices = faces
        .iter()
        .flat_map(|&[a, b, c, d]| [a, b, c, a, c, d])
        .collect();
    SurfaceMesh {
        positions,
        normals,
        indices,
    }
}

/// `mesh` with every triangle on its own copies of its corners, as at
/// texture seams.
fn seamed(mesh: &SurfaceMesh) -> SurfaceMesh {
    let mut out = SurfaceMesh::default();
    for &i in &mesh.indices {
        out.positions.push(mesh.positions[i as usize]);
        out.normals.push(mesh.normals[i as usize]);
        out.indices.push(out.indices.len() as u32);
    }
    out
}

/// A V-shaped trough along z, with walls rising 1.5 m per metre on either
/// side, so clumps on one wall face the other across a narrowing gap.
fn crevice() -> SurfaceMesh {
    let mut mesh = SurfaceMesh::ground(1.0, 16);
    for (p, n) in mesh.positions.iter_mut().zip(&mut mesh.normals) {
        p[1] = 1.5 * p[0].abs();
        let side = if p[0] > 0.0 {
            -1.5
        } else if p[0] < 0.0 {
            1.5
        } else {
            0.0
        };
        *n = Vec3::new(side, 1.0, 0.0).normalize().to_array();
    }
    mesh
}

/// A procgena_stone flat slab at `resolution` cells per axis.
fn slab(resolution: u32) -> SurfaceMesh {
    let mut options = procgena_stone::StonePreset::FlatSlab.options();
    options.mesh.detail =
        procgena_stone::StoneDetail::single(resolution, procgena_stone::TextureSize::Size1k);
    let stone = procgena_stone::Stone::new(options).unwrap();
    let mesh = stone.mesh();
    SurfaceMesh {
        positions: mesh.positions.clone(),
        normals: mesh.normals.clone(),
        indices: mesh.indices.clone(),
    }
}

/// Returns each welded position's normal as the CPU computes it: the sum of
/// its vertices' normals in vertex order, made unit length.
fn cpu_welded_normals(mesh: &SurfaceMesh) -> Vec<Vec3> {
    let (weld, count) = mesh.surface().unwrap().welded();
    let mut sums = vec![Vec3::ZERO; count];
    for (v, &id) in weld.iter().enumerate() {
        sums[id as usize] += Vec3::from(mesh.normals[v]);
    }
    sums.into_iter().map(|n| n.normalize_or_zero()).collect()
}

/// One input of the prepare test: name, mesh, unused vertices and indices
/// in front of it, transform, and closed fact.
type PrepareCase = (&'static str, SurfaceMesh, u32, u32, Affine3A, Option<bool>);

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_prepare_welds_and_finds_closed_inputs_like_the_cpu() {
    let (device, queue) = device();
    let pipelines = Pipelines::new(&device);
    let max_groups = device.limits().max_compute_workgroups_per_dimension;

    // Inputs: (mesh, leading vertices, leading indices, transform, closed fact).
    let mut bumpy = SurfaceMesh::ground(2.0, 24);
    for p in &mut bumpy.positions {
        p[1] = (p[0] * 3.1).sin() * 0.15 + (p[2] * 2.3).cos() * 0.1;
    }
    let placed = Affine3A::from_scale_rotation_translation(
        Vec3::splat(2.0),
        Quat::from_rotation_y(0.7),
        Vec3::new(1.0, -2.0, 3.0),
    );
    let cases: Vec<PrepareCase> = vec![
        (
            "ground",
            SurfaceMesh::ground(1.0, 4),
            0,
            0,
            Affine3A::IDENTITY,
            None,
        ),
        ("box", sharp_box(0.3), 0, 0, Affine3A::IDENTITY, None),
        (
            "seamed box",
            seamed(&sharp_box(0.3)),
            0,
            0,
            Affine3A::IDENTITY,
            None,
        ),
        ("slab", slab(48), 0, 0, Affine3A::IDENTITY, None),
        (
            "bumpy, offset draw",
            bumpy,
            37,
            12,
            Affine3A::IDENTITY,
            None,
        ),
        ("placed slab", slab(32), 0, 0, placed, None),
        (
            "ground said closed",
            SurfaceMesh::ground(1.0, 2),
            0,
            0,
            Affine3A::IDENTITY,
            Some(true),
        ),
    ];
    let uploads: Vec<GpuMesh> = cases
        .iter()
        .map(|(_, mesh, lv, li, _, _)| GpuMesh::upload(&device, mesh, *lv, *li))
        .collect();
    let options = ClumpOptions::default();
    let requests: Vec<StageRequest> = cases
        .iter()
        .zip(&uploads)
        .map(|((_, mesh, _, _, transform, closed), up)| StageRequest {
            input: up.input(mesh, *transform, *closed),
            options: &options,
            out_vertices: 0,
            out_indices: 0,
        })
        .collect();

    let batch = BatchResources::new(&device, &pipelines, &requests);
    let mut encoder = device.create_command_encoder(&Default::default());
    batch.record_prepare(&mut encoder, &pipelines, max_groups);
    let read = read_words(&device, &queue, encoder, &[&batch.counters, &batch.arena]);
    let (counters, arena) = (&read[0], &read[1]);
    let regions = &batch.plan.regions;

    for (r, (name, mesh, _, _, transform, closed)) in cases.iter().enumerate() {
        let c = |which: u32| counters[r * COUNTER_STRIDE as usize + which as usize];
        let slices = batch.plan.slices[r];
        let surface = mesh.surface().unwrap();
        let (weld, welded) = surface.welded();
        assert_eq!(c(counter::OVERFLOW), 0, "{name}: overflow");
        assert_eq!(
            c(counter::TRIANGLES) as usize,
            mesh.indices.len() / 3,
            "{name}: triangles"
        );
        assert_eq!(
            c(counter::VERTICES) as usize,
            mesh.positions.len(),
            "{name}: vertices"
        );
        assert_eq!(c(counter::WELDED) as usize, welded, "{name}: welded count");

        // Weld ids exactly as the CPU numbers positions.
        let ids = &arena[(regions[REGION_WELD_ID] + slices.vertex_base) as usize..][..weld.len()];
        assert_eq!(ids, weld.as_slice(), "{name}: weld ids");

        // Closed exactly when the CPU says so, unless the input says.
        let open = c(counter::OPEN_EDGES);
        let gpu_closed = closed.unwrap_or(open == 0);
        assert_eq!(
            gpu_closed,
            closed.unwrap_or_else(|| surface.is_closed()),
            "{name}: closed"
        );

        // Welded normals: within GPU rounding of the CPU's (division and
        // square roots aren't exact on every GPU; the interpreter tests
        // check the shader math bit for bit), turned by the transform.
        let first = arena[(regions[REGION_WELD_RANK] + slices.vertex_base) as usize];
        let cofactor = |n: Vec3| {
            let m = transform.matrix3;
            let (a, b, c) = (
                Vec3::from(m.x_axis),
                Vec3::from(m.y_axis),
                Vec3::from(m.z_axis),
            );
            (b.cross(c) * n.x + c.cross(a) * n.y + a.cross(b) * n.z).normalize_or_zero()
        };
        for (id, cpu) in cpu_welded_normals(mesh).iter().enumerate() {
            let at = (regions[REGION_WELDED_NORMALS] + 3 * (first + id as u32)) as usize;
            let gpu = Vec3::new(
                f32::from_bits(arena[at]),
                f32::from_bits(arena[at + 1]),
                f32::from_bits(arena[at + 2]),
            );
            let tolerance = if *transform == Affine3A::IDENTITY {
                1e-6
            } else {
                1e-5
            };
            assert!(
                gpu.distance(cofactor(*cpu)) < tolerance,
                "{name}: normal {id}: {gpu} vs {}",
                cofactor(*cpu)
            );
        }

        // Positions placed by the transform.
        for (v, p) in mesh.positions.iter().enumerate() {
            let at = (regions[REGION_POSITIONS] + 3 * (slices.vertex_base + v as u32)) as usize;
            let gpu = Vec3::new(
                f32::from_bits(arena[at]),
                f32::from_bits(arena[at + 1]),
                f32::from_bits(arena[at + 2]),
            );
            let cpu = transform.transform_point3(Vec3::from(*p));
            assert!(
                gpu.distance(cpu) <= 1e-5 * cpu.length().max(1.0),
                "{name}: vertex {v}"
            );
        }
    }
}

/// One clump read back from a GPU batch: its status and its mesh.
struct GpuClumpMesh {
    status: super::GpuClumpStatus,
    draw_args: [u32; 5],
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    fade: Vec<f32>,
    indices: Vec<u32>,
}

/// Records `requests`, runs them, and reads every clump back.
fn run_batch(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    generator: &super::GpuClumpGenerator,
    requests: &[super::GpuClumpRequest],
) -> Vec<GpuClumpMesh> {
    let mut encoder = device.create_command_encoder(&Default::default());
    let batch = generator
        .encode(device, &mut encoder, requests)
        .expect("encode");
    let read = read_words(
        device,
        queue,
        encoder,
        &[
            &batch.status,
            &batch.draw_args,
            &batch.vertices,
            &batch.indices,
        ],
    );
    let (status, draw, vertices, indices) = (&read[0], &read[1], &read[2], &read[3]);
    batch
        .ranges
        .iter()
        .enumerate()
        .map(|(i, range)| {
            let s = &status[16 * i..16 * i + 16];
            let bytes: Vec<u8> = s.iter().flat_map(|w| w.to_le_bytes()).collect();
            let status = super::GpuClumpStatus::decode(&bytes);
            let draw_args: [u32; 5] = draw[5 * i..5 * i + 5].try_into().unwrap();
            let base = range.vertex_base as usize;
            let vertex = |v: usize, k: usize| f32::from_bits(vertices[7 * (base + v) + k]);
            let n = status.vertex_count as usize;
            GpuClumpMesh {
                status,
                draw_args,
                positions: (0..n)
                    .map(|v| Vec3::new(vertex(v, 0), vertex(v, 1), vertex(v, 2)))
                    .collect(),
                normals: (0..n)
                    .map(|v| Vec3::new(vertex(v, 3), vertex(v, 4), vertex(v, 5)))
                    .collect(),
                fade: (0..n).map(|v| vertex(v, 6)).collect(),
                indices: indices[range.index_base as usize..][..status.index_count as usize]
                    .to_vec(),
            }
        })
        .collect()
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_scattered_clumps_match_the_cpu() {
    use crate::{Clump, ClumpPreset, Direction, Placement};

    let (device, queue) = device();
    let generator = super::GpuClumpGenerator::new(&device);

    // Inputs, and the settings to scatter on them.
    let mut bumpy = SurfaceMesh::ground(2.0, 24);
    for (p, n) in bumpy.positions.iter_mut().zip(&mut bumpy.normals) {
        let (x, z) = (p[0], p[2]);
        p[1] = (x * 3.1).sin() * 0.15 + (z * 2.3).cos() * 0.1;
        let slope = Vec3::new(-(x * 3.1).cos() * 0.465, 1.0, (z * 2.3).sin() * 0.23);
        *n = slope.normalize().to_array();
    }
    let mut exposed = ClumpPreset::MossCushions.options();
    exposed.directions = vec![Direction::DOWN, Direction::new([1.0, -0.5, 0.0])];
    exposed.mask.exposure_rays = 16;
    exposed.mask.exposure_power = 1.0;
    exposed.mesh.closed = false;
    exposed.placement.count = 30;
    exposed.seed = 7;
    let mut many = ClumpPreset::MossCushions.options();
    many.placement.count = 60;
    many.placement.min_spacing = 0.05;
    // Overlapping cushions, so strict mode leaves some out.
    let mut crowded = ClumpPreset::MossCushions.options();
    crowded.placement.count = 40;
    crowded.placement.min_spacing = 0.0;
    crowded.placement.radius = 0.12;
    crowded.mesh.strict = true;
    let cases: Vec<(&str, SurfaceMesh, ClumpOptions)> = vec![
        (
            "cushions on ground",
            SurfaceMesh::ground(1.0, 8),
            ClumpPreset::MossCushions.options(),
        ),
        ("burl on slab", slab(48), ClumpPreset::Burl.options()),
        ("exposed cushions on bumpy ground", bumpy.clone(), exposed),
        ("many cushions on slab", slab(48), many),
        (
            "strict crowded cushions",
            SurfaceMesh::ground(1.0, 8),
            crowded.clone(),
        ),
        ("strict crowded open cushions", bumpy.clone(), {
            let mut open = crowded;
            open.mesh.closed = false;
            open.seed = 11;
            open
        }),
    ];
    assert!(
        cases
            .iter()
            .all(|(_, _, o)| o.placement.mode == Placement::Scatter)
    );
    let uploads: Vec<GpuMesh> = cases
        .iter()
        .map(|(_, mesh, _)| GpuMesh::upload(&device, mesh, 0, 0))
        .collect();
    let requests: Vec<super::GpuClumpRequest> = cases
        .iter()
        .zip(&uploads)
        .map(|((_, mesh, options), up)| {
            super::GpuClumpRequest::new(
                up.input(mesh, Affine3A::IDENTITY, None),
                options.clone(),
                0.0,
            )
        })
        .collect();
    let gpu = run_batch(&device, &queue, &generator, &requests);

    for ((name, mesh, options), got) in cases.iter().zip(&gpu) {
        let cpu = Clump::new(mesh.surface().unwrap(), options).unwrap();
        let want = cpu.mesh();
        let status = &got.status;
        eprintln!(
            "{name}: {} clumps, {} vertices, {} indices (CPU {} clumps, {} vertices, {} indices)",
            status.clumps,
            status.vertex_count,
            status.index_count,
            cpu.stats().clumps,
            want.positions.len(),
            want.indices.len()
        );

        // The same clumps, meshed the same way, drawable.
        assert!(
            !status.input_overflow && !status.vertex_overflow && !status.index_overflow,
            "{name}: {status:?}"
        );
        assert_eq!(status.clumps as usize, cpu.stats().clumps, "{name}: clumps");
        if options.mesh.strict {
            // Strict mode really left some clumps out here.
            assert!(
                cpu.stats().clumps < options.placement.count as usize,
                "{name}"
            );
        }
        assert_eq!(
            status.vertex_count as usize,
            want.positions.len(),
            "{name}: vertices"
        );
        assert_eq!(
            status.index_count as usize,
            want.indices.len(),
            "{name}: indices"
        );
        assert_eq!(got.indices, want.indices, "{name}: indices");
        assert_eq!(got.draw_args[0], status.index_count, "{name}: draw count");

        // Vertices in the same places, within GPU rounding: about a
        // millionth of the input's size. A blob vertex drapes to the closest
        // point on the input, and where two triangles are nearly equally
        // close, the CPU and GPU can each pick a different, equally valid
        // one. That moved one burl vertex on the slab by 2.3e-5 m, where a
        // 1 µm shift of the query moves the CPU's own drape by 2.3e-5 m. So a
        // few vertices may move further, but never far.
        let size = {
            let (lo, hi) = mesh.surface().unwrap().bounds();
            (hi - lo).length()
        };
        let distances: Vec<f32> = got
            .positions
            .iter()
            .zip(&want.positions)
            .map(|(p, q)| p.distance(Vec3::from(*q)))
            .collect();
        let far = distances.iter().filter(|&&d| d > 1e-5 * size).count();
        let worst = distances.iter().fold(0.0f32, |a, &b| a.max(b));
        eprintln!("{name}: {far} vertices beyond 1e-5 of the size, worst {worst:e} m");
        assert!(
            far <= 1 + distances.len() / 500,
            "{name}: {far} vertices moved"
        );
        assert!(worst <= 1e-3 * size, "{name}: a vertex moved {worst} m");

        // Normals follow the vertices: a moved vertex also tilts its up to
        // six neighbours' normals.
        let normals_far = got
            .normals
            .iter()
            .zip(&want.normals)
            .filter(|(n, m)| n.distance(Vec3::from(**m)) > 1e-3)
            .count();
        assert!(
            normals_far <= 7 * far,
            "{name}: {normals_far} normals differ"
        );
        for (v, (f, g)) in got.fade.iter().zip(&want.fade).enumerate() {
            assert!((f - g).abs() <= 1e-4, "{name}: fade {v}: {f} vs {g}");
        }
    }
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_overflowing_clumps_draw_nothing_until_grown_and_batches_repeat() {
    use crate::ClumpPreset;

    let (device, queue) = device();
    let generator = super::GpuClumpGenerator::new(&device);
    let ground = SurfaceMesh::ground(1.0, 8);
    let up = GpuMesh::upload(&device, &ground, 0, 0);
    let input = up.input(&ground, Affine3A::IDENTITY, None);
    let options = ClumpPreset::MossCushions.options();

    // Too little room: the clump reports what it needs and draws nothing,
    // while a request beside it is unaffected.
    let roomy = super::GpuClumpRequest::new(input, options.clone(), 0.0);
    let mut tight = roomy.clone();
    tight.vertex_capacity = 100;
    tight.index_capacity = 300;
    let first = run_batch(&device, &queue, &generator, &[tight.clone(), roomy.clone()]);
    let status = first[0].status;
    assert!(
        status.vertex_overflow && status.index_overflow,
        "{status:?}"
    );
    assert_eq!(first[0].draw_args[0], 0);
    assert!(!first[1].status.vertex_overflow && first[1].draw_args[0] > 0);

    // Grown to fit, it builds the same clump as the roomy request.
    tight.grow_to_fit(&status);
    let second = run_batch(&device, &queue, &generator, &[tight, roomy.clone()]);
    assert!(!second[0].status.vertex_overflow && !second[0].status.index_overflow);
    assert_eq!(second[0].indices, second[1].indices);
    assert_eq!(second[0].positions, second[1].positions);

    // The same batch twice gives the same bits.
    let again = run_batch(&device, &queue, &generator, &[roomy.clone(), roomy]);
    let bits = |m: &GpuClumpMesh| {
        let floats = m
            .positions
            .iter()
            .chain(&m.normals)
            .flat_map(|v| v.to_array());
        floats
            .chain(m.fade.iter().copied())
            .map(f32::to_bits)
            .collect::<Vec<_>>()
    };
    assert_eq!(bits(&again[0]), bits(&second[1]));
    assert_eq!(again[0].indices, second[1].indices);
}

/// Runs every stage of a batch of `cases` (mesh, settings) and returns the
/// batch's layout with its counters and arena read back.
fn run_stages(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cases: &[(SurfaceMesh, ClumpOptions)],
) -> (super::batch::BatchPlan, Vec<u32>, Vec<u32>) {
    let (plan, counters, arena, _) = run_stages_sized(device, queue, cases, 4.0);
    (plan, counters, arena)
}

/// Runs [`run_stages`] with output capacities estimated for `covered_area`
/// square metres per request, and also returns the flags buffer.
fn run_stages_sized(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cases: &[(SurfaceMesh, ClumpOptions)],
    covered_area: f32,
) -> (super::batch::BatchPlan, Vec<u32>, Vec<u32>, Vec<u32>) {
    let (plan, counters, arena, flags, _) =
        run_stages_limited(device, queue, cases, covered_area, None);
    (plan, counters, arena, flags)
}

/// Runs [`run_stages_sized`], recording only `strict_passes` strict passes
/// when given: 0 records only the first check for crossings. Also returns
/// each request's status, decoded.
fn run_stages_limited(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cases: &[(SurfaceMesh, ClumpOptions)],
    covered_area: f32,
    strict_passes: Option<u32>,
) -> (
    super::batch::BatchPlan,
    Vec<u32>,
    Vec<u32>,
    Vec<u32>,
    Vec<super::GpuClumpStatus>,
) {
    let pipelines = Pipelines::new(device);
    let uploads: Vec<GpuMesh> = cases
        .iter()
        .map(|(m, _)| GpuMesh::upload(device, m, 0, 0))
        .collect();
    let requests: Vec<StageRequest> = cases
        .iter()
        .zip(&uploads)
        .map(|((mesh, options), up)| {
            let (out_vertices, out_indices) = super::estimate_capacity(options, covered_area);
            StageRequest {
                input: up.input(mesh, Affine3A::IDENTITY, None),
                options,
                out_vertices,
                out_indices,
            }
        })
        .collect();
    let mut batch = BatchResources::new(device, &pipelines, &requests);
    if let Some(passes) = strict_passes {
        batch.plan.strict_passes = passes;
    }
    let outputs = batch.outputs(device, &pipelines);
    let mut encoder = device.create_command_encoder(&Default::default());
    batch.record(
        &mut encoder,
        &pipelines,
        &outputs,
        device.limits().max_compute_workgroups_per_dimension,
    );
    let mut read = read_words(
        device,
        queue,
        encoder,
        &[&batch.counters, &batch.arena, &batch.flags, &outputs.status],
    );
    let status = read.pop().unwrap();
    let flags = read.pop().unwrap();
    let arena = read.pop().unwrap();
    let counters = read.pop().unwrap();
    let status = status
        .chunks(16)
        .take(cases.len())
        .map(|words| {
            let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
            super::GpuClumpStatus::decode(&bytes)
        })
        .collect();
    (batch.plan, counters, arena, flags, status)
}

/// Reads three floats from `words` at `at`.
fn vec3_at(words: &[u32], at: u32) -> Vec3 {
    let at = at as usize;
    Vec3::new(
        f32::from_bits(words[at]),
        f32::from_bits(words[at + 1]),
        f32::from_bits(words[at + 2]),
    )
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_facing_triangles_match_the_cpu() {
    use crate::patch::{facing_triangles, welded_normals};
    use crate::{ClumpPreset, Direction};

    let (device, queue) = device();
    let mut bumpy = SurfaceMesh::ground(2.0, 24);
    for p in &mut bumpy.positions {
        p[1] = (p[0] * 3.1).sin() * 0.15 + (p[2] * 2.3).cos() * 0.1;
    }
    let sideways = ClumpOptions {
        directions: vec![Direction::new([1.0, -0.2, 0.0])],
        ..ClumpOptions::default()
    };
    let cases = vec![
        (SurfaceMesh::ground(1.0, 8), ClumpOptions::default()),
        (bumpy, ClumpPreset::MossPatches.options()),
        (slab(48), ClumpOptions::default()),
        (seamed(&sharp_box(0.3)), sideways),
        // A scattered request alongside makes no working surface.
        (slab(32), ClumpPreset::MossCushions.options()),
    ];
    let (plan, counters, arena) = run_stages(&device, &queue, &cases);
    let regions = &plan.regions;

    for (r, (mesh, options)) in cases.iter().enumerate() {
        let c = |which: u32| counters[r * COUNTER_STRIDE as usize + which as usize];
        let (points_counter, tris_counter) = (26, 27);
        let slices = plan.slices[r];
        if options.placement.mode == crate::Placement::Scatter {
            assert_eq!((c(points_counter), c(tris_counter)), (0, 0), "scatter {r}");
            continue;
        }
        let field = crate::field::ClumpField::new(mesh.surface().unwrap(), options);
        let (points, tris) = facing_triangles(&field, &welded_normals(&field));
        assert!(!tris.is_empty(), "request {r} faces nothing");
        assert_eq!(
            c(points_counter) as usize,
            points.len(),
            "request {r}: points"
        );
        assert_eq!(
            c(tris_counter) as usize,
            tris.len(),
            "request {r}: triangles"
        );

        // Points: the same positions, and smooth normals within GPU rounding.
        for (i, p) in points.iter().enumerate() {
            let at = regions[REGION_PATCH_POINTS] + POINT_WORDS * (slices.vertex_base + i as u32);
            assert_eq!(vec3_at(&arena, at), p.point, "request {r}: point {i}");
            assert!(
                vec3_at(&arena, at + 3).distance(p.normal) < 1e-6,
                "request {r}: normal {i}"
            );
        }

        // Triangles: the same corners, and source normals within rounding.
        for (i, t) in tris.iter().enumerate() {
            let at =
                regions[REGION_PATCH_TRIS] + PATCH_TRI_WORDS * (slices.triangle_base + i as u32);
            let corners = [0, 1, 2].map(|k| arena[(at + k) as usize]);
            assert_eq!(corners, t.corners, "request {r}: triangle {i}");
            // A cross product rounds relative to its terms, |b - a| |c - a|,
            // not to its result, which is small for slim triangles.
            let n = vec3_at(&arena, at + 3);
            let [a, b, c] = t.corners.map(|k| points[k as usize].point);
            let terms = (b - a).length() * (c - a).length();
            assert!(
                n.distance(t.source_normal) <= 1e-6 * terms,
                "request {r}: source normal {i}: {n} vs {}",
                t.source_normal
            );
        }
    }
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_simplified_surfaces_match_the_cpu() {
    use crate::patch::Patch;

    let (device, queue) = device();
    let mut bumpy = SurfaceMesh::ground(2.0, 100);
    for (p, n) in bumpy.positions.iter_mut().zip(&mut bumpy.normals) {
        let (x, z) = (p[0], p[2]);
        p[1] = 0.05 * (5.0 * x).sin() * (4.0 * z).cos();
        let dx = 0.25 * (5.0 * x).cos() * (4.0 * z).cos();
        let dz = -0.2 * (5.0 * x).sin() * (4.0 * z).sin();
        *n = Vec3::new(-dx, 1.0, -dz).normalize().to_array();
    }
    let at = |edge_length: f32| {
        let mut options = ClumpOptions::default();
        options.mask.exposure_rays = 0;
        options.mesh.edge_length = edge_length;
        options
    };
    // On a perfectly regular grid, mirrored merges score exactly equal
    // quality, so rounding decides between them: the CPU's own result moves
    // between 243 and 252 points when the grid is nudged by 1e-7 m. There
    // the GPU must give a valid simplification of about the same size; on
    // irregular input it must give the CPU's.
    let cases = [
        (bumpy.clone(), at(0.04), true),
        (bumpy, at(0.1), true),
        (SurfaceMesh::ground(1.0, 64), at(0.1), false),
        (slab(64), at(0.1), true),
        (slab(64), at(0.3), true),
    ];
    let inputs: Vec<(SurfaceMesh, ClumpOptions)> = cases
        .iter()
        .map(|(m, o, _)| (m.clone(), o.clone()))
        .collect();
    let (plan, counters, arena) = run_stages(&device, &queue, &inputs);
    let regions = &plan.regions;
    let (collapsed_counter, capped_counter, points_counter, tris_counter) = (30, 31, 32, 33);

    for (r, (mesh, options, exact)) in cases.iter().enumerate() {
        let c = |which: u32| counters[r * COUNTER_STRIDE as usize + which as usize];
        let slices = plan.slices[r];
        let field = crate::field::ClumpField::new(mesh.surface().unwrap(), options);
        let edge_length = options.mesh.edge_length * options.scale;
        let (points, tris, collapsed) = Patch::simplified(&field, edge_length);
        eprintln!(
            "case {r}: CPU {} points, {} triangles; GPU {} points, {} triangles",
            points.len(),
            tris.len(),
            c(points_counter),
            c(tris_counter)
        );
        assert_eq!(c(capped_counter), 0, "case {r}: selection rounds ran out");
        assert_eq!(c(collapsed_counter) != 0, collapsed, "case {r}: collapsed");
        if !exact {
            let gpu_points = c(points_counter) as f32;
            assert!(
                (gpu_points / points.len() as f32 - 1.0).abs() < 0.05,
                "case {r}: point count"
            );
            assert_valid_simplification(&arena, regions, &slices, c(tris_counter), edge_length, r);
            continue;
        }
        assert_eq!(c(points_counter) as usize, points.len(), "case {r}: points");
        assert_eq!(c(tris_counter) as usize, tris.len(), "case {r}: triangles");

        // The same triangles, and points in the same places.
        let (lo, hi) = mesh.surface().unwrap().bounds();
        let tolerance = 1e-5 * (hi - lo).length();
        for (i, t) in tris.iter().enumerate() {
            let at =
                regions[REGION_COARSE_TRIS] + PATCH_TRI_WORDS * (slices.triangle_base + i as u32);
            let corners = [0, 1, 2].map(|k| arena[(at + k) as usize]);
            assert_eq!(corners, t.corners, "case {r}: triangle {i}");
        }
        for (i, p) in points.iter().enumerate() {
            let at = regions[REGION_COARSE_POINTS] + POINT_WORDS * (slices.vertex_base + i as u32);
            let gpu = vec3_at(&arena, at);
            assert!(
                gpu.distance(p.point) <= tolerance,
                "case {r}: point {i}: {gpu} vs {}",
                p.point
            );
        }
    }
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_working_surfaces_match_the_cpu() {
    use crate::ClumpPreset;
    use crate::options::MAX_DIRECTIONS;
    use crate::patch::Patch;

    let (device, queue) = device();
    let mut bumpy = SurfaceMesh::ground(2.0, 100);
    for (p, n) in bumpy.positions.iter_mut().zip(&mut bumpy.normals) {
        let (x, z) = (p[0], p[2]);
        p[1] = 0.05 * (5.0 * x).sin() * (4.0 * z).cos();
        let dx = 0.25 * (5.0 * x).cos() * (4.0 * z).cos();
        let dz = -0.2 * (5.0 * x).sin() * (4.0 * z).sin();
        *n = Vec3::new(-dx, 1.0, -dz).normalize().to_array();
    }
    let with = |mut options: ClumpOptions, edge_length: f32| {
        options.mesh.edge_length = edge_length;
        options
    };
    // Snow caps measure exposure and drop steep triangles; the default
    // options keep every facing triangle. The fine grid at 4 cm merges
    // nothing, so its rise and dip stay 0.
    let cases = [
        (
            bumpy.clone(),
            with(ClumpPreset::DirtySnowCap.options(), 0.1),
        ),
        (bumpy, with(ClumpOptions::default(), 0.04)),
        (slab(64), with(ClumpPreset::DirtySnowCap.options(), 0.3)),
        (slab(64), with(ClumpOptions::default(), 0.1)),
        (
            SurfaceMesh::ground(1.0, 8),
            with(ClumpOptions::default(), 0.04),
        ),
    ];
    let (plan, counters, arena) = run_stages(&device, &queue, &cases);
    let regions = &plan.regions;
    let (points_counter, layer_tris_counter) = (32, 34);
    let mut tied = 0;

    for (r, (mesh, options)) in cases.iter().enumerate() {
        let c = |which: u32| counters[r * COUNTER_STRIDE as usize + which as usize];
        let slices = plan.slices[r];
        let field = crate::field::ClumpField::new(mesh.surface().unwrap(), options);
        let patch = Patch::new(&field, options.mesh.edge_length * options.scale);
        eprintln!(
            "case {r}: CPU {} points, {} triangles; GPU {} points, {} triangles",
            patch.points.len(),
            patch.tris.len(),
            c(points_counter),
            c(layer_tris_counter)
        );
        assert_eq!(
            c(points_counter) as usize,
            patch.points.len(),
            "case {r}: points"
        );
        assert_eq!(
            c(layer_tris_counter) as usize,
            patch.tris.len(),
            "case {r}: covered triangles"
        );

        // The same covered triangles.
        for (i, t) in patch.tris.iter().enumerate() {
            let at =
                regions[REGION_LAYER_TRIS] + PATCH_TRI_WORDS * (slices.triangle_base + i as u32);
            let corners = [0, 1, 2].map(|k| arena[(at + k) as usize]);
            assert_eq!(corners, t.corners, "case {r}: triangle {i}");
        }

        // Rise and dip within what any choice among tied closest triangles
        // allows: a fine point whose closest point lies on an edge between
        // two simplified triangles is equally close to both, and the CPU and
        // GPU may raise different corners.
        let (lo, hi) = mesh.surface().unwrap().bounds();
        let tolerance = 1e-5 * (hi - lo).length();
        let bounds = envelope_bounds(&field, options, tolerance);

        // The same measurements, rise and dip at every point.
        let sources = options.directions.len().min(MAX_DIRECTIONS);
        for (i, p) in patch.points.iter().enumerate() {
            let at = (regions[REGION_COARSE_POINTS] + POINT_WORDS * (slices.vertex_base + i as u32))
                as usize;
            let word = |k: usize| f32::from_bits(arena[at + k]);
            for d in 0..sources {
                assert_eq!(
                    word(6 + d),
                    p.exposure[d],
                    "case {r}: point {i} exposure {d}"
                );
            }
            // The GPU's "unlimited" is the largest finite float.
            let room = if word(14) == f32::MAX {
                f32::INFINITY
            } else {
                word(14)
            };
            assert!(
                room == p.room || (room - p.room).abs() <= tolerance,
                "case {r}: point {i} room {room} vs {}",
                p.room
            );
            let [rise, dip] = bounds[i];
            for (name, gpu, cpu, (least, most)) in [
                ("rise", word(15), p.rise, rise),
                ("dip", word(16), p.dip, dip),
            ] {
                let within = |x: f32| x >= least - tolerance && x <= most + tolerance;
                assert!(
                    within(cpu),
                    "case {r}: point {i} CPU {name} {cpu} outside {least}..{most}"
                );
                assert!(
                    within(gpu),
                    "case {r}: point {i} GPU {name} {gpu} outside {least}..{most}"
                );
                if (gpu - cpu).abs() > tolerance {
                    tied += 1;
                }
            }
        }
    }
    eprintln!("{tied} rise or dip values differ through tied closest triangles");
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_mixed_batches_and_grown_layers_match_the_cpu() {
    use crate::{Clump, ClumpPreset};

    let (device, queue) = device();
    let generator = super::GpuClumpGenerator::new(&device);
    let mut snow = ClumpPreset::DirtySnowCap.options();
    snow.mesh.closed = false;
    let mut closed_snow = ClumpPreset::DirtySnowCap.options();
    closed_snow.mesh.closed = true;
    let mut strict = ClumpOptions::default();
    strict.mesh.strict = true;
    let mut crowded = ClumpPreset::MossCushions.options();
    crowded.placement.count = 40;
    crowded.placement.min_spacing = 0.0;
    crowded.placement.radius = 0.12;
    crowded.mesh.strict = true;
    let cases: Vec<(&str, SurfaceMesh, ClumpOptions)> = vec![
        ("snow cap on slab", slab(48), snow),
        (
            "cushions on ground",
            SurfaceMesh::ground(1.0, 8),
            ClumpPreset::MossCushions.options(),
        ),
        (
            "strict crowded cushions",
            SurfaceMesh::ground(1.0, 8),
            crowded,
        ),
        ("closed snow cap on slab", slab(48), closed_snow),
        ("strict default on slab", slab(48), strict),
    ];
    let uploads: Vec<GpuMesh> = cases
        .iter()
        .map(|(_, mesh, _)| GpuMesh::upload(&device, mesh, 0, 0))
        .collect();
    let request = |i: usize, covered_area: f32| {
        let (_, mesh, options) = &cases[i];
        super::GpuClumpRequest::new(
            uploads[i].input(mesh, Affine3A::IDENTITY, None),
            options.clone(),
            covered_area,
        )
    };

    // Each request alone, then all of them in one batch: the same clumps.
    let alone: Vec<GpuClumpMesh> = (0..cases.len())
        .map(|i| run_batch(&device, &queue, &generator, &[request(i, 8.0)]).remove(0))
        .collect();
    let requests: Vec<_> = (0..cases.len()).map(|i| request(i, 8.0)).collect();
    let mixed = run_batch(&device, &queue, &generator, &requests);
    for (((name, mesh, options), got), single) in cases.iter().zip(&mixed).zip(&alone) {
        let want = Clump::new(mesh.surface().unwrap(), options).unwrap();
        let status = &got.status;
        eprintln!("{name}: {status:?}");
        assert!(
            !status.vertex_overflow && !status.index_overflow,
            "{name}: {status:?}"
        );
        assert_eq!(*status, single.status, "{name}: status alone and mixed");
        assert_eq!(
            got.indices, single.indices,
            "{name}: indices alone and mixed"
        );
        assert_eq!(
            got.positions, single.positions,
            "{name}: positions alone and mixed"
        );
        assert_eq!(got.indices, want.mesh().indices, "{name}: indices");
        assert_eq!(
            status.clumps as usize,
            want.stats().clumps,
            "{name}: clumps"
        );
    }

    // Layers with too little room, open, closed and strict: grown and
    // retried until they fit, they match the CPU.
    for i in [0, 3, 4] {
        let name = cases[i].0;
        let mut small = request(i, 0.0);
        small.vertex_capacity = 16;
        small.index_capacity = 48;
        let mut tries = 0;
        let grown = loop {
            tries += 1;
            assert!(tries <= 16, "{name}: never fit");
            let got =
                run_batch(&device, &queue, &generator, std::slice::from_ref(&small)).remove(0);
            if !got.status.vertex_overflow && !got.status.index_overflow {
                break got;
            }
            assert_eq!(
                got.draw_args[0], 0,
                "{name}: an overflowing clump draws nothing"
            );
            small.grow_to_fit(&got.status);
        };
        eprintln!("{name}: grown layer fit after {tries} tries");
        assert_eq!(grown.indices, mixed[i].indices, "{name}: grown layer");
        assert_eq!(grown.positions, mixed[i].positions, "{name}: grown layer");
    }
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_layers_at_the_facing_limit_stay_close_to_the_cpu() {
    use crate::{Clump, ClumpPreset};

    // Moss patches start covering at a facing of exactly 0, which is where a
    // box's sides face, so whether they may be covered rests on the last bit
    // of their normals, which blend the box's corner normals. Tilting every
    // input normal down by 1e-7 changes the CPU's own clump, so the GPU,
    // which rounds normals differently, can only match it closely, not
    // exactly.
    let mut options = ClumpPreset::MossPatches.options();
    options.mesh.closed = false;
    let mesh = seamed(&sharp_box(0.3));
    let mut tilted = mesh.clone();
    for n in &mut tilted.normals {
        n[1] -= 1e-7;
    }
    let cpu = Clump::new(mesh.surface().unwrap(), &options).unwrap();
    let cpu_tilted = Clump::new(tilted.surface().unwrap(), &options).unwrap();
    let largest_move = |a: &crate::ClumpMesh, b: &crate::ClumpMesh| -> Option<f32> {
        (a.indices == b.indices).then(|| {
            a.positions
                .iter()
                .zip(&b.positions)
                .map(|(p, q)| Vec3::from(*p).distance(Vec3::from(*q)))
                .fold(0.0, f32::max)
        })
    };
    let cpu_move = largest_move(cpu.mesh(), cpu_tilted.mesh());
    eprintln!("CPU, tilted by 1e-7: largest move {cpu_move:?}");
    assert_ne!(cpu_move, Some(0.0), "the tilt changes the CPU's clump");

    // The GPU's clump: the same triangles, within a quarter of the
    // thickness everywhere.
    let (device, queue) = device();
    let generator = super::GpuClumpGenerator::new(&device);
    let upload = GpuMesh::upload(&device, &mesh, 0, 0);
    let request = super::GpuClumpRequest::new(
        upload.input(&mesh, Affine3A::IDENTITY, None),
        options.clone(),
        8.0,
    );
    let got = run_batch(&device, &queue, &generator, &[request]).remove(0);
    assert_eq!(got.indices, cpu.mesh().indices);
    let gpu_move = got
        .positions
        .iter()
        .zip(&cpu.mesh().positions)
        .map(|(p, q)| p.distance(Vec3::from(*q)))
        .fold(0.0, f32::max);
    eprintln!("GPU: largest move {gpu_move}");
    assert!(gpu_move <= 0.25 * options.shape.thickness * options.scale);

    // The GPU's rounding lands on the tilted side of the edge, and there it
    // matches the CPU as closely as anywhere.
    assert_eq!(got.indices, cpu_tilted.mesh().indices);
    let to_tilted = got
        .positions
        .iter()
        .zip(&cpu_tilted.mesh().positions)
        .map(|(p, q)| p.distance(Vec3::from(*q)))
        .fold(0.0, f32::max);
    eprintln!("GPU against the tilted CPU clump: largest move {to_tilted}");
    assert!(to_tilted <= 1e-6, "{to_tilted}");
}

#[test]
fn only_batches_with_layers_reserve_sheet_space() {
    let shape = |layer: bool| RequestShape {
        vertex_capacity: 1000,
        triangle_capacity: 2000,
        attempts: if layer { 0 } else { 640 },
        seeds: if layer { 0 } else { 10 },
        rays: 0,
        out_vertices: 5000,
        out_indices: 30000,
        strict_scatter: false,
        layer,
        closed_layer: false,
        strict_layer: false,
    };
    let scatter = BatchPlan::new(&[shape(false)]);
    let layer = BatchPlan::new(&[shape(true)]);
    let sheet =
        |plan: &BatchPlan| plan.regions[REGION_SHELL_TRI_STATE] - plan.regions[REGION_SHEET_POINTS];
    // Only the three scan totals remain without layers.
    assert_eq!(sheet(&scatter), 3);
    assert!(sheet(&layer) > 3);
    assert!(scatter.arena_words < layer.arena_words);
    eprintln!(
        "arena words: scatter {}, layer {}",
        scatter.arena_words, layer.arena_words
    );
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_open_layers_match_the_cpu() {
    use crate::{Clump, ClumpPreset};

    let (device, queue) = device();
    let generator = super::GpuClumpGenerator::new(&device);
    let mut bumpy = SurfaceMesh::ground(2.0, 24);
    for (p, n) in bumpy.positions.iter_mut().zip(&mut bumpy.normals) {
        let (x, z) = (p[0], p[2]);
        p[1] = (x * 3.1).sin() * 0.15 + (z * 2.3).cos() * 0.1;
        let slope = Vec3::new(-(x * 3.1).cos() * 0.465, 1.0, (z * 2.3).sin() * 0.23);
        *n = slope.normalize().to_array();
    }
    let open = |preset: ClumpPreset| {
        let mut options = preset.options();
        options.mesh.closed = false;
        options
    };
    let mut default = ClumpOptions::default();
    default.mesh.closed = false;
    let cases: Vec<(&str, SurfaceMesh, ClumpOptions)> = vec![
        ("default on ground", SurfaceMesh::ground(1.0, 8), default),
        (
            "snow cap on bumpy ground",
            bumpy.clone(),
            open(ClumpPreset::DirtySnowCap),
        ),
        (
            "moss patches on bumpy ground",
            bumpy,
            open(ClumpPreset::MossPatches),
        ),
        (
            "snow cap on slab",
            slab(48),
            open(ClumpPreset::DirtySnowCap),
        ),
        // See `gpu_open_sheets_match_the_cpu` for why `facing_min` is lifted.
        ("moss patches on a box", seamed(&sharp_box(0.3)), {
            let mut clear_of_sides = open(ClumpPreset::MossPatches);
            clear_of_sides.mask.facing_min = 0.01;
            clear_of_sides
        }),
        ("deep moss in a crevice", crevice(), {
            let mut deep = open(ClumpPreset::MossPatches);
            deep.shape.thickness = 0.2;
            deep.mask.any_facing = true;
            deep
        }),
    ];
    let uploads: Vec<GpuMesh> = cases
        .iter()
        .map(|(_, mesh, _)| GpuMesh::upload(&device, mesh, 0, 0))
        .collect();
    let requests: Vec<super::GpuClumpRequest> = cases
        .iter()
        .zip(&uploads)
        .map(|((_, mesh, options), up)| {
            super::GpuClumpRequest::new(
                up.input(mesh, Affine3A::IDENTITY, None),
                options.clone(),
                8.0,
            )
        })
        .collect();
    let gpu = run_batch(&device, &queue, &generator, &requests);

    // The batch again, warm, for its cost: thousands of fixed dispatches.
    let start = Instant::now();
    run_batch(&device, &queue, &generator, &requests);
    eprintln!(
        "warm batch of {} layers: {:?}",
        requests.len(),
        start.elapsed()
    );
    let start = Instant::now();
    for (_, mesh, options) in &cases {
        Clump::new(mesh.surface().unwrap(), options).unwrap();
    }
    eprintln!("the same layers on the CPU: {:?}", start.elapsed());

    for ((name, mesh, options), got) in cases.iter().zip(&gpu) {
        let cpu = Clump::new(mesh.surface().unwrap(), options).unwrap();
        let want = cpu.mesh();
        let status = &got.status;
        eprintln!(
            "{name}: {} vertices, {} indices, {} folded, {} thinned (CPU {} vertices, {} indices)",
            status.vertex_count,
            status.index_count,
            status.folded,
            status.thinned,
            want.positions.len(),
            want.indices.len()
        );
        assert!(
            !status.input_overflow && !status.vertex_overflow && !status.index_overflow,
            "{name}: {status:?}"
        );
        assert!(
            !status.slope_unsettled && !status.simplify_rounds_capped,
            "{name}: {status:?}"
        );
        assert_eq!(status.folded as usize, cpu.stats().folded, "{name}: folded");
        assert_eq!(
            status.thinned as usize,
            cpu.stats().thinned,
            "{name}: thinned"
        );
        assert_eq!(
            status.vertex_count as usize,
            want.positions.len(),
            "{name}: vertices"
        );
        assert_eq!(got.indices, want.indices, "{name}: indices");
        assert_eq!(
            got.draw_args[0] as usize,
            want.indices.len(),
            "{name}: draw"
        );

        // Positions within a little of the input's size; the simplified
        // surface's merged points already differ that much.
        let (lo, hi) = mesh.surface().unwrap().bounds();
        let tolerance = 1e-5 * (hi - lo).length();
        let mut worst = 0.0f32;
        let mut far = 0;
        for (v, (&p, w)) in got.positions.iter().zip(&want.positions).enumerate() {
            let d = p.distance(Vec3::from(*w));
            worst = worst.max(d);
            if d > tolerance {
                far += 1;
                if far <= 10 {
                    eprintln!("{name}: vertex {v}: {p} vs {w:?} ({d})");
                }
            }
        }
        assert_eq!(far, 0, "{name}: vertices out of place");
        for (v, (&n, w)) in got.normals.iter().zip(&want.normals).enumerate() {
            assert!(
                n.distance(Vec3::from(*w)) <= 1e-3,
                "{name}: normal {v}: {n} vs {w:?}"
            );
        }
        for (v, (&f, &w)) in got.fade.iter().zip(&want.fade).enumerate() {
            assert!((f - w).abs() <= 1e-5, "{name}: fade {v}: {f} vs {w}");
        }
        eprintln!("{name}: farthest vertex {worst}");
    }
}

/// Returns how many directed edges of `indices` have no reverse, and how
/// many appear more than once. A watertight, edge-manifold mesh has (0, 0).
fn unpaired_edges(indices: &[u32]) -> (usize, usize) {
    let mut count = std::collections::HashMap::<(u32, u32), u32>::new();
    for &[a, b, c] in indices.as_chunks::<3>().0 {
        for e in [(a, b), (b, c), (c, a)] {
            *count.entry(e).or_default() += 1;
        }
    }
    let unmatched = count
        .keys()
        .filter(|&&(a, b)| !count.contains_key(&(b, a)))
        .count();
    let repeated = count.values().filter(|&&n| n > 1).count();
    (unmatched, repeated)
}

/// Runs `requests` like [`run_batch`], growing those that overflow and
/// running the batch again until none do. Panics after 4 tries.
fn run_batch_grown(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    generator: &super::GpuClumpGenerator,
    requests: &mut [super::GpuClumpRequest],
) -> Vec<GpuClumpMesh> {
    for tries in 1..=4 {
        let got = run_batch(device, queue, generator, requests);
        let mut grown = false;
        for (request, mesh) in requests.iter_mut().zip(&got) {
            let s = &mesh.status;
            if s.vertex_overflow || s.index_overflow {
                request.grow_to_fit(s);
                grown = true;
            }
        }
        if !grown {
            eprintln!("fit after {tries} tries");
            return got;
        }
    }
    panic!("requests still overflow after 4 tries");
}

/// Returns the total area of `mesh`'s triangles, in square metres.
fn surface_area(mesh: &SurfaceMesh) -> f32 {
    mesh.indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| Vec3::from(mesh.positions[i as usize]));
            0.5 * (b - a).cross(c - a).length()
        })
        .sum()
}

/// Returns the volume enclosed by a closed mesh, positive when its
/// triangles face outward.
fn enclosed_volume(positions: &[Vec3], indices: &[u32]) -> f32 {
    indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| positions[i as usize]);
            a.dot(b.cross(c)) / 6.0
        })
        .sum()
}

/// Asserts that layer clump `got`, built on the GPU from `mesh` with
/// `options`, fit its room and is the CPU's clump: the same counts,
/// indices and stats, with positions, normals and fade within GPU rounding.
fn assert_layer_matches_cpu(
    name: &str,
    mesh: &SurfaceMesh,
    options: &ClumpOptions,
    got: &GpuClumpMesh,
) {
    let surface = mesh.surface().unwrap();
    let cpu = crate::Clump::new(surface, options).unwrap();
    let want = cpu.mesh();
    let status = &got.status;
    eprintln!(
        "{name}: {} vertices, {} indices, {} folded, {} thinned, {} crossings (CPU {} vertices, {} indices)",
        status.vertex_count,
        status.index_count,
        status.folded,
        status.thinned,
        status.crossings,
        want.positions.len(),
        want.indices.len()
    );
    assert!(
        !status.input_overflow && !status.vertex_overflow && !status.index_overflow,
        "{name}: {status:?}"
    );
    assert!(!status.slope_unsettled, "{name}: {status:?}");

    // Counts, stats and indices exactly.
    assert_eq!(status.folded as usize, cpu.stats().folded, "{name}: folded");
    assert_eq!(
        status.thinned as usize,
        cpu.stats().thinned,
        "{name}: thinned"
    );
    assert_eq!(
        status.crossings as usize,
        cpu.stats().crossings,
        "{name}: crossings"
    );
    assert_eq!(
        status.vertex_count as usize,
        want.positions.len(),
        "{name}: vertices"
    );
    assert_eq!(got.indices, want.indices, "{name}: indices");
    assert_eq!(
        got.draw_args[0] as usize,
        want.indices.len(),
        "{name}: draw"
    );

    // Positions within a little of the input's size; normals and fade
    // within GPU rounding.
    let (lo, hi) = surface.bounds();
    let tolerance = 1e-5 * (hi - lo).length();
    let mut worst = 0.0f32;
    let mut far = 0;
    for (v, (&p, w)) in got.positions.iter().zip(&want.positions).enumerate() {
        let d = p.distance(Vec3::from(*w));
        worst = worst.max(d);
        if d > tolerance {
            far += 1;
            if far <= 10 {
                eprintln!("{name}: vertex {v}: {p} vs {w:?} ({d})");
            }
        }
    }
    assert_eq!(far, 0, "{name}: vertices out of place");
    for (v, (&n, w)) in got.normals.iter().zip(&want.normals).enumerate() {
        assert!(
            n.distance(Vec3::from(*w)) <= 1e-3,
            "{name}: normal {v}: {n} vs {w:?}"
        );
    }
    for (v, (&f, &w)) in got.fade.iter().zip(&want.fade).enumerate() {
        assert!((f - w).abs() <= 1e-5, "{name}: fade {v}: {f} vs {w}");
    }
    eprintln!("{name}: farthest vertex {worst}");
}

/// Returns `got` as a CPU clump mesh, for the crate's diagnostics.
fn clump_mesh(got: &GpuClumpMesh) -> crate::ClumpMesh {
    crate::ClumpMesh {
        positions: got.positions.iter().map(|p| p.to_array()).collect(),
        normals: got.normals.iter().map(|n| n.to_array()).collect(),
        fade: got.fade.clone(),
        indices: got.indices.clone(),
    }
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_strict_layers_match_the_cpu() {
    use crate::ClumpPreset;

    let (device, queue) = device();
    let generator = super::GpuClumpGenerator::new(&device);
    let strict = |mut options: ClumpOptions, closed: bool| {
        options.mesh.closed = closed;
        options.mesh.strict = true;
        options
    };
    let mut deep = ClumpPreset::MossPatches.options();
    deep.shape.thickness = 0.2;
    deep.mask.any_facing = true;
    let mut flat_cap = ClumpOptions::default();
    flat_cap.shape.lumpiness = 0.0;
    flat_cap.mask.exposure_rays = 0;
    let cases: Vec<(&str, SurfaceMesh, ClumpOptions)> = vec![
        (
            "closed default on slab",
            slab(48),
            strict(ClumpOptions::default(), true),
        ),
        (
            "open default on slab",
            slab(48),
            strict(ClumpOptions::default(), false),
        ),
        (
            "closed deep moss in a crevice",
            crevice(),
            strict(deep.clone(), true),
        ),
        (
            "open deep moss in a crevice",
            crevice(),
            strict(deep, false),
        ),
        (
            "closed flat cap on ground",
            SurfaceMesh::ground(1.0, 4),
            strict(flat_cap, true),
        ),
    ];
    let uploads: Vec<GpuMesh> = cases
        .iter()
        .map(|(_, mesh, _)| GpuMesh::upload(&device, mesh, 0, 0))
        .collect();
    let mut requests: Vec<super::GpuClumpRequest> = cases
        .iter()
        .zip(&uploads)
        .map(|((_, mesh, options), up)| {
            let area = surface_area(mesh);
            super::GpuClumpRequest::new(
                up.input(mesh, Affine3A::IDENTITY, None),
                options.clone(),
                area,
            )
        })
        .collect();
    let gpu = run_batch_grown(&device, &queue, &generator, &mut requests);

    // The batch again, warm, for its cost.
    let start = Instant::now();
    run_batch(&device, &queue, &generator, &requests);
    eprintln!(
        "warm batch of {} strict layers: {:?}",
        requests.len(),
        start.elapsed()
    );
    let start = Instant::now();
    for (_, mesh, options) in &cases {
        crate::Clump::new(mesh.surface().unwrap(), options).unwrap();
    }
    eprintln!("the same layers on the CPU: {:?}", start.elapsed());

    for ((name, mesh, options), got) in cases.iter().zip(&gpu) {
        assert_layer_matches_cpu(name, mesh, options, got);

        // Nothing passes through anything else, and closed clumps stay
        // watertight.
        assert!(!got.indices.is_empty(), "{name}: empty");
        let crossings = crate::diagnostics::crossing_count(&clump_mesh(got));
        assert_eq!(crossings, 0, "{name}: triangles still cross");
        if options.mesh.closed {
            assert_eq!(unpaired_edges(&got.indices), (0, 0), "{name}: open edges");
        }
    }

    // The room each reports needing is enough for every pass: reserving
    // exactly that gives the same clumps.
    for (request, got) in requests.iter_mut().zip(&gpu) {
        request.vertex_capacity = got.status.work_vertex_count;
        request.index_capacity = got.status.work_index_count;
    }
    let exact = run_batch(&device, &queue, &generator, &requests);
    for ((name, _, _), (got, again)) in cases.iter().zip(gpu.iter().zip(&exact)) {
        let s = &again.status;
        assert!(
            !s.vertex_overflow && !s.index_overflow,
            "{name}: exact room: {s:?}"
        );
        assert_eq!(again.indices, got.indices, "{name}: exact room");
    }
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_closed_layers_match_the_cpu() {
    use super::batch::{REGION_CLOSE_COPIES, REGION_CLOSE_KEPT};
    use crate::ClumpPreset;

    let (device, queue) = device();
    let generator = super::GpuClumpGenerator::new(&device);
    let mut bumpy = SurfaceMesh::ground(2.0, 24);
    for (p, n) in bumpy.positions.iter_mut().zip(&mut bumpy.normals) {
        let (x, z) = (p[0], p[2]);
        p[1] = (x * 3.1).sin() * 0.15 + (z * 2.3).cos() * 0.1;
        let slope = Vec3::new(-(x * 3.1).cos() * 0.465, 1.0, (z * 2.3).sin() * 0.23);
        *n = slope.normalize().to_array();
    }
    let closed = |preset: ClumpPreset| {
        let mut options = preset.options();
        options.mesh.closed = true;
        options
    };
    // An even cap: no lumps, no exposure test, full coverage on flat ground.
    let mut flat_cap = ClumpOptions::default();
    flat_cap.shape.lumpiness = 0.0;
    flat_cap.mask.exposure_rays = 0;
    let mut no_rays = ClumpOptions::default();
    no_rays.mask.exposure_rays = 0;
    let cases: Vec<(&str, SurfaceMesh, ClumpOptions)> = vec![
        (
            "flat cap on ground",
            SurfaceMesh::ground(1.0, 4),
            flat_cap.clone(),
        ),
        (
            "snow cap on bumpy ground",
            bumpy.clone(),
            closed(ClumpPreset::DirtySnowCap),
        ),
        (
            "moss patches on bumpy ground",
            bumpy,
            closed(ClumpPreset::MossPatches),
        ),
        (
            "snow cap on slab",
            slab(48),
            closed(ClumpPreset::DirtySnowCap),
        ),
        // See `gpu_open_sheets_match_the_cpu` for why `facing_min` is lifted.
        ("moss patches on a box", seamed(&sharp_box(0.3)), {
            let mut clear_of_sides = closed(ClumpPreset::MossPatches);
            clear_of_sides.mask.facing_min = 0.01;
            clear_of_sides
        }),
        ("deep moss in a crevice", crevice(), {
            let mut deep = closed(ClumpPreset::MossPatches);
            deep.shape.thickness = 0.2;
            deep.mask.any_facing = true;
            deep
        }),
        ("default on a thin sharp slab", sharp_box(0.04), no_rays),
    ];
    let uploads: Vec<GpuMesh> = cases
        .iter()
        .map(|(_, mesh, _)| GpuMesh::upload(&device, mesh, 0, 0))
        .collect();
    // Room estimated from the input's area, grown until everything fits.
    let mut requests: Vec<super::GpuClumpRequest> = cases
        .iter()
        .zip(&uploads)
        .map(|((_, mesh, options), up)| {
            let area = surface_area(mesh);
            super::GpuClumpRequest::new(
                up.input(mesh, Affine3A::IDENTITY, None),
                options.clone(),
                area,
            )
        })
        .collect();
    let gpu = run_batch_grown(&device, &queue, &generator, &mut requests);

    for ((name, mesh, options), got) in cases.iter().zip(&gpu) {
        assert_layer_matches_cpu(name, mesh, options, got);

        // A watertight, edge-manifold solid that faces outward.
        assert!(!got.indices.is_empty(), "{name}: empty");
        assert_eq!(unpaired_edges(&got.indices), (0, 0), "{name}: open edges");
        assert!(
            enclosed_volume(&got.positions, &got.indices) > 0.0,
            "{name}: inside out"
        );
    }

    // The flat cap holds its thickness above the ground plus the underside
    // depth below it.
    let s = &flat_cap.shape;
    let expected = 1.0 * (s.thickness + s.sink + 0.25 * s.thickness);
    let volume = enclosed_volume(&gpu[0].positions, &gpu[0].indices);
    assert!(
        (volume - expected).abs() < expected * 0.01,
        "{volume} vs {expected}"
    );

    // On the thin slab, every rim and underside vertex stays inside the
    // input or on its surface.
    let thin = cases.last().unwrap().1.surface().unwrap();
    let rim: Vec<[f32; 3]> = gpu
        .last()
        .unwrap()
        .positions
        .iter()
        .zip(&gpu.last().unwrap().fade)
        .filter(|&(_, &f)| f == 0.0)
        .map(|(p, _)| p.to_array())
        .collect();
    let inside = crate::diagnostics::inside_closed(&thin, &rim);
    let distance = crate::diagnostics::signed_distances(&thin, &rim);
    for ((p, inside), d) in rim.iter().zip(inside).zip(distance) {
        assert!(inside || d.abs() < 1e-4, "{p:?} is outside the slab");
    }

    // These cases split pinch points and leave out folded triangles, so
    // both paths are compared above. Each case's working layout, run on its
    // own, gives its pinch copies and kept top triangles.
    let (copies, kept): (Vec<u32>, Vec<u32>) = cases
        .iter()
        .map(|(_, mesh, options)| {
            let case = [(mesh.clone(), options.clone())];
            let (plan, _, arena, _) =
                run_stages_sized(&device, &queue, &case, 2.0 * surface_area(mesh));
            let s = plan.slices[0];
            let total = |region: usize, first: u32, count: u32| {
                let at = plan.regions[region] + first;
                arena[(at + count) as usize] - arena[at as usize]
            };
            (
                total(
                    REGION_CLOSE_COPIES,
                    s.out_vertex_base,
                    s.out_vertex_capacity,
                ),
                total(
                    REGION_CLOSE_KEPT,
                    s.out_index_base / 3,
                    s.out_index_capacity / 3,
                ),
            )
        })
        .unzip();
    eprintln!("pinch copies per case: {copies:?}; kept top triangles: {kept:?}");
    assert!(copies.iter().any(|&c| c > 0), "no case has a pinch point");
    assert!(
        gpu.iter().any(|g| g.status.folded > 0),
        "no case leaves out a folded triangle"
    );
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_open_sheets_match_the_cpu() {
    use crate::ClumpPreset;
    use crate::patch::Patch;
    use crate::shell::open_sheet;

    let (device, queue) = device();
    let open = |preset: ClumpPreset| {
        let mut options = preset.options();
        options.mesh.closed = false;
        options
    };
    // The box's sides face exactly sideways, so with a `facing_min` of 0,
    // whether they may be covered rests on the last bit of their normals,
    // which the GPU rounds differently; lifting it keeps them clear.
    let mut clear_of_sides = open(ClumpPreset::MossPatches);
    clear_of_sides.mask.facing_min = 0.01;
    let cases = [(seamed(&sharp_box(0.3)), clear_of_sides)];
    let (plan, counters, arena, flags) = run_stages_sized(&device, &queue, &cases, 8.0);
    let regions = &plan.regions;

    for (r, (mesh, options)) in cases.iter().enumerate() {
        let c = |which: u32| counters[r * COUNTER_STRIDE as usize + which as usize];
        let slices = plan.slices[r];
        let field = crate::field::ClumpField::new(mesh.surface().unwrap(), options);
        let edge_length = options.mesh.edge_length * options.scale;
        let patch = Patch::new(&field, edge_length);
        let first_new = patch.points.len();
        let mut number = vec![u32::MAX; first_new];
        let mut next = 0u32;
        for t in &patch.tris {
            for v in t.corners {
                number[v as usize] = 0;
            }
        }
        for n in &mut number {
            if *n == 0 {
                *n = next;
                next += 1;
            }
        }
        let sheet = open_sheet(&field, patch, edge_length, options.shape.max_slope);
        number.extend((0..sheet.points.len() - first_new).map(|i| next + i as u32));
        eprintln!(
            "case {r}: blur passes CPU {} GPU {}, thinned CPU {} GPU {}",
            sheet.blur_passes,
            c(38),
            sheet.thinned,
            c(40)
        );

        // The GPU's heights are read after its fold passes, the CPU's
        // before, so this only fits inputs where nothing turns over.
        let mut differ = [0usize; 4];
        for (v, &n) in number.iter().enumerate() {
            if n == u32::MAX {
                continue;
            }
            let s = slices.out_vertex_base + n;
            let point = vec3_at(&arena, regions[REGION_SHEET_POINTS] + POINT_WORDS * s);
            assert_eq!(point, sheet.points[v].point, "case {r}: point {v}");
            let dir = vec3_at(&arena, regions[REGION_SHELL_DIRS] + 3 * s);
            let along = vec3_at(&arena, regions[REGION_SHELL_ALONG] + 3 * s);
            let weight = f32::from_bits(arena[(regions[REGION_SHELL_WEIGHTS] + s) as usize]);
            let bits = flags[(regions[FLAG_REGION_SHELL_TOPS] + s) as usize];
            let top = f32::from_bits(if bits & 0x8000_0000 != 0 {
                bits & 0x7fff_ffff
            } else {
                !bits
            });
            for (k, same) in [
                dir.distance(sheet.dirs[v]) <= 1e-5,
                along.distance(sheet.along[v]) <= 1e-5,
                (weight - sheet.weights[v]).abs() <= 1e-6,
                (top - sheet.tops[v]).abs() <= 1e-6,
            ]
            .into_iter()
            .enumerate()
            {
                if !same {
                    differ[k] += 1;
                    if differ[k] <= 3 {
                        eprintln!(
                            "case {r}: point {v} stage {k}: dir {dir} vs {}, along {along} vs {}, weight {weight} vs {}, top {top} vs {}",
                            sheet.dirs[v], sheet.along[v], sheet.weights[v], sheet.tops[v]
                        );
                    }
                }
            }
        }
        eprintln!("case {r}: dirs, along, weights, tops differ at {differ:?}");
        assert_eq!(differ, [0; 4], "case {r}");
    }
}

/// Returns strict layer `r`'s assembled working mesh, read back from
/// `arena` after the output scans, with its triangles' slots.
fn working_mesh(
    plan: &super::batch::BatchPlan,
    arena: &[u32],
    r: usize,
) -> (crate::ClumpMesh, Vec<u32>) {
    use super::batch::{REGION_WORK_CORNERS, REGION_WORK_KEPT, REGION_WORK_POSITIONS};

    let s = plan.slices[r];
    let regions = &plan.regions;
    let positions: Vec<[f32; 3]> = (0..s.out_vertex_capacity)
        .map(|v| {
            vec3_at(
                arena,
                regions[REGION_WORK_POSITIONS] + 3 * (s.out_vertex_base + v),
            )
            .to_array()
        })
        .collect();
    let first = s.out_index_base / 3;
    let kept = |t: u32| {
        let at = (regions[REGION_WORK_KEPT] + t) as usize;
        arena[at + 1] != arena[at]
    };
    let slots: Vec<u32> = (first..first + s.out_index_capacity / 3)
        .filter(|&t| kept(t))
        .collect();
    let indices = slots
        .iter()
        .flat_map(|&t| {
            (0..3).map(move |k| {
                arena[(regions[REGION_WORK_CORNERS] + 3 * t + k) as usize] - s.out_vertex_base
            })
        })
        .collect();
    let mesh = crate::ClumpMesh {
        fade: vec![1.0; positions.len()],
        positions,
        indices,
        ..crate::ClumpMesh::default()
    };
    (mesh, slots)
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_strict_crossings_match_the_cpu() {
    use super::batch::REGION_STRICT_CROSSING;
    use crate::ClumpPreset;
    use crate::shell::crossing_triangles;

    // The first check only, before anything is thinned: the GPU's crossing
    // triangles are the CPU's on the very same mesh.
    let (device, queue) = device();
    let strict = |closed: bool| {
        let mut options = ClumpOptions::default();
        options.mesh.closed = closed;
        options.mesh.strict = true;
        options
    };
    let mut deep = ClumpPreset::MossPatches.options();
    deep.shape.thickness = 0.2;
    deep.mask.any_facing = true;
    deep.mesh.closed = true;
    deep.mesh.strict = true;
    let mut deep_open = deep.clone();
    deep_open.mesh.closed = false;
    let cases = [
        (slab(48), strict(true)),
        (slab(48), strict(false)),
        (crevice(), deep),
        (crevice(), deep_open),
    ];
    let mut crossing = Vec::new();
    for (r, case) in cases.iter().enumerate() {
        let one = std::slice::from_ref(case);
        let area = 2.0 * surface_area(&case.0);
        let (plan, _, arena, _, _) = run_stages_limited(&device, &queue, one, area, Some(0));
        let (mesh, slots) = working_mesh(&plan, &arena, 0);
        let want: Vec<u32> = crossing_triangles(&mesh)
            .into_iter()
            .map(|i| slots[i])
            .collect();
        let got: Vec<u32> = slots
            .iter()
            .copied()
            .filter(|&t| arena[(plan.regions[REGION_STRICT_CROSSING] + t) as usize] != 0)
            .collect();
        eprintln!(
            "case {r}: {} triangles, {} crossing on the GPU, {} on the CPU",
            slots.len(),
            got.len(),
            want.len()
        );
        assert_eq!(got, want, "case {r}: crossing triangles");
        crossing.push(!want.is_empty());
    }
    eprintln!("cases with crossings: {crossing:?}");
    assert!(crossing[0], "the closed slab crosses itself");
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_strict_layers_out_of_passes_report_their_crossings() {
    use crate::ClumpPreset;

    // The deep crevice needs 10 passes; with 2 it still crosses itself, and
    // the count is that of the mesh it outputs.
    let (device, queue) = device();
    let mut deep = ClumpPreset::MossPatches.options();
    deep.shape.thickness = 0.2;
    deep.mask.any_facing = true;
    deep.mesh.closed = true;
    deep.mesh.strict = true;
    let case = [(crevice(), deep)];
    let area = 2.0 * surface_area(&case[0].0);
    let (plan, _, arena, _, status) = run_stages_limited(&device, &queue, &case, area, Some(2));
    let reported = status[0].crossings;
    let (mesh, _) = working_mesh(&plan, &arena, 0);
    let actual = crate::diagnostics::crossing_count(&mesh);
    eprintln!("after 2 passes: {reported} reported, {actual} in the mesh");
    assert!(reported > 0, "nothing reported");
    assert_eq!(reported as usize, actual, "reported crossings");
    assert!(
        !status[0].vertex_overflow && !status[0].index_overflow,
        "{:?}",
        status[0]
    );
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_closed_undersides_match_the_cpu() {
    use super::batch::{REGION_CLOSE_DEPTHS, REGION_CLOSE_SINK_DIRS};
    use crate::ClumpPreset;
    use crate::patch::Patch;
    use crate::shell::{BOTTOM_GAP, open_sheet, sink};

    let (device, queue) = device();
    let mut options = ClumpPreset::MossPatches.options();
    options.mesh.closed = true;
    options.mask.facing_min = 0.01;
    let cases = [(seamed(&sharp_box(0.3)), options)];
    let (plan, _, arena, _) = run_stages_sized(&device, &queue, &cases, 8.0);
    let regions = &plan.regions;

    for (r, (mesh, options)) in cases.iter().enumerate() {
        let slices = plan.slices[r];
        let field = crate::field::ClumpField::new(mesh.surface().unwrap(), options);
        let edge_length = options.mesh.edge_length * options.scale;
        let patch = Patch::new(&field, edge_length);

        // GPU point numbers: the patch's points in order of use, then the
        // points refine adds, in order.
        let first_new = patch.points.len();
        let mut number = vec![u32::MAX; first_new];
        let mut next = 0u32;
        for t in &patch.tris {
            for v in t.corners {
                number[v as usize] = 0;
            }
        }
        for n in &mut number {
            if *n == 0 {
                *n = next;
                next += 1;
            }
        }
        let sheet = open_sheet(&field, patch, edge_length, options.shape.max_slope);
        number.extend((0..sheet.points.len() - first_new).map(|i| next + i as u32));

        // Every point of a kept triangle sinks its underside as the CPU's.
        let full_depth = field.sink() + BOTTOM_GAP * field.thickness();
        let mut seen = vec![false; sheet.points.len()];
        let mut differ = 0;
        for v in sheet.kept.iter().flat_map(|t| t.corners) {
            let v = v as usize;
            if std::mem::replace(&mut seen[v], true) {
                continue;
            }
            let p = &sheet.points[v];
            let (dir, depth) = sink(&field, p, sheet.dirs[v], full_depth + p.dip);
            let s = slices.out_vertex_base + number[v];
            let gpu_depth = f32::from_bits(arena[(regions[REGION_CLOSE_DEPTHS] + s) as usize]);
            let gpu_dir = vec3_at(&arena, regions[REGION_CLOSE_SINK_DIRS] + 3 * s);
            if (gpu_depth - depth).abs() > 1e-6 || gpu_dir.distance(dir) > 1e-5 {
                differ += 1;
                if differ <= 5 {
                    let spans = [sheet.dirs[v], p.normal].map(|d| field.inside_span(p.point, -d));
                    eprintln!(
                        "case {r}: point {v} at {}: depth {gpu_depth} vs {depth}, dir {gpu_dir} vs {dir}; push {} normal {}; CPU spans {spans:?}",
                        p.point, sheet.dirs[v], p.normal
                    );
                }
            }
        }
        eprintln!("case {r}: {differ} undersides differ");
        assert_eq!(differ, 0, "case {r}");
    }
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_refined_sheets_match_the_cpu() {
    use crate::ClumpPreset;
    use crate::patch::Patch;
    use crate::shell::refine;

    let (device, queue) = device();
    let mut bumpy = SurfaceMesh::ground(2.0, 24);
    for p in &mut bumpy.positions {
        p[1] = (p[0] * 3.1).sin() * 0.15 + (p[2] * 2.3).cos() * 0.1;
    }
    let with = |mut options: ClumpOptions, edge_length: f32| {
        options.mesh.edge_length = edge_length;
        options
    };
    // Large input triangles split over several passes; snow caps leave
    // steep triangles whole, so splits stop at their edges.
    let cases = [
        (
            SurfaceMesh::ground(1.0, 8),
            with(ClumpOptions::default(), 0.04),
        ),
        (slab(24), with(ClumpPreset::DirtySnowCap.options(), 0.03)),
        (
            bumpy.clone(),
            with(ClumpPreset::MossPatches.options(), 0.05),
        ),
        (bumpy, with(ClumpPreset::DirtySnowCap.options(), 0.1)),
    ];
    let (plan, counters, arena, _) = run_stages_sized(&device, &queue, &cases, 12.0);
    let regions = &plan.regions;
    let (overflow_counter, points_counter, tris_counter, failed_counter) = (4, 35, 36, 37);

    for (r, (mesh, options)) in cases.iter().enumerate() {
        let c = |which: u32| counters[r * COUNTER_STRIDE as usize + which as usize];
        let slices = plan.slices[r];
        let field = crate::field::ClumpField::new(mesh.surface().unwrap(), options);
        let edge_length = options.mesh.edge_length * options.scale;

        // The CPU's working surface, with the GPU's rise and dip, which may
        // differ where closest triangles tie.
        let mut patch = Patch::new(&field, edge_length);
        for (i, p) in patch.points.iter_mut().enumerate() {
            let at = (regions[REGION_COARSE_POINTS] + POINT_WORDS * (slices.vertex_base + i as u32))
                as usize;
            p.rise = f32::from_bits(arena[at + 15]);
            p.dip = f32::from_bits(arena[at + 16]);
        }
        let first_new = patch.points.len();
        let mut used = vec![false; first_new];
        for t in &patch.tris {
            for v in t.corners {
                used[v as usize] = true;
            }
        }
        let (mut points, mut tris) = (patch.points, patch.tris);
        refine(&field, &mut points, &mut tris, edge_length);
        eprintln!(
            "case {r}: CPU {} points, {} triangles; GPU {} points, {} triangles",
            points.len(),
            tris.len(),
            c(points_counter),
            c(tris_counter)
        );
        assert_eq!(c(overflow_counter), 0, "case {r}: overflow");
        assert_eq!(c(failed_counter), 0, "case {r}: failed");

        // The sheet keeps only used points, in order; midpoints follow.
        let kept = used.iter().filter(|&&u| u).count();
        let mut number = vec![u32::MAX; points.len()];
        let mut next = 0u32;
        for (i, &u) in used.iter().enumerate() {
            if u {
                number[i] = next;
                next += 1;
            }
        }
        for (i, n) in number.iter_mut().enumerate().skip(first_new) {
            *n = (kept + i - first_new) as u32;
        }
        assert_eq!(
            c(points_counter) as usize,
            kept + points.len() - first_new,
            "case {r}: points"
        );
        assert_eq!(c(tris_counter) as usize, tris.len(), "case {r}: triangles");

        // The same triangles, and the same points. Merged points of the
        // simplified surface match within 1e-5 of the input's size (see
        // `gpu_simplified_surfaces_match_the_cpu`), and so do positions,
        // normals and room built from them; the blended values match exactly.
        let (lo, hi) = mesh.surface().unwrap().bounds();
        let tolerance = 1e-5 * (hi - lo).length();
        let tri_base = slices.out_index_base / 3;
        for (i, t) in tris.iter().enumerate() {
            let at =
                (regions[REGION_SHEET_TRIS_A] + PATCH_TRI_WORDS * (tri_base + i as u32)) as usize;
            let corners = [0, 1, 2].map(|k| arena[at + k]);
            assert_eq!(
                corners,
                t.corners.map(|v| number[v as usize]),
                "case {r}: triangle {i}"
            );
            // Simplify recomputes this from merged points, which match only
            // within the tolerance below, so compare directions.
            let normal = vec3_at(&arena, at as u32 + 3);
            assert!(
                normal.normalize().distance(t.source_normal.normalize()) <= 1e-3,
                "case {r}: normal {i}: {normal} vs {}",
                t.source_normal
            );
        }
        for (i, p) in points.iter().enumerate() {
            if number[i] == u32::MAX {
                continue;
            }
            let at = (regions[REGION_SHEET_POINTS]
                + POINT_WORDS * (slices.out_vertex_base + number[i])) as usize;
            let word = |k: usize| f32::from_bits(arena[at + k]);
            let room = if word(14) == f32::MAX {
                f32::INFINITY
            } else {
                word(14)
            };
            let point = vec3_at(&arena, at as u32);
            let normal = vec3_at(&arena, at as u32 + 3);
            assert!(
                point.distance(p.point) <= tolerance,
                "case {r}: point {i}: {point} vs {}",
                p.point
            );
            assert!(
                normal.distance(p.normal) <= 1e-3,
                "case {r}: normal of {i}: {normal} vs {}",
                p.normal
            );
            assert!(
                room == p.room || (room - p.room).abs() <= tolerance,
                "case {r}: room of {i}: {room} vs {}",
                p.room
            );
            assert_eq!(
                std::array::from_fn::<f32, 8, _>(|d| word(6 + d)),
                p.exposure,
                "case {r}: exposure of {i}"
            );
            assert_eq!(
                [word(15), word(16)],
                [p.rise, p.dip],
                "case {r}: rise and dip of {i}"
            );
        }
    }
}

/// Returns, per simplified point of `field`'s working surface at its
/// options' edge length, the least and most rise and dip that matching each
/// fine point to any of its closest simplified triangles can give: a
/// triangle within `tie` of the closest distance counts as closest. Brute
/// force, for checking the GPU's envelope.
fn envelope_bounds(
    field: &crate::field::ClumpField,
    options: &ClumpOptions,
    tie: f32,
) -> Vec<[(f32, f32); 2]> {
    use crate::bvh::closest_on_triangle;
    use crate::patch::{Patch, facing_triangles, welded_normals};

    let (fine, _) = facing_triangles(field, &welded_normals(field));
    let edge_length = options.mesh.edge_length * options.scale;
    let (points, tris, collapsed) = Patch::simplified(field, edge_length);
    let mut bounds = vec![[(0.0f32, 0.0f32); 2]; points.len()];
    if !collapsed {
        return bounds;
    }
    let corners: Vec<[Vec3; 3]> = tris
        .iter()
        .map(|t| t.corners.map(|v| points[v as usize].point))
        .collect();

    for f in &fine {
        // Every triangle about as close as the closest.
        let distances: Vec<f32> = corners
            .iter()
            .map(|&c| f.point.distance(closest_on_triangle(f.point, c).0))
            .collect();
        let nearest = distances.iter().copied().fold(f32::INFINITY, f32::min);
        let candidates: Vec<usize> = (0..tris.len())
            .filter(|&i| distances[i] <= nearest + tie)
            .collect();

        // A sole candidate raises the least and most; tied ones only the most.
        for &i in &candidates {
            let [a, b, c] = corners[i];
            let face = (b - a).cross(c - a).normalize_or_zero();
            if face.dot(f.normal) <= 0.0 {
                continue;
            }
            let h = (f.point - closest_on_triangle(f.point, corners[i]).0).dot(face);
            let side = if h > 0.0 { 0 } else { 1 };
            for v in tris[i].corners {
                let (least, most) = &mut bounds[v as usize][side];
                *most = most.max(h.abs());
                if candidates.len() == 1 {
                    *least = least.max(h.abs());
                }
            }
        }
    }
    bounds
}

/// Asserts that a GPU-simplified surface of a flat ground grid is whole and
/// valid, as `simplifying_keeps_the_surface_whole` checks on the CPU: no
/// edge has more than two triangles, no edge is longer than `edge_length`,
/// and every triangle still faces up.
fn assert_valid_simplification(
    arena: &[u32],
    regions: &[u32],
    slices: &super::batch::RequestSlices,
    triangles: u32,
    edge_length: f32,
    case: usize,
) {
    let tri = |i: u32| regions[REGION_COARSE_TRIS] + PATCH_TRI_WORDS * (slices.triangle_base + i);
    let point = |p: u32| {
        vec3_at(
            arena,
            regions[REGION_COARSE_POINTS] + POINT_WORDS * (slices.vertex_base + p),
        )
    };
    let mut uses = std::collections::HashMap::<(u32, u32), u32>::new();
    for i in 0..triangles {
        let at = tri(i);
        let corners = [0, 1, 2].map(|k| arena[(at + k) as usize]);
        for k in 0..3 {
            let (a, b) = (corners[k], corners[(k + 1) % 3]);
            *uses.entry((a.min(b), a.max(b))).or_default() += 1;
            assert!(
                point(a).distance(point(b)) <= edge_length * 1.0001,
                "case {case}: long edge"
            );
        }
        assert!(
            vec3_at(arena, at + 3).y > 0.0,
            "case {case}: triangle {i} turned over"
        );
    }
    assert!(
        uses.values().all(|&n| n <= 2),
        "case {case}: an edge has three triangles"
    );
}

/// A test kernel running the BVH queries: per query, request, origin,
/// direction, ray length and a point (12 words in); ray hit distance and
/// slot, blocked flag, closest distance² and slot (5 words out).
const BVH_QUERY_TEST: &str = r#"
import package::bvh::{ray_hit, ray_blocked, closest_point};
import package::batch::{item_index, WORKGROUP_SIZE};

struct Query {
    request: u32,
    origin: array<f32, 3>,
    dir: array<f32, 3>,
    t_max: f32,
    point: array<f32, 3>,
    _pad: u32,
}

@group(1) @binding(0) var<storage, read> queries: array<Query>;
@group(1) @binding(1) var<storage, read_write> results: array<u32>;

fn v3(a: array<f32, 3>) -> vec3<f32> { return vec3<f32>(a[0], a[1], a[2]); }

@compute @workgroup_size(WORKGROUP_SIZE)
fn bvh_query_test(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let i = item_index(global_id, num_workgroups);
    if i >= arrayLength(&queries) {
        return;
    }
    let q = queries[i];
    let hit = ray_hit(q.request, v3(q.origin), v3(q.dir), q.t_max);
    results[5u * i] = bitcast<u32>(hit.t);
    results[5u * i + 1u] = hit.slot;
    results[5u * i + 2u] = select(0u, 1u, ray_blocked(q.request, v3(q.origin), v3(q.dir), q.t_max));
    let near = closest_point(q.request, v3(q.point));
    results[5u * i + 3u] = bitcast<u32>(near.distance_squared);
    results[5u * i + 4u] = near.slot;
}
"#;

/// Compiles a test module that can import every shader module, to WGSL.
fn compile_test_module(source: &str) -> String {
    let mut resolver = wesl::resolver::VirtualResolver::new();
    for (path, module) in crate::shaders::SHADER_MODULES {
        resolver.add_module(path.parse().unwrap(), module.to_string().into());
    }
    resolver.add_module("package::main".parse().unwrap(), source.to_string().into());
    wesl::Compiler::new_with_resolver(wesl::CompileOptions::default(), resolver)
        .compile_module(&"package::main".parse().unwrap())
        .unwrap_or_else(|e| panic!("test module: {e}"))
        .to_string()
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_bvh_queries_match_the_cpu_bvh() {
    let (device, queue) = device();
    let pipelines = Pipelines::new(&device);
    let max_groups = device.limits().max_compute_workgroups_per_dimension;

    // Inputs of different sizes, including a lone triangle.
    let mut bumpy = SurfaceMesh::ground(2.0, 24);
    for p in &mut bumpy.positions {
        p[1] = (p[0] * 3.1).sin() * 0.15 + (p[2] * 2.3).cos() * 0.1;
    }
    let lone = SurfaceMesh {
        positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
        normals: vec![[0.0, 1.0, 0.0]; 3],
        indices: vec![0, 2, 1],
    };
    let meshes = [
        ("slab", slab(48)),
        ("bumpy", bumpy),
        ("box", sharp_box(0.3)),
        ("lone triangle", lone),
        ("crevice slab", slab(96)),
    ];
    let uploads: Vec<GpuMesh> = meshes
        .iter()
        .map(|(_, m)| GpuMesh::upload(&device, m, 0, 0))
        .collect();
    let options = ClumpOptions::default();
    let requests: Vec<StageRequest> = meshes
        .iter()
        .zip(&uploads)
        .map(|((_, m), up)| StageRequest {
            input: up.input(m, Affine3A::IDENTITY, None),
            options: &options,
            out_vertices: 0,
            out_indices: 0,
        })
        .collect();
    let batch = BatchResources::new(&device, &pipelines, &requests);

    // Random queries per input: rays from around it and from points on it
    // (stepped off along the normal, like exposure rays), some cut short.
    let mut rng = Rng::new(31);
    let mut queries: Vec<(usize, Vec3, Vec3, f32, Vec3)> = Vec::new();
    for (r, (_, mesh)) in meshes.iter().enumerate() {
        let (lo, hi) = mesh.surface().unwrap().bounds();
        let size = (hi - lo).max(Vec3::splat(0.1));
        for k in 0..600 {
            let around = lo - size * 0.25
                + Vec3::new(rng.next_f32(), rng.next_f32(), rng.next_f32()) * size * 1.5;
            let dir = Vec3::new(
                rng.range(-1.0, 1.0),
                rng.range(-1.0, 1.0),
                rng.range(-1.0, 1.0),
            )
            .normalize_or(Vec3::Y);
            let origin = if k % 2 == 0 {
                around
            } else {
                // A point on a random triangle, stepped off along its normal.
                let t = (rng.next_f32() * (mesh.indices.len() / 3) as f32) as usize;
                let [a, b, c] =
                    [0, 1, 2].map(|j| Vec3::from(mesh.positions[mesh.indices[3 * t + j] as usize]));
                let (u, v) = (rng.next_f32(), rng.next_f32());
                let (u, v) = if u + v > 1.0 {
                    (1.0 - u, 1.0 - v)
                } else {
                    (u, v)
                };
                let n = (b - a).cross(c - a).normalize_or(Vec3::Y);
                a + (b - a) * u + (c - a) * v + n * 1e-5
            };
            let t_max = if k % 3 == 0 {
                rng.range(0.01, 1.0)
            } else {
                f32::INFINITY
            };
            queries.push((r, origin, dir, t_max, around));
        }
    }
    let query_words: Vec<u32> = queries
        .iter()
        .flat_map(|&(r, o, d, t, p)| {
            let mut w = vec![r as u32];
            w.extend(o.to_array().map(f32::to_bits));
            w.extend(d.to_array().map(f32::to_bits));
            w.push(t.to_bits());
            w.extend(p.to_array().map(f32::to_bits));
            w.push(0);
            w
        })
        .collect();

    // The test kernel, after the prepare stage.
    let wgsl = compile_test_module(BVH_QUERY_TEST);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("bvh query test"),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });
    let io_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[0, 1].map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage {
                    read_only: binding == 0,
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&pipelines.batch_layout), Some(&io_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("bvh query test"),
        layout: Some(&layout),
        module: &module,
        entry_point: Some("bvh_query_test"),
        compilation_options: Default::default(),
        cache: None,
    });
    let query_buffer = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("queries"),
        contents: &query_words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>(),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let results = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("results"),
        size: 4 * 5 * queries.len() as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let io_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &io_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: query_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: results.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    batch.record_prepare(&mut encoder, &pipelines, max_groups);
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &batch.batch_group, &[]);
        pass.set_bind_group(1, &io_group, &[]);
        pass.dispatch_workgroups((queries.len() as u32).div_ceil(64), 1, 1);
    }
    let out = &read_words(&device, &queue, encoder, &[&results])[0];

    // Compare with the CPU BVH over each input.
    let bvhs: Vec<crate::bvh::Bvh> = meshes
        .iter()
        .map(|(_, m)| crate::bvh::Bvh::new(&m.surface().unwrap()))
        .collect();
    let (mut hit_mismatches, mut blocked_mismatches) = (0, 0);
    for (i, &(r, origin, dir, t_max, point)) in queries.iter().enumerate() {
        let (name, mesh) = &meshes[r];
        let base = batch.plan.slices[r].triangle_base;
        let o = &out[5 * i..5 * i + 5];
        let bvh = &bvhs[r];

        // Distances agree to within GPU rounding: relative, plus a floor of
        // a millionth of the input's size for distances near 0.
        let (lo, hi) = mesh.surface().unwrap().bounds();
        let floor = 1e-6 * (hi - lo).length();
        let same = |a: f32, b: f32| (a - b).abs() <= 1e-5 * a.abs() + floor;

        // Ray hits: the same triangle, or one hit at the same distance.
        let cpu = bvh.ray_hit(origin, dir, t_max);
        let gpu = (o[1] != 0x8000_0000).then(|| (f32::from_bits(o[0]), (o[1] - base) as usize));
        let agree = match (cpu, gpu) {
            (None, None) => true,
            (Some((ct, ctri)), Some((gt, gtri))) => {
                let close = same(ct, gt);
                close
                    && (ctri == gtri || {
                        // A tie: the GPU's triangle is hit at the same distance.
                        let corners = bvh.corners(gtri);
                        let single = crate::bvh::Bvh::new(
                            &SurfaceMesh {
                                positions: corners.map(|c| c.to_array()).to_vec(),
                                normals: vec![[0.0, 1.0, 0.0]; 3],
                                indices: vec![0, 1, 2],
                            }
                            .surface()
                            .unwrap(),
                        );
                        single
                            .ray_hit(origin, dir, t_max)
                            .is_some_and(|(t, _)| same(ct, t))
                    })
            }
            _ => false,
        };
        if !agree {
            hit_mismatches += 1;
            eprintln!("{name} query {i}: ray hit CPU {cpu:?} vs GPU {gpu:?}");
        }

        // Blocked rays agree.
        if (o[2] != 0) != bvh.ray_blocked(origin, dir, t_max) {
            blocked_mismatches += 1;
            eprintln!("{name} query {i}: blocked GPU {}", o[2]);
        }

        // Closest points: the same distance.
        let near = bvh.closest_point(point);
        let (gpu_d, cpu_d) = (f32::from_bits(o[3]).sqrt(), near.distance_squared.sqrt());
        assert!(
            same(cpu_d, gpu_d),
            "{name} query {i}: closest distance GPU {gpu_d} vs CPU {cpu_d}"
        );
    }

    // Only rays grazing an edge exactly (which the CPU settles in f64) may
    // differ: at most 0.1% of them.
    let allowed = queries.len() / 1000;
    assert!(
        hit_mismatches <= allowed,
        "{hit_mismatches} ray hits differ"
    );
    assert!(
        blocked_mismatches <= allowed,
        "{blocked_mismatches} blocked rays differ"
    );
}

/// Shader for the dispatch-cost measurement: `setup` writes the next
/// dispatch's arguments (0 workgroups), `work` would add to a counter.
const DISPATCH_COST_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read_write> args: array<u32>;
@group(0) @binding(1) var<storage, read_write> counter: array<atomic<u32>>;

@compute @workgroup_size(1)
fn setup() {
    args[0] = 0u;
    args[1] = 1u;
    args[2] = 1u;
}

@compute @workgroup_size(64)
fn work() {
    atomicAdd(&counter[0], 1u);
}
"#;

/// How long encoding and running one batch of dispatches took.
#[derive(Debug, Clone, Copy)]
struct Timing {
    encode: Duration,
    total: Duration,
}

/// Measures what a batch's loop structure costs when nearly every loop step
/// has finished early: thousands of indirect dispatches with 0 workgroups.
///
/// Each dispatch costs a few microseconds, so the generator runs a whole
/// batch in lockstep and pays for its loops once per batch; see "lockstep
/// batches" in the GPU plan. This checks that a batch's ~4,000 loop
/// dispatches stay within that budget.
#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_loop_dispatches_fit_the_batch_budget() {
    let mut worst = Duration::ZERO;
    for (name, flags) in [
        ("default flags", wgpu::InstanceFlags::default()),
        (
            "no indirect validation",
            wgpu::InstanceFlags::default() - wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL,
        ),
    ] {
        for (pattern, t) in dispatch_costs(flags) {
            eprintln!(
                "{name}, {pattern}: encode {:?}, total {:?}",
                t.encode, t.total
            );
            if pattern.starts_with('A') || pattern.starts_with('B') {
                worst = worst.max(t.total);
            }
        }
    }

    // About 4,000 loop dispatches per batch must cost under 40 ms. Each
    // measured pattern has 5,000 dispatches, so this is slightly stricter.
    assert!(worst < Duration::from_millis(40), "{worst:?}");
}

/// Times three dispatch patterns on a device from an instance with `flags`:
/// A, 5,000 indirect dispatches of 0 workgroups; B, 2,500 × (a setup kernel
/// writing the arguments, then the indirect dispatch reading them); and C,
/// 5,000 direct dispatches of one workgroup.
fn dispatch_costs(flags: wgpu::InstanceFlags) -> Vec<(&'static str, Timing)> {
    let (device, queue) = device_with(flags);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("dispatch cost"),
        source: wgpu::ShaderSource::Wgsl(DISPATCH_COST_WGSL.into()),
    });
    let pipeline = |entry: &str| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: None,
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    };
    let (setup, work) = (pipeline("setup"), pipeline("work"));

    // Two argument buffers, so pattern A can read one that nothing writes.
    let buffer = |usage: wgpu::BufferUsages| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 16,
            usage: usage | wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        })
    };
    let args = buffer(wgpu::BufferUsages::INDIRECT);
    let fixed_args = buffer(wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST);
    queue.write_buffer(&fixed_args, 0, &[0; 16]);
    let counter = buffer(wgpu::BufferUsages::empty());
    let bind_group = |layout: &wgpu::BindGroupLayout, binding: u32, buffer: &wgpu::Buffer| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding,
                resource: buffer.as_entire_binding(),
            }],
        })
    };
    let setup_group = bind_group(&setup.get_bind_group_layout(0), 0, &args);
    let work_group = bind_group(&work.get_bind_group_layout(0), 1, &counter);

    // Records `steps` loop steps of `pattern` in one pass and times encoding
    // plus running.
    let run = |steps: u32, pattern: char| -> Timing {
        let start = Instant::now();
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            for _ in 0..steps {
                if pattern == 'B' {
                    pass.set_pipeline(&setup);
                    pass.set_bind_group(0, &setup_group, &[]);
                    pass.dispatch_workgroups(1, 1, 1);
                }
                pass.set_pipeline(&work);
                pass.set_bind_group(0, &work_group, &[]);
                match pattern {
                    'A' => pass.dispatch_workgroups_indirect(&fixed_args, 0),
                    'B' => pass.dispatch_workgroups_indirect(&args, 0),
                    _ => pass.dispatch_workgroups(1, 1, 1),
                }
            }
        }
        let commands = encoder.finish();
        let encode = start.elapsed();
        queue.submit([commands]);
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll");
        Timing {
            encode,
            total: start.elapsed(),
        }
    };

    // Warm up, then take the fastest of three runs of each pattern.
    for pattern in ['A', 'B', 'C'] {
        run(100, pattern);
    }
    let best = |steps: u32, pattern: char| {
        (0..3)
            .map(|_| run(steps, pattern))
            .min_by_key(|t| t.total)
            .unwrap()
    };
    vec![
        ("A: 5000 empty indirect", best(5000, 'A')),
        ("B: 2500 x (setup + empty indirect)", best(2500, 'B')),
        ("C: 5000 direct, 1 workgroup", best(5000, 'C')),
    ]
}
