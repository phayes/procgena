//! Compares GPU and CPU clump generation on real rocks.
//!
//! Run `cargo run --release -p procgena_clump --example bench_gpu --features gpu`.
//!
//! Generates a batch of rocks with `procgena_stone` on the GPU, then grows
//! each clump preset on every rock twice: on the GPU, straight from the
//! rocks' GPU buffers, and on the CPU, from the same rocks read back. GPU
//! time runs from recording the commands until the GPU has finished, the
//! median of several runs once the batch fits. CPU time is one run per rock,
//! single-threaded. Layers often run in batches of one or two rocks, the most
//! that fit the device's buffer limits. The last column counts the clumps
//! whose triangles match the CPU's exactly; each one that differs is listed
//! with its counts.

use std::time::{Duration, Instant};

use futures_lite::future::block_on;
use glam::{Affine3A, Vec3};
use procgena_clump::gpu::{
    GPU_STATUS_SIZE, GpuClumpGenerator, GpuClumpRequest, GpuClumpStatus, GpuError, GpuSurfaceInput,
    GpuVertexFormat,
};
use procgena_clump::{Clump, ClumpOptions, ClumpPreset};
use procgena_specimen::SurfaceMesh;
use procgena_stone::StoneOptions;
use procgena_stone::gpu::{
    GPU_VERTEX_SIZE as STONE_VERTEX_SIZE, GpuStoneBatch, GpuStoneGenerator, GpuStoneRequest,
    GpuStoneStatus,
};

/// Rocks in the batch, their mesh resolution, and timed GPU runs per row.
const ROCKS: u32 = 8;
const RESOLUTION: u32 = 48;
const RUNS: usize = 5;

fn main() {
    // Use the default adapter.
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .expect("a GPU adapter is available");
    println!("adapter: {}", adapter.get_info().name);
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
        .expect("the adapter provides a device");

    // The rocks, on the GPU and read back for the CPU.
    let stones = GpuStoneGenerator::new(&device);
    let stone_requests: Vec<_> = (0..ROCKS)
        .map(|seed| {
            let options = StoneOptions {
                default_seed: seed,
                ..StoneOptions::default()
            };
            GpuStoneRequest::new(options, RESOLUTION)
        })
        .collect();
    let mut encoder = device.create_command_encoder(&Default::default());
    let rocks = stones
        .encode(&device, &mut encoder, &stone_requests)
        .expect("the rock batch fits the device");
    queue.submit([encoder.finish()]);
    let meshes = read_rocks(&device, &queue, &rocks);

    // Each preset, and the snow cap in strict mode.
    let mut cases: Vec<(String, ClumpOptions)> = ClumpPreset::all()
        .map(|p| (p.key.to_string(), p.clump.clone()))
        .collect();
    let mut strict = ClumpPreset::dirty_snow_cap().clump.clone();
    strict.mesh.strict = true;
    cases.push(("dirty_snow_cap strict".to_string(), strict));

    let generator = GpuClumpGenerator::new(&device);
    println!(
        "{:<22} {:>6} {:>6} {:>10} {:>10} {:>8} {:>7}",
        "preset", "tries", "batch", "GPU ms", "CPU ms", "speedup", "same"
    );
    for (name, options) in &cases {
        // Grow the requests until every clump fits.
        let mut requests: Vec<GpuClumpRequest> = (0..ROCKS as usize)
            .map(|i| {
                let range = &rocks.ranges[i];
                let input = GpuSurfaceInput {
                    vertices: &rocks.vertices,
                    indices: &rocks.indices,
                    draw_args: &rocks.draw_args,
                    draw_args_offset: range.draw_args_offset,
                    format: GpuVertexFormat::POSITION_NORMAL,
                    transform: Affine3A::from_scale(Vec3::splat(range.scale)),
                    vertex_capacity: range.vertex_capacity,
                    triangle_capacity: range.index_capacity / 3,
                    closed: None,
                };
                GpuClumpRequest::new(input, options.clone(), area(&meshes[i]))
            })
            .collect();
        let mut tries = 0;
        let (clumps, size) = loop {
            tries += 1;
            let size = batch_size(&device, &generator, &requests);
            let clumps = run(&device, &queue, &generator, &requests, size, true)
                .expect("read back on request");
            let mut grown = false;
            for (request, (status, _)) in requests.iter_mut().zip(&clumps) {
                if status.vertex_overflow || status.index_overflow {
                    request.grow_to_fit(status);
                    grown = true;
                }
            }
            if !grown {
                break (clumps, size);
            }
        };

        // GPU: the median of several runs. CPU: one run per rock.
        let mut times: Vec<Duration> = (0..RUNS)
            .map(|_| {
                let start = Instant::now();
                run(&device, &queue, &generator, &requests, size, false);
                start.elapsed()
            })
            .collect();
        times.sort();
        let gpu = times[RUNS / 2];
        let start = Instant::now();
        let cpu: Vec<Clump> = meshes
            .iter()
            .map(|mesh| {
                Clump::new(mesh.surface().expect("rock meshes are valid"), options).unwrap()
            })
            .collect();
        let cpu_time = start.elapsed();
        let same = clumps
            .iter()
            .zip(&cpu)
            .filter(|((_, indices), clump)| *indices == clump.mesh().indices)
            .count();
        println!(
            "{:<22} {:>6} {:>6} {:>10.1} {:>10.1} {:>7.1}x {:>4}/{}",
            name,
            tries,
            size,
            gpu.as_secs_f64() * 1000.0,
            cpu_time.as_secs_f64() * 1000.0,
            cpu_time.as_secs_f64() / gpu.as_secs_f64(),
            same,
            ROCKS
        );

        // How far each clump that differs is from the CPU's.
        for (rock, ((status, indices), clump)) in clumps.iter().zip(&cpu).enumerate() {
            let want = clump.mesh();
            if *indices != want.indices {
                let first = indices.iter().zip(&want.indices).position(|(a, b)| a != b);
                println!(
                    "  rock {rock}: {} vertices, {} indices; CPU {}, {}; first different index {:?}",
                    status.vertex_count,
                    indices.len(),
                    want.positions.len(),
                    want.indices.len(),
                    first
                );
            }
        }
    }
}

