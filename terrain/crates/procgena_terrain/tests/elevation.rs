//! Terrains standing on the world elevation E.

mod common;

use common::{DEFAULT, TILT, TILTED, preset, two_terrains};
use glam::DVec2;
use procgena_terrain::{
    BorderTarget, EdgeTarget, Elevation, ElevationOptions, RegionId, TerrainGenerator,
    TerrainPreset, sample_count,
};

/// Where the samples of an edge agreement lie: `count` evenly spaced points
/// along `polyline`, ends included.
fn sample_points(polyline: &[DVec2], count: usize) -> Vec<DVec2> {
    let lengths: Vec<f64> = polyline.windows(2).map(|w| w[0].distance(w[1])).collect();
    let total: f64 = lengths.iter().sum();
    (0..count)
        .map(|i| {
            let mut along = total * i as f64 / (count - 1) as f64;
            for (w, &length) in polyline.windows(2).zip(&lengths) {
                if along <= length {
                    return w[0].lerp(w[1], along / length);
                }
                along -= length;
            }
            *polyline.last().unwrap()
        })
        .collect()
}

#[test]
fn a_plane_under_both_sides_shifts_the_agreement_by_that_plane() {
    let flat = two_terrains(DVec2::ZERO, DEFAULT);
    let tilted = two_terrains(DVec2::ZERO, TILTED);
    let edges = &flat.terrains()[0].region().edges;
    let mut checked = 0;
    for (a, b) in flat.agreements().iter().zip(tilted.agreements()) {
        assert_eq!(a.edge, b.edge);
        let edge = edges.iter().find(|e| e.id == a.edge).unwrap();
        let points = sample_points(&edge.polyline, sample_count(edge.length()));
        let normals: Vec<DVec2> = points
            .windows(2)
            .map(|w| (w[1] - w[0]).normalize().perp())
            .collect();
        for (i, ((sa, sb), p)) in a.profile.iter().zip(&b.profile).zip(&points).enumerate() {
            let plane = TILT.dot(*p) as f32;
            let normal = normals[i.min(normals.len() - 1)];
            // The agreement is smoothed along the edge (a Gaussian 48 m
            // wide), and along a bent edge the plane isn't linear in the
            // distance along it, so within three widths (150 m) of a bend
            // or an end it shifts by the plane only nearly: by at most
            // Δ·σ/√(2π) for a bend changing the plane's slope by Δ, and Δ
            // is at most twice the tilt.
            let bend = edge
                .polyline
                .iter()
                .map(|v| v.distance(*p))
                .fold(f64::INFINITY, f64::min);
            let (height_allowed, slope_allowed) = if bend < 150.0 {
                let kink = 2.0 * TILT.length();
                (
                    (kink * 48.0 / (2.0 * std::f64::consts::PI).sqrt()) as f32,
                    kink as f32,
                )
            } else {
                (1e-3, 1e-4)
            };
            assert!(
                (sb.height - sa.height - plane).abs() < height_allowed,
                "{p}, {bend:.0} m from a bend: {} on the tilt against {} flat, plane {plane}",
                sb.height,
                sa.height
            );
            assert!(
                (sb.slope - sa.slope - TILT.dot(normal) as f32).abs() < slope_allowed,
                "{p}, {bend:.0} m from a bend: slope {} against {}",
                sb.slope,
                sa.slope
            );
            assert_eq!(sb.detail, sa.detail);
            checked += 1;
        }
    }
    assert!(checked > 100);
}

#[test]
fn elevation_and_edge_targets_may_be_set_in_either_order() {
    let world = two_terrains(DVec2::ZERO, DEFAULT);
    let region = world.terrains()[0].region().clone();
    let (plains, mountains) = (
        preset(TerrainPreset::Plains, 1),
        preset(TerrainPreset::Mountains, 1),
    );
    let target = BorderTarget {
        amplitude: 30.0,
        blend: vec![(RegionId(1), 0.6), (RegionId(2), 0.4)],
        roads: plains.features.roads.grades(),
    };
    let targets: Vec<EdgeTarget> = region
        .edges
        .iter()
        .filter(|e| e.neighbor.is_some())
        .map(|e| EdgeTarget {
            edge: e.id,
            middle: target.clone(),
            start: target.clone(),
            end: target.clone(),
        })
        .collect();
    let neighbours = [(RegionId(2), mountains)];
    let elevation = Elevation::new(ElevationOptions::default(), 1).unwrap();

    let first = TerrainGenerator::new(plains.clone())
        .unwrap()
        .with_elevation(elevation.clone())
        .with_edge_targets(&region, &targets, &neighbours, 0.5)
        .unwrap();
    let last = TerrainGenerator::new(plains)
        .unwrap()
        .with_edge_targets(&region, &targets, &neighbours, 0.5)
        .unwrap()
        .with_elevation(elevation.clone());
    for i in 0..400 {
        // Points from deep inside to the border and a little past it.
        let p = DVec2::new(-1900.0 + 5.0 * f64::from(i), 37.0 * f64::from(i % 7));
        assert_eq!(first.natural_height(p), last.natural_height(p), "{p}");
    }
}

