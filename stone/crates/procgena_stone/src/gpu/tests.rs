//! GPU generator tests. Tests marked `#[ignore]` need a GPU; run them with
//! `cargo test -p procgena_stone --features gpu --lib -- --ignored gpu`.

use std::collections::HashMap;

use futures_lite::future::block_on;

use super::*;
use crate::grid::SampleGrid;
use crate::{MIN_RESOLUTION, StoneMesh, mesh_field};

/// Creates a device on the default adapter.
fn device() -> (wgpu::Device, wgpu::Queue) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .expect("a GPU adapter is available");
    block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("device")
}

/// Copies `buffer` to a mappable buffer after the commands in `encoder` and
/// returns its bytes.
fn read_buffers(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    mut encoder: wgpu::CommandEncoder,
    buffers: &[&wgpu::Buffer],
) -> Vec<Vec<u8>> {
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
        .map(|copy| copy.get_mapped_range(..).expect("mapped").to_vec())
        .collect()
}

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&w| u32::from_le_bytes(w))
        .collect()
}

fn floats(bytes: &[u8]) -> Vec<f32> {
    words(bytes).into_iter().map(f32::from_bits).collect()
}

/// Generates `requests` and reads back every output buffer.
struct Readback {
    vertices: Vec<f32>,
    indices: Vec<u32>,
    draw_args: Vec<u32>,
    status: Vec<GpuStoneStatus>,
    /// Raw status words interpreted as floats to inspect rotation and offset.
    status_floats: Vec<f32>,
    samples: Vec<f32>,
    ranges: Vec<GpuMeshRange>,
}

/// Number of 32-bit words in one shader status entry, including padding.
const STATUS_WORDS: usize = (STATUS_SIZE / 4) as usize;

impl Readback {
    /// Returns mesh `i`'s ground rotation, unscaled offset, and suggested burial depth.
    fn placement(&self, i: usize) -> (glam::Mat3, glam::Vec3, f32) {
        let s = &self.status_floats[i * STATUS_WORDS..][..STATUS_WORDS];
        let col = |k: usize| glam::Vec3::new(s[4 + 4 * k], s[5 + 4 * k], s[6 + 4 * k]);
        (
            glam::Mat3::from_cols(col(0), col(1), col(2)),
            glam::Vec3::new(s[16], s[17], s[18]),
            s[19],
        )
    }
}

fn generate(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    generator: &GpuStoneGenerator,
    requests: &[GpuStoneRequest],
) -> Readback {
    let mut encoder = device.create_command_encoder(&Default::default());
    let (batch, scratch) = generator
        .encode_with_scratch(device, &mut encoder, requests)
        .unwrap();
    let [vertices, indices, draw_args, status, samples] = read_buffers(
        device,
        queue,
        encoder,
        &[
            &batch.vertices,
            &batch.indices,
            &batch.draw_args,
            &batch.status,
            &scratch.samples,
        ],
    )
    .try_into()
    .unwrap();
    Readback {
        vertices: floats(&vertices),
        indices: words(&indices),
        draw_args: words(&draw_args),
        status: GpuStoneStatus::decode(&status),
        status_floats: floats(&status),
        samples: floats(&samples),
        ranges: batch.ranges,
    }
}

/// Extracts mesh `i` and undoes its GPU placement for comparison with the
/// original field. Panics if capacity or draw-argument checks fail; `what`
/// identifies the failing case.
fn gpu_mesh(out: &Readback, i: usize, what: &str) -> StoneMesh {
    let range = out.ranges[i];
    let status = out.status[i];
    assert!(status.fits(), "{what}: overflowed {status:?}");
    let args = &out.draw_args[(range.draw_args_offset / 4) as usize..][..5];
    assert_eq!(
        args,
        [
            status.index_count,
            1,
            range.index_base,
            range.vertex_base,
            0
        ],
        "{what}: draw arguments"
    );
    let vertices =
        &out.vertices[range.vertex_base as usize * 6..][..status.vertex_count as usize * 6];
    let first = range.index_base as usize;
    let vertices = vertices.as_chunks::<6>().0;
    let (rotation, translation, _) = out.placement(i);
    let back = rotation.transpose();
    let position = |v: &[f32; 6]| back * (glam::Vec3::new(v[0], v[1], v[2]) - translation);
    let normal = |v: &[f32; 6]| back * glam::Vec3::new(v[3], v[4], v[5]);
    StoneMesh {
        positions: vertices.iter().map(|v| position(v).to_array()).collect(),
        normals: vertices.iter().map(|v| normal(v).to_array()).collect(),
        indices: out.indices[first..first + status.index_count as usize].to_vec(),
    }
}

