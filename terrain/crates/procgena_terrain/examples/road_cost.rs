//! Measures what planning a region's roads costs, beside the rest of
//! building the region, on the shipped presets and the default world.
//!
//! ```bash
//! cargo run --release -p procgena_terrain --example road_cost
//! cargo run --release -p procgena_terrain --example road_cost -- --corpus --stats
//! ```
//!
//! Options:
//!
//! - `--preset <key>`: only this preset (default: every shipped preset).
//! - `--seeds <n>`: world seeds 1 to n (default 3).
//! - `--regions <n>`: regions timed per world, nearest the origin first
//!   (default 3).
//! - `--repeat <n>`: how many times each region's roads are planned again
//!   for the median (default 1).
//! - `--corpus`: the fixed benchmark corpus: every shipped preset, seeds 1
//!   to 3, 3 regions each (the defaults above, which it overrides).
//! - `--scale <class>`: the World scale class, `small`, `medium_small`
//!   (default), `medium`, `large` or `real_life`, with its region size,
//!   border warp and landscape stretch.
//! - `--stats`: per region, what the route search did (from
//!   `Terrain::road_stats`) and how good the roads are (`road_quality`
//!   against the region's ground before roads), then a summary.
//!
//! For each region it first builds the rough tier (the region's plan, its
//! neighbours' plans and its ground without roads), then times the fine
//! tier on its own, which today is the roads: routing, levels, and how far
//! they move the ground. It prints both times, the number of road lines
//! and their total length.

use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::DVec2;
use procgena_terrain::{
    BorderWarp, Elevation, FeatureTier, RegionGraphOptions, RegionId, RoadQuality, RouteStats,
    TerrainPreset, WorldScale, WorldTerrain, WorldTerrainOptions, road_quality,
};

/// One World scale class, as `procgena_world`'s presets set it.
struct ScaleClass {
    key: &'static str,
    /// Horizontal and vertical landscape stretch.
    horizontal: f64,
    vertical: f64,
    /// Region size, in metres.
    size: f64,
    /// Border warp amplitude and wavelength in metres, `None` for the
    /// default warp.
    warp: Option<(f64, f64)>,
}

/// The World scale classes.
const SCALES: [ScaleClass; 5] = [
    ScaleClass {
        key: "small",
        horizontal: 0.5,
        vertical: 0.5,
        size: 2000.0,
        warp: None,
    },
    ScaleClass {
        key: "medium_small",
        horizontal: 1.0,
        vertical: 1.0,
        size: 2000.0,
        warp: None,
    },
    ScaleClass {
        key: "medium",
        horizontal: 2.0,
        vertical: 2.0,
        size: 4000.0,
        warp: Some((100.0, 800.0)),
    },
    ScaleClass {
        key: "large",
        horizontal: 6.0,
        vertical: 4.0,
        size: 8000.0,
        warp: Some((100.0, 800.0)),
    },
    ScaleClass {
        key: "real_life",
        horizontal: 20.0,
        vertical: 8.0,
        size: 16_000.0,
        warp: Some((100.0, 800.0)),
    },
];

