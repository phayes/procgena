//! Chunk meshes: which chunks are chosen around a viewer, and that chunks
//! one level apart, provisional or canonical, join without cracks.

mod common;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use common::{DEFAULT, RIVER_SEEDS, hills_into_plains};
use glam::{DVec2, DVec3};
use procgena_core::chain::ViewScale;
use procgena_core::{Lod, LodLevel};
use procgena_terrain::{
    CHUNK_CELLS, ChunkDetail, ChunkKey, ChunkMesh, ChunkSamples, ChunkSelection, ChunkSide,
    ChunkSource, EdgeAgreement, Elevation, FeatureTier, RegionGraphOptions, RegionId,
    RegionProgress, RegionStage, Terrain, Transitions, WorldTerrain, WorldTerrainOptions,
    chunk_lod, fixtures,
};

/// Four chunk levels, switching at 96, 192 and 384 m, drawn out to
/// 1000 m: small enough to mesh every chunk in a test.
fn test_lod() -> Lod {
    let levels = [0.0, 96.0, 192.0, 384.0]
        .map(|d| LodLevel::new(d, 0.1, ()))
        .to_vec();
    Lod::new(64.0, levels).with_cull(1000.0)
}

/// The plains of [`hills_into_plains`] with its roads, and the same
/// terrain built without them.
fn plains() -> (Terrain, Terrain) {
    let world = hills_into_plains(RIVER_SEEDS[0], DEFAULT);
    let full = world.terrains()[0].clone();
    let generator = full.generator().clone();
    let region = full.region().clone();
    let mine: Vec<EdgeAgreement> = world
        .agreements()
        .iter()
        .filter(|a| region.edges.iter().any(|e| e.id == a.edge))
        .cloned()
        .collect();
    let trace = generator.trace_rivers(&region);
    let bare = generator
        .build_traced(region, &mine, &trace)
        .expect("the plains build without roads");
    assert!(full.tier() == FeatureTier::Fine && !full.roads().is_empty());
    (full, bare)
}

/// A point on a vertex, exactly: world x and z, and the height's bits.
type Point = (u64, u32, u64);

/// The world position of `mesh`'s vertex `v`, exactly.
fn point(mesh: &ChunkMesh, v: u32) -> Point {
    let p = mesh.positions[v as usize];
    (
        (mesh.origin.x + f64::from(p.x)).to_bits(),
        p.y.to_bits(),
        (mesh.origin.y + f64::from(p.z)).to_bits(),
    )
}

/// World x and z of a point.
fn xz(p: Point) -> DVec2 {
    DVec2::new(f64::from_bits(p.0), f64::from_bits(p.2))
}

/// The edges of `mesh` that belong to one triangle only, in the direction
/// their triangle runs them.
fn open_edges(mesh: &ChunkMesh) -> Vec<(Point, Point)> {
    let mut count: HashMap<(u32, u32), usize> = HashMap::new();
    let mut directed = Vec::new();
    for t in mesh.indices.chunks(3) {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            *count.entry((a.min(b), a.max(b))).or_default() += 1;
            directed.push((a, b));
        }
    }
    directed
        .into_iter()
        .filter(|&(a, b)| count[&(a.min(b), a.max(b))] == 1)
        .map(|(a, b)| (point(mesh, a), point(mesh, b)))
        .collect()
}

/// The chosen chunk whose footprint holds `p` (each chunk holds its
/// smallest sides, not its largest), if any.
fn holder(selection: &ChunkSelection, p: DVec2) -> Option<ChunkKey> {
    selection.iter().map(|(k, _)| k).find(|k| {
        let (lo, hi) = (k.min_corner(), k.max_corner());
        lo.x <= p.x && p.x < hi.x && lo.y <= p.y && p.y < hi.y
    })
}

/// Meshes every chunk of `selection` from `source`, with no skirts.
fn meshes(source: &impl ChunkSource, selection: &ChunkSelection) -> Vec<ChunkMesh> {
    selection
        .iter()
        .map(|(key, transitions)| {
            ChunkSamples::sample(source, key, FeatureTier::Fine)
                .expect("the terrain has ground everywhere")
                .mesh(transitions, 0.0)
        })
        .collect()
}