/// How a GPU mesh differs from the CPU mesh for the same settings.
#[derive(Debug)]
struct SurfaceDiff {
    /// Same counts and index buffer, so vertices correspond one to one.
    identical: bool,
    /// GPU count minus CPU count.
    vertex_delta: i64,
    index_delta: i64,
    /// Distance from each vertex of either mesh to the nearest vertex of the
    /// other, in grid cells, sorted. Vertices with nothing within about one
    /// cell get infinity.
    distances: Vec<f32>,
    /// Largest normal difference (vector length) between vertices of the two
    /// meshes that are within 0.01 cell of each other.
    normal_error: f32,
    /// Vertices of either mesh further than [`CLOSE`] cells from the nearest
    /// vertex of the other.
    far: Vec<[f32; 3]>,
}

/// Distance in cells within which GPU and CPU vertices count as matching.
const CLOSE: f32 = 0.05;

impl SurfaceDiff {
    fn new(gpu: &StoneMesh, cpu: &StoneMesh, resolution: u32) -> Self {
        let r = resolution as f32;
        let mut distances = Vec::with_capacity(gpu.vertex_count() + cpu.vertex_count());
        let mut normal_error = 0f32;
        let mut far = Vec::new();
        for (from, to) in [(gpu, cpu), (cpu, gpu)] {
            // Bucket `to` by grid cell, then search the 27 cells around each vertex.
            let cell = |p: &[f32; 3]| p.map(|x| ((x + 0.5) * r).floor() as i32);
            let mut buckets: HashMap<[i32; 3], Vec<usize>> = HashMap::new();
            for (v, p) in to.positions.iter().enumerate() {
                buckets.entry(cell(p)).or_default().push(v);
            }
            for (p, n) in from.positions.iter().zip(&from.normals) {
                let c = cell(p);
                let mut best = (f32::INFINITY, 0);
                for d in 0..27 {
                    let key = [c[0] + d % 3 - 1, c[1] + d / 3 % 3 - 1, c[2] + d / 9 - 1];
                    for &v in buckets.get(&key).into_iter().flatten() {
                        let dist =
                            (glam::Vec3::from(*p) - glam::Vec3::from(to.positions[v])).length();
                        if dist < best.0 {
                            best = (dist, v);
                        }
                    }
                }
                let cells = best.0 * r;
                distances.push(cells);
                if cells > CLOSE {
                    far.push(*p);
                }
                if cells < 0.01 {
                    let delta = glam::Vec3::from(*n) - glam::Vec3::from(to.normals[best.1]);
                    normal_error = normal_error.max(delta.length());
                }
            }
        }
        distances.sort_by(f32::total_cmp);
        Self {
            identical: gpu.indices == cpu.indices && gpu.vertex_count() == cpu.vertex_count(),
            vertex_delta: gpu.vertex_count() as i64 - cpu.vertex_count() as i64,
            index_delta: gpu.indices.len() as i64 - cpu.indices.len() as i64,
            distances,
            normal_error,
            far,
        }
    }

    /// Distance below which fraction `q` of the vertices lie, in cells.
    fn quantile(&self, q: f64) -> f32 {
        self.distances[((self.distances.len() - 1) as f64 * q) as usize]
    }
}

/// Compares mesh `i` of a batch with the CPU mesh for `request` and checks
/// that they describe the same surface.
fn check_mesh(out: &Readback, i: usize, request: &GpuStoneRequest) -> SurfaceDiff {
    let options = &request.options;
    let (mesher, resolution) = (options.mesh.mesher, request.resolution);
    let what = format!(
        "{mesher:?} R={resolution} seed {} (request {i})",
        options.default_seed
    );
    let gpu = gpu_mesh(out, i, &what);
    let field = StoneField::new(options).unwrap();
    let cpu = mesh_field(&field, mesher, resolution).unwrap();
    let diff = SurfaceDiff::new(&gpu, &cpu, resolution);
    let grid = SampleGrid::sample(&field, resolution);
    assert_same_surface(&diff, &gpu, &grid, mesher, &what);
    diff
}

