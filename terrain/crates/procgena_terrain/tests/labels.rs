//! Labels: right on known rivers, ponds, roads, hilltops and hollows, the
//! same every time, and the same on both sides of every border.

mod common;

use common::{DEFAULT, RIVER_SEEDS, WARPED, hills_into_plains, triple_junction};
use glam::{DVec2, I64Vec2, UVec2};
use procgena_terrain::{
    BANK_HEIGHT, BANK_WIDTH, CLIFF_SLOPE, FeatureLine, Flags, HeightTile, LabelField, Labels,
    Landform, MEASURE_REACH, RoadTags, SHAPE_CURVATURE, SHAPE_STEP, STEEP_SLOPE, SampleDetail,
    StitchedWorld, Terrain, TerrainSample, WET_HEIGHT, WorldScale,
};

/// The scale of the test worlds' terrain settings: medium, twice as long
/// and as high as scale 1.
const MEDIUM: WorldScale = WorldScale {
    horizontal: 2.0,
    vertical: 2.0,
};

/// The step and bend threshold at which labels read shapes at [`MEDIUM`].
fn shape() -> (f64, f32) {
    let WorldScale {
        horizontal: h,
        vertical: v,
    } = MEDIUM;
    (
        SHAPE_STEP * h,
        (f64::from(SHAPE_CURVATURE) * v / (h * h)) as f32,
    )
}

/// The test worlds, at [`MEDIUM`] scale: plains and hills with a river
/// crossing their border (one per river seed), and three regions meeting,
/// on flat land and with warped borders. Built once and shared by every
/// test.
fn worlds() -> &'static [StitchedWorld] {
    static WORLDS: std::sync::OnceLock<Vec<StitchedWorld>> = std::sync::OnceLock::new();
    WORLDS.get_or_init(|| {
        let mut worlds = Vec::new();
        for config in [DEFAULT, WARPED[0]] {
            worlds.extend(RIVER_SEEDS.map(|s| hills_into_plains(s, config).with_scale(MEDIUM)));
            worlds.push(triple_junction(config).with_scale(MEDIUM));
        }
        worlds
    })
}

/// Everything `terrain` says about `p`, labels included.
fn labelled(terrain: &Terrain, p: DVec2) -> (TerrainSample, Labels) {
    let sample = terrain.query(p, SampleDetail::Labels);
    let labels = sample.labels.expect("labels were asked for");
    (sample, labels)
}

/// Points spread over `terrain`'s region, inside it, about `step` apart.
fn points_in(terrain: &Terrain, step: f64) -> Vec<DVec2> {
    let boundary = &terrain.region().boundary;
    let procgena_terrain::Boundary::Polygon(points) = boundary else {
        panic!("test regions have polygons");
    };
    let (lo, hi) = points
        .iter()
        .fold((DVec2::INFINITY, DVec2::NEG_INFINITY), |(lo, hi), &q| {
            (lo.min(q), hi.max(q))
        });
    let mut out = Vec::new();
    let mut z = lo.y + 0.37 * step;
    while z < hi.y {
        let mut x = lo.x + 0.61 * step;
        while x < hi.x {
            let p = DVec2::new(x, z);
            if boundary.contains(p) {
                out.push(p);
            }
            x += step;
        }
        z += step;
    }
    out
}

/// The middle of every segment of every line in `lines` inside `terrain`'s
/// region, with the way across the line there and its half width.
fn along_lines(terrain: &Terrain, lines: &[FeatureLine]) -> Vec<(DVec2, DVec2, f64)> {
    let mut out = Vec::new();
    for line in lines {
        for (ends, widths) in line.points.windows(2).zip(line.widths.windows(2)) {
            let middle = (ends[0] + ends[1]) * 0.5;
            let across = (ends[1] - ends[0]).perp().normalize_or_zero();
            if across != DVec2::ZERO && terrain.region().boundary.contains(middle) {
                let half = 0.25 * f64::from(widths[0] + widths[1]);
                out.push((middle, across, half));
            }
        }
    }
    out
}

/// The share of `count` in `total`, in per cent.
fn percent(count: usize, total: usize) -> f64 {
    100.0 * count as f64 / total.max(1) as f64
}

