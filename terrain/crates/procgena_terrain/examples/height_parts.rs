//! Times what one terrain height read costs, layer by layer, on one thread
//! with rivers and roads switched off, and checks whether border blending
//! changes anything where both regions of a border share their settings.
//!
//! ```bash
//! cargo run --release -p procgena_terrain --example height_parts
//! ```
//!
//! Options: `--preset <key>` one preset everywhere (default: the viewer's
//! mix by elevation), `--seed <n>` (default 1), `--step <m>` grid spacing
//! of the probe points (default 8), `--features` keeps rivers and roads.
//!
//! For the region at the origin, it reads every probe point inside the
//! region five ways, each adding one layer to the one before:
//!
//! 1. the relief and E alone, from a fresh generator with the region's
//!    settings (no convergence);
//! 2. the converged relief (`natural_height`): adds the fade towards the
//!    neighbours' relief;
//! 3. the uncarved height: adds the border pulls towards the agreed
//!    profiles;
//! 4. the height: adds carving (rivers, ponds, roads);
//! 5. the world's height: adds finding the region that owns the point.
//!
//! It prints nanoseconds per point for each, for all points, for points
//! the border touches (where layers 2 or 3 change the height) and for the
//! rest; then the same for a query with slope and with measures. Last, for
//! the regions round the origin, how many border edges join two regions
//! with equal settings, and the largest height change the border makes on
//! points next to those edges.

use std::sync::Arc;
use std::time::Instant;

use glam::DVec2;
use procgena_terrain::{
    Boundary, Elevation, FeatureTier, RegionId, SampleDetail, Terrain, TerrainGenerator,
    TerrainOptions, TerrainPreset, WorldTerrain, WorldTerrainOptions,
};

fn main() {
    // Options.
    let args: Vec<String> = std::env::args().collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let preset = value("--preset").map(|key| {
        TerrainPreset::named(&key)
            .unwrap_or_else(|| panic!("no terrain preset called {key:?}"))
            .terrain
            .clone()
    });
    let seed: u32 = value("--seed").map_or(1, |s| s.parse().expect("--seed needs a number"));
    let step: f64 = value("--step").map_or(8.0, |s| s.parse().expect("--step needs metres"));
    let features = args.iter().any(|a| a == "--features");

    // The world, as the viewer builds it, with rivers and roads off.
    let source = move |_: RegionId, site: DVec2, elevation: &Elevation| {
        let mut options: TerrainOptions =
            preset
                .clone()
                .unwrap_or_else(|| match elevation.height(site) {
                    h if h < 0.0 => TerrainPreset::plains().terrain.clone(),
                    h if h < 150.0 => TerrainPreset::rolling_hills().terrain.clone(),
                    _ => TerrainPreset::mountains().terrain.clone(),
                });
        if !features {
            options.features.roads.enabled = false;
            options.features.rivers.density = 0.0;
        }
        options
    };
    let world = WorldTerrain::new(
        WorldTerrainOptions {
            seed,
            ..WorldTerrainOptions::default()
        },
        Arc::new(source),
    )
    .expect("a valid world");

    // The centre region, built at the rough tier (the only one that
    // differs is roads, which are off), and its probe points.
    let centre = world.region_at(DVec2::ZERO);
    let started = Instant::now();
    let terrain = world
        .terrain_tier(centre, FeatureTier::Rough)
        .expect("the centre region builds");
    println!(
        "centre region {} built (with its ring of plans) in {:.2} s",
        centre.0,
        started.elapsed().as_secs_f64()
    );
    let points = probe_points(&terrain, step);
    let fresh = TerrainGenerator::new(terrain.generator().options().clone())
        .expect("valid settings")
        .with_elevation(world.elevation().clone());

    // Each layer's heights, to tell which points the border touches.
    let relief: Vec<f32> = points.iter().map(|&p| fresh.natural_height(p)).collect();
    let faded: Vec<f32> = points
        .iter()
        .map(|&p| terrain.generator().natural_height(p))
        .collect();
    let bent: Vec<f32> = points.iter().map(|&p| terrain.uncarved_height(p)).collect();
    let touched: Vec<bool> = (0..points.len())
        .map(|i| relief[i] != faded[i] || faded[i] != bent[i])
        .collect();
    let count = touched.iter().filter(|&&t| t).count();
    println!(
        "{} probe points every {step} m, {count} ({:.1}%) changed by the border",
        points.len(),
        100.0 * count as f64 / points.len() as f64
    );

    // Time each way of reading, best of three, per point.
    println!("ns per point: all / border-touched / untouched");
    let ways: [(&str, &dyn Fn(DVec2) -> f32); 8] = [
        ("relief and E only", &|p| fresh.natural_height(p)),
        ("+ convergence fade", &|p| {
            terrain.generator().natural_height(p)
        }),
        ("+ border pulls", &|p| terrain.uncarved_height(p)),
        ("+ carving (height)", &|p| terrain.height(p)),
        ("+ owner lookup (world)", &|p| world.height(p)),
        ("query with slope", &|p| {
            terrain.query(p, SampleDetail::Gradient).height
        }),
        ("query with measures", &|p| {
            terrain.query(p, SampleDetail::Measures).height
        }),
        ("query with labels", &|p| {
            terrain.query(p, SampleDetail::Labels).height
        }),
    ];
    for (name, read) in ways {
        let time = |keep: bool| {
            let chosen: Vec<DVec2> = points
                .iter()
                .zip(&touched)
                .filter(|&(_, &t)| t == keep)
                .map(|(&p, _)| p)
                .collect();
            per_point(&chosen, read)
        };
        println!(
            "  {name:<24} {:>8.0} {:>8.0} {:>8.0}",
            per_point(&points, read),
            time(true),
            time(false)
        );
    }

    homogeneous_edges(&world, centre);
}