/// Asserts that every open edge of `meshes` is matched by the same edge,
/// run the other way, in another chunk's mesh, unless it lies on the
/// outside of the drawn area. A T-junction or a height mismatch leaves an
/// edge unmatched.
fn assert_watertight(selection: &ChunkSelection, meshes: &[ChunkMesh]) {
    let mut owner: HashMap<(Point, Point), ChunkKey> = HashMap::new();
    for mesh in meshes {
        for edge in open_edges(mesh) {
            owner.insert(edge, mesh.key);
        }
    }
    let mut inner = 0;
    for (&(a, b), &key) in &owner {
        if let Some(&other) = owner.get(&(b, a)) {
            assert_ne!(other, key, "a chunk's open edge is matched by itself");
            inner += 1;
            continue;
        }
        // Unmatched: it must be on the outside, with nothing drawn beyond.
        let (pa, pb) = (xz(a), xz(b));
        let mid = (pa + pb) * 0.5;
        let across = (pb - pa).normalize().perp() * 0.25;
        let outside = [mid + across, mid - across]
            .into_iter()
            .find(|&q| holder(selection, q) != Some(key))
            .expect("one side of an edge is outside its chunk");
        assert_eq!(
            holder(selection, outside),
            None,
            "{key}: open edge from {pa} to {pb} is not matched by its neighbour"
        );
    }
    assert!(inner > 0, "some chunks meet");
}

#[test]
fn chosen_chunks_tile_the_view_and_are_balanced() {
    let lod = test_lod();
    let viewer = DVec3::new(130.0, 2.0, -70.0);
    let selection = ChunkSelection::around(&lod, 1.0, &[viewer], None).unwrap();
    let levels: HashSet<u8> = selection.iter().map(|(k, _)| k.level).collect();
    assert_eq!(levels.len(), 4, "every level is used: {levels:?}");

    // The chunk under the viewer is at level 0, and every chunk ends up
    // within reach of the view.
    let here = holder(&selection, DVec2::new(viewer.x, viewer.z)).unwrap();
    assert_eq!(here.level, 0);

    // No two chunks overlap: the chunks' areas add up to the area of
    // their union, measured on a 4 m grid of points.
    let total: f64 = selection.iter().map(|(k, _)| k.size() * k.size()).sum();
    let mut covered = 0.0;
    for z in -400..400 {
        for x in -400..400 {
            let p = DVec2::new(f64::from(x) * 4.0 + 2.0, f64::from(z) * 4.0 + 2.0);
            let holding = selection
                .iter()
                .filter(|(k, _)| {
                    let (lo, hi) = (k.min_corner(), k.max_corner());
                    lo.x <= p.x && p.x < hi.x && lo.y <= p.y && p.y < hi.y
                })
                .count();
            assert!(holding <= 1, "{holding} chunks hold {p}");
            covered += holding as f64 * 16.0;
        }
    }
    assert_eq!(covered, total, "the grid sees every chunk whole");

    // Neighbours along a side or across a corner are at most a level
    // apart, and each chunk's transitions name exactly its finer ones.
    for (key, transitions) in selection.iter() {
        let (lo, hi) = (key.min_corner(), key.max_corner());
        let centre = (lo + hi) * 0.5;
        let half = key.size() * 0.5;
        for side in ChunkSide::ALL {
            let (dx, dz) = side.direction();
            // Probe a little past the side, near both of its ends.
            let along = DVec2::new(dz as f64, dx as f64).abs() * (half - 0.5);
            let out = centre + DVec2::new(dx as f64, dz as f64) * (half + 0.5);
            let mut finer = false;
            for probe in [out + along, out - along] {
                if let Some(other) = holder(&selection, probe) {
                    assert!(
                        other.level.abs_diff(key.level) <= 1,
                        "{key} touches {other}"
                    );
                    finer |= other.level < key.level;
                }
            }
            assert_eq!(transitions.side(side), finer, "{key}, side {side:?}");
        }
        for corner in procgena_terrain::ChunkCorner::ALL {
            let (dx, dz) = corner.direction();
            let probe = centre + DVec2::new(dx as f64, dz as f64) * (half + 0.5);
            if let Some(other) = holder(&selection, probe) {
                assert!(
                    other.level.abs_diff(key.level) <= 1,
                    "{key} touches {other}"
                );
                assert_eq!(transitions.corner(corner), other.level < key.level);
            } else {
                assert!(!transitions.corner(corner));
            }
        }
    }
}