#[test]
fn water_and_roads_get_their_labels() {
    let (mut beds, mut river_points) = (0, 0);
    let (mut banks, mut bank_candidates) = (0, 0);
    let (mut roads, mut road_points) = (0, 0);
    let (mut ponds, mut cliffs) = (0, 0);
    for world in worlds() {
        for terrain in world.terrains() {
            // Wherever water covers the ground, its bed, under water.
            for &(p, across, half) in &along_lines(terrain, terrain.rivers()) {
                let (sample, labels) = labelled(terrain, p);
                river_points += 1;
                if sample.water.is_some() {
                    assert!(
                        matches!(labels.landform, Landform::Riverbed | Landform::Pondbed),
                        "{p}: {labels:?} under water"
                    );
                    assert!(labels.flags.contains(Flags::UNDERWATER), "{p}: {labels:?}");
                }
                if labels.landform == Landform::Riverbed {
                    assert!(labels.flags.contains(Flags::RIVER), "{p}: {labels:?}");
                    beds += 1;
                }

                // Low dry ground just beside the river: its bank, unless a
                // road or a cliff is there, or a pond is as near.
                for out in [1.0, 3.0, 6.0] {
                    for side in [-1.0, 1.0] {
                        let q = p + across * side * (half + out);
                        if !terrain.region().boundary.contains(q) {
                            continue;
                        }
                        let (sample, labels) = labelled(terrain, q);
                        let m = sample.measures.expect("labels come with measures");
                        let low = m.height_above_water.is_some_and(|h| h <= BANK_HEIGHT);
                        let plain_ground = sample.water.is_none()
                            && sample.tags.is_empty()
                            && m.slope < CLIFF_SLOPE
                            && m.water_distance <= BANK_WIDTH;
                        if low && plain_ground && !labels.flags.contains(Flags::POND) {
                            bank_candidates += 1;
                            assert_eq!(labels.landform, Landform::Riverbank, "{q}: {m:?}");
                            assert!(labels.flags.contains(Flags::RIVER), "{q}: {labels:?}");
                            if m.height_above_water.is_some_and(|h| h < WET_HEIGHT) {
                                assert!(labels.flags.contains(Flags::WET), "{q}: {m:?}");
                            }
                            banks += 1;
                        }
                    }
                }
            }

            // A pond's middle is its bed.
            for pond in terrain.ponds() {
                if terrain.region().boundary.contains(pond.centre) {
                    let labels = terrain.annotate(pond.centre);
                    assert_eq!(
                        labels.landform,
                        Landform::Pondbed,
                        "{}: water {:?}, road tags {:?}, pond {pond:?}",
                        pond.centre,
                        terrain.water_level(pond.centre),
                        terrain.road_tags(pond.centre).names()
                    );
                    assert!(labels.flags.contains(Flags::UNDERWATER | Flags::POND));
                    ponds += 1;
                }
            }

            // A road's middle is road, unless it runs through water.
            for &(p, _, _) in &along_lines(terrain, terrain.roads()) {
                let (sample, labels) = labelled(terrain, p);
                if sample.water.is_none() {
                    road_points += 1;
                    if labels.landform == Landform::Road {
                        assert!(labels.flags.contains(Flags::ROAD));
                        roads += 1;
                    }
                }
            }

            // A road bank of 65° or more is a cliff.
            for p in points_in(terrain, 8.0) {
                let (sample, labels) = labelled(terrain, p);
                if sample.tags.contains(RoadTags::CLIFF) && !sample.tags.contains(RoadTags::ROAD) {
                    assert_eq!(labels.landform, Landform::Cliff, "{p}");
                    assert!(labels.flags.contains(Flags::ROAD_BANK), "{p}");
                    cliffs += 1;
                }
            }
        }
    }
    eprintln!(
        "riverbed {beds} of {river_points} river points ({:.1}%), riverbank {banks} of {bank_candidates}, road {roads} of {road_points} ({:.1}%), {ponds} ponds, {cliffs} road cliffs",
        percent(beds, river_points),
        percent(roads, road_points),
    );
    assert!(river_points > 500 && beds * 100 >= river_points * 95);
    assert!(road_points > 500 && roads * 100 >= road_points * 99);
    assert!(banks > 200, "only {banks} bank points");
    assert!(ponds > 0, "no ponds in the test worlds");
}

