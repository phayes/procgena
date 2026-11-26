//! Measures canonical chunk meshes against the regular 1 m chunk mesh:
//! triangles per chunk, how far each misses the ground between samples,
//! on open land and near rivers and ponds, and what building one costs, on
//! the shipped presets and the default world.
//!
//! ```bash
//! cargo run --release -p procgena_terrain --example tin_cost
//! ```
//!
//! Options: `--preset <key>` (default: every shipped preset), `--seed <n>`
//! world seed (default 1), `--chunks <n>` an n × n block of chunks
//! (default 4), round the origin, or with `--along-river` round the middle
//! of the longest river of the region at the origin; `--gradient <g>`
//! checks open land's slope to `g` (rise over run) while refining, which
//! [`TinTolerance::GENERAL`] leaves unchecked.
//!
//! The error is measured at points between the 1 m samples (every half
//! metre, a quarter metre off the samples), where neither mesh was fitted:
//! in height, and in slope (against the ground's slope from differences a
//! quarter metre either side, away from water's edges, where the ground
//! creases). Points within the tolerance's water band of a river or pond
//! count as near water. Times are per chunk, on a world whose regions are
//! already built: sampling is the 65 × 65 samples of one chunk, building
//! is the whole canonical mesh, sampling included.

use std::sync::Arc;
use std::time::Instant;

use glam::DVec2;
use procgena_terrain::tin::{
    DomainTolerance, Mm, ProbeAddr, TinChunk, TinTolerance, TinTriangle, build_chunk,
    sample_heights,
};
use procgena_terrain::{
    ChunkFeature, ChunkFeatureShape, ChunkKey, ChunkSource, Elevation, FeatureKind, FeatureTier,
    RegionId, TerrainPreset, WorldTerrain, WorldTerrainOptions,
};

/// Slope is not measured this close to a water's edge, in metres.
const CREASE_CLEARANCE: f64 = 1.5;

fn main() {
    // Options.
    let args: Vec<String> = std::env::args().collect();
    let option = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let only = option("--preset");
    let seed: u32 = option("--seed").map_or(1, |s| s.parse().expect("--seed <n>"));
    let n: i64 = option("--chunks").map_or(4, |s| s.parse().expect("--chunks <n>"));
    let along_river = args.iter().any(|a| a == "--along-river");
    let gradient: Option<f64> = option("--gradient").map(|s| s.parse().expect("--gradient <g>"));
    let tolerance = TinTolerance {
        general: DomainTolerance {
            gradient,
            ..TinTolerance::GENERAL.general
        },
        ..TinTolerance::GENERAL
    };
    println!(
        "seed {seed}, {n} × {n} chunks{}, tolerance {tolerance:?}, rayon {}",
        if along_river { " along a river" } else { "" },
        cfg!(feature = "rayon")
    );
    println!(
        "{:<16} {:>6} {:>6} {:>5} {:>18} {:>25} {:>13} {:>18} {:>18} {:>7} {:>7}",
        "preset",
        "tris",
        "verts",
        "unmet",
        "land p50/p95/max",
        "water p50/p95/max (n)",
        "slope p95 l/w",
        "grid land",
        "grid water",
        "samp ms",
        "build ms"
    );

    for preset in TerrainPreset::all() {
        if only.as_deref().is_some_and(|key| key != preset.key) {
            continue;
        }
        let options = preset.terrain.clone();
        let source = move |_: RegionId, _: DVec2, _: &Elevation| options.clone();
        let world = WorldTerrain::new(
            WorldTerrainOptions {
                seed,
                ..WorldTerrainOptions::default()
            },
            Arc::new(source),
        )
        .expect("the default world options are valid");

        // The block's middle chunk.
        let centre = if along_river {
            let Some(mid) = world
                .terrain(world.region_at(DVec2::ZERO))
                .ok()
                .and_then(|t| {
                    t.rivers()
                        .iter()
                        .max_by_key(|r| r.points.len())
                        .map(|r| r.points[r.points.len() / 2])
                })
            else {
                println!("{:<16} no river in the region at the origin", preset.key);
                continue;
            };
            ((mid.x / 64.0).floor() as i64, (mid.y / 64.0).floor() as i64)
        } else {
            (0, 0)
        };

        let mut stats = Stats::default();
        for z in -n / 2..n - n / 2 {
            for x in -n / 2..n - n / 2 {
                let key = ChunkKey::new(centre.0 + x, centre.1 + z, 0);
                if let Err(why) = stats.measure(&world, key, &tolerance) {
                    println!("{:<16} chunk {key:?}: {why}", preset.key);
                }
            }
        }
        stats.print(&preset.key);
    }
    println!("(the regular 1 m chunk mesh has 8192 triangles and 4225 vertices; errors in mm)");
}