/// [`check_mesh`] for every mesh of a batch.
fn check_meshes(out: &Readback, requests: &[GpuStoneRequest]) -> Vec<SurfaceDiff> {
    requests
        .iter()
        .enumerate()
        .map(|(i, request)| check_mesh(out, i, request))
        .collect()
}

/// Checks that a GPU mesh describes the same surface as the CPU mesh, and that
/// it is closed and faces outward.
///
/// GPU rounding moves samples within [`SAMPLE_TOLERANCE`] of the surface to
/// the other side, which adds, removes, or moves a few vertices next to them.
/// Every other vertex must match a vertex of the CPU mesh within [`CLOSE`].
fn assert_same_surface(
    diff: &SurfaceDiff,
    gpu: &StoneMesh,
    grid: &SampleGrid,
    mesher: Mesher,
    what: &str,
) {
    // A few flipped samples change the counts by a few crossings.
    assert!(
        diff.vertex_delta.abs() <= 16 && diff.index_delta.abs() <= 96,
        "{what}: counts differ by {} vertices and {} indices",
        diff.vertex_delta,
        diff.index_delta
    );

    // Nearly every vertex matches closely, and none is more than a cell away.
    let typical = match mesher {
        Mesher::MarchingCubes => 0.01,
        Mesher::DualContouring => 0.1,
    };
    let (q, max) = (diff.quantile(0.999), diff.quantile(1.0));
    assert!(
        q <= typical && max <= 1.0,
        "{what}: 99.9% of vertices within {q} cells, all within {max}"
    );

    // Vertices that do not match lie next to a sample that rounding can flip.
    assert!(
        !diff.identical || diff.far.is_empty(),
        "{what}: identical meshes have {} unmatched vertices",
        diff.far.len()
    );
    let r = grid.resolution() as i64;
    for p in &diff.far {
        let cell = p.map(|x| (((x + 0.5) * r as f32).floor() as i64).clamp(0, r - 1));
        let near_surface = (0..64).any(|k| {
            let s = [
                cell[0] + k % 4 - 1,
                cell[1] + k / 4 % 4 - 1,
                cell[2] + k / 16 - 1,
            ];
            s.iter().all(|&c| (0..=r).contains(&c))
                && grid.value(s.map(|c| c as u32)).abs() <= SAMPLE_TOLERANCE
        });
        assert!(
            near_surface,
            "{what}: vertex {p:?} is unmatched with no sample near the surface around it"
        );
    }
    assert!(
        diff.normal_error <= 0.05,
        "{what}: matching vertices' normals differ by {}",
        diff.normal_error
    );

    // Closed and outward-facing; marching cubes is also manifold.
    let topology = crate::mesh::tests::topology(gpu);
    assert_eq!(topology.unbalanced_edges, 0, "{what}: {topology:?}");
    assert!(topology.signed_volume > 0.0, "{what}: {topology:?}");
    if mesher == Mesher::MarchingCubes {
        assert_eq!(topology.non_manifold_edges, 0, "{what}: {topology:?}");
    }
}

/// Pseudo-random count pairs for scan tests.
fn scan_input(count: u32) -> Vec<u32> {
    (0..2 * count)
        .map(|i| procgena_core::hash::pcg3d([i, count, 7])[0] % 13)
        .collect()
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_scan_matches_cpu_prefix_sum() {
    let (device, queue) = device();
    let generator = GpuStoneGenerator::new(&device);
    let alignment = device.limits().min_uniform_buffer_offset_alignment as u64;
    // One block, block edges, two and three levels.
    for count in [1, 255, 256, 257, 65_536, 65_537, 200_000] {
        assert!(scan_levels(count).len() <= 3);
        let input = scan_input(count);
        let data = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("test scan data"),
            contents: &input
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        let scan = ScanResources::new(
            &device,
            &generator.layouts.scan,
            &data,
            count,
            &[count],
            alignment,
        );
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            let p = &generator.pipelines;
            let max_groups = device.limits().max_compute_workgroups_per_dimension;
            scan.record(
                &mut pass,
                &p.scan_blocks,
                &p.add_block_offsets,
                0,
                max_groups,
            );
        }
        let out = words(&read_buffers(&device, &queue, encoder, &[&data])[0]);

        let mut total = [0u32; 2];
        for (i, pair) in input.chunks(2).enumerate() {
            assert_eq!(out[2 * i..2 * i + 2], total, "count {count}, entry {i}");
            total = [total[0] + pair[0], total[1] + pair[1]];
        }
    }
}