/// The bends of `terrain` at `p` across its most convex and most concave
/// directions, as the labels read them: second differences of the height
/// at step `s` (see [`shape`]), worked out here on their own.
fn bends(terrain: &Terrain, p: DVec2, s: f64) -> [f32; 2] {
    let h = |x: f64, z: f64| f64::from(terrain.height(p + DVec2::new(x, z)));
    let xx = (h(s, 0.0) + h(-s, 0.0) - 2.0 * h(0.0, 0.0)) / (s * s);
    let zz = (h(0.0, s) + h(0.0, -s) - 2.0 * h(0.0, 0.0)) / (s * s);
    let xz = (h(s, s) - h(s, -s) - h(-s, s) + h(-s, -s)) / (4.0 * s * s);
    let mean = 0.5 * (xx + zz);
    let spread = (0.25 * (xx - zz).powi(2) + xz * xz).sqrt();
    [-(mean - spread) as f32, -(mean + spread) as f32]
}

/// The spacing, in metres, of the grid that [`hilltops_and_hollows`]
/// searches.
const GRID: f64 = 16.0;

/// How many grid steps around a hilltop or hollow it must be the highest
/// or lowest point.
const WINDOW: i64 = 3;

/// How far, in metres, a hilltop must stand above (or a hollow sink
/// below) the mean of the eight grid points [`WINDOW`] steps around it.
const PROMINENCE: f32 = 1.5;

/// The hilltops and hollows of `terrain`, found on its own heights apart
/// from the labels: grid points [`GRID`] apart, inside its region and
/// clear of its border, that are the highest (or lowest) within
/// [`WINDOW`] steps and stand out by [`PROMINENCE`].
fn hilltops_and_hollows(terrain: &Terrain) -> (Vec<DVec2>, Vec<DVec2>) {
    let procgena_terrain::Boundary::Polygon(corners) = &terrain.region().boundary else {
        panic!("test regions have polygons");
    };
    let (lo, hi) = corners
        .iter()
        .fold((DVec2::INFINITY, DVec2::NEG_INFINITY), |(lo, hi), &q| {
            (lo.min(q), hi.max(q))
        });
    let origin = (lo / GRID).floor().as_i64vec2();
    let size = ((hi - lo) / GRID).ceil().as_uvec2() + UVec2::splat(2);
    let tile = HeightTile::sample(terrain, origin, size, GRID);
    let h = |i: i64, j: i64| tile.get(i as u32, j as u32);

    let (mut tops, mut hollows) = (Vec::new(), Vec::new());
    for j in WINDOW..i64::from(size.y) - WINDOW {
        for i in WINDOW..i64::from(size.x) - WINDOW {
            let p = (origin + I64Vec2::new(i, j)).as_dvec2() * GRID;
            let boundary = &terrain.region().boundary;
            if !boundary.contains(p) || boundary.distance_to_border(p) < 2.0 * GRID * WINDOW as f64
            {
                continue;
            }
            let centre = h(i, j);
            let window = (-WINDOW..=WINDOW)
                .flat_map(|dj| (-WINDOW..=WINDOW).map(move |di| (di, dj)))
                .filter(|&d| d != (0, 0))
                .map(|(di, dj)| h(i + di, j + dj));
            let (low, high) = window.fold((f32::INFINITY, f32::NEG_INFINITY), |(l, u), v| {
                (l.min(v), u.max(v))
            });
            let ring = [
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
            ];
            let mean = ring
                .iter()
                .map(|&(di, dj)| h(i + di * WINDOW, j + dj * WINDOW))
                .sum::<f32>()
                / 8.0;
            if centre > high && centre - mean >= PROMINENCE {
                tops.push(p);
            } else if centre < low && mean - centre >= PROMINENCE {
                hollows.push(p);
            }
        }
    }
    (tops, hollows)
}

#[test]
fn hilltops_and_hollows_get_their_labels() {
    // The labels of hilltops and hollows found apart from the labels, away
    // from water and roads.
    let mut tops = std::collections::BTreeMap::<Landform, usize>::new();
    let mut hollows = std::collections::BTreeMap::<Landform, usize>::new();
    for world in worlds() {
        for terrain in world.terrains() {
            let (found_tops, found_hollows) = hilltops_and_hollows(terrain);
            for (points, counts) in [(found_tops, &mut tops), (found_hollows, &mut hollows)] {
                for p in points {
                    let (sample, labels) = labelled(terrain, p);
                    let m = sample.measures.expect("labels come with measures");
                    if m.water_distance < MEASURE_REACH || m.road_distance < MEASURE_REACH {
                        continue;
                    }
                    *counts.entry(labels.landform).or_default() += 1;
                }
            }
        }
    }
    eprintln!("hilltops: {tops:?}\nhollows: {hollows:?}");

    // Hilltops are mostly peaks or ridges and never valleys or basins;
    // hollows the other way. Some are cliffs or slopes (a top on a steep
    // flank), or plain where the shape is too broad to bend past the
    // threshold.
    let count =
        |map: &std::collections::BTreeMap<Landform, usize>, l| map.get(&l).copied().unwrap_or(0);
    let all_tops: usize = tops.values().sum();
    let all_hollows: usize = hollows.values().sum();
    assert!(
        all_tops > 50 && all_hollows > 20,
        "{all_tops} tops, {all_hollows} hollows"
    );
    assert_eq!(
        count(&tops, Landform::Valley) + count(&tops, Landform::Basin),
        0,
        "{tops:?}"
    );
    assert_eq!(
        count(&hollows, Landform::Ridge) + count(&hollows, Landform::Peak),
        0,
        "{hollows:?}"
    );
    assert!(
        count(&tops, Landform::Peak) + count(&tops, Landform::Ridge) > all_tops / 2,
        "{tops:?}"
    );
    assert!(
        count(&hollows, Landform::Basin) + count(&hollows, Landform::Valley) > all_hollows / 2,
        "{hollows:?}"
    );
}