/// What [`Stats::measure`] gathers over a block of chunks.
#[derive(Default)]
struct Stats {
    triangles: Vec<f64>,
    vertices: Vec<f64>,
    unmet: usize,
    /// Height errors of the canonical mesh, on land and near water.
    tin: [Vec<f64>; 2],
    /// The same for the regular mesh.
    grid: [Vec<f64>; 2],
    /// Slope errors of the canonical mesh, on land and near water.
    slope: [Vec<f64>; 2],
    sample_ms: Vec<f64>,
    build_ms: Vec<f64>,
}

impl Stats {
    /// Builds chunk `key` of `world` both ways and measures it.
    fn measure(
        &mut self,
        world: &WorldTerrain,
        key: ChunkKey,
        tolerance: &TinTolerance,
    ) -> Result<(), String> {
        let lattice = lattice(key);
        // Build the regions first, so only the chunk is timed.
        sample_heights(world, &lattice).map_err(|e| e.to_string())?;
        let started = Instant::now();
        let heights = sample_heights(world, &lattice).map_err(|e| e.to_string())?;
        self.sample_ms
            .push(started.elapsed().as_secs_f64() * 1000.0);
        let started = Instant::now();
        let mesh = build_chunk(world, key, tolerance).map_err(|e| e.to_string())?;
        self.build_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        self.triangles.push(mesh.triangles.len() as f64);
        self.vertices.push(mesh.vertices.len() as f64);
        self.unmet += mesh.unmet.len();
        if !mesh.dropped.is_empty() {
            println!("chunk {key:?} dropped {:?}", mesh.dropped);
        }

        // Errors between the samples, for both meshes.
        let water: Vec<ChunkFeature> = world
            .lines_within(key.min_corner(), key.max_corner(), FeatureTier::Fine)
            .into_iter()
            .filter(|f| f.id.kind != FeatureKind::Road)
            .collect();
        let (points, truth, slopes) = between(world, key);
        let grid: Vec<f64> = heights.iter().map(|h| f64::from(h.0)).collect();
        for ((&p, &t), &ground_slope) in points.iter().zip(&truth).zip(&slopes) {
            let at = DVec2::new(p.0 as f64, p.1 as f64) / 1000.0;
            let gap = water_gap(&water, at);
            let near = usize::from(gap < tolerance.water_band);
            let (height, slope) = tin_at(&mesh, p);
            self.tin[near].push((height - t).abs());
            self.grid[near].push((grid_height(&grid, key, p) - t).abs());
            if gap.abs() >= CREASE_CLEARANCE {
                self.slope[near].push((slope - ground_slope).length());
            }
        }
        Ok(())
    }

    /// Prints one line for preset `key`.
    fn print(mut self, key: &str) {
        if self.triangles.is_empty() {
            return;
        }
        let at = |v: &[f64], q: f64| v[((v.len() - 1) as f64 * q) as usize];
        let spread = |v: &mut Vec<f64>| {
            if v.is_empty() {
                return "-".to_string();
            }
            v.sort_by(f64::total_cmp);
            format!("{:.0} / {:.0} / {:.0}", at(v, 0.5), at(v, 0.95), at(v, 1.0))
        };
        let p95 = |v: &mut Vec<f64>| {
            if v.is_empty() {
                return "-".to_string();
            }
            v.sort_by(f64::total_cmp);
            format!("{:.3}", at(v, 0.95))
        };
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        let near_water = self.tin[1].len();
        let [tin_land, tin_water] = &mut self.tin;
        let [grid_land, grid_water] = &mut self.grid;
        let [slope_land, slope_water] = &mut self.slope;
        println!(
            "{:<16} {:>6.0} {:>6.0} {:>5} {:>18} {:>25} {:>13} {:>18} {:>18} {:>7.2} {:>7.2}",
            key,
            mean(&self.triangles),
            mean(&self.vertices),
            self.unmet,
            spread(tin_land),
            format!("{} ({near_water})", spread(tin_water)),
            format!("{}/{}", p95(slope_land), p95(slope_water)),
            spread(grid_land),
            spread(grid_water),
            mean(&self.sample_ms),
            mean(&self.build_ms),
        );
    }
}

/// How far `p` is from the water's edge of the rivers and ponds among
/// `features`, in metres: negative in the water, infinite with none.
fn water_gap(features: &[ChunkFeature], p: DVec2) -> f64 {
    let mut gap = f64::INFINITY;
    for f in features {
        match &f.shape {
            ChunkFeatureShape::Line(line) => {
                for (k, w) in line.points.windows(2).enumerate() {
                    let hw = 0.5 * f64::from(line.widths[k].max(line.widths[k + 1]));
                    let ab = w[1] - w[0];
                    let t = ((p - w[0]).dot(ab) / ab.length_squared().max(1e-18)).clamp(0.0, 1.0);
                    gap = gap.min(p.distance(w[0] + ab * t) - hw);
                }
            }
            ChunkFeatureShape::Pond(pond) => gap = gap.min(p.distance(pond.centre) - pond.radius),
        }
    }
    gap
}