#[test]
fn a_viewer_high_above_the_ground_sees_coarser_chunks() {
    let lod = test_lod();
    let low = ChunkSelection::around(&lod, 1.0, &[DVec3::new(0.0, 2.0, 0.0)], None).unwrap();
    let high = ChunkSelection::around(&lod, 1.0, &[DVec3::new(0.0, 300.0, 0.0)], None).unwrap();
    let finest = |s: &ChunkSelection| s.iter().map(|(k, _)| k.level).min().unwrap();
    assert_eq!(finest(&low), 0);
    // 300 m up, level 2 (from 192 m) is the finest.
    assert_eq!(finest(&high), 2);
    assert!(high.len() < low.len());
    // Lower quality pulls coarse chunks closer.
    let coarse = ChunkSelection::around(&lod, 0.5, &[DVec3::new(0.0, 2.0, 0.0)], None).unwrap();
    assert!(coarse.len() < low.len());
    // Beyond the cull distance nothing is drawn.
    let gone = ChunkSelection::around(&lod, 1.0, &[DVec3::new(0.0, 1200.0, 0.0)], None).unwrap();
    assert!(gone.is_empty());
}

#[test]
fn hysteresis_keeps_chunks_while_the_viewer_wavers() {
    let lod = test_lod();
    // Level 1 starts at 96 m: from chunk (0, 0) at level 1 (0 to 128 m),
    // a viewer 95 m off its smallest x side wants it split, at 97 m not.
    let at = |x: f64| DVec3::new(x, 0.0, 64.0);
    let key = ChunkKey::new(0, 0, 1);
    let split = |s: &ChunkSelection| s.get(key).is_none();

    let near = ChunkSelection::around(&lod, 1.0, &[at(-95.0)], None).unwrap();
    let far = ChunkSelection::around(&lod, 1.0, &[at(-97.0)], None).unwrap();
    assert!(
        split(&near) && !split(&far),
        "without hysteresis, it switches at 96 m"
    );

    // With the last selection, 97 m keeps it split and 95 m keeps it whole.
    let kept = ChunkSelection::around(&lod, 1.0, &[at(-97.0)], Some(&near)).unwrap();
    assert!(split(&kept));
    let kept = ChunkSelection::around(&lod, 1.0, &[at(-95.0)], Some(&far)).unwrap();
    assert!(!split(&kept));
    // Well past the band, it switches.
    let merged = ChunkSelection::around(&lod, 1.0, &[at(-110.0)], Some(&near)).unwrap();
    assert!(!split(&merged));
}

#[test]
fn the_usual_levels_fit_any_view_distance() {
    let viewer = [DVec3::ZERO];
    // Every view distance gives levels that can choose chunks, with only
    // the levels that start inside it.
    for (view, levels) in [
        (50.0, 1),
        (300.0, 1),
        (320.0, 2),
        (3000.0, 5),
        (4000.0, 5),
        (6000.0, 6),
        (12000.0, 7),
    ] {
        let lod = chunk_lod(view);
        assert_eq!(lod.levels.len(), levels, "view distance {view} m");
        assert_eq!(lod.cull_distance, Some(view));
        let selection = ChunkSelection::around(&lod, 1.0, &viewer, None).unwrap();
        assert!(!selection.is_empty());
    }
    // Levels without a cull distance, or a factor that isn't positive, are
    // refused.
    let mut endless = chunk_lod(6000.0);
    endless.cull_distance = None;
    assert!(ChunkSelection::around(&endless, 1.0, &viewer, None).is_err());
    assert!(ChunkSelection::around(&chunk_lod(6000.0), 0.0, &viewer, None).is_err());
}

