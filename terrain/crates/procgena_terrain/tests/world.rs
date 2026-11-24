//! `WorldTerrain`: lattice worlds built region by region from a closure
//! source, joined without seams, the same in any order.

use std::sync::Arc;

use glam::{DVec2, I64Vec2, IVec2, UVec2};
use procgena_terrain::{
    BorderWarp, Convergence, Elevation, ElevationOptions, HeightBump, HeightField, HeightTile,
    NoiseLayer, OptionsError, RegionCorner, RegionId, RegionKey, RegionSpec, StitchedWorld,
    Terrain, TerrainOptions, TerrainPreset, WorldError, WorldTerrain, WorldTerrainOptions,
};

/// The world seed of every test world.
const SEED: u32 = 7;

/// The land a test world stands on.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Land {
    /// Flat land at sea level.
    Flat,
    /// A steady slope of 0.02 along x and 0.005 along z.
    Tilted,
    /// The default continents and uplands, with a 40 m bump at the origin.
    Curved,
}

const LANDS: [Land; 3] = [Land::Flat, Land::Tilted, Land::Curved];

impl Land {
    fn elevation(self) -> ElevationOptions {
        let flat = NoiseLayer {
            height: 0.0,
            ..ElevationOptions::default().uplift
        };
        match self {
            Land::Flat => ElevationOptions {
                continents: flat.clone(),
                uplift: flat,
                tilt: DVec2::ZERO,
                max_slope: 0.05,
                bumps: Vec::new(),
            },
            Land::Tilted => ElevationOptions {
                continents: flat.clone(),
                uplift: flat,
                tilt: DVec2::new(0.02, 0.005),
                max_slope: 0.05,
                bumps: Vec::new(),
            },
            Land::Curved => ElevationOptions {
                max_slope: 0.1,
                bumps: vec![HeightBump {
                    centre: DVec2::ZERO,
                    radius: 3000.0,
                    height: 40.0,
                }],
                ..ElevationOptions::default()
            },
        }
    }
}

/// Every built-in preset, picked for a region by a hash of its id.
fn preset_for(region: RegionId) -> TerrainOptions {
    let presets: Vec<&TerrainPreset> = TerrainPreset::all().collect();
    let hash = region.0.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 40;
    presets[hash as usize % presets.len()].terrain.clone()
}

/// The closure source of the test worlds.
fn source(region: RegionId, _site: DVec2, _elevation: &Elevation) -> TerrainOptions {
    preset_for(region)
}

/// The settings of a test world on `land`, keeping `cache` terrains, with
/// borders warped by the default warp.
fn options(land: Land, cache: usize) -> WorldTerrainOptions {
    WorldTerrainOptions {
        seed: SEED,
        elevation: land.elevation(),
        cache,
        ..WorldTerrainOptions::default()
    }
}

/// A test world on `land`.
fn world(land: Land) -> WorldTerrain {
    WorldTerrain::new(options(land, 64), Arc::new(source)).expect("test worlds are valid")
}