/// The 65 × 65 samples of chunk `key`, a metre apart.
fn lattice(key: ChunkKey) -> Vec<ProbeAddr> {
    (0..65 * 65)
        .map(|i| {
            let (col, row) = (i % 65, i / 65);
            ProbeAddr::new(
                Mm(key.x * 64_000 + col * 1000),
                Mm(key.z * 64_000 + row * 1000),
            )
        })
        .collect()
}

/// Points between the samples of chunk `key` (millimetres), the ground
/// there (millimetres, unrounded), and its slope (rise over run) from
/// differences a quarter metre either side.
fn between(world: &WorldTerrain, key: ChunkKey) -> (Vec<(i64, i64)>, Vec<f64>, Vec<DVec2>) {
    let points: Vec<(i64, i64)> = (0..128 * 128)
        .map(|i| {
            let (col, row) = (i % 128, i / 128);
            (
                key.x * 64_000 + 250 + col * 500,
                key.z * 64_000 + 250 + row * 500,
            )
        })
        .collect();
    let step = 0.25;
    let offsets = [
        DVec2::ZERO,
        DVec2::new(step, 0.0),
        DVec2::new(-step, 0.0),
        DVec2::new(0.0, step),
        DVec2::new(0.0, -step),
    ];
    let at: Vec<DVec2> = points
        .iter()
        .flat_map(|&(x, z)| {
            let p = DVec2::new(x as f64, z as f64) / 1000.0;
            offsets.map(|o| p + o)
        })
        .collect();
    let h: Vec<f64> = world
        .surfaces(&at, FeatureTier::Fine)
        .iter()
        .map(|s| f64::from(s.height))
        .collect();
    let truth = h.chunks(5).map(|s| s[0] * 1000.0).collect();
    let slopes = h
        .chunks(5)
        .map(|s| DVec2::new(s[1] - s[2], s[3] - s[4]) / (2.0 * step))
        .collect();
    (points, truth, slopes)
}

/// The height (millimetres) and slope (rise over run) of `mesh` at `p`
/// (millimetres).
fn tin_at(mesh: &TinChunk, p: (i64, i64)) -> (f64, DVec2) {
    mesh.triangles
        .iter()
        .find_map(|&TinTriangle([a, b, c])| {
            let [a, b, c] = [a, b, c].map(|i| mesh.vertices[i as usize]);
            let orient = |q: (i64, i64), r: (i64, i64), s: (i64, i64)| {
                -(((r.0 - q.0) * (s.1 - q.1) - (r.1 - q.1) * (s.0 - q.0)) as f64)
            };
            let (pa, pb, pc) = ((a.x.0, a.z.0), (b.x.0, b.z.0), (c.x.0, c.z.0));
            let (wa, wb, wc) = (orient(pb, pc, p), orient(pc, pa, p), orient(pa, pb, p));
            (wa >= 0.0 && wb >= 0.0 && wc >= 0.0).then(|| {
                let area = orient(pa, pb, pc);
                let height =
                    (wa * f64::from(a.h.0) + wb * f64::from(b.h.0) + wc * f64::from(c.h.0)) / area;
                // The plane's slope, from its corners.
                let (ux, uz, uh) = (
                    (b.x.0 - a.x.0) as f64,
                    (b.z.0 - a.z.0) as f64,
                    f64::from(b.h.0 - a.h.0),
                );
                let (vx, vz, vh) = (
                    (c.x.0 - a.x.0) as f64,
                    (c.z.0 - a.z.0) as f64,
                    f64::from(c.h.0 - a.h.0),
                );
                let det = ux * vz - uz * vx;
                let slope = DVec2::new((uh * vz - vh * uz) / det, (vh * ux - uh * vx) / det);
                (height, slope)
            })
        })
        .expect("the mesh covers its chunk")
}

/// The height at `p` (millimetres) of the regular 1 m mesh through
/// `heights` (65 × 65, row by row), each square split corner to corner.
fn grid_height(heights: &[f64], key: ChunkKey, p: (i64, i64)) -> f64 {
    let (fx, fz) = (
        (p.0 - key.x * 64_000) as f64 / 1000.0,
        (p.1 - key.z * 64_000) as f64 / 1000.0,
    );
    let (col, row) = (fx.floor().min(63.0) as usize, fz.floor().min(63.0) as usize);
    let (u, v) = (fx - col as f64, fz - row as f64);
    let h = |c: usize, r: usize| heights[r * 65 + c];
    if u >= v {
        h(col, row)
            + u * (h(col + 1, row) - h(col, row))
            + v * (h(col + 1, row + 1) - h(col + 1, row))
    } else {
        h(col, row)
            + v * (h(col, row + 1) - h(col, row))
            + u * (h(col + 1, row + 1) - h(col, row + 1))
    }
}