#[test]
fn each_level_takes_over_at_its_size_on_screen() {
    // Where a level takes over, one of its cells covers the detail's share
    // of the screen, for any camera.
    for view in [ViewScale::DEFAULT, ViewScale::perspective(0.4)] {
        for cell_on_screen in [0.004, 0.01, 0.03] {
            let detail = ChunkDetail {
                cell_on_screen,
                view,
            };
            let lod = detail.lod(1.0e6);
            assert_eq!(lod.levels.len(), 7);
            for (level, l) in lod.levels.iter().enumerate().skip(1) {
                let cell = ChunkKey::new(0, 0, level as u8).step() as f32;
                let on_screen = cell * view.get() / l.distance;
                assert!(
                    (on_screen - cell_on_screen).abs() < 1e-6,
                    "level {level}: {on_screen} of the screen"
                );
            }
        }
    }
    // A narrower field of view keeps each level farther out.
    let wide = ChunkDetail::DEFAULT.lod(1.0e6);
    let narrow = ChunkDetail {
        view: ViewScale::perspective(0.4),
        ..ChunkDetail::DEFAULT
    }
    .lod(1.0e6);
    assert!(narrow.levels[1].distance > wide.levels[1].distance);
    // The usual levels are the default detail's.
    assert_eq!(chunk_lod(4000.0), ChunkDetail::default().lod(4000.0));
}

#[test]
fn chunks_one_level_apart_join_without_cracks() {
    let (full, _) = plains();
    // Stand on a road, so roads cross chunk sides at several levels.
    let road = &full.roads()[0].points;
    let p = road[road.len() / 2];
    let selection =
        ChunkSelection::around(&test_lod(), 1.0, &[DVec3::new(p.x, 2.0, p.y)], None).unwrap();
    assert!(selection.iter().any(|(_, t)| t != Transitions::NONE));
    let meshes = meshes(&full, &selection);
    assert_watertight(&selection, &meshes);

    // Where chunks share a vertex, they shade it alike.
    let mut normals: HashMap<Point, (ChunkKey, glam::Vec3)> = HashMap::new();
    for mesh in &meshes {
        for (v, &n) in mesh.normals.iter().enumerate() {
            let at = point(mesh, v as u32);
            if let Some(&(other, seen)) = normals.get(&at) {
                assert!(
                    seen.abs_diff_eq(n, 1e-6),
                    "{} and {other} shade {} differently: {seen} and {n}",
                    mesh.key,
                    xz(at)
                );
            } else {
                normals.insert(at, (mesh.key, n));
            }
        }
    }
}

#[test]
fn a_chunk_without_finer_neighbours_is_a_plain_grid() {
    let (full, _) = plains();
    let samples = ChunkSamples::sample(&full, ChunkKey::new(-3, 2, 1), FeatureTier::Fine).unwrap();
    let cells = CHUNK_CELLS as usize;
    let plain = samples.mesh(Transitions::NONE, 0.0);
    assert_eq!(plain.positions.len(), (cells + 1) * (cells + 1));
    assert_eq!(plain.indices.len(), cells * cells * 6);
    // Every side split adds a vertex and a triangle per cell along it.
    let all = samples.mesh(Transitions::ALL, 0.0);
    assert_eq!(all.positions.len(), (cells + 1) * (cells + 1) + 4 * cells);
    assert_eq!(all.indices.len(), (2 * cells * cells + 4 * cells) * 3);
    // Skirts add a lowered copy of every side's vertices.
    let skirted = samples.mesh(Transitions::NONE, 5.0);
    assert_eq!(
        skirted.positions.len(),
        plain.positions.len() + 4 * (cells + 1)
    );
    assert_eq!(skirted.heights.0, plain.heights.0 - 5.0);
    // The vertices sit on the ground.
    let v = samples.vertex(10, 20);
    let at = ChunkKey::new(-3, 2, 1).min_corner() + DVec2::new(20.0, 40.0);
    assert_eq!(v.height, full.height(at));

    // Level 0 has no finer level, so its transitions are ignored.
    let finest = ChunkSamples::sample(&full, ChunkKey::new(-3, 2, 0), FeatureTier::Fine).unwrap();
    let mesh = finest.mesh(Transitions::ALL, 0.0);
    assert_eq!(mesh.transitions, Transitions::NONE);
    assert_eq!(mesh, finest.mesh(Transitions::NONE, 0.0));
}

