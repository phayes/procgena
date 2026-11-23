//! SPIKE (Phase 2.8, step 1): throwaway measurements for river valleys and
//! analytic stream-power erosion. Test-only and ignored; delete after the
//! spike is reviewed. Run with
//! `cargo test -p procgena_terrain --release --lib spike_erosion -- --ignored --nocapture`.

use glam::DVec2;

use crate::grid::Grid;
use crate::region::RegionId;
use crate::stitch::{RegionSpec, StitchedWorld};
use crate::{Convergence, FeatureLine, Terrain, TerrainPreset};

/// The concavity of stream-power profiles, m / n: river slope falls as
/// drainage area to this power. 0.45 is the usual value for real rivers.
const CONCAVITY: f64 = 0.45;

/// A world of one region, 4 km square, with `preset` and `seed`.
fn one_region(preset: TerrainPreset, seed: u32) -> StitchedWorld {
    let mut options = preset.options();
    options.seed = seed;
    StitchedWorld::build(
        vec![RegionSpec {
            id: RegionId(1),
            polygon: vec![
                DVec2::new(-2000.0, -2000.0),
                DVec2::new(2000.0, -2000.0),
                DVec2::new(2000.0, 2000.0),
                DVec2::new(-2000.0, 2000.0),
            ],
            options,
        }],
        &Convergence::default(),
    )
    .expect("one-region world builds")
}

/// The region's polygon.
fn polygon(t: &Terrain) -> Vec<DVec2> {
    match &t.region().boundary {
        crate::region::Boundary::Polygon(p) => p.clone(),
        crate::region::Boundary::Unbounded => Vec::new(),
    }
}

/// Drainage area per cell, in square metres: each cell drains to its
/// steepest lower neighbour, and passes on everything that drains into it.
/// Cells in hollows keep what reaches them.
fn drainage_area(grid: &Grid) -> Vec<f64> {
    let n = grid.heights.len();
    let cell_area = grid.cell * grid.cell;
    let mut area: Vec<f64> = (0..n)
        .map(|i| if grid.inside[i] { cell_area } else { 0.0 })
        .collect();
    let mut order: Vec<usize> = (0..n).filter(|&i| grid.inside[i]).collect();
    order.sort_by(|&a, &b| grid.heights[b].total_cmp(&grid.heights[a]));
    for &c in &order {
        let receiver = grid
            .neighbours(c)
            .map(|(j, d)| (j, f64::from(grid.heights[c] - grid.heights[j]) / d))
            .filter(|&(_, drop)| drop > 0.0)
            .max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((j, _)) = receiver {
            area[j] += area[c];
        }
    }
    area
}

/// The steady-state stream-power profile along `river` (source first),
/// fixed at the river's last level and scaled to the same total drop as
/// its current levels: the slope at each step falls as `area^-CONCAVITY`.
fn stream_power(river: &FeatureLine, areas: &[f64]) -> Vec<f64> {
    let last = river.points.len() - 1;
    // Unscaled: the rise from the end to each point.
    let mut rise = vec![0.0f64; river.points.len()];
    for i in (0..last).rev() {
        let ds = river.points[i].distance(river.points[i + 1]);
        let a = areas[i].max(1.0);
        rise[i] = rise[i + 1] + a.powf(-CONCAVITY) * ds;
    }
    let drop = f64::from(river.levels[0] - river.levels[last]);
    let scale = if rise[0] > 0.0 { drop / rise[0] } else { 0.0 };
    rise.iter()
        .map(|r| f64::from(river.levels[last]) + scale * r)
        .collect()
}

/// How concave a profile is: its mean slope over the upper third of the
/// river divided by its mean slope over the lower third (above 1: steep at
/// the source and flattening downstream).
fn concavity(points: &[DVec2], levels: &[f64]) -> f64 {
    let mut along = vec![0.0f64; points.len()];
    for i in 1..points.len() {
        along[i] = along[i - 1] + points[i].distance(points[i - 1]);
    }
    let total = along[along.len() - 1];
    let at = |s: f64| {
        along
            .iter()
            .position(|&a| a >= s)
            .unwrap_or(along.len() - 1)
    };
    let (a, b, c) = (at(total / 3.0), at(2.0 * total / 3.0), along.len() - 1);
    let upper = (levels[0] - levels[a]) / along[a].max(1.0);
    let lower = (levels[b] - levels[c]) / (along[c] - along[b]).max(1.0);
    upper / lower.max(1e-6)
}