/// The points of a grid `step` metres apart that lie inside `terrain`'s
/// region.
fn probe_points(terrain: &Terrain, step: f64) -> Vec<DVec2> {
    let Boundary::Polygon(polygon) = &terrain.region().boundary else {
        panic!("the centre region has a polygon");
    };
    let (min, max) = polygon.iter().fold(
        (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
        |(lo, hi), &p| (lo.min(p), hi.max(p)),
    );
    let (nx, nz) = (
        ((max.x - min.x) / step) as i64,
        ((max.y - min.y) / step) as i64,
    );
    (0..=nz)
        .flat_map(|j| (0..=nx).map(move |i| min + DVec2::new(i as f64, j as f64) * step))
        .filter(|&p| terrain.region().boundary.contains(p))
        .collect()
}

/// Nanoseconds per point to read every point of `points`, best of three.
fn per_point(points: &[DVec2], read: &dyn Fn(DVec2) -> f32) -> f64 {
    if points.is_empty() {
        return f64::NAN;
    }
    let mut best = f64::INFINITY;
    for _ in 0..3 {
        let started = Instant::now();
        let mut sum = 0.0f32;
        for &p in points {
            sum += read(p);
        }
        std::hint::black_box(sum);
        best = best.min(started.elapsed().as_secs_f64());
    }
    best * 1e9 / points.len() as f64
}

/// Prints how many border edges of the regions round `centre` join two
/// regions with equal settings, and for those, the largest height change
/// the border makes at points along the edge, a metre inside each side.
fn homogeneous_edges(world: &WorldTerrain, centre: RegionId) {
    let mut regions = vec![centre];
    regions.extend(world.region(centre).edges.iter().filter_map(|e| e.neighbor));
    let (mut edges, mut equal, mut largest) = (0, 0, 0.0f32);
    let mut seen = std::collections::BTreeSet::new();
    for &id in &regions {
        let Ok(terrain) = world.terrain_tier(id, FeatureTier::Rough) else {
            continue;
        };
        for edge in &world.region(id).edges {
            let Some(other) = edge.neighbor else { continue };
            if !seen.insert((id.0.min(other.0), id.0.max(other.0))) {
                continue;
            }
            let Ok(neighbour) = world.terrain_tier(other, FeatureTier::Rough) else {
                continue;
            };
            edges += 1;
            if terrain.generator().options() != neighbour.generator().options() {
                continue;
            }
            equal += 1;
            let fresh = TerrainGenerator::new(terrain.generator().options().clone())
                .expect("valid settings")
                .with_elevation(world.elevation().clone());
            for w in edge.polyline.windows(2) {
                for k in 0..8 {
                    let t = f64::from(k) / 8.0;
                    let on = w[0] + (w[1] - w[0]) * t;
                    let normal = (w[1] - w[0]).perp().normalize_or_zero();
                    for side in [-1.0, 1.0, -20.0, 20.0] {
                        let p = on + normal * side;
                        let owner = world.region_at(p);
                        let Ok(t) = world.terrain_tier(owner, FeatureTier::Rough) else {
                            continue;
                        };
                        largest =
                            largest.max((t.uncarved_height(p) - fresh.natural_height(p)).abs());
                    }
                }
            }
        }
    }
    println!(
        "border edges round the centre: {edges}, of which {equal} join equal settings; \
         largest height change by the border along those: {largest:.4} m"
    );
}
