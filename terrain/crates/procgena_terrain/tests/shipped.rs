//! The shipped, Valheim-sized presets on the default world layout (2 km
//! regions, the default border warp and world elevation): borders join,
//! the presets make no rivers, ponds or roads, with rivers and roads
//! switched back on they carry on across borders, and relief stays within
//! about 500 m.

use std::sync::Arc;

use glam::DVec2;
use procgena_terrain::{
    Elevation, FeatureKind, FeatureLine, RegionCorner, RegionId, Terrain, TerrainOptions,
    TerrainPreset, WorldTerrain, WorldTerrainOptions,
};

/// The world seeds tried.
const SEEDS: [u32; 3] = [1, 2, 3];

/// The shipped preset for `region`, picked by a hash of its id.
fn mixed_preset(region: RegionId) -> &'static TerrainPreset {
    let presets: Vec<&TerrainPreset> = TerrainPreset::all().collect();
    let hash = region.0.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 40;
    presets[hash as usize % presets.len()]
}

/// The settings of [`mixed_preset`].
fn mixed(region: RegionId) -> TerrainOptions {
    mixed_preset(region).terrain.clone()
}

/// A default world seeded with `seed`, each region with a shipped preset.
fn world(seed: u32) -> WorldTerrain {
    let source = |region: RegionId, _: DVec2, _: &Elevation| mixed(region);
    let options = WorldTerrainOptions {
        seed,
        ..WorldTerrainOptions::default()
    };
    WorldTerrain::new(options, Arc::new(source)).expect("the default world is valid")
}

/// The river sources per km² each shipped preset was tuned for. The
/// presets ship with rivers and roads switched off.
const RIVER_DENSITIES: [(&str, f32); 3] =
    [("plains", 0.6), ("rolling_hills", 1.0), ("mountains", 1.2)];

/// [`world`] with each preset's rivers and roads switched back on.
fn world_with_features(seed: u32) -> WorldTerrain {
    let source = |region: RegionId, _: DVec2, _: &Elevation| {
        let preset = mixed_preset(region);
        let mut options = preset.terrain.clone();
        let (_, density) = RIVER_DENSITIES
            .iter()
            .find(|(key, _)| *key == preset.key)
            .expect("every shipped preset has a river density");
        options.features.rivers.density = *density;
        options.features.roads.enabled = true;
        options
    };
    let options = WorldTerrainOptions {
        seed,
        ..WorldTerrainOptions::default()
    };
    WorldTerrain::new(options, Arc::new(source)).expect("the default world is valid")
}

/// A default world seeded with `seed`, with `preset` in every region.
fn world_of(preset: &'static TerrainPreset, seed: u32) -> WorldTerrain {
    let source = move |_: RegionId, _: DVec2, _: &Elevation| preset.terrain.clone();
    let options = WorldTerrainOptions {
        seed,
        ..WorldTerrainOptions::default()
    };
    WorldTerrain::new(options, Arc::new(source)).expect("the default world is valid")
}

/// The region at the origin and every region on its edges and corners.
fn patch(world: &WorldTerrain) -> Vec<RegionId> {
    let centre = world.region_at(DVec2::ZERO);
    let mut out: Vec<RegionId> = world
        .region(centre)
        .corners
        .iter()
        .flat_map(|c| c.incident.iter().copied())
        .collect();
    out.sort();
    out.dedup();
    out.retain(|&r| r != centre);
    out.insert(0, centre);
    out
}

/// The slope of `terrain` at `q` going along `d`, by a second-order
/// one-sided difference over 0.5 m and 1 m.
fn slope_along(terrain: &Terrain, q: DVec2, d: DVec2) -> f32 {
    let [f0, f1, f2] = [0.0, 0.5, 1.0].map(|k| f64::from(terrain.height(q + d * k)));
    (-3.0 * f0 + 4.0 * f1 - f2) as f32
}

#[test]
fn borders_join_in_height_and_slope() {
    for seed in SEEDS {
        let world = world(seed);
        let regions = patch(&world);
        let mut edges = 0;
        for &id in &regions {
            let terrain = world.terrain(id).expect("regions build");
            for edge in &terrain.region().edges {
                let Some(neighbor) = edge.neighbor.filter(|n| regions.contains(n) && id < *n)
                else {
                    continue;
                };
                edges += 1;
                let other = world.terrain(neighbor).expect("regions build");
                let (first, last) = (edge.polyline[0], *edge.polyline.last().unwrap());
                for w in edge.polyline.windows(2) {
                    let normal = (w[1] - w[0]).normalize().perp();
                    for k in 1..4 {
                        let q = w[0].lerp(w[1], f64::from(k) / 4.0);
                        if q.distance(first) < 30.0 || q.distance(last) < 30.0 {
                            continue;
                        }
                        let (a, b) = if world.region_at(q + normal) == id {
                            (&terrain, &other)
                        } else {
                            (&other, &terrain)
                        };
                        let (ha, hb) = (a.height(q), b.height(q));
                        assert!(
                            (ha - hb).abs() < 1e-3,
                            "seed {seed}: heights differ at {q}: {ha} vs {hb}"
                        );
                        let (sa, sb) = (slope_along(a, q, normal), -slope_along(b, q, -normal));
                        assert!(
                            (sa - sb).abs() < 0.05,
                            "seed {seed}: slopes differ at {q}: {sa} vs {sb}"
                        );
                    }
                }
            }
        }
        assert!(edges >= regions.len(), "seed {seed}: only {edges} edges");
    }
}

