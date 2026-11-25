//! Drainage area: how much ground drains through each place of a region,
//! and along each of its rivers, with the steady-state stream-power profile
//! it implies.
//!
//! Phase 2.8 measures with these and doesn't build anything on them yet:
//! sizing rivers, choosing river types and shaping profiles from drainage
//! area all change the approved look, and wait for the user (see
//! "River valleys and erosion" in `terrain/plan.md`). Run the measurements
//! with
//! `cargo test -p procgena_terrain --release --lib drainage -- --ignored --nocapture`.

use glam::DVec2;

use crate::carve::FeatureLine;
use crate::grid::Grid;
use crate::road::distance_along;

/// The concavity of stream-power profiles, m / n: river slope falls as
/// drainage area to this power. 0.45 is the usual value for real rivers.
pub(crate) const CONCAVITY: f64 = 0.45;

/// Drainage area per cell of `grid`, in square metres: each cell drains to
/// its steepest lower neighbour and passes on everything that drains into
/// it. Cells outside the grid's polygon have none; cells in hollows keep
/// what reaches them.
pub(crate) fn drainage_area(grid: &Grid) -> Vec<f64> {
    let n = grid.heights.len();
    let cell_area = grid.cell * grid.cell;
    let mut area: Vec<f64> = (0..n)
        .map(|i| if grid.inside[i] { cell_area } else { 0.0 })
        .collect();

    // From the highest cell down, each passes its area on.
    let mut order: Vec<usize> = (0..n).filter(|&i| grid.inside[i]).collect();
    order.sort_by(|&a, &b| grid.heights[b].total_cmp(&grid.heights[a]).then(a.cmp(&b)));
    for &c in &order {
        let receiver = grid
            .neighbours(c)
            .map(|(j, d)| (j, f64::from(grid.heights[c] - grid.heights[j]) / d))
            .filter(|&(_, drop)| drop > 0.0)
            .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)));
        if let Some((j, _)) = receiver {
            area[j] += area[c];
        }
    }
    area
}

/// The drainage area at each point of each of `rivers`, in square metres,
/// from the per-cell `areas` of `grid` (see [`drainage_area`]).
///
/// Along a river the area never falls: each point has at least the area
/// of the point above it and of its own cell. A river whose last point is a
/// point of another river, other than that river's last, joins it there,
/// and its area is added to that river's from there on downstream. Rivers
/// may join in any order, but not in a loop. Points outside the grid count
/// no cell area.
pub(crate) fn river_areas(rivers: &[FeatureLine], grid: &Grid, areas: &[f64]) -> Vec<Vec<f64>> {
    // Where each river joins another: (trunk, point on the trunk).
    let joins: Vec<Option<(usize, usize)>> = rivers
        .iter()
        .enumerate()
        .map(|(i, river)| {
            let end = *river.points.last()?;
            rivers.iter().enumerate().find_map(|(j, trunk)| {
                if j == i {
                    return None;
                }
                // Rivers that end at one place both join a third there.
                let flowing_on = &trunk.points[..trunk.points.len() - 1];
                flowing_on.iter().position(|&q| q == end).map(|k| (j, k))
            })
        })
        .collect();

    // Tributaries before their trunks: repeatedly take a river all of
    // whose tributaries are done.
    let mut inflow: Vec<Vec<f64>> = rivers.iter().map(|r| vec![0.0; r.points.len()]).collect();
    let mut out: Vec<Option<Vec<f64>>> = vec![None; rivers.len()];
    while out.iter().any(Option::is_none) {
        let ready = (0..rivers.len()).find(|&i| {
            out[i].is_none()
                && joins
                    .iter()
                    .enumerate()
                    .all(|(t, join)| !matches!(join, Some((j, _)) if *j == i) || out[t].is_some())
        });
        let i = ready.expect("rivers don't join in a loop");
        let mut along = Vec::with_capacity(rivers[i].points.len());
        let mut carried = 0.0_f64;
        for (k, &p) in rivers[i].points.iter().enumerate() {
            let own = grid.cell_of(p).map_or(0.0, |c| areas[c]);
            carried = carried.max(own) + inflow[i][k];
            along.push(carried);
        }
        if let Some((trunk, k)) = joins[i] {
            inflow[trunk][k] += *along.last().expect("a river has points");
        }
        out[i] = Some(along);
    }
    out.into_iter()
        .map(|a| a.expect("every river done"))
        .collect()
}