/// The region at the origin and every region on its edges and corners.
fn patch(world: &WorldTerrain) -> Vec<RegionId> {
    let centre = world.region_at(DVec2::ZERO);
    let region = world.region(centre);
    let mut out: Vec<RegionId> = region
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

/// The largest difference in slope across a border that still counts as
/// continuous.
const SLOPE_LIMIT: f32 = 0.05;

/// The slope of `terrain` at `q` going along `d`, by a second-order
/// one-sided difference over 0.5 m and 1 m (f32 heights are too coarse for
/// shorter steps on high ground).
fn slope_along(terrain: &Terrain, q: DVec2, d: DVec2) -> f32 {
    let [f0, f1, f2] = [0.0, 0.5, 1.0].map(|k| f64::from(terrain.height(q + d * k)));
    (-3.0 * f0 + 4.0 * f1 - f2) as f32
}

/// Checks that the two terrains on every edge between regions of
/// `regions` meet with the same height and slope, at three points on each
/// straight piece away from the corners.
fn assert_edges_join(world: &WorldTerrain, regions: &[RegionId], label: &str) {
    let mut edges = 0;
    for &id in regions {
        let terrain = world.terrain(id).expect("test regions build");
        for edge in &terrain.region().edges {
            let Some(neighbor) = edge.neighbor.filter(|n| regions.contains(n) && id < *n) else {
                continue;
            };
            edges += 1;
            let other = world.terrain(neighbor).expect("test regions build");
            let (first, last) = (edge.polyline[0], *edge.polyline.last().unwrap());
            for w in edge.polyline.windows(2) {
                let normal = (w[1] - w[0]).normalize().perp();
                for k in 1..4 {
                    let q = w[0].lerp(w[1], f64::from(k) / 4.0);
                    if q.distance(first) < 30.0 || q.distance(last) < 30.0 {
                        continue;
                    }
                    // Which terrain lies on the side the normal points to.
                    let (a, b) = if world.region_at(q + normal) == id {
                        (&terrain, &other)
                    } else {
                        (&other, &terrain)
                    };
                    let (ha, hb) = (a.height(q), b.height(q));
                    assert!(
                        (ha - hb).abs() < 1e-3,
                        "{label}: heights differ at {q} between {id:?} and {neighbor:?}: {ha} vs {hb}"
                    );
                    let (sa, sb) = (slope_along(a, q, normal), -slope_along(b, q, -normal));
                    assert!(
                        (sa - sb).abs() < SLOPE_LIMIT,
                        "{label}: slopes differ at {q} between {id:?} and {neighbor:?}: {sa} vs {sb}"
                    );
                }
            }
        }
    }
    assert!(
        edges >= regions.len(),
        "{label}: only {edges} shared edges checked"
    );
}

#[test]
fn lattice_borders_join_continuously() {
    for land in LANDS {
        let world = world(land);
        let regions = patch(&world);
        let kinds: std::collections::BTreeSet<_> = regions
            .iter()
            .map(|&r| preset_for(r).base.elevation.to_bits())
            .collect();
        assert!(kinds.len() >= 2, "the patch should mix presets");
        assert_edges_join(&world, &regions, &format!("{land:?}"));
    }
}

#[test]
fn corners_meet_at_one_height() {
    for land in LANDS {
        let world = world(land);
        let centre = world.region_at(DVec2::ZERO);
        for corner in &world.region(centre).corners {
            assert_corner_meets(&world, corner, &format!("{land:?}"));
        }
    }
}

#[test]
fn a_corner_of_four_regions_meets() {
    // The regions at a merged corner include some that share no edge with
    // each other, which a region still needs for its corner offer and
    // blend.
    let world = world(Land::Curved);
    let corner = (-6..=6)
        .flat_map(|z| (-6..=6).map(move |x| RegionKey(IVec2::new(x, z)).id()))
        .flat_map(|id| world.region(id).corners.clone())
        .find(|c| c.incident.len() >= 4)
        .expect("a merged corner near the origin");
    let ids: Vec<RegionId> = corner.incident.clone();
    let shares_no_edge = ids.iter().any(|&a| {
        ids.iter()
            .any(|&b| a != b && !world.region(a).edges.iter().any(|e| e.neighbor == Some(b)))
    });
    assert!(
        shares_no_edge,
        "two regions at {} share no edge",
        corner.position
    );
    assert_corner_meets(&world, &corner, "merged corner");
}

/// Checks that every terrain at `corner` reaches the same height there,
/// and that the world's ground has no step on a small ring round it.
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
        .map(|k| {
            let a = f64::from(k).to_radians();
            world.height(corner.position + DVec2::from_angle(a) * 0.5)
        })
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

/// The largest height change between neighbouring samples of `field`
/// along `path` (from 0 to 1), in `count` steps.
fn largest_jump(field: &impl HeightField, path: &impl Fn(f64) -> DVec2, count: usize) -> f32 {
    let mut last = field.height(path(0.0));
    let mut largest = 0.0f32;
    for k in 1..=count {
        let h = field.height(path(k as f64 / count as f64));
        assert!(
            h.is_finite(),
            "no ground at {}",
            path(k as f64 / count as f64)
        );
        largest = largest.max((h - last).abs());
        last = h;
    }
    largest
}

#[test]
fn no_cracks_across_many_regions() {
    // Lines 6 km long through the patch. On smooth ground the largest jump
    // shrinks about tenfold when the steps do; across a crack it doesn't.
    // A steep narrow feature (a river bank) is measured again finer.
    let world = world(Land::Curved);
    let length = 6000.0;
    for (from, to) in [
        (DVec2::new(-3000.0, 130.0), DVec2::new(3000.0, 130.0)),
        (DVec2::new(-410.0, -3000.0), DVec2::new(-410.0, 3000.0)),
        (DVec2::new(-2100.0, -2100.0), DVec2::new(2100.0, 2100.0)),
    ] {
        let path = |s: f64| from.lerp(to, s);
        let coarse_count = (length / 0.5) as usize;
        let coarse = largest_jump(&world, &path, coarse_count);
        let fine = largest_jump(&world, &path, coarse_count * 10);
        if fine < 0.2 * coarse + 1e-3 {
            continue;
        }
        let finest = largest_jump(&world, &path, coarse_count * 100);
        assert!(
            finest < 0.5 * fine + 1e-4,
            "{from} to {to}: largest jump {finest} at 5 mm steps, {fine} at 5 cm, {coarse} at 50 cm"
        );
    }
}

/// A tile across the patch, 50 m apart.
fn tile(world: &WorldTerrain) -> HeightTile {
    HeightTile::sample(world, I64Vec2::new(-60, -60), UVec2::splat(121), 50.0)
}

fn assert_same_tile(a: &HeightTile, b: &HeightTile, label: &str) {
    for (i, (x, y)) in a.heights.iter().zip(&b.heights).enumerate() {
        assert_eq!(x.to_bits(), y.to_bits(), "{label}: sample {i}: {x} vs {y}");
    }
}

#[test]
fn the_same_world_in_any_order_and_on_threads() {
    let land = Land::Curved;

    // Asked for cold, sample by sample, on several threads.
    let first = tile(&world(land));
    assert!(first.heights.iter().all(|h| h.is_finite()));

    // The regions built first, farthest first, four threads at a time,
    // each asking for every region.
    let warm = world(land);
    let mut regions = patch(&warm);
    regions.reverse();
    std::thread::scope(|s| {
        for t in 0..4 {
            let (warm, regions) = (&warm, &regions);
            s.spawn(move || {
                for k in 0..regions.len() {
                    let r = regions[(k + t) % regions.len()];
                    warm.terrain(r).expect("test regions build");
                }
            });
        }
    });
    assert_same_tile(&first, &tile(&warm), "built first on threads");
}

#[test]
fn a_dropped_terrain_builds_again_identically() {
    let world =
        WorldTerrain::new(options(Land::Tilted, 2), Arc::new(source)).expect("valid settings");
    let regions = patch(&world);
    let points: Vec<DVec2> = (0..200)
        .map(|k| world.site(regions[0]) + DVec2::from_angle(f64::from(k)) * f64::from(k * 7))
        .collect();
    let before = world.terrain(regions[0]).unwrap();
    let heights: Vec<u32> = points.iter().map(|&p| before.height(p).to_bits()).collect();

    // Two other regions push the first out of a cache of two.
    for &r in &regions[1..3] {
        world.terrain(r).unwrap();
    }
    let after = world.terrain(regions[0]).unwrap();
    assert!(
        !Arc::ptr_eq(&before, &after),
        "the terrain should be built again"
    );
    let again: Vec<u32> = points.iter().map(|&p| after.height(p).to_bits()).collect();
    assert_eq!(heights, again);
}

#[test]
fn each_region_builds_from_its_source() {
    let world = world(Land::Curved);
    for id in patch(&world) {
        let terrain = world.terrain(id).unwrap();
        let mut expected = source(id, world.site(id), world.elevation());
        expected.default_seed = SEED;
        assert_eq!(terrain.generator().options(), &expected, "region {id:?}");
        assert_eq!(terrain.region().id, id);

        // As a Ground the world gives the owner's height and a slope.
        let site = world.site(id);
        let ground = procgena_core::Ground::sample(&world, site);
        assert_eq!(ground.height.to_bits(), terrain.height(site).to_bits());
        assert!(ground.gradient.is_finite(), "slope at {site}");
    }
}

#[test]
fn water_lies_where_the_owners_rivers_run() {
    let world = world(Land::Flat);
    let mut wet = 0;
    for id in patch(&world) {
        let terrain = world.terrain(id).unwrap();
        for river in terrain.rivers() {
            for &p in river.points.iter().step_by(5) {
                if world.region_at(p) != id {
                    continue;
                }
                let level = world.water_level(p);
                assert_eq!(level, terrain.water_level(p), "at {p}");
                wet += usize::from(level.is_some());
            }
        }
    }
    assert!(wet > 0, "the patch should have rivers");
}

#[test]
fn the_slope_is_the_owners_own() {
    let world = world(Land::Curved);
    let mut checked = 0;
    for id in patch(&world) {
        let terrain = world.terrain(id).unwrap();
        for river in terrain.rivers() {
            for &q in river.points.iter().step_by(7) {
                let p = q + DVec2::new(13.0, -9.0);
                if world.region_at(p) != id {
                    continue;
                }
                let sample = procgena_core::Ground::sample(&world, p);
                assert_eq!(sample, terrain.sample(p), "at {p}");
                assert_eq!(procgena_core::Ground::gradient(&world, p), sample.gradient);
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "the patch should have rivers");
}

#[test]
fn the_owner_is_the_graphs_region() {
    let world = world(Land::Flat);
    for k in 0..400 {
        let p = DVec2::from_angle(f64::from(k) * 2.4) * (f64::from(k) * 17.0);
        let owner = world.region_at(p);
        let region = world.region(owner);
        assert!(
            region.boundary.contains(p) || region.boundary.distance_to_border(p) < 1e-6,
            "{p} isn't in {owner:?}"
        );
        let graph = world.graph().region_at(p).id();
        assert!(
            graph == owner || region.boundary.distance_to_border(p) < 1e-6,
            "{p}: {owner:?} against the graph's {graph:?}"
        );
    }
}

#[test]
fn the_centre_matches_a_stitched_world_of_two_rings() {
    // A StitchedWorld of the centre region and two rings round it gives
    // the centre region exactly the same neighbours to agree with, so its
    // terrain must be the same bit for bit.
    let land = Land::Curved;
    let world = world(land);
    let ring1 = patch(&world);
    let mut ring2: Vec<RegionId> = ring1
        .iter()
        .flat_map(|&r| {
            world
                .region(r)
                .corners
                .iter()
                .flat_map(|c| c.incident.clone())
                .collect::<Vec<_>>()
        })
        .collect();
    ring2.sort();
    ring2.dedup();
    let specs = ring2
        .iter()
        .map(|&id| {
            let procgena_terrain::Boundary::Polygon(polygon) = world.region(id).boundary.clone()
            else {
                unreachable!("lattice regions are polygons")
            };
            let mut options = source(id, world.site(id), world.elevation());
            options.default_seed = SEED;
            RegionSpec {
                id,
                polygon,
                options,
            }
        })
        .collect();
    let stitched = StitchedWorld::build(specs, &world.options().convergence, world.elevation())
        .expect("the stitched world builds");
    let centre = ring1[0];
    let theirs = stitched
        .terrains()
        .iter()
        .find(|t| t.region().id == centre)
        .unwrap();
    let mine = world.terrain(centre).unwrap();
    let site = world.site(centre);
    for j in -40..=40 {
        for i in -40..=40 {
            let p = site + DVec2::new(f64::from(i), f64::from(j)) * 60.0;
            assert_eq!(
                mine.height(p).to_bits(),
                theirs.height(p).to_bits(),
                "at {p}"
            );
        }
    }
}

#[test]
fn invalid_settings_are_refused() {
    let new = |options| WorldTerrain::new(options, Arc::new(source)).map(|_| ());
    let base = options(Land::Flat, 4);
    assert_eq!(
        new(WorldTerrainOptions {
            cache: 0,
            ..base.clone()
        }),
        Err(WorldError::Cache)
    );
    let convergence = |meeting, blend| WorldTerrainOptions {
        convergence: Convergence { meeting, blend },
        ..base.clone()
    };
    assert!(matches!(
        new(convergence(f32::NAN, 0.5)),
        Err(WorldError::ReliefMeeting(_))
    ));
    assert!(matches!(
        new(convergence(-1.0, 1.5)),
        Err(WorldError::ReliefBlend(_))
    ));
    let mut regions = base.regions;
    regions.warp = BorderWarp {
        amplitude: 100.0,
        wavelength: 10.0,
    };
    assert!(matches!(
        new(WorldTerrainOptions {
            regions,
            ..base.clone()
        }),
        Err(WorldError::Regions(_))
    ));
    let mut elevation = base.elevation.clone();
    elevation.max_slope = 0.0;
    elevation.tilt = DVec2::new(0.1, 0.0);
    assert!(matches!(
        new(WorldTerrainOptions { elevation, ..base }),
        Err(WorldError::Elevation(_))
    ));
}

#[test]
fn a_region_with_invalid_settings_has_no_ground() {
    let centre = world(Land::Flat).region_at(DVec2::ZERO);
    let bad = move |region: RegionId, site: DVec2, elevation: &Elevation| {
        let mut options = source(region, site, elevation);
        if region == centre {
            options.edges.margin = f32::NAN;
        }
        options
    };
    let world = WorldTerrain::new(options(Land::Flat, 4), Arc::new(bad)).unwrap();
    let error = WorldError::Options(
        centre,
        OptionsError::NotFinite {
            field: "edges.margin",
        },
    );
    assert_eq!(world.terrain(centre).map(|_| ()), Err(error.clone()));
    // Its neighbours can't agree their borders with it either.
    let neighbour = world.region(centre).edges[0].neighbor.unwrap();
    assert_eq!(world.terrain(neighbour).map(|_| ()), Err(error));
    assert!(world.height(world.site(centre)).is_nan());
    assert_eq!(world.water_level(world.site(centre)), None);
}