#[test]
fn rough_chunks_match_fine_ones_away_from_roads() {
    let (full, bare) = plains();
    assert_eq!(
        ChunkSource::tier(&full, FeatureTier::Rough),
        FeatureTier::Fine
    );
    assert_eq!(
        ChunkSource::tier(&bare, FeatureTier::Fine),
        FeatureTier::Rough
    );

    // A level 2 chunk a road runs through.
    let road = &full.roads()[0].points;
    let key = ChunkKey::containing(road[road.len() / 2], 2);
    let fine = ChunkSamples::sample(&full, key, FeatureTier::Fine).unwrap();
    let rough = ChunkSamples::sample(&bare, key, FeatureTier::Fine).unwrap();
    assert_eq!(fine.tier(), FeatureTier::Fine);
    assert_eq!(rough.tier(), FeatureTier::Rough);
    let a = fine.mesh(Transitions::NONE, 0.0);
    let b = rough.mesh(Transitions::NONE, 0.0);
    assert_eq!(b.tier, FeatureTier::Rough);

    // Outside every road's bounds, the ground is the same.
    let roads: Vec<[DVec2; 2]> = full
        .features()
        .filter(|f| f.id.kind == procgena_terrain::FeatureKind::Road)
        .map(|f| f.bounds)
        .collect();
    let (mut same, mut differ) = (0, 0);
    for (p, q) in a.positions.iter().zip(&b.positions) {
        let at = a.origin + DVec2::new(f64::from(p.x), f64::from(p.z));
        let near_road = roads
            .iter()
            .any(|[lo, hi]| lo.x <= at.x && at.x <= hi.x && lo.y <= at.y && at.y <= hi.y);
        if near_road {
            differ += usize::from(p.y != q.y);
        } else {
            assert_eq!(p.y.to_bits(), q.y.to_bits(), "{at} is away from the roads");
            same += 1;
        }
    }
    assert!(
        same > 0 && differ > 0,
        "{same} vertices alike, {differ} cut by roads"
    );
}