/// The steady-state stream-power profile through `points` (source first)
/// with drainage areas `areas`: its slope falls as `area^−`[`CONCAVITY`],
/// it ends at `outlet` and rises to `source` at the first point. Both are
/// water levels in metres, `source` at least `outlet`.
pub(crate) fn steady_profile(
    points: &[DVec2],
    areas: &[f64],
    source: f64,
    outlet: f64,
) -> Vec<f64> {
    let last = points.len() - 1;
    // The unscaled rise from the outlet to each point.
    let mut rise = vec![0.0_f64; points.len()];
    for i in (0..last).rev() {
        let run = points[i].distance(points[i + 1]);
        rise[i] = rise[i + 1] + areas[i].max(1.0).powf(-CONCAVITY) * run;
    }
    let scale = if rise[0] > 0.0 {
        (source - outlet) / rise[0]
    } else {
        0.0
    };
    rise.iter().map(|r| outlet + scale * r).collect()
}

/// How concave a profile is: its mean slope over the upper third of the
/// river divided by its mean slope over the lower third. Above 1 it is
/// steep at the source and flattens downstream; a level lower third gives
/// a large number.
pub(crate) fn concavity(points: &[DVec2], levels: &[f64]) -> f64 {
    let along = distance_along(points);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carve::FeatureKind;
    use crate::region::{Boundary, RegionId};
    use crate::stitch::{RegionSpec, StitchedWorld};
    use crate::{Convergence, Elevation, Terrain, TerrainPreset, fixtures};

    /// A square of `half` metres either side of the origin.
    fn square(half: f64) -> Vec<DVec2> {
        vec![
            DVec2::new(-half, -half),
            DVec2::new(half, -half),
            DVec2::new(half, half),
            DVec2::new(-half, half),
        ]
    }

    fn river(points: &[(f64, f64)]) -> FeatureLine {
        FeatureLine {
            kind: FeatureKind::River,
            points: points.iter().map(|&(x, z)| DVec2::new(x, z)).collect(),
            levels: vec![0.0; points.len()],
            widths: vec![4.0; points.len()],
            depth: 1.0,
        }
    }

    #[test]
    fn a_tilted_plane_drains_downhill_and_keeps_every_square_metre() {
        // Ground falling towards −x: every cell drains west, so the area
        // grows by one cell per column, and the total is the square's.
        let grid = Grid::new(&square(400.0), |p| (0.01 * p.x + 0.001 * p.y) as f32);
        let areas = drainage_area(&grid);
        let cell = grid.cell * grid.cell;
        let inside = grid.inside.iter().filter(|&&i| i).count() as f64;
        // Only cells with no lower neighbour (the low corner) keep their
        // area; summed over them it is the whole square.
        let sinks: f64 = (0..areas.len())
            .filter(|&c| grid.inside[c])
            .filter(|&c| {
                grid.neighbours(c)
                    .all(|(j, _)| grid.heights[j] >= grid.heights[c])
            })
            .map(|c| areas[c])
            .sum();
        assert!((sinks - inside * cell).abs() < 1e-6 * inside * cell);
        // Every cell keeps at least its own area.
        for c in (0..areas.len()).filter(|&c| grid.inside[c]) {
            assert!(areas[c] >= cell);
        }
    }

    #[test]
    fn areas_never_fall_along_a_river_and_add_up_at_a_join() {
        let grid = Grid::new(&square(500.0), |p| (0.02 * p.length()) as f32);
        let areas = drainage_area(&grid);
        // A trunk running west, and a tributary joining it at (0, 0), listed
        // first, so the joins must be ordered.
        let tributary = river(&[(0.0, 400.0), (0.0, 200.0), (0.0, 0.0)]);
        let trunk = river(&[
            (400.0, 0.0),
            (200.0, 0.0),
            (0.0, 0.0),
            (-200.0, 0.0),
            (-400.0, 0.0),
        ]);
        let along = river_areas(&[tributary, trunk], &grid, &areas);
        for a in &along {
            assert!(a.windows(2).all(|w| w[1] >= w[0]), "{a:?}");
        }
        let (tributary, trunk) = (&along[0], &along[1]);
        let into = tributary[2];
        assert!(trunk[2] >= trunk[1] + into, "{trunk:?} gains {into}");
        assert!(trunk[4] >= trunk[1] + into);
    }

    #[test]
    fn steady_profiles_are_concave_and_meet_their_ends() {
        let points: Vec<DVec2> = (0..=20)
            .map(|i| DVec2::new(f64::from(i) * 50.0, 0.0))
            .collect();
        let areas: Vec<f64> = (0..=20).map(|i| 1e4 * f64::from(i + 1).powi(2)).collect();
        let profile = steady_profile(&points, &areas, 120.0, 20.0);
        assert!((profile[0] - 120.0).abs() < 1e-9);
        assert!((profile[20] - 20.0).abs() < 1e-9);
        assert!(profile.windows(2).all(|w| w[1] <= w[0]));
        assert!(concavity(&points, &profile) > 3.0);
    }

    /// A world of one region, 4 km square, with `preset` and `seed`.
    fn one_region(preset: &TerrainPreset, seed: u32) -> StitchedWorld {
        let mut options = preset.terrain.clone();
        options.default_seed = seed;
        StitchedWorld::build(
            vec![RegionSpec {
                id: RegionId(1),
                polygon: square(2000.0),
                options,
            }],
            &Convergence::default(),
            &Elevation::flat(),
        )
        .expect("one-region world builds")
    }

    /// The terrain's region polygon.
    fn polygon(t: &Terrain) -> &[DVec2] {
        match &t.region().boundary {
            Boundary::Polygon(p) => p,
            Boundary::Unbounded => &[],
        }
    }

    /// The Phase 2.8 exit measurements, on one-region 4 km worlds, seeds 1
    /// to 6, per preset: how deep rivers cut below the natural ground, how
    /// concave the longest rivers' profiles are against the steady-state
    /// stream-power profile through their ends, how much deeper that
    /// profile would cut if kept under today's levels, and what drainage
    /// area costs per region.
    #[test]
    #[ignore]
    fn measure_phase_2_8_exit() {
        for (name, preset) in [
            ("plains", fixtures::plains_medium()),
            ("rolling hills", fixtures::rolling_hills_medium()),
            ("mountains", fixtures::mountains_medium()),
        ] {
            let mut cuts: Vec<f64> = Vec::new();
            let mut extra: Vec<f64> = Vec::new();
            let mut ratios: Vec<(f64, f64)> = Vec::new();
            let mut costs: Vec<f64> = Vec::new();
            let mut monotone = true;
            for seed in 1..=6 {
                let world = one_region(preset, seed);
                let t = &world.terrains()[0];
                let started = std::time::Instant::now();
                let grid = Grid::new(polygon(t), |p| t.uncarved_height(p));
                let cells = drainage_area(&grid);
                let along = river_areas(t.rivers(), &grid, &cells);
                costs.push(started.elapsed().as_secs_f64() * 1000.0);

                for (river, areas) in t.rivers().iter().zip(&along) {
                    monotone &= areas.windows(2).all(|w| w[1] >= w[0]);
                    let levels: Vec<f64> = river.levels.iter().map(|&l| f64::from(l)).collect();
                    for (&p, &l) in river.points.iter().zip(&levels) {
                        cuts.push(f64::from(t.uncarved_height(p)) - l);
                    }
                    // The steady profile through the river's ends, kept
                    // under today's levels (which never float): what it
                    // would add to the cut.
                    let last = levels.len() - 1;
                    let steady = steady_profile(&river.points, areas, levels[0], levels[last]);
                    extra.extend(steady.iter().zip(&levels).map(|(s, l)| (l - s).max(0.0)));
                }
                if let Some((river, areas)) = t
                    .rivers()
                    .iter()
                    .zip(&along)
                    .max_by_key(|(r, _)| r.points.len())
                {
                    let levels: Vec<f64> = river.levels.iter().map(|&l| f64::from(l)).collect();
                    let last = levels.len() - 1;
                    let steady = steady_profile(&river.points, areas, levels[0], levels[last]);
                    ratios.push((
                        concavity(&river.points, &levels),
                        concavity(&river.points, &steady),
                    ));
                }
            }
            if cuts.is_empty() {
                eprintln!("{name}: no rivers");
                continue;
            }
            let quantiles = |v: &mut Vec<f64>| {
                v.sort_by(f64::total_cmp);
                let q = |f: f64| v[((v.len() - 1) as f64 * f) as usize];
                (q(0.5), q(0.9), q(0.99), v[v.len() - 1])
            };
            let over = |v: &[f64], d: f64| {
                v.iter().filter(|&&c| c > d).count() as f64 / v.len() as f64 * 100.0
            };
            let (m, n9, n99, max) = quantiles(&mut cuts);
            eprintln!(
                "{name}: {} river points; cut median {m:.1} m, 90% {n9:.1} m, 99% {n99:.1} m, max {max:.1} m; \
                 over 10 m {:.0}%, over 20 m {:.0}%",
                cuts.len(),
                over(&cuts, 10.0),
                over(&cuts, 20.0)
            );
            let (m, n9, n99, max) = quantiles(&mut extra);
            eprintln!(
                "  steady profile under today's levels adds: median {m:.1} m, 90% {n9:.1} m, \
                 99% {n99:.1} m, max {max:.1} m"
            );
            eprintln!(
                "  longest rivers' concavity (upper / lower slope), today against steady: {}",
                ratios
                    .iter()
                    .map(|(a, b)| format!("{a:.1}/{b:.1}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            costs.sort_by(f64::total_cmp);
            eprintln!(
                "  drainage area per region: {:.0}–{:.0} ms; areas never fall along rivers: {monotone}",
                costs[0],
                costs[costs.len() - 1]
            );
        }
    }
}
