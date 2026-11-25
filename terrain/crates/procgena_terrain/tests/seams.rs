//! Borders between terrains: continuity of height and slope, corners, and
//! repeatable tiles.

mod common;

use common::{
    CONFIGS, CURVED, FOUR_REGION_ENDS, JUNCTION, RIVER_SEEDS, TWO_REGION_BORDER, WARPED, WITH_LAND,
    assert_no_crack, four_regions, hills_into_plains, preset, triple_junction, two_regions,
    two_regions_warped, two_terrains,
};
use glam::{DVec2, I64Vec2, UVec2};
use procgena_terrain::{HeightField, HeightTile, StitchedWorld, Terrain, fixtures};

/// Points spread along a polyline (excluding its corners), each with the
/// unit normal of its segment.
fn along(polyline: &[DVec2], per_segment: usize) -> Vec<(DVec2, DVec2)> {
    let mut out = Vec::new();
    for w in polyline.windows(2) {
        let normal = (w[1] - w[0]).normalize().perp();
        for k in 1..per_segment {
            let t = k as f64 / per_segment as f64;
            out.push((w[0].lerp(w[1], t), normal));
        }
    }
    out
}

/// The terrain of `world` whose region contains `p`.
fn side(world: &StitchedWorld, p: DVec2) -> &Terrain {
    world.owner(p).expect("point inside the world")
}

/// Checks height and slope continuity across a border of `world`.
fn assert_continuous(world: &StitchedWorld, border: &[DVec2], label: &str) {
    assert_continuous_along(world, border, 40, label);
}

/// Every border between two regions of `world`, once each.
fn shared_borders(world: &StitchedWorld) -> Vec<Vec<DVec2>> {
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for edge in world.terrains().iter().flat_map(|t| &t.region().edges) {
        if edge.neighbor.is_some() && !seen.contains(&edge.id) {
            seen.push(edge.id);
            out.push(edge.polyline.clone());
        }
    }
    out
}

/// [`assert_continuous`], at `per_segment` − 1 points on each straight
/// piece of `border`.
fn assert_continuous_along(
    world: &StitchedWorld,
    border: &[DVec2],
    per_segment: usize,
    label: &str,
) {
    assert_continuous_with_step(world, border, per_segment, Step::Short, label);
}

/// The largest difference in slope across a border that still counts as
/// continuous.
const SLOPE_LIMIT: f32 = 0.05;

/// How far from the border slopes are read.
#[derive(Clone, Copy)]
enum Step {
    /// First-order differences over 5 cm.
    Short,
    /// Second-order differences over 0.5 m and 1 m, so curved ground reads
    /// the same slope from both sides.
    Long,
}

/// The slope across a border at `q`: field `a` on the side `n` points to and
/// field `b` on the other, each read only on its own side.
fn slope_gap(
    a: impl Fn(DVec2) -> f32,
    b: impl Fn(DVec2) -> f32,
    q: DVec2,
    n: DVec2,
    step: Step,
) -> (f32, f32) {
    let one_sided = |f: &dyn Fn(DVec2) -> f32, d: DVec2| match step {
        Step::Short => (f(q + d * 0.05) - f(q)) / 0.05,
        Step::Long => {
            let [f0, f1, f2] = [0.0, 0.5, 1.0].map(|k| f64::from(f(q + d * k)));
            ((-3.0 * f0 + 4.0 * f1 - f2) / 1.0) as f32
        }
    };
    (one_sided(&a, n), -one_sided(&b, -n))
}