#[test]
#[ignore]
fn spike_stream_power_profiles() {
    for (name, preset) in [
        ("rolling hills", TerrainPreset::RollingHills),
        ("mountains", TerrainPreset::Mountains),
    ] {
        for seed in [1, 2, 3] {
            let world = one_region(preset, seed);
            let t = &world.terrains()[0];
            let started = std::time::Instant::now();
            let grid = Grid::new(&polygon(t), |p| t.uncarved_height(p));
            let areas_grid = drainage_area(&grid);
            let cost = started.elapsed();

            // The longest river.
            let Some(river) = t.rivers().iter().max_by_key(|r| r.points.len()) else {
                eprintln!("{name}, seed {seed}: no rivers");
                continue;
            };
            let areas: Vec<f64> = river
                .points
                .iter()
                .map(|&p| grid.cell_of(p).map_or(0.0, |c| areas_grid[c]))
                .collect();
            let profile = stream_power(river, &areas);
            let current: Vec<f64> = river.levels.iter().map(|&l| f64::from(l)).collect();
            let ground: Vec<f64> = river
                .points
                .iter()
                .map(|&p| f64::from(t.uncarved_height(p)))
                .collect();
            let deepest = |levels: &[f64]| {
                ground
                    .iter()
                    .zip(levels)
                    .map(|(g, l)| g - l)
                    .fold(f64::NEG_INFINITY, f64::max)
            };
            let highest = |levels: &[f64]| {
                ground
                    .iter()
                    .zip(levels)
                    .map(|(g, l)| l - g)
                    .fold(f64::NEG_INFINITY, f64::max)
            };
            eprintln!(
                "{name}, seed {seed}: {} rivers; longest {} points, drop {:.0} m, area {:.2} → {:.2} km²; \
                 drainage {:.0} ms for {} cells",
                t.rivers().len(),
                river.points.len(),
                current[0] - current[current.len() - 1],
                areas[0] / 1e6,
                areas[areas.len() - 1] / 1e6,
                cost.as_secs_f64() * 1000.0,
                grid.heights.len(),
            );
            eprintln!(
                "  concavity (upper/lower slope): current {:.2}, stream power {:.2}; \
                 deepest cut below ground: current {:.1} m, stream power {:.1} m; \
                 highest above ground: current {:.1} m, stream power {:.1} m",
                concavity(&river.points, &current),
                concavity(&river.points, &profile),
                deepest(&current),
                deepest(&profile),
                highest(&current),
                highest(&profile),
            );
            for k in 0..=6 {
                let i = k * (river.points.len() - 1) / 6;
                eprintln!(
                    "    point {i:4}: area {:7.3} km², ground {:6.1}, current {:6.1}, stream power {:6.1}",
                    areas[i] / 1e6,
                    ground[i],
                    current[i],
                    profile[i]
                );
            }
        }
    }
}

#[test]
#[ignore]
fn spike_river_cut_depths() {
    // How deep rivers cut below the natural ground today, over all their
    // points: where valleys would matter.
    for (name, preset) in [
        ("plains", TerrainPreset::Plains),
        ("rolling hills", TerrainPreset::RollingHills),
        ("mountains", TerrainPreset::Mountains),
    ] {
        let mut cuts: Vec<f64> = Vec::new();
        for seed in 1..=6 {
            let world = one_region(preset, seed);
            let t = &world.terrains()[0];
            for river in t.rivers() {
                for (&p, &l) in river.points.iter().zip(&river.levels) {
                    cuts.push(f64::from(t.uncarved_height(p) - l));
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        if cuts.is_empty() {
            eprintln!("{name}: no rivers");
            continue;
        }
        let q = |f: f64| cuts[((cuts.len() - 1) as f64 * f) as usize];
        let over =
            |d: f64| cuts.iter().filter(|&&c| c > d).count() as f64 / cuts.len() as f64 * 100.0;
        eprintln!(
            "{name}: {} river points; cut below ground median {:.1} m, 90% {:.1} m, 99% {:.1} m, max {:.1} m; \
             over 3 m {:.1}%, over 10 m {:.1}%, over 20 m {:.1}%",
            cuts.len(),
            q(0.5),
            q(0.9),
            q(0.99),
            cuts[cuts.len() - 1],
            over(3.0),
            over(10.0),
            over(20.0),
        );
    }
}

#[test]
#[ignore]
fn spike_what_makes_deep_cuts() {
    // For mountain rivers: at each point cut more than 10 m below the
    // natural ground, is the ground rising downstream (the river breaches a
    // ridge, a placement problem), or falling more slowly than the river
    // already fell (the river dropped too early, a profile problem)? And
    // where along the river do deep cuts sit?
    const LOOK: f64 = 100.0;
    let (mut total, mut deep, mut ridge, mut lagging) = (0usize, 0usize, 0usize, 0usize);
    let mut by_third = [0usize; 3];
    let mut deep_slopes: Vec<f64> = Vec::new();
    let mut all_slopes: Vec<f64> = Vec::new();
    for seed in 1..=6 {
        let world = one_region(TerrainPreset::Mountains, seed);
        let t = &world.terrains()[0];
        for river in t.rivers() {
            let along = crate::road::distance_along(&river.points);
            let length = along[along.len() - 1];
            if length < 3.0 * LOOK {
                continue;
            }
            let ground: Vec<f64> = river
                .points
                .iter()
                .map(|&p| f64::from(t.uncarved_height(p)))
                .collect();
            for i in 0..river.points.len() {
                // The point LOOK metres upstream (or the source).
                let j = (0..=i)
                    .rev()
                    .find(|&j| along[i] - along[j] >= LOOK)
                    .unwrap_or(0);
                let run = (along[i] - along[j]).max(1.0);
                let river_fall = (f64::from(river.levels[j]) - f64::from(river.levels[i])) / run;
                total += 1;
                all_slopes.push(river_fall);
                let cut = ground[i] - f64::from(river.levels[i]);
                if cut <= 10.0 {
                    continue;
                }
                deep += 1;
                deep_slopes.push(river_fall);
                by_third[((along[i] / length * 3.0) as usize).min(2)] += 1;
                if ground[i] > ground[j] {
                    ridge += 1;
                } else {
                    lagging += 1;
                }
            }
        }
    }
    let median = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    eprintln!(
        "mountains: {total} river points, {deep} cut over 10 m ({:.0}%)",
        deep as f64 / total as f64 * 100.0
    );
    eprintln!(
        "  of those: ground rising downstream over {LOOK} m (ridge breach) {ridge} ({:.0}%), \
         ground falling (river ahead of the ground) {lagging} ({:.0}%)",
        ridge as f64 / deep as f64 * 100.0,
        lagging as f64 / deep as f64 * 100.0
    );
    eprintln!(
        "  along the river: first third {}, middle {}, last {}",
        by_third[0], by_third[1], by_third[2]
    );
    eprintln!(
        "  river fall over {LOOK} m: median {:.4} at deep cuts, {:.4} everywhere",
        median(&mut deep_slopes),
        median(&mut all_slopes)
    );
}
