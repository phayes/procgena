//! Measures average GPU generation time per rock for both meshing methods.
//!
//! Run `cargo run --release -p procgena_stone --example bench_gpu --features gpu`.
//! Compare with the CPU numbers from the `bench` example. Each row generates a
//! batch of rocks with different seeds; time runs from recording the commands
//! until the GPU has finished, so it includes the CPU work of preparing the
//! batch.

use std::time::Instant;

use futures_lite::future::block_on;
use procgena_stone::gpu::{GpuStoneGenerator, GpuStoneRequest};
use procgena_stone::{Mesher, StoneOptions};

const RESOLUTIONS: [u32; 4] = [32, 64, 100, 128];
const BATCH: u32 = 16;
const RUNS: u32 = 3;

fn main() {
    // Use the default adapter.
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .expect("a GPU adapter is available");
    println!("adapter: {}", adapter.get_info().name);
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
        .expect("the adapter provides a device");
    let generator = GpuStoneGenerator::new(&device);

    // Generates one batch and waits for the GPU to finish.
    let run = |requests: &[GpuStoneRequest]| {
        let mut encoder = device.create_command_encoder(&Default::default());
        let batch = generator
            .encode(&device, &mut encoder, requests)
            .expect("the batch fits the device");
        queue.submit([encoder.finish()]);
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the GPU finishes");
        batch
    };

    println!(
        "{:<16} {:>5} {:>7} {:>10}",
        "mesher", "res", "batch", "ms/rock"
    );
    for mesher in [Mesher::MarchingCubes, Mesher::DualContouring] {
        for resolution in RESOLUTIONS {
            let requests: Vec<_> = (0..BATCH)
                .map(|seed| {
                    let mut options = StoneOptions {
                        seed,
                        ..StoneOptions::default()
                    };
                    options.mesh.mesher = mesher;
                    GpuStoneRequest::new(options, resolution)
                })
                .collect();

            // Run once untimed to compile pipelines and allocate memory.
            run(&requests);
            let start = Instant::now();
            for _ in 0..RUNS {
                run(&requests);
            }
            let ms = start.elapsed().as_secs_f64() * 1000.0 / (RUNS * BATCH) as f64;
            println!(
                "{:<16} {:>5} {:>7} {:>10.2}",
                format!("{mesher:?}"),
                resolution,
                BATCH,
                ms
            );
        }
    }
}