/// [`assert_continuous_along`], with slopes read at `step`.
///
/// Heights are `f32`, so on high ground they carry noise of about half a
/// millimetre from point to point; [`Step::Short`] can read that as a slope
/// change of a few hundredths. [`Step::Long`] reads it as under 0.005, and
/// still catches a real change of slope (see
/// `a_kink_just_over_the_limit_is_caught_at_half_metre_steps`).
fn assert_continuous_with_step(
    world: &StitchedWorld,
    border: &[DVec2],
    per_segment: usize,
    step: Step,
    label: &str,
) {
    for (q, n) in along(border, per_segment) {
        // Both terrains meet the same height on the border.
        let [a, b] = [side(world, q + n * 1.0), side(world, q - n * 1.0)];
        assert_ne!(
            a.region().id,
            b.region().id,
            "{label}: {q} should lie on a border"
        );
        let (ha, hb) = (a.height(q), b.height(q));
        assert!(
            (ha - hb).abs() < 1e-3,
            "{label}: heights differ at {q}: {ha} vs {hb}"
        );

        // The slope across the border matches, from one-sided differences.
        let (slope_a, slope_b) = slope_gap(|p| a.height(p), |p| b.height(p), q, n, step);
        assert!(
            (slope_a - slope_b).abs() < SLOPE_LIMIT,
            "{label}: slopes differ at {q}: {slope_a} vs {slope_b}"
        );
    }
}

#[test]
fn a_kink_just_over_the_limit_is_caught_at_half_metre_steps() {
    // Two planes meeting along a slanted line at the height of the warped
    // test worlds, as f32 like terrain heights, with slopes across the line
    // differing by just over the limit, or just under it.
    let n = DVec2::new(0.6, 0.8);
    let plane = |slope: f64| move |p: DVec2| (109.4 + slope * p.dot(n)) as f32;
    for (change, kinked) in [(1.1 * SLOPE_LIMIT, true), (0.8 * SLOPE_LIMIT, false)] {
        let change = f64::from(change);
        for k in 0..200 {
            let q = n.perp() * (f64::from(k) * 13.7 - 1300.0);
            let (a, b) = slope_gap(plane(0.02 + change), plane(0.02), q, n, Step::Long);
            assert_eq!(
                (a - b).abs() >= SLOPE_LIMIT,
                kinked,
                "a slope change of {change} at {q} reads as {}",
                a - b
            );
        }
    }
}

fn border(offset: DVec2) -> Vec<DVec2> {
    TWO_REGION_BORDER
        .iter()
        .map(|&(x, z)| DVec2::new(x, z) + offset)
        .collect()
}

#[test]
fn every_preset_pair_joins_continuously() {
    for config in WITH_LAND {
        for (i, west) in fixtures::all().enumerate() {
            for (j, east) in fixtures::all().enumerate() {
                let world = two_regions(
                    DVec2::ZERO,
                    preset(west, config.seed(1, i as u32 + 1)),
                    preset(east, config.seed(1, j as u32 + 11)),
                    config.convergence(),
                    &config.elevation(),
                );
                assert_continuous(
                    &world,
                    &border(DVec2::ZERO),
                    &format!("{} | {}, {config:?}", west.name, east.name),
                );
            }
        }
    }
}

#[test]
fn borders_with_river_crossings_join_continuously() {
    for config in WITH_LAND {
        for seed in RIVER_SEEDS {
            assert_continuous(
                &hills_into_plains(seed, config),
                &border(DVec2::ZERO),
                &format!("hills into plains, seed {seed}, {config:?}"),
            );
        }
    }
}

#[test]
fn borders_join_far_from_the_origin() {
    let offset = DVec2::splat(1.0e7);
    // Flat land, and the curved land (this far out only its bounded noise
    // layers: the bump is near the origin). Not the tilted land: it stands
    // 250 km high this far out, beyond what f32 heights resolve.
    for config in CONFIGS.into_iter().chain([CURVED]) {
        assert_continuous(
            &two_terrains(offset, config),
            &border(offset),
            &format!("far, {config:?}"),
        );
    }
}

/// Straight lines along x and along z across `area` (min and max corners),
/// and rings around each of `vertices`.
fn assert_no_cracks(world: &StitchedWorld, area: (DVec2, DVec2), vertices: &[DVec2], label: &str) {
    let (min, max) = area;
    let size = max - min;
    for k in 0..24 {
        let t = (f64::from(k) + 0.37) / 24.0;
        let z = min.y + size.y * t;
        let x = min.x + size.x * t;
        assert_no_crack(
            world,
            |s| DVec2::new(min.x + size.x * s, z),
            size.x,
            &format!("{label} z = {z}"),
        );
        assert_no_crack(
            world,
            |s| DVec2::new(x, min.y + size.y * s),
            size.y,
            &format!("{label} x = {x}"),
        );
    }
    for &v in vertices {
        for r in [2.0, 15.0, 60.0, 200.0] {
            let ring = |s: f64| v + DVec2::from_angle(s * std::f64::consts::TAU) * r;
            assert_no_crack(
                world,
                ring,
                std::f64::consts::TAU * r,
                &format!("{label} ring {r} m around {v}"),
            );
        }
    }
}

