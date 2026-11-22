//! Phase 1 spike: builds snow caps on test shapes, prints measurements, and writes OBJ files to `target/spike/` for viewing.
//!
//! Run with `cargo run --release -p procgena_clump --example spike`.

mod shapes;

use std::fmt::Write as _;
use std::time::Instant;

use procgena_clump::diagnostics::{edge_defects, signed_distances};
use procgena_clump::{Clump, ClumpMesh, ClumpOptions, ClumpPreset, Direction, SurfaceMesh};
use procgena_stone::StonePreset;

/// One test: an input and how to adjust the default snow cap.
struct Case {
    name: &'static str,
    input: SurfaceMesh,
    adjust: fn(&mut ClumpOptions),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/spike");
    std::fs::create_dir_all(&out_dir)?;

    let cases = [
        Case {
            name: "ground",
            input: SurfaceMesh::ground(3.0, 48),
            adjust: |_| {},
        },
        Case {
            name: "ground_1quad",
            input: SurfaceMesh::ground(3.0, 1),
            adjust: |_| {},
        },
        Case {
            name: "ground_small",
            input: SurfaceMesh::ground(3.0, 48),
            adjust: small_clump,
        },
        Case {
            name: "1quad_small",
            input: SurfaceMesh::ground(3.0, 1),
            adjust: small_clump,
        },
        Case {
            name: "crevice",
            input: shapes::rock(1.0, 5, true),
            adjust: |_| {},
        },
        Case {
            name: "crevice_coarse",
            input: shapes::rock(1.0, 4, true),
            adjust: |_| {},
        },
        Case {
            name: "crevice_fine",
            input: shapes::rock(1.0, 6, true),
            adjust: |_| {},
        },
        Case {
            name: "crevice_edge2cm",
            input: shapes::rock(1.0, 5, true),
            adjust: |o| o.mesh.edge_length = 0.02,
        },
        Case {
            name: "crevice_edge8cm",
            input: shapes::rock(1.0, 5, true),
            adjust: |o| o.mesh.edge_length = 0.08,
        },
        Case {
            name: "branches",
            input: shapes::branch_fork(),
            adjust: |o| o.scale = 0.4,
        },
        Case {
            name: "thin_ground",
            input: SurfaceMesh::ground(3.0, 48),
            adjust: |o| {
                o.shape.thickness = 0.02;
                o.shape.sink = 0.005;
            },
        },
        Case {
            name: "upside_down",
            input: shapes::upside_down_plane(),
            adjust: |_| {},
        },
    ];
    let stones = [
        ("stone_boulder", StonePreset::Boulder),
        ("stone_lichen_boulder", StonePreset::LichenBoulder),
        ("stone_river_stone", StonePreset::RiverStone),
        ("stone_flat_slab", StonePreset::FlatSlab),
        ("stone_cliff_chunk", StonePreset::CliffChunk),
        ("stone_standing_stone", StonePreset::StandingStone),
        ("stone_shard", StonePreset::Shard),
        ("stone_pebble", StonePreset::Pebble),
    ];
    let strict_stones = [
        ("strict_boulder", StonePreset::Boulder),
        ("strict_lichen_boulder", StonePreset::LichenBoulder),
        ("strict_flat_slab", StonePreset::FlatSlab),
        ("strict_cliff_chunk", StonePreset::CliffChunk),
        ("strict_pebble", StonePreset::Pebble),
    ];
    let cases: Vec<Case> = cases
        .into_iter()
        .chain(stones.map(|(name, preset)| Case {
            name,
            input: shapes::stone(preset).0,
            adjust: |_| {},
        }))
        .chain(strict_stones.map(|(name, preset)| Case {
            name,
            input: shapes::stone(preset).0,
            adjust: |o| o.mesh.strict = true,
        }))
        .collect();
    let mut gallery = Vec::new();