#[test]
fn labels_are_the_same_every_time_and_every_way() {
    let world = &worlds()[0];
    let again = hills_into_plains(RIVER_SEEDS[0], DEFAULT).with_scale(MEDIUM);
    let (origin, size, step) = (I64Vec2::new(-120, -120), UVec2::splat(240), 10.0);
    let tile = HeightTile::sample_labelled(world, origin, size, step);
    for j in 0..size.y {
        for i in 0..size.x {
            let p = (origin + I64Vec2::new(i64::from(i), i64::from(j))).as_dvec2() * step;
            let labels = world.annotate(p);
            // The same from a tile, a world, a rebuilt world and a query.
            assert_eq!(tile.labels(i, j), labels, "{p}");
            assert_eq!(again.annotate(p), labels, "{p}");
            let terrain = world.owner(p).expect("the tile is inside the world");
            assert_eq!(terrain.query(p, SampleDetail::Labels).labels, labels, "{p}");
            assert_eq!(tile.get(i, j), terrain.height(p));
        }
    }
    // Lower details give no labels.
    let p = DVec2::new(10.0, 20.0);
    let terrain = world.owner(p).unwrap();
    assert_eq!(terrain.query(p, SampleDetail::Measures).labels, None);
    assert_eq!(HeightTile::sample(world, origin, size, step).labels, None);
}

/// Points along every shared border of `world`, each with the two terrains
/// on either side.
fn border_points(world: &StitchedWorld) -> Vec<(DVec2, &Terrain, &Terrain)> {
    let mut out = Vec::new();
    for a in world.terrains() {
        for edge in &a.region().edges {
            let Some(id) = edge.neighbor else { continue };
            if id < a.region().id {
                continue;
            }
            let b = world
                .terrains()
                .iter()
                .find(|t| t.region().id == id)
                .expect("the neighbour is built");
            for w in edge.polyline.windows(2) {
                let n = (w[0].distance(w[1]) / 2.0).ceil().max(1.0) as usize;
                for k in 0..n {
                    out.push((w[0].lerp(w[1], k as f64 / n as f64), a, b));
                }
            }
        }
    }
    out
}

/// Whether `a` and `b`, the same point's value on two sides of a border,
/// lie on either side of, or within `margin` of, the threshold `t`.
fn straddles(a: f32, b: f32, t: f32, margin: f32) -> bool {
    (a.min(b) - margin..=a.max(b) + margin).contains(&t)
}

/// Whether `landform` comes from water or a road: the labels that must
/// agree exactly on both sides of a border.
fn from_water_or_road(landform: Landform) -> bool {
    matches!(
        landform,
        Landform::Riverbed | Landform::Pondbed | Landform::Road
    )
}