/// Settings with no relief and no rivers or roads: ground that is E alone.
fn bare() -> procgena_terrain::TerrainOptions {
    let mut options = preset(TerrainPreset::Plains, 1);
    let base = &mut options.base;
    base.elevation = 0.0;
    base.swells.height = 0.0;
    base.hills.height = 0.0;
    base.mountains.height = 0.0;
    options.features.rivers.density = 0.0;
    options.features.roads.enabled = false;
    options
}

/// Ground with no relief on the curved test land, against E: where the
/// border protocol changes E, and by how much. On the shared edge it also
/// smooths E along the edge as the negotiation does (a Gaussian 48 m
/// wide, over the edge's samples), to see whether that accounts for the
/// difference. Run with `--ignored --nocapture`.
#[test]
#[ignore]
fn measure_bare_ground_against_e() {
    let elevation = common::CURVED.elevation();
    let world = common::two_regions(
        DVec2::ZERO,
        bare(),
        bare(),
        DEFAULT.convergence(),
        &elevation,
    );

    // Everywhere, by distance from the shared edge.
    let edge = world.terrains()[0]
        .region()
        .edges
        .iter()
        .find(|e| e.neighbor.is_some())
        .unwrap();
    let to_edge = |p: DVec2| {
        edge.polyline
            .windows(2)
            .map(|w| {
                let t = ((p - w[0]).dot(w[1] - w[0]) / w[0].distance_squared(w[1])).clamp(0.0, 1.0);
                p.distance(w[0].lerp(w[1], t))
            })
            .fold(f64::INFINITY, f64::min)
    };
    let mut by_distance = [0.0f32; 6];
    let mut worst = (0.0f32, DVec2::ZERO);
    for i in 0..200 {
        for j in 0..150 {
            let p = DVec2::new(-1990.0 + 20.0 * f64::from(i), -1490.0 + 20.0 * f64::from(j));
            if world.owner(p).is_none() {
                continue;
            }
            let bin = ((to_edge(p) / 100.0) as usize).min(5);
            let off =
                (procgena_terrain::HeightField::height(&world, p) - elevation.height(p)).abs();
            by_distance[bin] = by_distance[bin].max(off);
            if off > worst.0 {
                worst = (off, p);
            }
        }
    }
    println!(
        "largest |height − E| by distance from the shared edge (0–100 m, …, 500 m and more): \
         {by_distance:?}; worst {} m at {}, {:.0} m from the edge",
        worst.0,
        worst.1,
        to_edge(worst.1)
    );

    // On the shared edge: the agreement, and E smoothed along the edge.
    let agreement = world
        .agreements()
        .iter()
        .find(|a| a.edge == edge.id)
        .unwrap();
    let count = sample_count(edge.length());
    let points = sample_points(&edge.polyline, count);
    let step = edge.length() / (count - 1) as f64;
    let e: Vec<f64> = points
        .iter()
        .map(|&p| f64::from(elevation.height(p)))
        .collect();
    let sigma = 48.0 / step;
    let reach = (3.0 * sigma).ceil() as isize;
    let (mut agreed, mut smoothed, mut residual) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..count as isize {
        let (mut sum, mut weights) = (0.0, 0.0);
        for k in -reach..=reach {
            let j = (i + k).clamp(0, count as isize - 1) as usize;
            let w = (-0.5 * (k as f64 / sigma).powi(2)).exp();
            sum += w * e[j];
            weights += w;
        }
        let i = i as usize;
        let a = f64::from(agreement.profile[i].height) - e[i];
        let s = sum / weights - e[i];
        agreed = agreed.max(a.abs());
        smoothed = smoothed.max(s.abs());
        residual = residual.max((a - s).abs());
    }
    let detail = agreement
        .profile
        .iter()
        .map(|s| s.detail.abs())
        .fold(0.0f32, f32::max);
    println!(
        "on the edge: agreement − E up to {agreed:.3} m; E smoothed by hand − E up to {smoothed:.3} m; \
         the two differ by up to {residual:.3} m; border detail up to {detail:.3} m"
    );
}

#[test]
fn river_sources_stay_spread_on_a_tilt() {
    // Hills east of the border, on land falling west: the hills' rivers
    // still start near the border about as often as on flat land, rather
    // than all at the tilt's high, far end.
    let near_border = |config| {
        (0..30)
            .map(|seed| {
                let world = common::hills_into_plains(seed, config);
                world.terrains()[1]
                    .rivers()
                    .iter()
                    .filter(|r| r.points[0].x < 800.0)
                    .count()
            })
            .sum::<usize>()
    };
    let (flat, tilted) = (near_border(DEFAULT), near_border(TILTED));
    assert!(flat > 4, "the test needs rivers near the border: {flat}");
    assert!(
        2 * tilted >= flat,
        "{tilted} on the tilt against {flat} flat"
    );
}

#[test]
fn every_height_stands_on_e() {
    let flat = two_terrains(DVec2::ZERO, DEFAULT);
    let tilted = two_terrains(DVec2::ZERO, TILTED);
    for (a, b) in flat.terrains().iter().zip(tilted.terrains()) {
        for i in 0..200 {
            let p = DVec2::new(-1900.0 + 19.0 * f64::from(i), -1400.0 + 14.0 * f64::from(i));
            let (ga, gb) = (a.generator(), b.generator());
            let plane = TILT.dot(p) as f32;
            assert!(
                (gb.natural_height(p) - ga.natural_height(p) - plane).abs() < 2e-3,
                "{p}"
            );
        }
    }
}
