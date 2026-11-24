//! Measures average CPU generation time and mesh size for both meshing methods.
//!
//! Run `cargo run --release -p procgena_stone --example bench`. Add
//! `--no-default-features` to measure generation on a single thread.
//! Each row averages three seeds at the displayed resolution; time includes
//! building the field and mesh.

use std::time::Instant;

use procgena_stone::{Mesher, StoneField, StoneOptions, mesh_field};

const RESOLUTIONS: [u32; 4] = [32, 64, 100, 128];
const SEEDS: [u32; 3] = [880, 1, 2];

fn main() {
    // Run one untimed mesh build to reduce first-run overhead in the measurements.
    let warm_up = StoneField::new(&StoneOptions::default()).expect("default options are valid");
    mesh_field(&warm_up, Mesher::MarchingCubes, 64).expect("resolution is in range");

    println!(
        "{:<16} {:>5} {:>10} {:>10} {:>10}",
        "mesher", "res", "ms/rock", "vertices", "triangles"
    );
    for mesher in [Mesher::MarchingCubes, Mesher::DualContouring] {
        for resolution in RESOLUTIONS {
            let start = Instant::now();
            let mut vertices = 0;
            let mut triangles = 0;
            for seed in SEEDS {
                let options = StoneOptions {
                    default_seed: seed,
                    ..StoneOptions::default()
                };
                let field = StoneField::new(&options).expect("default options are valid");
                let mesh = mesh_field(&field, mesher, resolution).expect("resolution is in range");
                vertices += mesh.vertex_count();
                triangles += mesh.triangle_count();
            }
            let n = SEEDS.len();
            let ms = start.elapsed().as_secs_f64() * 1000.0 / n as f64;
            println!(
                "{:<16} {:>5} {:>10.1} {:>10} {:>10}",
                format!("{mesher:?}"),
                resolution,
                ms,
                vertices / n,
                triangles / n
            );
        }
    }
}
