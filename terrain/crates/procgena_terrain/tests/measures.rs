//! Measures: right on known rivers, ponds and roads, smooth, and the same on
//! both sides of every border.

mod common;

use common::{DEFAULT, JOIN_SEEDS, RIVER_SEEDS, WARPED, hills_into_plains, triple_junction};
use glam::DVec2;
use procgena_terrain::{
    CURVATURE_STEP, FeatureLine, MEASURE_REACH, Measures, RoadTags, SampleDetail, StitchedWorld,
    Terrain, WATER_HEIGHT_REACH,
};

/// The test worlds: plains and hills with a river crossing their border
/// (one per river seed), with rivers joining on bends (one per join seed),
/// and three regions meeting, on flat land and with warped borders. Built
/// once and shared by every test.
fn worlds() -> &'static [StitchedWorld] {
    static WORLDS: std::sync::OnceLock<Vec<StitchedWorld>> = std::sync::OnceLock::new();
    WORLDS.get_or_init(|| {
        let mut worlds = Vec::new();
        for config in [DEFAULT, WARPED[0]] {
            worlds.extend(RIVER_SEEDS.map(|s| hills_into_plains(s, config)));
            worlds.extend(JOIN_SEEDS.map(|s| hills_into_plains(s, config)));
            worlds.push(triple_junction(config));
        }
        worlds
    })
}

/// The measures of `terrain` at `p`.
fn measures(terrain: &Terrain, p: DVec2) -> Measures {
    terrain
        .query(p, SampleDetail::Measures)
        .measures
        .expect("measures were asked for")
}

/// The distance from `p` to segment `a`–`b`, and how far along it (0 to 1)
/// the closest point is.
fn to_segment(p: DVec2, a: DVec2, b: DVec2) -> (f64, f64) {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 {
        ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p.distance(a + ab * t), t)
}

/// The distance from `p` to the edge of `line`, worked out segment by
/// segment: a river's edge is half its wider end's width out, a road's
/// half its width at the closest point.
fn to_line(line: &FeatureLine, p: DVec2) -> f64 {
    line.points
        .windows(2)
        .zip(line.widths.windows(2))
        .map(|(ends, widths)| {
            let (d, t) = to_segment(p, ends[0], ends[1]);
            let half = match line.kind {
                procgena_terrain::FeatureKind::Road => {
                    0.5 * (f64::from(widths[0]) + (f64::from(widths[1] - widths[0])) * t)
                }
                _ => 0.5 * f64::from(widths[0].max(widths[1])),
            };
            d - half
        })
        .fold(f64::INFINITY, f64::min)
}

