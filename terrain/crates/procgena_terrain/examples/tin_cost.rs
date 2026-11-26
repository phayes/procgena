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
//! (default 4), round the origin, or with `--along-river` (or
//! `--along-road`) round the middle of the longest river (or road) of the
//! region at the origin; `--fixtures` uses the medium-scale fixtures on
//! their world, with rivers and roads on, instead of the shipped presets;
//! `--gradient <g>` checks open land's slope to `g` (rise over run) while
//! refining, which [`TinTolerance::GENERAL`] leaves unchecked (and which
//! samples every metre); `--level <name>` starts from a named tolerance
//! (`dense`, `general`, `low_poly` or `very_low_poly`; default `general`,
//! see [`TinTolerance::named`]);
//! `--repeats <n>` builds each chunk `n` times and reports the fastest
//! (default 3).
//!
//! The error is measured at points between the 1 m samples (every half
//! metre, a quarter metre off the samples), where neither mesh was fitted:
//! in height, and in slope (against the ground's slope from differences a
//! quarter metre either side, away from water's edges, where the ground
//! creases). Points within the tolerance's water band of a river or pond
//! count as near water. It also counts the 1 m lattice points the
//! canonical mesh misses by more than their tolerance, with the worst
//! excess: the mesh is built from only some of them. Times are per chunk,
//! on a world whose regions are already built: sampling is the 65 × 65
//! samples of one chunk, building is the whole canonical mesh, sampling
//! included. Reads are the ground heights each build asks for, and those
//! of the four edge contracts alone.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::time::Instant;

use glam::DVec2;
use procgena_terrain::tin::{
    CM_PER_M, ChunkEdgeKey, Cm, ContractCache, DomainTolerance, EdgeContract, ProbeAddr, TinChunk,
    TinTolerance, TinTriangle, build_chunk, build_chunk_with, sample_heights,
};
use procgena_terrain::{
    ChunkFeature, ChunkFeatureShape, ChunkKey, ChunkSource, Elevation, FeatureKind, FeatureTier,
    RegionId, SurfacePoint, TerrainPreset, WorldTerrain, WorldTerrainOptions, fixtures,
};

/// A finest-level chunk's width in centimetres.
const CHUNK_CM: i64 = 6_400;

/// Slope is not measured this close to a water's edge, in metres.
const CREASE_CLEARANCE: f64 = 1.5;

/// A world that counts the ground heights read from it.
struct Counting<'a> {
    world: &'a WorldTerrain,
    reads: AtomicUsize,
    /// Nanoseconds spent reading heights.
    nanos: AtomicUsize,
}

impl<'a> Counting<'a> {
    fn new(world: &'a WorldTerrain) -> Self {
        Self {
            world,
            reads: AtomicUsize::new(0),
            nanos: AtomicUsize::new(0),
        }
    }

    /// The heights read since the last call.
    fn take(&self) -> usize {
        self.reads.swap(0, AtomicOrdering::Relaxed)
    }

    /// The milliseconds spent reading heights since the last call.
    fn take_ms(&self) -> f64 {
        self.nanos.swap(0, AtomicOrdering::Relaxed) as f64 / 1e6
    }
}