#[test]
fn skirts_sized_by_the_tier_difference_close_gaps_between_tiers() {
    let (full, bare) = plains();
    let moved = ChunkSource::tier_difference(&full, DVec2::ZERO, DVec2::ZERO, FeatureTier::Rough);
    assert!(moved > 1.0, "the roads move the ground: {moved} m");
    assert_eq!(full.tier_difference(FeatureTier::Fine), 0.0);
    assert_eq!(bare.tier_difference(FeatureTier::Rough), 0.0);

    // Chunks at levels 0 to 2 along every road, where it crosses from one
    // chunk into the next: one drawn fine, its neighbour rough.
    let mut checked = 0;
    let mut largest = 0.0f32;
    for road in full.roads() {
        for w in road.points.windows(2) {
            for level in 0..=2 {
                let (a, b) = (
                    ChunkKey::containing(w[0], level),
                    ChunkKey::containing(w[1], level),
                );
                if a == b || a.level != b.level || (a.x - b.x).abs() + (a.z - b.z).abs() != 1 {
                    continue;
                }
                let fine = ChunkSamples::sample(&full, a, FeatureTier::Fine).unwrap();
                let rough = ChunkSamples::sample(&bare, b, FeatureTier::Fine).unwrap();
                let n = CHUNK_CELLS;
                // The vertex pairs along the shared side.
                let pairs: Vec<((u32, u32), (u32, u32))> = (0..=n)
                    .map(|k| match (b.x - a.x, b.z - a.z) {
                        (1, 0) => ((n, k), (0, k)),
                        (-1, 0) => ((0, k), (n, k)),
                        (0, 1) => ((k, n), (k, 0)),
                        _ => ((k, 0), (k, n)),
                    })
                    .collect();
                for ((fi, fj), (ri, rj)) in pairs {
                    let (hf, hr) = (fine.vertex(fi, fj).height, rough.vertex(ri, rj).height);
                    // The higher side's skirt, `moved` deep, reaches the
                    // lower side.
                    assert!(
                        hf.max(hr) - moved <= hf.min(hr),
                        "{a} fine and {b} rough part by {} m, more than {moved} m",
                        (hf - hr).abs()
                    );
                    largest = largest.max((hf - hr).abs());
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 0, "roads cross chunk sides");
    assert!(largest > 0.5, "the road moves the shared side: {largest} m");
}

#[test]
fn rivers_get_a_water_surface() {
    let (full, _) = plains();
    let river = &full.rivers()[0].points;
    let p = river[river.len() / 2];
    let key = ChunkKey::containing(p, 0);
    let mesh = ChunkSamples::sample(&full, key, FeatureTier::Fine)
        .unwrap()
        .mesh(Transitions::NONE, 0.0);
    let water = mesh.water.as_ref().expect("the river is in the chunk");
    assert_eq!(water.indices.len() % 3, 0);
    // Water stands at the river's own level wherever the river has water.
    let mut wet = 0;
    for w in &water.positions {
        let at = mesh.origin + DVec2::new(f64::from(w.x), f64::from(w.z));
        if let Some(level) = full.water_level(at) {
            assert_eq!(w.y, level);
            wet += 1;
        }
    }
    assert!(wet > 0);
    // A chunk far from water has none.
    let dry = ChunkSamples::sample(&full, ChunkKey::new(400, 400, 0), FeatureTier::Fine)
        .unwrap()
        .mesh(Transitions::NONE, 0.0);
    assert!(dry.water.is_none());
}

#[test]
fn a_world_gives_provisional_and_canonical_chunks() {
    // Small regions, so a few build quickly.
    let options = WorldTerrainOptions {
        regions: RegionGraphOptions {
            size: 2000.0,
            min_edge: procgena_terrain::MIN_EDGE,
            ..fixtures::regions_medium()
        },
        ..fixtures::world_medium()
    };
    let source = |_: RegionId, _: DVec2, _: &Elevation| fixtures::plains_medium().terrain.clone();
    let told: Arc<Mutex<Vec<RegionProgress>>> = Arc::default();
    let hook = {
        let told = Arc::clone(&told);
        Arc::new(move |p: &RegionProgress| told.lock().unwrap().push(*p))
    };
    let world = WorldTerrain::new(options, Arc::new(source))
        .unwrap()
        .with_progress(hook);

    let p = DVec2::new(100.0, -50.0);
    let region = world.region_at(p);
    let rough = world.terrain_tier(region, FeatureTier::Rough).unwrap();

    // Building the rough tier planned the region and every region round
    // it, and built only this region's rough ground.
    let after_ground = told.lock().unwrap().clone();
    let finished = |events: &[RegionProgress], stage| -> Vec<RegionId> {
        events
            .iter()
            .filter_map(|e| match *e {
                RegionProgress::Finished {
                    region,
                    stage: s,
                    built,
                    ..
                } if s == stage => {
                    assert!(built);
                    Some(region)
                }
                _ => None,
            })
            .collect()
    };
    let planned = finished(&after_ground, RegionStage::Plan);
    assert!(
        planned.contains(&region) && planned.len() > 3,
        "{planned:?}"
    );
    let rough_stage = RegionStage::Tier(FeatureTier::Rough);
    let fine_stage = RegionStage::Tier(FeatureTier::Fine);
    assert_eq!(finished(&after_ground, rough_stage), vec![region]);
    assert!(finished(&after_ground, fine_stage).is_empty());
    // Every stage that started also finished, after it started.
    for (i, e) in after_ground.iter().enumerate() {
        if let RegionProgress::Started { region, stage } = *e {
            assert!(after_ground[i..].iter().any(|later| matches!(
                *later,
                RegionProgress::Finished { region: r, stage: s, .. } if r == region && s == stage
            )));
        }
    }

    // The fine tier adds only the roads; nothing is built twice.
    let fine = world.terrain(region).unwrap();
    let all = told.lock().unwrap().clone();
    assert_eq!(
        finished(&all[after_ground.len()..], fine_stage),
        vec![region]
    );
    assert_eq!(all.len(), after_ground.len() + 2);
    assert!(rough.tier() == FeatureTier::Rough && fine.tier() == FeatureTier::Fine);
    assert!(rough.roads().is_empty() && !fine.roads().is_empty());
    assert_eq!(rough.rivers(), fine.rivers());
    // With the fine tier built, the rough tier is still the rough one.
    let again = world.terrain_tier(region, FeatureTier::Rough).unwrap();
    assert!(Arc::ptr_eq(&again, &rough));

    // A coarse chunk from each tier: the tier is what was asked for, and
    // the heights are each tier's own.
    let key = ChunkKey::containing(p, 3);
    for (tier, terrain) in [(FeatureTier::Rough, &rough), (FeatureTier::Fine, &fine)] {
        let samples = ChunkSamples::sample(&world, key, tier).unwrap();
        assert_eq!(samples.tier(), tier);
        let v = samples.vertex(32, 32);
        let at = key.min_corner() + DVec2::splat(32.0 * key.step());
        if world.region_at(at) == region {
            assert_eq!(v.height, terrain.height(at));
        }
    }
}