/// Returns the most requests per batch, halving from all of them, for which
/// every batch fits the device's buffer limits.
fn batch_size(
    device: &wgpu::Device,
    generator: &GpuClumpGenerator,
    requests: &[GpuClumpRequest],
) -> usize {
    let mut size = requests.len();
    loop {
        let fits = requests.chunks(size).all(|chunk| {
            let mut encoder = device.create_command_encoder(&Default::default());
            match generator.encode(device, &mut encoder, chunk) {
                Ok(_) => true,
                Err(GpuError::BufferTooLarge { .. }) => false,
                Err(error) => panic!("the requests are valid: {error}"),
            }
        });
        if fits || size == 1 {
            return size;
        }
        size = size.div_ceil(2);
    }
}

/// Records `requests` in batches of `size`, runs them and waits for the
/// GPU. With `read_back`, returns each clump's status and indices.
fn run(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    generator: &GpuClumpGenerator,
    requests: &[GpuClumpRequest],
    size: usize,
    read_back: bool,
) -> Option<Vec<(GpuClumpStatus, Vec<u32>)>> {
    let batches: Vec<_> = requests
        .chunks(size)
        .map(|chunk| {
            let mut encoder = device.create_command_encoder(&Default::default());
            let batch = generator
                .encode(device, &mut encoder, chunk)
                .expect("the clump batch fits the device");
            queue.submit([encoder.finish()]);
            batch
        })
        .collect();
    if !read_back {
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the GPU finishes");
        return None;
    }
    let mut clumps = Vec::with_capacity(requests.len());
    for batch in &batches {
        let [status, indices] = read(device, queue, [&batch.status, &batch.indices]);
        let indices = words(&indices);
        clumps.extend(batch.ranges.iter().map(|range| {
            let at = range.status_offset as usize;
            let status = GpuClumpStatus::decode(&status[at..at + GPU_STATUS_SIZE as usize]);
            let first = range.index_base as usize;
            (
                status,
                indices[first..first + status.index_count as usize].to_vec(),
            )
        }));
    }
    Some(clumps)
}

/// Reads the rocks in `batch` back as surface meshes in metres.
fn read_rocks(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    batch: &GpuStoneBatch,
) -> Vec<SurfaceMesh> {
    let [vertices, indices, status] = read(
        device,
        queue,
        [&batch.vertices, &batch.indices, &batch.status],
    );
    let floats: Vec<f32> = words(&vertices).into_iter().map(f32::from_bits).collect();
    let indices = words(&indices);
    let statuses = GpuStoneStatus::decode(&status);
    batch
        .ranges
        .iter()
        .zip(&statuses)
        .map(|(range, status)| {
            assert!(status.fits(), "rock meshes fit their estimate");
            let floats_per_vertex = (STONE_VERTEX_SIZE / 4) as usize;
            let vertex = |v: usize, k: usize| {
                floats[floats_per_vertex * (range.vertex_base as usize + v) + k]
            };
            let count = status.vertex_count as usize;
            let first = range.index_base as usize;
            SurfaceMesh {
                positions: (0..count)
                    .map(|v| [0, 1, 2].map(|k| vertex(v, k) * range.scale))
                    .collect(),
                normals: (0..count)
                    .map(|v| [3, 4, 5].map(|k| vertex(v, k)))
                    .collect(),
                indices: indices[first..first + status.index_count as usize].to_vec(),
            }
        })
        .collect()
}

/// Copies `buffers` to mappable buffers and returns their bytes.
fn read<const N: usize>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    buffers: [&wgpu::Buffer; N],
) -> [Vec<u8>; N] {
    let mut encoder = device.create_command_encoder(&Default::default());
    let staging = buffers.map(|buffer| {
        let copy = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bench read-back"),
            size: buffer.size(),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(buffer, 0, &copy, 0, buffer.size());
        copy
    });
    queue.submit([encoder.finish()]);
    for copy in &staging {
        copy.slice(..)
            .map_async(wgpu::MapMode::Read, |result| result.expect("buffer maps"));
    }
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("the GPU finishes");
    staging.map(|copy| {
        copy.slice(..)
            .get_mapped_range()
            .expect("the copy is mapped")
            .to_vec()
    })
}

/// Returns `bytes` as little-endian `u32` words.
fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .collect()
}

/// Returns the total area of `mesh`'s triangles, in square metres.
fn area(mesh: &SurfaceMesh) -> f32 {
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