    println!(
        "{:<16} {:>6} {:>7} {:>8} {:>6} {:>6} {:>5} {:>5} {:>7} {:>6} {:>8} {:>7} {:>4} {:>5}",
        "input",
        "in",
        "tris",
        "ms",
        "fold%",
        "thin",
        "bound",
        "over",
        "buried",
        "wrong",
        "edge_max",
        "deepest",
        "out",
        "cross"
    );
    for case in &cases {
        // Build the snow cap.
        let surface = case.input.surface()?;
        let mut options = ClumpOptions::default();
        (case.adjust)(&mut options);
        let start = Instant::now();
        let clump = Clump::new(surface, &options)?;
        let ms = start.elapsed().as_secs_f64() * 1000.0;

        // Measure the result against the input.
        let mesh = clump.mesh();
        let stats = clump.stats();
        let thickness = options.shape.thickness * options.scale;
        let sink = options.shape.sink * options.scale;
        let (boundary, overfull) = edge_defects(&mesh.indices);
        let heights = signed_distances(&surface, &mesh.positions);
        let edge_max = mesh
            .fade
            .iter()
            .zip(&heights)
            .filter(|(f, _)| **f == 0.0)
            .map(|(_, h)| *h)
            .fold(f32::NEG_INFINITY, f32::max);
        let deepest = heights.iter().copied().fold(f32::INFINITY, f32::min);
        // On closed inputs, count rim and underside vertices (fade 0) that a
        // robust inside test puts outside the input. Only those near or above
        // the surface by the nearest-normal measure need checking.
        let outside = if surface.is_closed() {
            let candidates: Vec<[f32; 3]> = (0..heights.len())
                .filter(|&i| mesh.fade[i] == 0.0 && heights[i] > -1e-3)
                .map(|i| mesh.positions[i])
                .collect();
            let inside = procgena_clump::diagnostics::inside_closed(&surface, &candidates);
            let surfaced = signed_distances(&surface, &candidates);
            inside
                .iter()
                .zip(&surfaced)
                .filter(|&(&inside, &h)| !inside && h > 1e-4)
                .count()
                .to_string()
        } else {
            "-".to_string()
        };
        println!(
            "{:<16} {:>6} {:>7} {:>8.1} {:>6.2} {:>6} {:>5} {:>5} {:>6.1}% {:>5.1}% {:>8.4} {:>7.2} {:>4} {:>5}",
            case.name,
            surface.triangle_count(),
            mesh.triangle_count(),
            ms,
            100.0 * stats.folded as f32 / mesh.triangle_count().max(1) as f32,
            stats.thinned,
            boundary,
            overfull,
            buried_share(mesh, &heights) * 100.0,
            misplaced_share(mesh, &heights, thickness, sink) * 100.0,
            edge_max,
            deepest / sink,
            outside,
            procgena_clump::diagnostics::crossing_count(mesh),
        );

        // Write the input and clump together for viewing.
        std::fs::write(
            out_dir.join(format!("{}.obj", case.name)),
            to_obj(&[("input", &case.input, mesh)]),
        )?;
        if case.name.starts_with("stone_") {
            gallery.push((&case.input, mesh.clone()));
        }
    }

    // Line the stones up along x in one file.
    let mut x = 0.0;
    let mut row = Vec::new();
    for (input, clump) in &gallery {
        let (lo, hi) = x_extent(input);
        let shift = x - lo;
        row.push((shifted(input, shift), shifted_clump(clump, shift)));
        x += hi - lo + 0.5;
    }
    let row_refs: Vec<_> = row.iter().map(|(i, c)| ("input", i, c)).collect();
    std::fs::write(out_dir.join("stones.obj"), to_obj(&row_refs))?;

    // Snow and moss galleries on four rocks, named for their textures.
    write_gallery(&out_dir)?;
    println!();
    println!("in: input triangles. fold: triangles turned over. thin: vertices held to half");
    println!("  the gap to a facing surface. bound/over: edges with 1 or 3+ triangles.");
    println!("buried: triangles fully below the input. wrong: top vertices whose height above");
    println!("  the nearest surface is off by more than a quarter of the thickness.");
    println!("edge_max: highest rim vertex (fade 0), should be <= 0.");
    println!("deepest: lowest vertex in sink depths, about -1 expected.");
    println!("out: rim or underside vertices outside a closed input by a ray-parity test;");
    println!("  should be 0. '-' for open inputs.");
    println!("cross: triangles that pass through another triangle of the clump.");
    println!("  strict_* rows build with mesh.strict on, which should bring it to 0.");
    println!("OBJ files are in {}", out_dir.display());
    Ok(())
}

/// Makes the snow cap a single 40 cm clump off-centre on the input.
fn small_clump(options: &mut ClumpOptions) {
    options.shape.at = Some([0.3, 0.0, -0.2]);
    options.shape.radius = Some(0.4);
}

/// Returns the fraction of triangles whose corners all lie below the input surface.
fn buried_share(mesh: &ClumpMesh, heights: &[f32]) -> f32 {
    if mesh.is_empty() {
        return 0.0;
    }
    let buried = mesh
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|tri| tri.iter().all(|&i| heights[i as usize] < 0.0))
        .count();
    buried as f32 / mesh.triangle_count() as f32
}

