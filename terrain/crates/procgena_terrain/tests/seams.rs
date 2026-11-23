//! Borders between terrains: continuity of height and slope, corners, and
//! repeatable tiles.

mod common;

use common::{
    CONFIGS, JUNCTION, RIVER_SEEDS, TWO_REGION_BORDER, assert_no_crack, hills_into_plains, preset,
    triple_junction, two_biomes, two_regions,
};
use glam::{DVec2, I64Vec2, UVec2};
use procgena_terrain::{HeightField, HeightTile, StitchedWorld, Terrain, TerrainPreset};

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
    let h = 0.05;
    for (q, n) in along(border, 40) {
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
        let slope_a = (a.height(q + n * h) - ha) / h as f32;
        let slope_b = (hb - b.height(q - n * h)) / h as f32;
        assert!(
            (slope_a - slope_b).abs() < 0.05,
            "{label}: slopes differ at {q}: {slope_a} vs {slope_b}"
        );
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
    for config in CONFIGS {
        for (i, west) in TerrainPreset::ALL.into_iter().enumerate() {
            for (j, east) in TerrainPreset::ALL.into_iter().enumerate() {
                let world = two_regions(
                    DVec2::ZERO,
                    preset(west, config.seed(1, i as u32 + 1)),
                    preset(east, config.seed(1, j as u32 + 11)),
                    config.convergence(),
                );
                assert_continuous(
                    &world,
                    &border(DVec2::ZERO),
                    &format!("{} | {}, {config:?}", west.name(), east.name()),
                );
            }
        }
    }
}

#[test]
fn borders_with_river_crossings_join_continuously() {
    for config in CONFIGS {
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
    for config in CONFIGS {
        assert_continuous(
            &two_biomes(offset, config),
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
fn no_cracks_in_the_margins_of_a_bent_border() {
    let area = (DVec2::new(-750.0, -1450.0), DVec2::new(750.0, 1450.0));
    for config in CONFIGS {
        let world = two_biomes(DVec2::ZERO, config);
        let label = format!("two biomes, {config:?}");
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
    for config in CONFIGS {
        let world = triple_junction(config);
        let label = format!("triple junction, {config:?}");
        assert_no_cracks(&world, area, &vertices, &label);
    }
}

#[test]
fn three_regions_meet_at_one_corner() {
    for config in CONFIGS {
        assert_three_regions_meet(&triple_junction(config));
    }
}

fn assert_three_regions_meet(world: &StitchedWorld) {
    let corner = DVec2::new(JUNCTION.0, JUNCTION.1);

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

    // Each of the three borders is continuous right up to the corner.
    for end in [
        DVec2::new(-800.0, -500.0),
        DVec2::new(-50.0, 900.0),
        DVec2::new(700.0, -600.0),
    ] {
        let border: Vec<DVec2> = (0..=1)
            .map(|t| corner.lerp(end, 0.2 * f64::from(t)))
            .collect();
        assert_continuous(world, &border, &format!("junction border to {end}"));
    }
}

#[test]
fn a_tile_is_the_same_alone_or_inside_a_larger_tile() {
    for config in CONFIGS {
        assert_tile_inside_larger(&two_biomes(DVec2::ZERO, config));
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
    for config in CONFIGS {
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
    for config in CONFIGS {
        let world = two_biomes(DVec2::ZERO, config);
        for (q, _) in along(&border(DVec2::ZERO), 25) {
            assert!(world.height(q).is_finite(), "no ground at {q}");
        }
        for &(x, z) in &TWO_REGION_BORDER {
            assert!(world.height(DVec2::new(x, z)).is_finite());
        }
    }
}