#[test]
fn warped_borders_join_continuously() {
    // Every preset pair on flat land, and the fixtures on every land.
    let flat = WARPED[0];
    for (i, west) in fixtures::all().enumerate() {
        for (j, east) in fixtures::all().enumerate() {
            let world = two_regions_warped(
                DVec2::ZERO,
                preset(west, flat.seed(1, i as u32 + 1)),
                preset(east, flat.seed(1, j as u32 + 11)),
                flat.convergence(),
                &flat.elevation(),
                flat.warp,
            );
            for border in shared_borders(&world) {
                assert_continuous_with_step(
                    &world,
                    &border,
                    4,
                    Step::Long,
                    &format!("warped {} | {}", west.name, east.name),
                );
            }
        }
    }
    for config in WARPED {
        let worlds = [
            ("two terrains", two_terrains(DVec2::ZERO, config)),
            ("triple junction", triple_junction(config)),
        ]
        .into_iter()
        .chain(RIVER_SEEDS.map(|s| ("hills into plains", hills_into_plains(s, config))));
        for (name, world) in worlds {
            let borders = shared_borders(&world);
            assert!(
                borders.iter().all(|b| b.len() > 10),
                "{name}: borders should be warped"
            );
            for border in borders {
                let label = format!("{name}, {config:?}");
                assert_continuous_with_step(&world, &border, 4, Step::Long, &label);
            }
        }
    }
}

#[test]
fn no_cracks_in_warped_borders() {
    let area = (DVec2::new(-750.0, -1450.0), DVec2::new(750.0, 1450.0));
    for config in WARPED {
        let world = two_terrains(DVec2::ZERO, config);
        let label = format!("warped two terrains, {config:?}");
        assert_no_cracks(&world, area, &border(DVec2::ZERO)[1..4], &label);
        let world = triple_junction(config);
        let area = (DVec2::new(-1200.0, -1100.0), DVec2::new(1300.0, 1400.0));
        let vertices = [
            DVec2::new(JUNCTION.0, JUNCTION.1),
            DVec2::new(-800.0, -500.0),
            DVec2::new(-50.0, 900.0),
            DVec2::new(700.0, -600.0),
        ];
        let label = format!("warped triple junction, {config:?}");
        assert_no_cracks(&world, area, &vertices, &label);
    }
}

#[test]
fn no_cracks_in_the_margins_of_a_bent_border() {
    let area = (DVec2::new(-750.0, -1450.0), DVec2::new(750.0, 1450.0));
    for config in WITH_LAND {
        let world = two_terrains(DVec2::ZERO, config);
        let label = format!("two terrains, {config:?}");
        assert_no_cracks(&world, area, &border(DVec2::ZERO)[1..4], &label);
    }
}

#[test]
fn no_cracks_around_a_triple_junction() {
    let vertices = [
        DVec2::new(JUNCTION.0, JUNCTION.1),
        DVec2::new(-800.0, -500.0),
        DVec2::new(-50.0, 900.0),
        DVec2::new(700.0, -600.0),
    ];
    let area = (DVec2::new(-1200.0, -1100.0), DVec2::new(1300.0, 1400.0));
    for config in WITH_LAND {
        let world = triple_junction(config);
        let label = format!("triple junction, {config:?}");
        assert_no_cracks(&world, area, &vertices, &label);
    }
}

#[test]
fn three_regions_meet_at_one_corner() {
    for config in WITH_LAND {
        assert_three_regions_meet(&triple_junction(config));
    }
}

fn assert_three_regions_meet(world: &StitchedWorld) {
    assert_regions_meet(
        world,
        DVec2::new(JUNCTION.0, JUNCTION.1),
        &[
            DVec2::new(-800.0, -500.0),
            DVec2::new(-50.0, 900.0),
            DVec2::new(700.0, -600.0),
        ],
    );
}