impl ChunkSource for Counting<'_> {
    fn surface(&self, p: DVec2, tier: FeatureTier) -> SurfacePoint {
        self.reads.fetch_add(1, AtomicOrdering::Relaxed);
        let started = Instant::now();
        let s = self.world.surface(p, tier);
        self.nanos.fetch_add(
            started.elapsed().as_nanos() as usize,
            AtomicOrdering::Relaxed,
        );
        s
    }
    fn surfaces(&self, points: &[DVec2], tier: FeatureTier) -> Vec<SurfacePoint> {
        self.reads.fetch_add(points.len(), AtomicOrdering::Relaxed);
        let started = Instant::now();
        let s = self.world.surfaces(points, tier);
        self.nanos.fetch_add(
            started.elapsed().as_nanos() as usize,
            AtomicOrdering::Relaxed,
        );
        s
    }
    fn tier(&self, tier: FeatureTier) -> FeatureTier {
        self.world.tier(tier)
    }
    fn tier_difference(&self, min: DVec2, max: DVec2, from: FeatureTier) -> f32 {
        self.world.tier_difference(min, max, from)
    }
    fn lines_within(&self, min: DVec2, max: DVec2, tier: FeatureTier) -> Vec<ChunkFeature> {
        self.world.lines_within(min, max, tier)
    }
}

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
    let along_road = args.iter().any(|a| a == "--along-road");
    let use_fixtures = args.iter().any(|a| a == "--fixtures");
    let repeats: usize = option("--repeats").map_or(3, |s| s.parse().expect("--repeats <n>"));
    assert!(repeats > 0, "--repeats must be at least 1");
    let gradient: Option<f64> = option("--gradient").map(|s| s.parse().expect("--gradient <g>"));
    let level = option("--level").map_or(TinTolerance::GENERAL, |name| {
        TinTolerance::named(&name).expect("--level dense|general|low_poly|very_low_poly")
    });
    let tolerance = TinTolerance {
        general: DomainTolerance {
            gradient,
            ..level.general
        },
        ..level
    };
    println!(
        "seed {seed}, {n} × {n} chunks{}, tolerance {tolerance:?}",
        if along_river {
            " along a river"
        } else if along_road {
            " along a road"
        } else {
            ""
        },
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
    println!(
        "{:<16} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "", "reads", "edge rd", "edge ms", "1m over", "worst", "norm rd", "norm ms", "H* ms"
    );

    let presets: Vec<&TerrainPreset> = if use_fixtures {
        fixtures::all().collect()
    } else {
        TerrainPreset::all().collect()
    };
    for preset in presets {
        if only.as_deref().is_some_and(|key| key != preset.key) {
            continue;
        }
        let options = preset.terrain.clone();
        let source = move |_: RegionId, _: DVec2, _: &Elevation| options.clone();
        let world_options = if use_fixtures {
            fixtures::world_medium()
        } else {
            WorldTerrainOptions::default()
        };
        let world = WorldTerrain::new(
            WorldTerrainOptions {
                seed,
                ..world_options
            },
            Arc::new(source),
        )
        .expect("the world options are valid");

        // The block's middle chunk.
        let centre = if along_river || along_road {
            let Some(mid) = world
                .terrain(world.region_at(DVec2::ZERO))
                .ok()
                .and_then(|t| {
                    let lines = if along_river { t.rivers() } else { t.roads() };
                    lines
                        .iter()
                        .max_by_key(|r| r.points.len())
                        .map(|r| r.points[r.points.len() / 2])
                })
            else {
                println!(
                    "{:<16} no such line in the region at the origin",
                    preset.key
                );
                continue;
            };
            ((mid.x / 64.0).floor() as i64, (mid.y / 64.0).floor() as i64)
        } else {
            (0, 0)
        };

        let mut stats = Stats {
            repeats,
            ..Stats::default()
        };
        for z in -n / 2..n - n / 2 {
            for x in -n / 2..n - n / 2 {
                let key = ChunkKey::new(centre.0 + x, centre.1 + z, 0);
                if let Err(why) = stats.measure(&world, key, &tolerance) {
                    println!("{:<16} chunk {key:?}: {why}", preset.key);
                }
            }
        }
        stats.print(&preset.key);

        // The block again, all through one contract cache, so neighbours
        // sample their shared edges once.
        let counting = Counting::new(&world);
        let cache = ContractCache::new(&tolerance);
        let mut built = 0;
        for z in -n / 2..n - n / 2 {
            for x in -n / 2..n - n / 2 {
                let key = ChunkKey::new(centre.0 + x, centre.1 + z, 0);
                built += usize::from(build_chunk_with(&counting, key, &tolerance, &cache).is_ok());
            }
        }
        println!(
            "{:<16} {:>8.0} reads per chunk with a contract cache over the block",
            "",
            counting.take() as f64 / built.max(1) as f64
        );
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
    /// Heights read building each chunk, its four edge contracts included.
    reads: Vec<f64>,
    /// Heights read building each chunk's four edge contracts alone.
    edge_reads: Vec<f64>,
    /// Of each chunk's fastest build, the milliseconds spent reading
    /// heights.
    read_ms: Vec<f64>,
    /// Heights read for each chunk's shading normals, and the time taken.
    normal_reads: Vec<f64>,
    normal_ms: Vec<f64>,
    /// Time building each chunk's four edge contracts alone.
    edge_ms: Vec<f64>,
    /// How many times each chunk is built; the fastest build is reported.
    repeats: usize,
    /// How many 1 m samples the mesh misses by more than their tolerance.
    over: usize,
    /// The most any of them is over, in millimetres (to the centimetre).
    over_worst: f64,
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
        // The whole mesh, then its edge contracts alone: the fastest of a
        // few builds, and the heights each reads.
        let counting = Counting::new(world);
        let mut mesh = None;
        let (mut fastest, mut fastest_reading) = (f64::INFINITY, 0.0);
        counting.take_ms();
        for _ in 0..self.repeats {
            let started = Instant::now();
            mesh = Some(build_chunk(&counting, key, tolerance).map_err(|e| e.to_string())?);
            let (took, reading) = (started.elapsed().as_secs_f64() * 1000.0, counting.take_ms());
            if took < fastest {
                (fastest, fastest_reading) = (took, reading);
            }
        }
        let mesh = mesh.expect("built at least once");
        self.build_ms.push(fastest);
        self.read_ms.push(fastest_reading);
        self.reads.push((counting.take() / self.repeats) as f64);
        let started = Instant::now();
        mesh.shading_normals(&counting).map_err(|e| e.to_string())?;
        self.normal_ms
            .push(started.elapsed().as_secs_f64() * 1000.0);
        self.normal_reads.push(counting.take() as f64);
        let mut fastest = f64::INFINITY;
        for _ in 0..self.repeats {
            let started = Instant::now();
            for edge in ChunkEdgeKey::of_chunk(key) {
                EdgeContract::build(&counting, edge, tolerance).map_err(|e| e.to_string())?;
            }
            fastest = fastest.min(started.elapsed().as_secs_f64() * 1000.0);
        }
        self.edge_ms.push(fastest);
        self.edge_reads
            .push((counting.take() / self.repeats) as f64);
        self.triangles.push(mesh.triangles.len() as f64);
        self.vertices.push(mesh.vertices.len() as f64);
        self.unmet += mesh.unmet.len();
        if !mesh.dropped.is_empty() {
            println!("chunk {key:?} dropped {:?}", mesh.dropped);
        }

        // Errors between the samples, for both meshes, in millimetres (the
        // meshes and points are in centimetres).
        let water: Vec<ChunkFeature> = world
            .lines_within(key.min_corner(), key.max_corner(), FeatureTier::Fine)
            .into_iter()
            .filter(|f| f.id.kind != FeatureKind::Road)
            .collect();
        let (points, truth, slopes) = between(world, key);
        let grid: Vec<f64> = heights.iter().map(|h| f64::from(h.0)).collect();
        for ((&p, &t), &ground_slope) in points.iter().zip(&truth).zip(&slopes) {
            let at = DVec2::new(p.0 as f64, p.1 as f64) / CM_PER_M;
            let gap = water_gap(&water, at);
            let near = usize::from(gap < tolerance.water_band);
            let (height, slope) = tin_at(&mesh, p);
            self.tin[near].push((height - t).abs() * 10.0);
            self.grid[near].push((grid_height(&grid, key, p) - t).abs() * 10.0);
            if gap.abs() >= CREASE_CLEARANCE {
                self.slope[near].push((slope - ground_slope).length());
            }
        }

        // The 1 m samples the mesh misses by more than their tolerance
        // (plus a centimetre of rounding).
        for (&p, h) in lattice.iter().zip(&heights) {
            let gap = water_gap(&water, p.to_m());
            let limit = if gap < tolerance.water_band {
                tolerance.water.height
            } else {
                tolerance.general.height
            } * CM_PER_M
                + 1.0;
            let miss = (tin_at(&mesh, (p.x.0, p.z.0)).0 - f64::from(h.0)).abs();
            if miss > limit {
                let over_mm = (miss - limit) * 10.0;
                if std::env::var_os("TIN_COST_DEBUG").is_some() {
                    eprintln!(
                        "over {key:?} at ({}, {}) cm by {over_mm:.0} mm",
                        p.x.0 - key.x * CHUNK_CM,
                        p.z.0 - key.z * CHUNK_CM,
                    );
                }
                self.over += 1;
                self.over_worst = self.over_worst.max(over_mm);
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
        println!(
            "{:<16} {:>8.0} {:>8.0} {:>8.2} {:>8} {:>8.0} {:>8.0} {:>8.2} {:>8.2}",
            "",
            mean(&self.reads),
            mean(&self.edge_reads),
            mean(&self.edge_ms),
            self.over,
            self.over_worst,
            mean(&self.normal_reads),
            mean(&self.normal_ms),
            mean(&self.read_ms),
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
                Cm(key.x * CHUNK_CM + col * 100),
                Cm(key.z * CHUNK_CM + row * 100),
            )
        })
        .collect()
}

/// Points between the samples of chunk `key` (centimetres), the ground
/// there (centimetres, unrounded), and its slope (rise over run) from
/// differences a quarter metre either side.
fn between(world: &WorldTerrain, key: ChunkKey) -> (Vec<(i64, i64)>, Vec<f64>, Vec<DVec2>) {
    let points: Vec<(i64, i64)> = (0..128 * 128)
        .map(|i| {
            let (col, row) = (i % 128, i / 128);
            (
                key.x * CHUNK_CM + 25 + col * 50,
                key.z * CHUNK_CM + 25 + row * 50,
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
            let p = DVec2::new(x as f64, z as f64) / CM_PER_M;
            offsets.map(|o| p + o)
        })
        .collect();
    let h: Vec<f64> = world
        .surfaces(&at, FeatureTier::Fine)
        .iter()
        .map(|s| f64::from(s.height))
        .collect();
    let truth = h.chunks(5).map(|s| s[0] * CM_PER_M).collect();
    let slopes = h
        .chunks(5)
        .map(|s| DVec2::new(s[1] - s[2], s[3] - s[4]) / (2.0 * step))
        .collect();
    (points, truth, slopes)
}

/// The height (centimetres) and slope (rise over run) of `mesh` at `p`
/// (centimetres).
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

/// The height at `p` (centimetres) of the regular 1 m mesh through
/// `heights` (65 × 65, row by row), each square split corner to corner.
fn grid_height(heights: &[f64], key: ChunkKey, p: (i64, i64)) -> f64 {
    let (fx, fz) = (
        (p.0 - key.x * CHUNK_CM) as f64 / CM_PER_M,
        (p.1 - key.z * CHUNK_CM) as f64 / CM_PER_M,
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
