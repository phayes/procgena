//! Terrains standing on the world elevation E.

mod common;

use common::{DEFAULT, TILT, TILTED, preset, two_terrains};
use glam::DVec2;
use procgena_terrain::{
    BorderTarget, EdgeTarget, Elevation, HeightField, RegionId, TerrainGenerator, fixtures,
    sample_count,
};

/// Where the samples of an edge agreement lie: `count` evenly spaced points
/// along `polyline`, ends included, each with the unit normal of its
/// segment (the segment turned a quarter turn anticlockwise).
fn sample_points(polyline: &[DVec2], count: usize) -> Vec<(DVec2, DVec2)> {
    let lengths: Vec<f64> = polyline.windows(2).map(|w| w[0].distance(w[1])).collect();
    let total: f64 = lengths.iter().sum();
    let normal = |w: &[DVec2]| (w[1] - w[0]).normalize().perp();
    (0..count)
        .map(|i| {
            let mut along = total * i as f64 / (count - 1) as f64;
            for (w, &length) in polyline.windows(2).zip(&lengths) {
                if along <= length {
                    return (w[0].lerp(w[1], along / length), normal(w));
                }
                along -= length;
            }
            let last = &polyline[polyline.len() - 2..];
            (last[1], normal(last))
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
        for ((sa, sb), &(p, normal)) in a.profile.iter().zip(&b.profile).zip(&points) {
            let plane = TILT.dot(p) as f32;
            // Exactly, up to rounding, at bends too: negotiation smooths
            // only the heights above E.
            let bend = edge
                .polyline
                .iter()
                .map(|v| v.distance(p))
                .fold(f64::INFINITY, f64::min);
            let (height_allowed, slope_allowed) = (1e-3, 1e-4);
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
        preset(fixtures::plains_medium(), 1),
        preset(fixtures::mountains_medium(), 1),
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
    let elevation = Elevation::new(fixtures::elevation_medium(), 1).unwrap();

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

/// Settings with no relief, no rivers or roads and no border
/// irregularities: ground that is E alone.
fn bare() -> procgena_terrain::TerrainOptions {
    let mut options = preset(fixtures::plains_medium(), 1);
    options.edges.roughness = 0.0;
    let base = &mut options.base;
    base.elevation = 0.0;
    base.swells.height = 0.0;
    base.hills.height = 0.0;
    base.mountains.height = 0.0;
    options.features.rivers.density = 0.0;
    options.features.roads.enabled = false;
    options
}

/// Bare ground (no relief, rivers, roads or border irregularities) on the
/// curved test land is E itself everywhere, along every edge and at the
/// corner where three regions meet: the border protocol passes E through
/// exactly.
#[test]
fn bare_ground_is_e_everywhere() {
    let elevation = common::CURVED.elevation();
    let convergence = DEFAULT.convergence();
    for (name, world) in [
        (
            "two regions",
            common::two_regions(DVec2::ZERO, bare(), bare(), convergence, &elevation),
        ),
        (
            "three regions",
            common::three_regions([bare(), bare(), bare()], convergence, &elevation),
        ),
    ] {
        let mut worst = (0.0f32, DVec2::ZERO);
        for i in 0..400 {
            for j in 0..400 {
                let p = DVec2::new(-1995.0 + 10.0 * f64::from(i), -1995.0 + 10.0 * f64::from(j));
                if world.owner(p).is_none() {
                    continue;
                }
                let off = (HeightField::height(&world, p) - elevation.height(p)).abs();
                if off > worst.0 {
                    worst = (off, p);
                }
            }
        }
        // The junction itself, and points close round it.
        let junction = DVec2::new(common::JUNCTION.0, common::JUNCTION.1);
        for k in 0..64 {
            let p = junction + DVec2::from_angle(f64::from(k) * 0.098) * (0.5 * f64::from(k % 8));
            if world.owner(p).is_some() {
                let off = (HeightField::height(&world, p) - elevation.height(p)).abs();
                if off > worst.0 {
                    worst = (off, p);
                }
            }
        }
        assert!(worst.0 < 2e-3, "{name}: {} m off E at {}", worst.0, worst.1);
    }
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
