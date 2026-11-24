//! A terrain as `Ground`: its analytic slope against central differences
//! of its heights, where things are planted on it.

mod common;

use common::{DEFAULT, RIVER_SEEDS, WARPED, hills_into_plains, two_terrains};
use glam::{DVec2, I64Vec2, UVec2};
use procgena_core::Ground;
use procgena_terrain::{HeightTile, StitchedWorld, Terrain};

/// A grove's worth of planting spots on `terrain` inside its region: a
/// jittered grid over its bounds, plus spots next to river and road points
/// and along every shared border, where carving and bending shape the
/// ground most.
fn grove(terrain: &Terrain) -> Vec<DVec2> {
    let region = terrain.region();
    let corners: Vec<DVec2> = region
        .edges
        .iter()
        .flat_map(|e| e.polyline.clone())
        .collect();
    let (lo, hi) = corners
        .iter()
        .fold((DVec2::INFINITY, DVec2::NEG_INFINITY), |(lo, hi), &q| {
            (lo.min(q), hi.max(q))
        });
    // A cheap, repeatable jitter.
    let jitter = |k: u64| {
        let h = k.wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(17);
        DVec2::new((h & 0xffff) as f64, (h >> 16 & 0xffff) as f64) / 65535.0 - 0.5
    };
    let mut spots = Vec::new();
    let mut k = 0;
    let spacing = 241.0;
    let mut z = lo.y;
    while z < hi.y {
        let mut x = lo.x;
        while x < hi.x {
            k += 1;
            spots.push(DVec2::new(x, z) + jitter(k) * spacing);
            x += spacing;
        }
        z += spacing;
    }
    for line in terrain.rivers().iter().chain(terrain.roads()) {
        for (i, &q) in line.points.iter().enumerate().step_by(5) {
            k += 1;
            let reach = [2.0, 6.0, 15.0, 40.0][(i / 5) % 4];
            spots.push(q + jitter(k).normalize_or_zero() * reach);
        }
    }
    for edge in region.edges.iter().filter(|e| e.neighbor.is_some()) {
        for w in edge.polyline.windows(2) {
            for t in [0.2, 0.5, 0.8] {
                k += 1;
                let normal = (w[1] - w[0]).normalize().perp();
                let depth = [0.5, 5.0, 20.0, 45.0][(k % 4) as usize];
                spots.push(w[0].lerp(w[1], t) + normal * depth);
                spots.push(w[0].lerp(w[1], t) - normal * depth);
            }
        }
    }
    spots.retain(|&p| region.boundary.contains(p));
    spots
}

/// How far either side of a spot, in metres, its heights are compared.
/// Mountain heights carry up to about 5e-3 m of f32 rounding, too much for
/// shorter spans, and it sets the tolerance.
const HALF: f64 = 2.0;

/// How many pieces the span is cut into to average the slope over it.
const STEPS: i32 = 48;

/// Plants a grove on every terrain of `world` through `Ground`, and checks
/// at each spot that the sample's height is the terrain's height exactly,
/// and that its slope matches central differences of its heights: along x
/// and z, the difference [`HALF`] metres either side must match the slope
/// averaged over the same span (Simpson's rule). Spans across a crease (a
/// sharp ridge crest, a steep road bank's edge), where the slope jumps
/// between neighbouring samples, are skipped, but at most a third: mountain
/// ridges crease every few metres.
fn assert_grove_slopes_match(world: &StitchedWorld, label: &str) {
    for (n, terrain) in world.terrains().iter().enumerate() {
        let ground: &dyn Ground = terrain;
        let spots = grove(terrain);
        let (mut skipped, mut total, mut worst) = (0, 0, 0.0_f64);
        for &p in &spots {
            let sample = ground.sample(p);
            assert_eq!(
                sample.height.to_bits(),
                ground.height(p).to_bits(),
                "{label}, terrain {n}: the sample's height at {p}"
            );
            assert_eq!(sample.gradient, ground.gradient(p));
            for axis in [DVec2::X, DVec2::Y] {
                total += 1;
                let slopes: Vec<f64> = (0..=STEPS)
                    .map(|k| {
                        let at = p + axis * HALF * (2.0 * f64::from(k) / f64::from(STEPS) - 1.0);
                        ground.gradient(at).as_dvec2().dot(axis)
                    })
                    .collect();
                if slopes.windows(2).any(|w| (w[1] - w[0]).abs() > 0.075) {
                    skipped += 1;
                    continue;
                }
                let mean = slopes
                    .iter()
                    .enumerate()
                    .map(|(k, &s)| match k as i32 {
                        0 | STEPS => s,
                        k if k % 2 == 1 => 4.0 * s,
                        _ => 2.0 * s,
                    })
                    .sum::<f64>()
                    / (3.0 * f64::from(STEPS));
                let difference = (f64::from(ground.height(p + axis * HALF))
                    - f64::from(ground.height(p - axis * HALF)))
                    / (2.0 * HALF);
                let error = (difference - mean).abs();
                worst = worst.max(error);
                assert!(
                    error < 4e-3 + 2e-3 * mean.abs(),
                    "{label}, terrain {n} at {p} along {axis}: \
                     mean slope {mean} against difference {difference}"
                );
            }
        }
        assert!(
            skipped * 3 <= total,
            "{label}, terrain {n}: {skipped} of {total} spans skipped"
        );
        println!(
            "{label}, terrain {n}: {} spots, {skipped} of {total} spans skipped, worst {worst:.1e}",
            spots.len()
        );
    }
}

#[test]
fn a_grove_on_a_terrain_matches_its_slope() {
    assert_grove_slopes_match(&two_terrains(DVec2::ZERO, DEFAULT), "two terrains");
    assert_grove_slopes_match(&two_terrains(DVec2::ZERO, WARPED[1]), "tilted, warped");
    assert_grove_slopes_match(&two_terrains(DVec2::ZERO, WARPED[2]), "curved, warped");
    assert_grove_slopes_match(
        &hills_into_plains(RIVER_SEEDS[0], DEFAULT),
        "hills into plains",
    );
}

#[test]
fn tiles_carry_the_terrains_own_gradient() {
    let world = two_terrains(DVec2::ZERO, DEFAULT);
    let terrain = &world.terrains()[1];
    let tile = HeightTile::sample(terrain, I64Vec2::new(40, -20), UVec2::new(16, 12), 8.0);
    for j in 0..tile.size.y {
        for i in 0..tile.size.x {
            let p = DVec2::new(f64::from(40 + i as i32), f64::from(-20 + j as i32)) * 8.0;
            let sample = terrain.sample(p);
            assert_eq!(tile.get(i, j), sample.height);
            assert_eq!(tile.gradient(i, j), sample.gradient);
        }
    }
}