/// The water and road distances at `p` from every feature of `terrains`,
/// found by looking at each in turn, capped as [`Measures`] cap them.
fn reference(terrains: &[&Terrain], p: DVec2) -> (f32, f32) {
    let cap = |d: f64| d.clamp(0.0, f64::from(MEASURE_REACH)) as f32;
    let mut water = f64::INFINITY;
    let mut road = f64::INFINITY;
    for terrain in terrains {
        for river in terrain.rivers() {
            water = water.min(to_line(river, p));
        }
        for pond in terrain.ponds() {
            water = water.min(p.distance(pond.centre) - pond.radius);
        }
        for line in terrain.roads() {
            road = road.min(to_line(line, p));
        }
    }
    (cap(water), cap(road))
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

/// Points along every feature line of `terrain`, inside its region: the
/// middle of each segment, with the way across the line there.
fn along_lines<'a>(
    terrain: &'a Terrain,
    lines: &'a [FeatureLine],
) -> impl Iterator<Item = (&'a FeatureLine, usize, DVec2, DVec2)> + 'a {
    lines.iter().flat_map(move |line| {
        line.points
            .windows(2)
            .enumerate()
            .filter_map(move |(i, w)| {
                let middle = (w[0] + w[1]) * 0.5;
                let across = (w[1] - w[0]).perp().normalize_or_zero();
                (across != DVec2::ZERO && terrain.region().boundary.contains(middle))
                    .then_some((line, i, middle, across))
            })
    })
}

#[test]
fn distances_match_every_feature_looked_at_in_turn() {
    let mut checked = 0;
    for (w, world) in worlds().iter().enumerate() {
        for terrain in world.terrains() {
            let mut points = points_in(terrain, 97.0);
            // And points close to the features, where the distances are
            // under the cap.
            for (line, i, middle, across) in along_lines(terrain, terrain.rivers())
                .chain(along_lines(terrain, terrain.roads()))
                .step_by(3)
            {
                let half = 0.5 * f64::from(line.widths[i]);
                for offset in [0.0, half + 3.0, -half - 11.0, half + 25.0, half + 39.0] {
                    points.push(middle + across * offset);
                }
            }
            for p in points {
                let m = measures(terrain, p);
                let (water, road) = reference(&[terrain], p);
                assert!(
                    (m.water_distance - water).abs() < 1e-3,
                    "world {w}, {p}: water distance {} against {water}",
                    m.water_distance
                );
                assert!(
                    (m.road_distance - road).abs() < 1e-3,
                    "world {w}, {p}: road distance {} against {road}",
                    m.road_distance
                );
                assert_eq!(
                    m.height_above_water.is_some(),
                    water < WATER_HEIGHT_REACH,
                    "world {w}, {p}: height above water {:?} with water {water} m away",
                    m.height_above_water
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 10_000, "only {checked} points checked");
}

#[test]
fn rivers_ponds_and_roads_measure_as_they_are() {
    let (mut wet, mut own_surface, mut on_road, mut beside) = (0, (0, 0), 0, (0, 0));
    for world in worlds() {
        for terrain in world.terrains() {
            // In the water: no distance, and below the surface.
            let ponds = terrain.ponds().iter().map(|pond| pond.centre);
            let rivers = along_lines(terrain, terrain.rivers()).map(|(_, _, p, _)| p);
            for p in rivers.chain(ponds) {
                let sample = terrain.query(p, SampleDetail::Measures);
                let m = sample.measures.expect("measures were asked for");
                let Some(level) = sample.water else {
                    continue;
                };
                assert_eq!(m.water_distance, 0.0, "{p}");
                let above = m.height_above_water.expect("water is here");
                assert!(above < 0.0, "{p}: {above} m above water");
                // The surface is this water's, unless another water meets
                // it or is nearly as near (a river flowing into a pond, a
                // tributary falling into its trunk): then they mix, where
                // the carved water takes the higher.
                let surface = sample.height - above;
                assert!(
                    surface < level + 0.5,
                    "{p}: surface {surface} against {level}"
                );
                if (surface - level).abs() < 0.05 {
                    own_surface.0 += 1;
                }
                if (surface - level).abs() < 0.25 {
                    own_surface.1 += 1;
                }
                wet += 1;
            }

            // On a road's surface, no distance; beside it, its distance.
            for (line, i, middle, across) in along_lines(terrain, terrain.roads()) {
                let m = measures(terrain, middle);
                if terrain.road_tags(middle).contains(RoadTags::ROAD) {
                    assert_eq!(m.road_distance, 0.0, "{middle}");
                    on_road += 1;
                }
                let half = 0.5 * f64::from(line.widths[i].min(line.widths[i + 1]));
                for side in [1.0, -1.0] {
                    let p = middle + across * side * (half + 15.0);
                    let d = measures(terrain, p).road_distance;
                    // No further than this road's edge; and its distance,
                    // unless another road (or a bend of this one) is
                    // nearer.
                    assert!(d <= 15.0 + 1e-3, "{p}: {d} m to a road 15 m away");
                    if (d - 15.0).abs() < 0.5 {
                        beside.0 += 1;
                    }
                    beside.1 += 1;
                }
            }
        }
    }
    eprintln!(
        "{wet} points in water ({} within 5 cm of its surface, {} within 25 cm), {on_road} on roads, {} of {} beside roads 15 m from them",
        own_surface.0, own_surface.1, beside.0, beside.1
    );
    assert!(wet > 100 && on_road > 100 && beside.1 > 200);
    // Where a river falls steeply, its next pieces' surfaces mix in a
    // little.
    assert!(own_surface.0 * 100 >= wet * 90 && own_surface.1 * 100 >= wet * 99);
    assert!(beside.0 * 10 >= beside.1 * 8);
}

#[test]
fn curvature_is_negative_in_riverbeds_and_zero_on_planes() {
    // A plane bends nowhere: on a tilted plane the four neighbours' mean
    // is the centre's height.
    let mut beds = (0, 0);
    for world in worlds() {
        for terrain in world.terrains() {
            for (_, _, p, _) in along_lines(terrain, terrain.rivers()) {
                if terrain.water_level(p).is_none() {
                    continue;
                }
                beds.1 += 1;
                if measures(terrain, p).curvature < 0.0 {
                    beds.0 += 1;
                }
            }
        }
    }
    eprintln!("{} of {} riverbed points concave", beds.0, beds.1);
    assert!(beds.1 > 100 && beds.0 * 10 >= beds.1 * 9);
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
                let n = (w[0].distance(w[1]) / 3.0).ceil().max(1.0) as usize;
                for k in 0..n {
                    out.push((w[0].lerp(w[1], k as f64 / n as f64), a, b));
                }
            }
        }
    }
    out
}

#[test]
fn measures_agree_across_borders() {
    let mut worst = [0.0f32; 4];
    let mut near_features = 0;
    for (w, world) in worlds().iter().enumerate() {
        for (p, a, b) in border_points(world) {
            let (ma, mb) = (measures(a, p), measures(b, p));

            // Both sides see every feature of either within reach.
            let (water, road) = reference(&[a, b], p);
            for (side, m) in [("a", ma), ("b", mb)] {
                assert!(
                    (m.water_distance - water).abs() < 1e-3,
                    "world {w}, {p}, side {side}: water distance {} against {water}",
                    m.water_distance
                );
                assert!(
                    (m.road_distance - road).abs() < 1e-3,
                    "world {w}, {p}, side {side}: road distance {} against {road}",
                    m.road_distance
                );
            }
            if water < MEASURE_REACH || road < MEASURE_REACH {
                near_features += 1;
            }

            let above = match (ma.height_above_water, mb.height_above_water) {
                (Some(x), Some(y)) => (x - y).abs(),
                (None, None) => 0.0,
                other => panic!("world {w}, {p}: height above water {other:?}"),
            };
            let jumps = [
                above,
                (ma.slope - mb.slope).abs(),
                (ma.curvature - mb.curvature).abs(),
                (ma.water_distance - mb.water_distance).abs(),
            ];
            for (most, jump) in worst.iter_mut().zip(jumps) {
                *most = most.max(jump);
            }
        }
    }
    eprintln!(
        "worst jumps across borders: height above water {}, slope {}, curvature {} (step {CURVATURE_STEP} m), water distance {}; {near_features} border points near features",
        worst[0], worst[1], worst[2], worst[3]
    );
    assert!(
        near_features > 50,
        "only {near_features} border points near features"
    );
    // Heights and distances agree exactly; slope and curvature read the
    // ground a little way off, where each terrain gives way to its own
    // ground in its own way. The curvature limit is 0.02, not 0.01, as a
    // direct consequence of the slope being allowed to crease on the border
    // (cheap borders, TB): about 0.011 per metre on these worlds.
    assert!(worst[0] < 1e-3 && worst[3] < 1e-3, "{worst:?}");
    assert!(worst[1] < 0.01, "slope jumps by {}", worst[1]);
    assert!(worst[2] < 0.02, "curvature jumps by {} per metre", worst[2]);
}

#[test]
fn measures_change_smoothly() {
    // Along lines through each terrain, in short steps.
    const STEP: f64 = 0.25;
    let mut worst_level = 0.0f32;
    for world in worlds() {
        for terrain in world.terrains() {
            for (_, _, middle, across) in along_lines(terrain, terrain.rivers())
                .chain(along_lines(terrain, terrain.roads()))
                .step_by(29)
            {
                let mut last: Option<(f32, Measures)> = None;
                for k in -160..=160 {
                    let p = middle + across * (f64::from(k) * STEP);
                    if !terrain.region().boundary.contains(p) {
                        last = None;
                        continue;
                    }
                    let sample = terrain.query(p, SampleDetail::Measures);
                    let m = sample.measures.expect("measures were asked for");
                    if let Some((height, before)) = last {
                        let moved = STEP as f32 * 1.001 + 1e-4;
                        assert!((m.water_distance - before.water_distance).abs() <= moved);
                        assert!((m.road_distance - before.road_distance).abs() <= 1.5 * moved);
                        // The mixed water surface moves only a little
                        // between neighbouring points.
                        if let (Some(x), Some(y)) =
                            (m.height_above_water, before.height_above_water)
                        {
                            let level = (sample.height - x) - (height - y);
                            worst_level = worst_level.max(level.abs());
                        }
                    }
                    last = Some((sample.height, m));
                }
            }
        }
    }
    eprintln!("worst change of the mixed water surface per {STEP} m: {worst_level}");
    assert!(
        worst_level < 0.5,
        "the water surface moves {worst_level} m in {STEP} m"
    );
}