#[test]
fn labels_agree_across_borders() {
    // Each side of a border reads its own ground past the border, so shape
    // labels may differ in a thin strip there (decided by the user,
    // 2026-11-25). Water, road and flag labels must agree exactly.
    let (mut points, mut differ, mut near_features) = (0, 0, 0);
    let mut gaps = Vec::new();
    let mut pairs = std::collections::BTreeMap::new();
    let (step, bend) = shape();
    for (w, world) in worlds().iter().enumerate() {
        for (p, a, b) in border_points(world) {
            let ((sa, la), (sb, lb)) = (labelled(a, p), labelled(b, p));
            let (ma, mb) = (sa.measures.unwrap(), sb.measures.unwrap());
            let (ba, bb) = (bends(a, p, step), bends(b, p, step));
            points += 1;
            if ma.water_distance < MEASURE_REACH || ma.road_distance < MEASURE_REACH {
                near_features += 1;
            }
            gaps.push((ba[0] - bb[0]).abs().max((ba[1] - bb[1]).abs()));

            // Everywhere: the same water, roads and flags, and the same
            // landform wherever either side's comes from water or a road.
            let here = format!("world {w}, {p}: {la:?} against {lb:?}");
            assert_eq!(sa.water.is_some(), sb.water.is_some(), "{here}");
            assert_eq!(sa.tags, sb.tags, "{here}");
            assert_eq!(la.flags, lb.flags, "{here}");
            if from_water_or_road(la.landform) || from_water_or_road(lb.landform) {
                assert_eq!(la.landform, lb.landform, "{here}");
            }
            if la == lb {
                continue;
            }

            // Elsewhere labels may differ only where the two sides' values
            // lie on either side of a threshold.
            differ += 1;
            *pairs
                .entry((la.landform.min(lb.landform), la.landform.max(lb.landform)))
                .or_insert(0usize) += 1;
            let hair = 1e-3;
            let above = |h: Option<f32>| h.unwrap_or(f32::INFINITY);
            let (ha, hb) = (above(ma.height_above_water), above(mb.height_above_water));
            let explained = straddles(ma.slope, mb.slope, CLIFF_SLOPE, hair)
                || straddles(ma.slope, mb.slope, STEEP_SLOPE, hair)
                || [bend, -bend].iter().any(|&t| {
                    straddles(ba[0], bb[0], t, hair * bend)
                        || straddles(ba[1], bb[1], t, hair * bend)
                })
                // A saddle as bent up as down: ridge or valley either way.
                || (ba[0] + ba[1]).abs().min((bb[0] + bb[1]).abs()) < 1e-5
                || straddles(ha, hb, BANK_HEIGHT, hair);
            assert!(
                explained,
                "{here}\n  {ma:?} bends {ba:?}\n  {mb:?} bends {bb:?}"
            );
        }
    }

    // How far apart the two sides' bends are, against the threshold.
    gaps.sort_by(f32::total_cmp);
    let share = |q: f64| gaps[((gaps.len() - 1) as f64 * q) as usize] / bend;
    let (p50, p99, most) = (share(0.5), share(0.99), share(1.0));
    eprintln!(
        "labels differ at {differ} of {points} border points ({:.2}%): {pairs:?}; {near_features} near water or roads; bend gap over the threshold p50 {p50:.3}, p99 {p99:.3}, max {most:.3}",
        percent(differ, points)
    );
    assert!(
        near_features > 50,
        "only {near_features} border points near features"
    );
    // Measured 2026-11-25 at medium scale: 2.9% differ; gap p50 0.07,
    // p99 0.80, max 1.53 of the threshold.
    assert!(
        differ * 20 <= points,
        "labels differ at {differ} of {points} border points"
    );
    assert!(
        p50 < 0.15 && p99 < 1.0 && most < 2.0,
        "bend gaps {p50} {p99} {most}"
    );
}

#[test]
fn a_world_gives_its_scale_to_every_terrain() {
    use procgena_terrain::{
        Elevation, RegionId, TerrainPreset, WorldError, WorldTerrain, WorldTerrainOptions,
    };
    let source = |_: RegionId, _: DVec2, _: &Elevation| TerrainPreset::plains().terrain.clone();
    let source = std::sync::Arc::new(source);

    // Unspecified, it is scale 1; given, every terrain has it.
    let world = WorldTerrain::new(WorldTerrainOptions::default(), source.clone()).unwrap();
    assert_eq!(
        world.terrain_at(DVec2::ZERO).unwrap().scale(),
        WorldScale::default()
    );
    let options = WorldTerrainOptions {
        scale: MEDIUM,
        ..WorldTerrainOptions::default()
    };
    let world = WorldTerrain::new(options, source.clone()).unwrap();
    assert_eq!(world.terrain_at(DVec2::ZERO).unwrap().scale(), MEDIUM);

    // A scale that isn't finite and positive is refused.
    let bad = WorldScale {
        horizontal: 0.0,
        vertical: 1.0,
    };
    let options = WorldTerrainOptions {
        scale: bad,
        ..WorldTerrainOptions::default()
    };
    assert_eq!(
        WorldTerrain::new(options, source).err(),
        Some(WorldError::Scale(bad))
    );
}