#[test]
fn four_regions_meet_at_one_corner() {
    for config in WITH_LAND {
        let world = four_regions(
            [
                preset(fixtures::plains_medium(), config.seed(5, 1)),
                preset(fixtures::mountains_medium(), config.seed(5, 2)),
                preset(fixtures::rolling_hills_medium(), config.seed(5, 3)),
                preset(fixtures::plains_medium(), config.seed(5, 4)),
            ],
            config.convergence(),
            &config.elevation(),
        );
        let corner = world
            .terrains()
            .iter()
            .flat_map(|t| &t.region().corners)
            .find(|c| c.incident.len() == 4)
            .expect("four regions share a corner");
        assert_eq!(corner.position, DVec2::new(JUNCTION.0, JUNCTION.1));
        assert_regions_meet(&world, corner.position, &FOUR_REGION_ENDS.map(DVec2::from));
    }
}

/// Checks that every terrain of `world` meets at `corner`, and that the
/// borders from `corner` towards each of `ends` are continuous up to it.
fn assert_regions_meet(world: &StitchedWorld, corner: DVec2, ends: &[DVec2]) {
    // Every terrain reaches the same height at the corner.
    let heights: Vec<f32> = world.terrains().iter().map(|t| t.height(corner)).collect();
    for h in &heights {
        assert!((h - heights[0]).abs() < 1e-3, "corner heights {heights:?}");
    }

    // Around the corner the ground is continuous: small steps on a small circle.
    let r = 0.5;
    let ring: Vec<f32> = (0..=360)
        .map(|k| {
            let a = f64::from(k).to_radians();
            world.height(corner + DVec2::new(a.cos(), a.sin()) * r)
        })
        .collect();
    for w in ring.windows(2) {
        assert!(
            (w[1] - w[0]).abs() < 0.05,
            "jump near the corner: {} to {}",
            w[0],
            w[1]
        );
    }

    // Each border is continuous right up to the corner.
    for &end in ends {
        let border: Vec<DVec2> = (0..=1)
            .map(|t| corner.lerp(end, 0.2 * f64::from(t)))
            .collect();
        assert_continuous(world, &border, &format!("border from {corner} to {end}"));
    }
}

#[test]
fn a_tile_is_the_same_alone_or_inside_a_larger_tile() {
    for config in WITH_LAND {
        assert_tile_inside_larger(&two_terrains(DVec2::ZERO, config));
    }
}

fn assert_tile_inside_larger(world: &StitchedWorld) {
    let chunk = HeightTile::sample(world, I64Vec2::new(-16, 40), UVec2::splat(64), 1.0);
    let larger = HeightTile::sample(world, I64Vec2::new(-80, -24), UVec2::splat(192), 1.0);
    for j in 0..64 {
        for i in 0..64 {
            let inner = larger.get(i + 64, j + 64);
            assert_eq!(chunk.get(i, j).to_bits(), inner.to_bits(), "sample {i},{j}");
        }
    }
    assert!(chunk.heights.iter().all(|h| h.is_finite()));
}

#[test]
fn neighbouring_tiles_share_their_border_samples() {
    let step = 2.0;
    for config in WITH_LAND {
        let world = triple_junction(config);
        let left = HeightTile::sample(&world, I64Vec2::new(-32, -40), UVec2::splat(65), step);
        let right = HeightTile::sample(&world, I64Vec2::new(32, -40), UVec2::splat(65), step);
        for j in 0..65 {
            assert_eq!(left.get(64, j).to_bits(), right.get(0, j).to_bits());
        }
    }
}

#[test]
fn points_on_a_border_have_ground() {
    // Lattice points exactly on polygon sides must not fall between regions.
    for config in WITH_LAND {
        let world = two_terrains(DVec2::ZERO, config);
        for (q, _) in along(&border(DVec2::ZERO), 25) {
            assert!(world.height(q).is_finite(), "no ground at {q}");
        }
        for &(x, z) in &TWO_REGION_BORDER {
            assert!(world.height(DVec2::new(x, z)).is_finite());
        }
    }
}