/// Checks that every terrain at `corner` reaches the same height there,
/// and that the ground has no step on a small ring round it.
fn assert_corner_meets(world: &WorldTerrain, corner: &RegionCorner, label: &str) {
    let heights: Vec<f32> = corner
        .incident
        .iter()
        .map(|&r| world.terrain(r).unwrap().height(corner.position))
        .collect();
    for h in &heights {
        assert!(
            (h - heights[0]).abs() < 1e-3,
            "{label}: corner heights at {}: {heights:?}",
            corner.position
        );
    }
    let ring: Vec<f32> = (0..=360)
        .map(|k| world.height(corner.position + DVec2::from_angle(f64::from(k).to_radians()) * 0.5))
        .collect();
    for w in ring.windows(2) {
        assert!(
            (w[1] - w[0]).abs() < 0.05,
            "{label}: jump near the corner at {}: {} to {}",
            corner.position,
            w[0],
            w[1]
        );
    }
}

#[test]
fn corners_meet() {
    for seed in SEEDS {
        let world = world(seed);
        let centre = world.region_at(DVec2::ZERO);
        for corner in &world.region(centre).corners {
            assert_corner_meets(&world, corner, &format!("seed {seed}"));
        }
    }
}

/// The point of `line` outside its own region, if the line crosses the
/// border: a crossing piece runs from inside one region into the next.
fn outside_point(terrain: &Terrain, line: &FeatureLine) -> Option<DVec2> {
    let inside = |p: DVec2| terrain.region().boundary.contains(p);
    if !line.points.iter().any(|&p| inside(p)) {
        return None;
    }
    line.points.iter().copied().find(|&p| !inside(p))
}

#[test]
fn the_presets_make_no_rivers_ponds_or_roads() {
    for seed in SEEDS {
        let world = world(seed);
        for id in patch(&world) {
            let terrain = world.terrain(id).expect("regions build");
            assert!(terrain.rivers().is_empty(), "seed {seed}: rivers in {id:?}");
            assert!(terrain.ponds().is_empty(), "seed {seed}: ponds in {id:?}");
            assert!(terrain.roads().is_empty(), "seed {seed}: roads in {id:?}");
        }
    }
}

#[test]
fn rivers_and_roads_carry_on_across_borders() {
    // Every piece crossing a border is carved the same by the terrain on
    // the other side.
    let (mut rivers, mut roads, mut crossings) = (0, 0, 0);
    for seed in SEEDS {
        let world = world_with_features(seed);
        for id in patch(&world) {
            let terrain = world.terrain(id).expect("regions build");
            rivers += terrain.rivers().len();
            roads += terrain.roads().len();
            for line in terrain.rivers().iter().chain(terrain.roads()) {
                let Some(outside) = outside_point(&terrain, line) else {
                    continue;
                };
                crossings += 1;
                let across = world.terrain_at(outside).expect("neighbours build");
                assert_ne!(
                    across.region().id,
                    id,
                    "seed {seed}: no neighbour at {outside}"
                );
                let lines = match line.kind {
                    FeatureKind::River => across.rivers(),
                    _ => across.roads(),
                };
                let mut reversed = line.points.clone();
                reversed.reverse();
                let same = lines
                    .iter()
                    .any(|other| other.points == line.points || other.points == reversed);
                assert!(
                    same,
                    "seed {seed}: the {:?} crossing at {outside} has no match across the border",
                    line.kind
                );
            }
        }
    }
    assert!(rivers > 0 && roads > 0, "{rivers} rivers and {roads} roads");
    assert!(crossings > 0, "nothing crossed a border");
}

/// The relief of each region of `regions`: the highest minus the lowest
/// ground above the world elevation E, sampled every 20 m.
fn relief(world: &WorldTerrain, regions: &[RegionId]) -> Vec<f32> {
    regions
        .iter()
        .map(|&id| {
            let terrain = world.terrain(id).expect("regions build");
            let site = world.site(id);
            let (mut low, mut high) = (f32::INFINITY, f32::NEG_INFINITY);
            for j in -75..=75 {
                for i in -75..=75 {
                    let p = site + DVec2::new(f64::from(i), f64::from(j)) * 20.0;
                    if !terrain.region().boundary.contains(p) {
                        continue;
                    }
                    let h = terrain.height(p) - world.elevation().height(p);
                    low = low.min(h);
                    high = high.max(h);
                }
            }
            high - low
        })
        .collect()
}

#[test]
fn relief_stays_within_about_500_m() {
    // Mountains everywhere, and the mixed world: no region's ground spans
    // more than 550 m above E. Mountains still make real mountains, more
    // than 150 m of relief in some region.
    let mountains = TerrainPreset::mountains();
    let (mut tallest, mut highest_mixed) = (0.0f32, 0.0f32);
    for seed in SEEDS {
        for (label, world) in [
            ("mountains", world_of(mountains, seed)),
            ("mixed", world(seed)),
        ] {
            let reliefs = relief(&world, &patch(&world));
            eprintln!("seed {seed}, {label}: relief per region {reliefs:.0?}");
            let most = reliefs.iter().copied().fold(0.0, f32::max);
            if label == "mountains" {
                tallest = tallest.max(most);
            } else {
                highest_mixed = highest_mixed.max(most);
            }
        }
    }
    assert!(
        tallest.max(highest_mixed) < 550.0,
        "{tallest} m of relief in the mountains, {highest_mixed} m in the mixed worlds"
    );
    assert!(
        tallest > 150.0,
        "the tallest mountains have only {tallest} m of relief"
    );
}