/// Returns the fraction of visible top vertices (fade above 0) whose height
/// above the nearest input surface differs from the intended
/// `fade × thickness` by more than a quarter of the thickness.
fn misplaced_share(mesh: &ClumpMesh, heights: &[f32], thickness: f32, sink: f32) -> f32 {
    let top: Vec<(f32, f32)> = mesh
        .fade
        .iter()
        .zip(heights)
        .filter(|&(&f, &h)| f > 0.0 && h > -0.5 * sink)
        .map(|(&f, &h)| (f, h))
        .collect();
    if top.is_empty() {
        return 0.0;
    }
    let wrong = top
        .iter()
        .filter(|&&(f, h)| (h - f * thickness).abs() > 0.25 * thickness)
        .count();
    wrong as f32 / top.len() as f32
}

/// Returns the lowest and highest x of the mesh's vertices.
fn x_extent(mesh: &SurfaceMesh) -> (f32, f32) {
    mesh.positions
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p[0]), hi.max(p[0]))
        })
}

/// Returns a copy of `mesh` moved `dx` along x.
fn shifted(mesh: &SurfaceMesh, dx: f32) -> SurfaceMesh {
    let mut out = mesh.clone();
    out.positions.iter_mut().for_each(|p| p[0] += dx);
    out
}

/// Returns a copy of `clump` moved `dx` along x.
fn shifted_clump(clump: &ClumpMesh, dx: f32) -> ClumpMesh {
    let mut out = clump.clone();
    out.positions.iter_mut().for_each(|p| p[0] += dx);
    out
}

/// Writes `stones_snow.obj` and `stones_moss.obj`: four rocks in a 2 × 2 grid
/// with a snow cap or a moss layer. Each rock's OBJ object is named
/// `input:<procgena_stone texture set>`, so the viewer can texture it.
fn write_gallery(out_dir: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let presets = [
        StonePreset::Boulder,
        StonePreset::LichenBoulder,
        StonePreset::CliffChunk,
        StonePreset::RiverStone,
    ];
    let rocks: Vec<(SurfaceMesh, String)> = presets.map(shapes::stone).into_iter().collect();
    let spacing = 3.2;
    // Snow falling from above plus wind-blown snow arriving from -X.
    let mut wind = ClumpPreset::SnowCap.options();
    wind.directions.push(Direction {
        vector: [1.0, -0.3, 0.2],
        strength: 0.8,
    });
    for (file, options) in [
        ("stones_snow.obj", ClumpPreset::SnowCap.options()),
        ("stones_moss.obj", ClumpPreset::MossPatches.options()),
        ("stones_wind.obj", wind),
    ] {
        let mut placed = Vec::new();
        for (i, (rock, texture)) in rocks.iter().enumerate() {
            let clump = Clump::new(rock.surface()?, &options)?;
            let offset = [(i % 2) as f32 * spacing, 0.0, (i / 2) as f32 * spacing];
            placed.push((
                format!("input:{texture}"),
                moved(rock, offset),
                moved_clump(clump.mesh(), offset),
            ));
        }
        let refs: Vec<_> = placed.iter().map(|(n, i, c)| (n.as_str(), i, c)).collect();
        std::fs::write(out_dir.join(file), to_obj(&refs))?;
    }
    Ok(())
}

/// Returns a copy of `mesh` moved by `offset`.
fn moved(mesh: &SurfaceMesh, offset: [f32; 3]) -> SurfaceMesh {
    let mut out = mesh.clone();
    out.positions
        .iter_mut()
        .for_each(|p| (0..3).for_each(|k| p[k] += offset[k]));
    out
}

/// Returns a copy of `clump` moved by `offset`.
fn moved_clump(clump: &ClumpMesh, offset: [f32; 3]) -> ClumpMesh {
    let mut out = clump.clone();
    out.positions
        .iter_mut()
        .for_each(|p| (0..3).for_each(|k| p[k] += offset[k]));
    out
}

/// Writes inputs and clumps as OBJ objects: each input under its given name,
/// each clump as `clump`.
fn to_obj(pairs: &[(&str, &SurfaceMesh, &ClumpMesh)]) -> String {
    let mut s = String::new();
    let mut base = 1;
    let objects = pairs.iter().flat_map(|(name, input, clump)| {
        [
            (*name, &input.positions, &input.normals, &input.indices),
            ("clump", &clump.positions, &clump.normals, &clump.indices),
        ]
    });
    for (name, positions, normals, indices) in objects {
        let _ = writeln!(s, "o {name}");
        for p in positions {
            let _ = writeln!(s, "v {} {} {}", p[0], p[1], p[2]);
        }
        for n in normals {
            let _ = writeln!(s, "vn {} {} {}", n[0], n[1], n[2]);
        }
        for tri in indices.as_chunks::<3>().0 {
            let [a, b, c] = tri.map(|i| i as usize + base);
            let _ = writeln!(s, "f {a}//{a} {b}//{b} {c}//{c}");
        }
        base += positions.len();
    }
    s
}