/// Seeds and settings for the parity tests.
fn parity_options(mesher: Mesher) -> Vec<StoneOptions> {
    (0..20)
        .map(|seed| {
            let mut options = StoneOptions {
                default_seed: seed,
                ..StoneOptions::default()
            };
            if seed % 5 == 4 {
                options.stretch = [1.0, 0.6, 0.8];
            }
            match seed % 5 {
                2 => options.carve.blockiness = 0.5,
                3 => options.carve.blockiness = 1.0,
                _ => {}
            }
            options.mesh.mesher = mesher;
            options
        })
        .collect()
}

/// Requests for every parity shape at `resolution` cells per axis.
fn parity_requests(mesher: Mesher, resolution: u32) -> Vec<GpuStoneRequest> {
    parity_options(mesher)
        .into_iter()
        .map(|options| GpuStoneRequest::new(options, resolution))
        .collect()
}

/// Largest allowed difference between GPU and CPU field samples. GPU drivers
/// may fuse multiply-adds and approximate square roots and division (Metal
/// does by default), so samples differ in their last bits. Samples this close
/// to the surface may also land on the other side of it.
const SAMPLE_TOLERANCE: f32 = 1e-5;

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_samples_match_cpu() {
    let (device, queue) = device();
    let generator = GpuStoneGenerator::new(&device);
    let mut differing = 0;
    let mut sign_flips = 0;
    let mut unexplained_flips = 0;
    let mut max_error = 0f32;
    let mut total = 0;
    for options in parity_options(Mesher::MarchingCubes) {
        let out = generate(
            &device,
            &queue,
            &generator,
            &[GpuStoneRequest::new(options.clone(), 48)],
        );
        let field = StoneField::new(&options).unwrap();
        let grid = SampleGrid::sample(&field, 48);
        let n = 49;
        for (i, &gpu) in out.samples[..n * n * n].iter().enumerate() {
            let i = i as u32;
            let cpu = grid.value([i % 49, i / 49 % 49, i / (49 * 49)]);
            total += 1;
            if gpu.to_bits() != cpu.to_bits() {
                differing += 1;
                max_error = max_error.max((gpu - cpu).abs());
            }
            if (gpu < 0.0) != (cpu < 0.0) {
                sign_flips += 1;
                if cpu.abs() > SAMPLE_TOLERANCE {
                    unexplained_flips += 1;
                }
            }
        }
    }
    println!("samples: {differing} of {total} differ (max {max_error:e}), {sign_flips} sign flips");
    assert!(max_error <= SAMPLE_TOLERANCE, "sample error {max_error}");
    assert!(
        unexplained_flips == 0,
        "{unexplained_flips} samples changed sign further than {SAMPLE_TOLERANCE} from the surface"
    );
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_meshes_match_cpu() {
    let (device, queue) = device();
    let generator = GpuStoneGenerator::new(&device);
    for mesher in [Mesher::MarchingCubes, Mesher::DualContouring] {
        for resolution in [16, 48, 96, 160] {
            let requests = parity_requests(mesher, resolution);
            let out = generate(&device, &queue, &generator, &requests);
            let diffs = check_meshes(&out, &requests);
            let identical = diffs.iter().filter(|d| d.identical).count();
            println!(
                "{mesher:?} R={resolution}: {identical} of {} meshes identical to the CPU's",
                diffs.len()
            );
        }
    }
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_placement_matches_cpu() {
    let (device, queue) = device();
    let generator = GpuStoneGenerator::new(&device);

    // Compare two detail levels per shape, both settling modes, and a burial cap.
    let mut shapes = parity_options(Mesher::MarchingCubes);
    for (k, options) in shapes.iter_mut().enumerate() {
        options.placement.settle = k % 4 != 3;
        if k % 3 == 2 {
            options.placement.max_embed_fraction = 0.3;
        }
    }
    let requests: Vec<_> = shapes
        .iter()
        .flat_map(|options| {
            let mut finer = options.clone();
            finer.scale = 4.0;
            [
                GpuStoneRequest::new(options.clone(), 24),
                GpuStoneRequest::new(finer, 48),
            ]
        })
        .collect();
    let out = generate(&device, &queue, &generator, &requests);

    let cell = 1.0 / crate::placement::ANALYSIS_RESOLUTION as f32;
    for (k, options) in shapes.iter().enumerate() {
        let what = format!(
            "seed {} settle {}",
            options.default_seed, options.placement.settle
        );
        let field = StoneField::new(options).unwrap();
        let cpu = crate::StonePlacement::new(&field, &options.placement);
        let (rotation, translation, embed) = out.placement(2 * k);

        // Changing mesh detail must preserve the exact shared placement values.
        assert_eq!(
            out.status_floats[2 * k * STATUS_WORDS..][4..STATUS_WORDS],
            out.status_floats[(2 * k + 1) * STATUS_WORDS..][4..STATUS_WORDS],
            "{what}: detail levels placed differently"
        );

        // For these shapes, CPU and GPU placement should agree within rounding tolerance.
        let placed = |p: glam::Vec3| rotation * p + translation;
        let probe = [
            glam::Vec3::X,
            glam::Vec3::Y,
            glam::Vec3::Z,
            glam::Vec3::ZERO,
        ];
        for p in probe.map(|p| p * 0.4) {
            let cpu_placed = cpu.field_position(placed(p));
            assert!(
                cpu_placed.distance(p) < 0.5 * cell,
                "{what}: placements differ by {}",
                cpu_placed.distance(p)
            );
        }
        assert!(
            (embed - cpu.embed_depth()).abs() <= 2.0 * cell,
            "{what}: embed {embed} vs {}",
            cpu.embed_depth()
        );
        assert_eq!(out.status[2 * k].embed_depth, embed);
        assert!((out.status[2 * k].bounding_radius - cpu.bounding_radius()).abs() < cell);
    }
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_overflow_is_reported_and_retry_fits() {
    let (device, queue) = device();
    let generator = GpuStoneGenerator::new(&device);
    for mesher in [Mesher::MarchingCubes, Mesher::DualContouring] {
        let mut requests = parity_requests(mesher, 32);
        requests.truncate(3);
        requests[1].vertex_capacity = 100;
        requests[1].index_capacity = 300;
        let out = generate(&device, &queue, &generator, &requests);

        // Neighbours are unaffected; the undersized mesh draws nothing but
        // reports what it needs.
        check_mesh(&out, 0, &requests[0]);
        check_mesh(&out, 2, &requests[2]);
        let status = out.status[1];
        assert!(
            status.vertex_overflow && status.index_overflow,
            "{status:?}"
        );
        assert_eq!(out.draw_args[5], 0, "overflowed mesh draws no indices");

        // Retrying with the reported counts fits exactly.
        requests[1].grow_to_fit(&status);
        assert_eq!(requests[1].vertex_capacity, status.vertex_count);
        assert_eq!(requests[1].index_capacity, status.index_count);
        let out = generate(&device, &queue, &generator, &requests);
        assert_eq!(
            (out.status[1].vertex_count, out.status[1].index_count),
            (status.vertex_count, status.index_count)
        );
        check_meshes(&out, &requests);
    }
}

#[test]
#[ignore = "gpu: needs a GPU adapter"]
fn gpu_handles_largest_resolution_and_empty_batches() {
    let (device, queue) = device();
    let generator = GpuStoneGenerator::new(&device);
    for mesher in [Mesher::MarchingCubes, Mesher::DualContouring] {
        let mut options = StoneOptions::default();
        options.mesh.mesher = mesher;
        let requests = [GpuStoneRequest::new(options, MAX_GPU_RESOLUTION)];
        let out = generate(&device, &queue, &generator, &requests);
        check_meshes(&out, &requests);
    }

    let mut encoder = device.create_command_encoder(&Default::default());
    let batch = generator.encode(&device, &mut encoder, &[]).unwrap();
    assert!(batch.ranges.is_empty());
    queue.submit([encoder.finish()]);
}

#[test]
fn requests_that_cannot_run_are_rejected() {
    let limits = BufferLimits { storage: 128 << 20 };
    let err = BatchPlan::new(
        &[
            GpuStoneRequest::new(StoneOptions::default(), 96),
            GpuStoneRequest::new(StoneOptions::default(), MAX_GPU_RESOLUTION + 1),
        ],
        &limits,
    )
    .unwrap_err();
    assert_eq!(
        err,
        GpuError::ResolutionTooHigh {
            request: 1,
            resolution: MAX_GPU_RESOLUTION + 1
        }
    );

    let options = StoneOptions {
        scale: -1.0,
        ..StoneOptions::default()
    };
    let err = BatchPlan::new(&[GpuStoneRequest::new(options, 96)], &limits).unwrap_err();
    assert!(matches!(err, GpuError::Options { request: 0, .. }));

    let err = BatchPlan::new(
        &[GpuStoneRequest::new(
            StoneOptions::default(),
            MIN_RESOLUTION - 1,
        )],
        &limits,
    )
    .unwrap_err();
    assert!(matches!(err, GpuError::Options { request: 0, .. }));

    let mut request = GpuStoneRequest::new(StoneOptions::default(), 96);
    request.vertex_capacity = u32::MAX;
    let err = BatchPlan::new(&[request], &limits).unwrap_err();
    assert!(matches!(
        err,
        GpuError::BufferTooLarge {
            buffer: "vertices",
            ..
        }
    ));
}

#[test]
fn largest_resolution_fits_default_limits() {
    let limits = BufferLimits::from(&wgpu::Limits::default());
    let mut options = StoneOptions::default();
    options.mesh.mesher = Mesher::DualContouring;
    BatchPlan::new(
        &[GpuStoneRequest::new(options, MAX_GPU_RESOLUTION)],
        &limits,
    )
    .unwrap();
    // One step higher, the prefix buffer would exceed the limit.
    let samples = (MAX_GPU_RESOLUTION as u64 + 2).pow(3);
    assert!((samples + 1) * 8 > limits.storage);
}

#[test]
fn scan_levels_end_with_one_block() {
    assert_eq!(scan_levels(1), [1]);
    assert_eq!(scan_levels(256), [256]);
    assert_eq!(scan_levels(257), [257, 2]);
    assert_eq!(scan_levels(65_537), [65_537, 257, 2]);
}

#[test]
fn estimate_covers_typical_rocks() {
    let variants: [fn(&mut StoneOptions); 5] = [
        |_| {},
        |o| o.carve.steps = 0,
        |o| o.carve.blockiness = 1.0,
        |o| o.surface.displacement_scale = 0.0,
        |o| o.surface.displacement_spread = 30.0,
    ];
    for resolution in [16, 32, 64] {
        let (vertices, indices) = estimate_capacity(resolution);
        for variant in variants {
            for seed in 0..6 {
                let mut options = StoneOptions {
                    default_seed: seed,
                    ..StoneOptions::default()
                };
                variant(&mut options);
                let field = StoneField::new(&options).unwrap();
                for mesher in [Mesher::MarchingCubes, Mesher::DualContouring] {
                    let mesh = mesh_field(&field, mesher, resolution).unwrap();
                    assert!(mesh.vertex_count() <= vertices as usize);
                    assert!(mesh.indices.len() <= indices as usize);
                }
            }
        }
    }
}

#[test]
fn status_decodes_overflow_bits_counts_and_placement() {
    let entry = |overflow: u32, vertices: u32, indices: u32, translation: [f32; 3], embed: f32| {
        let mut words = vec![overflow, vertices, indices, 0];
        words.extend([0; 12]);
        words.extend(translation.map(f32::to_bits));
        words.push(embed.to_bits());
        words
    };
    let mut words = entry(0, 10, 60, [0.0, 0.3, 0.4], 0.25);
    words.extend(entry(OVERFLOW_CROSSINGS, 5, 600, [0.0; 3], 0.0));
    words.push(0);
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let status = GpuStoneStatus::decode(&bytes);
    assert_eq!(status.len(), 2);
    assert!(status[0].fits());
    assert_eq!((status[0].vertex_count, status[0].index_count), (10, 60));
    assert_eq!(status[0].embed_depth, 0.25);
    assert!((status[0].bounding_radius - (0.5 + 0.75f32.sqrt())).abs() < 1e-6);
    assert!(!status[1].vertex_overflow && status[1].index_overflow);
}