fn main() {
    // Options.
    let args: Vec<String> = std::env::args().collect();
    let option = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let flag = |name: &str| args.iter().any(|a| a == name);
    let corpus = flag("--corpus");
    let show_stats = flag("--stats");
    let only = if corpus { None } else { option("--preset") };
    let seeds: u32 = if corpus {
        3
    } else {
        option("--seeds").map_or(3, |s| s.parse().expect("--seeds <n>"))
    };
    let regions: usize = if corpus {
        3
    } else {
        option("--regions").map_or(3, |s| s.parse().expect("--regions <n>"))
    };
    let repeat: usize = option("--repeat").map_or(1, |s| s.parse().expect("--repeat <n>"));
    let scale_key = option("--scale").unwrap_or_else(|| "medium_small".to_owned());
    let &ScaleClass {
        horizontal,
        vertical,
        size,
        warp,
        ..
    } = SCALES
        .iter()
        .find(|s| s.key == scale_key)
        .unwrap_or_else(|| panic!("--scale: one of small, medium_small, medium, large, real_life"));
    println!(
        "scale {scale_key}, seeds 1..={seeds}, {regions} regions per world, repeat {repeat}, rayon {}",
        cfg!(feature = "rayon")
    );
    println!(
        "{:<16} {:>4} {:>8} {:>9} {:>9} {:>6} {:>8}",
        "preset", "seed", "region", "rough ms", "roads ms", "lines", "length m"
    );

    // Each preset over every seed.
    let mut all_roads = Vec::new();
    let mut summary = Summary::default();
    for preset in TerrainPreset::all() {
        if only.as_deref().is_some_and(|key| key != preset.key) {
            continue;
        }
        let mut preset_roads = Vec::new();
        for seed in 1..=seeds {
            let options = preset.terrain.clone();
            let source = move |_: RegionId, _: DVec2, _: &Elevation| options.clone();
            let world_options = WorldTerrainOptions {
                seed,
                scale: WorldScale {
                    horizontal,
                    vertical,
                },
                regions: RegionGraphOptions {
                    size,
                    warp: warp.map_or_else(BorderWarp::default, |(amplitude, wavelength)| {
                        BorderWarp {
                            amplitude,
                            wavelength,
                        }
                    }),
                    ..RegionGraphOptions::default()
                },
                ..WorldTerrainOptions::default()
            };
            let probe = WorldTerrain::new(world_options.clone(), Arc::new(source.clone()))
                .expect("the scale classes' world options are valid");

            // The region at the origin, then those around it nearest first.
            let centre = probe.region_at(DVec2::ZERO);
            let mut ids: Vec<RegionId> = probe
                .region(centre)
                .corners
                .iter()
                .flat_map(|c| c.incident.iter().copied())
                .filter(|&r| r != centre)
                .collect();
            ids.sort();
            ids.dedup();
            ids.sort_by(|&a, &b| probe.site(a).length().total_cmp(&probe.site(b).length()));
            ids.insert(0, centre);
            ids.truncate(regions);

            for (rank, id) in ids.into_iter().enumerate() {
                // A fresh world for each repeat, so nothing is kept.
                let mut rough = Vec::new();
                let mut roads = Vec::new();
                let mut lines = 0;
                let mut length = 0.0;
                let mut detail = None;
                for _ in 0..repeat {
                    let world = WorldTerrain::new(world_options.clone(), Arc::new(source.clone()))
                        .expect("the scale classes' world options are valid");
                    let started = Instant::now();
                    let Ok(before) = world.terrain_tier(id, FeatureTier::Rough) else {
                        break;
                    };
                    rough.push(started.elapsed());
                    let started = Instant::now();
                    let Ok(fine) = world.terrain(id) else {
                        break;
                    };
                    roads.push(started.elapsed());
                    lines = fine.roads().len();
                    length = fine
                        .roads()
                        .iter()
                        .map(|r| {
                            r.points
                                .windows(2)
                                .map(|w| w[0].distance(w[1]))
                                .sum::<f64>()
                        })
                        .sum();
                    if show_stats && detail.is_none() {
                        let quality: Vec<RoadQuality> = fine
                            .roads()
                            .iter()
                            .map(|r| road_quality(r, before.as_ref(), fine.rivers()))
                            .collect();
                        detail = Some((
                            fine.road_stats(FeatureTier::Mid).unwrap_or_default(),
                            fine.road_stats(FeatureTier::Fine).unwrap_or_default(),
                            quality,
                        ));
                    }
                }
                if roads.is_empty() {
                    println!("{:<16} {seed:>4} {rank:>8} could not be built", preset.key);
                    continue;
                }
                let roads_ms = median(&mut roads);
                println!(
                    "{:<16} {seed:>4} {rank:>8} {:>9.0} {:>9.0} {lines:>6} {length:>8.0}",
                    preset.key,
                    median(&mut rough),
                    roads_ms,
                );
                if let Some((outline, fine, quality)) = detail {
                    print_stats("outline", &outline);
                    print_stats("fine", &fine);
                    print_quality(&quality);
                    summary.add(&outline, &fine, &quality);
                }
                preset_roads.push(roads_ms);
            }
        }
        if !preset_roads.is_empty() {
            println!(
                "{:<16} roads median {:.0} ms, max {:.0} ms",
                preset.key,
                median_ms(&mut preset_roads.clone()),
                preset_roads.iter().copied().fold(0.0, f64::max)
            );
        }
        all_roads.extend(preset_roads);
    }
    if !all_roads.is_empty() {
        println!(
            "all presets: roads median {:.0} ms, max {:.0} ms over {} regions",
            median_ms(&mut all_roads.clone()),
            all_roads.iter().copied().fold(0.0, f64::max),
            all_roads.len()
        );
    }
    if show_stats {
        summary.print();
    }
}

/// Prints what one tier's route search did.
fn print_stats(label: &str, s: &RouteStats) {
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    println!(
        "    {label:<7} cells {} active {:.0}% starts {} unconnected {} searches {} \
         pushed {} popped {} refused {} crowded {}/{} peak {:.1} MB",
        s.grid_cells,
        100.0 * s.active_cells as f64 / s.grid_cells.max(1) as f64,
        s.starts,
        s.unconnected,
        s.searches,
        s.states_pushed,
        s.states_popped,
        s.refused_joins,
        s.crowded_retries,
        s.crowded_kept,
        s.peak_state_bytes as f64 / 1e6,
    );
    println!(
        "    {:<7} ms: grid {:.0} setup {:.0} field {:.0} search {:.0} levels {:.0} total {:.0}",
        "",
        ms(s.grid_time),
        ms(s.setup_time),
        ms(s.field_time),
        ms(s.search_time),
        ms(s.level_time),
        ms(s.total_time()),
    );
}

/// Prints the worst of each quality measure over a region's roads.
fn print_quality(quality: &[RoadQuality]) {
    let w = worst(quality);
    println!(
        "    quality max grade {:.3} p95 {:.3} min radius {:.1} m cut {:.1} m fill {:.1} m \
         crossings {} min angle {:.0}° crowded points {}",
        w.max_grade,
        w.p95_grade,
        w.min_radius,
        w.max_cut,
        w.max_fill,
        w.river_crossings,
        w.min_crossing_angle,
        w.crowded_points,
    );
}

/// The worst of each measure over `quality`; counts are summed.
fn worst(quality: &[RoadQuality]) -> RoadQuality {
    quality.iter().fold(
        RoadQuality {
            min_radius: f64::INFINITY,
            min_crossing_angle: 90.0,
            ..RoadQuality::default()
        },
        |w, q| RoadQuality {
            length: w.length + q.length,
            max_grade: w.max_grade.max(q.max_grade),
            p95_grade: w.p95_grade.max(q.p95_grade),
            min_radius: w.min_radius.min(q.min_radius),
            max_cut: w.max_cut.max(q.max_cut),
            max_fill: w.max_fill.max(q.max_fill),
            river_crossings: w.river_crossings + q.river_crossings,
            min_crossing_angle: w.min_crossing_angle.min(q.min_crossing_angle),
            crowded_points: w.crowded_points + q.crowded_points,
        },
    )
}

/// Totals over every region measured with `--stats`.
#[derive(Default)]
struct Summary {
    regions: usize,
    starts: usize,
    unconnected: usize,
    searches: usize,
    popped: u64,
    crowded_kept: usize,
    search_ms: Vec<f64>,
    total_ms: Vec<f64>,
    peak_bytes: usize,
    quality: Vec<RoadQuality>,
}

impl Summary {
    /// Adds one region's stats and road quality.
    fn add(&mut self, outline: &RouteStats, fine: &RouteStats, quality: &[RoadQuality]) {
        self.regions += 1;
        self.starts += fine.starts;
        self.unconnected += fine.unconnected;
        self.searches += outline.searches + fine.searches;
        self.popped += outline.states_popped + fine.states_popped;
        self.crowded_kept += outline.crowded_kept + fine.crowded_kept;
        self.search_ms
            .push((outline.search_time + fine.search_time).as_secs_f64() * 1000.0);
        self.total_ms
            .push((outline.total_time() + fine.total_time()).as_secs_f64() * 1000.0);
        self.peak_bytes = self
            .peak_bytes
            .max(outline.peak_state_bytes)
            .max(fine.peak_state_bytes);
        self.quality.push(worst(quality));
    }

    /// Prints the totals.
    fn print(&self) {
        if self.regions == 0 {
            return;
        }
        let w = worst(&self.quality);
        println!(
            "summary over {} regions: ports {}/{} connected, searches {}, states popped {}, \
             crowded kept {}, peak {:.1} MB",
            self.regions,
            self.starts - self.unconnected,
            self.starts,
            self.searches,
            self.popped,
            self.crowded_kept,
            self.peak_bytes as f64 / 1e6,
        );
        println!(
            "  routing ms (outline + fine): median {:.0} max {:.0}; search alone median {:.0} max {:.0}",
            median_ms(&mut self.total_ms.clone()),
            self.total_ms.iter().copied().fold(0.0, f64::max),
            median_ms(&mut self.search_ms.clone()),
            self.search_ms.iter().copied().fold(0.0, f64::max),
        );
        println!(
            "  worst quality: max grade {:.3} p95 {:.3} min radius {:.1} m cut {:.1} m fill {:.1} m \
             crossings {} min angle {:.0}° crowded points {}; road length {:.0} m",
            w.max_grade,
            w.p95_grade,
            w.min_radius,
            w.max_cut,
            w.max_fill,
            w.river_crossings,
            w.min_crossing_angle,
            w.crowded_points,
            w.length,
        );
    }
}

/// The median of `times`, in milliseconds.
fn median(times: &mut [Duration]) -> f64 {
    times.sort();
    times[times.len() / 2].as_secs_f64() * 1000.0
}

/// The median of `values`.
fn median_ms(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}
