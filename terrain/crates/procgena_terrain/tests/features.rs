//! Rivers and roads: running downhill, crossing borders, and ending safely.

mod common;

use common::{
    CONFIGS, JOIN_SEEDS, RIVER_SEEDS, WARPED, WITH_LAND, assert_no_crack, hills_into_plains,
    triple_junction, two_terrains,
};
use std::collections::BTreeMap;

use glam::DVec2;
use procgena_terrain::{
    FeatureKind, FeatureLine, Port, PortKind, RegionEdge, RoadNodeId, StitchedWorld, Terrain,
};

/// The test worlds, for each of [`WITH_LAND`] and then [`WARPED`] in turn:
/// two with only road crossings, one per river seed (with a river crossing
/// on flat land), and one per join seed with rivers joining on bends. Built
/// once and shared by every test.
fn worlds() -> &'static [StitchedWorld] {
    static WORLDS: std::sync::OnceLock<Vec<StitchedWorld>> = std::sync::OnceLock::new();
    WORLDS.get_or_init(|| {
        let mut worlds = Vec::new();
        for config in WITH_LAND.into_iter().chain(WARPED) {
            worlds.push(two_terrains(DVec2::ZERO, config));
            worlds.push(triple_junction(config));
            worlds.extend(RIVER_SEEDS.map(|s| hills_into_plains(s, config)));
            worlds.extend(JOIN_SEEDS.map(|s| hills_into_plains(s, config)));
        }
        worlds
    })
}

/// Every agreed crossing in `world`, with its edge.
fn crossings(world: &StitchedWorld) -> Vec<(RegionEdge, Port)> {
    let mut out = Vec::new();
    for agreement in world.agreements() {
        let edge = world
            .terrains()
            .iter()
            .flat_map(|t| &t.region().edges)
            .find(|e| e.id == agreement.edge)
            .expect("every agreement has an edge")
            .clone();
        for port in &agreement.ports {
            out.push((edge.clone(), *port));
        }
    }
    out
}

/// The terrain with region id `id`.
fn terrain(world: &StitchedWorld, id: procgena_terrain::RegionId) -> &Terrain {
    world
        .terrains()
        .iter()
        .find(|t| t.region().id == id)
        .expect("region exists")
}

/// The other region on `edge`, seen from `id`.
fn other(
    world: &StitchedWorld,
    edge: &RegionEdge,
    id: procgena_terrain::RegionId,
) -> procgena_terrain::RegionId {
    world
        .terrains()
        .iter()
        .map(|t| t.region().id)
        .find(|&r| r != id && t_has_edge(world, r, edge))
        .expect("edge has two regions")
}

fn t_has_edge(world: &StitchedWorld, id: procgena_terrain::RegionId, edge: &RegionEdge) -> bool {
    terrain(world, id)
        .region()
        .edges
        .iter()
        .any(|e| e.id == edge.id)
}

/// The end of a crossing's straight piece inside `terrain`'s region.
fn end_inside(terrain: &Terrain, edge: &RegionEdge, port: &Port) -> DVec2 {
    let (left, right) = port.ends(edge);
    if terrain.region().boundary.contains(left) {
        left
    } else {
        right
    }
}

/// Returns true if segments `a`–`b` and `c`–`d` cross (touching ends don't
/// count, nor do segments lying along one line, whose side tests are only
/// rounding).
fn segments_cross(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    // A point within a nanometre of a line counts as on it.
    let side = |p: DVec2, q: DVec2, r: DVec2| {
        let s = (q - p).perp_dot(r - p);
        if s.abs() <= 1e-9 * (q - p).length() {
            0.0
        } else {
            s
        }
    };
    side(c, d, a) * side(c, d, b) < 0.0 && side(a, b, c) * side(a, b, d) < 0.0
}

#[test]
fn rivers_never_cross_each_other_or_themselves() {
    for (w, world) in worlds().iter().enumerate() {
        for t in world.terrains() {
            let rivers = t.rivers();
            for (i, r) in rivers.iter().enumerate() {
                for (j, s) in rivers.iter().enumerate().skip(i) {
                    for (ka, a) in r.points.windows(2).enumerate() {
                        for (kb, b) in s.points.windows(2).enumerate() {
                            // Neighbouring segments of one river share a point.
                            if i == j && kb <= ka + 1 {
                                continue;
                            }
                            assert!(
                                !segments_cross(a[0], a[1], b[0], b[1]),
                                "world {w}, region {}: river {i} crosses river {j} near {}",
                                t.region().id.0,
                                a[0]
                            );
                        }
                    }
                }
            }
        }
    }
}

fn falls(line: &FeatureLine) -> bool {
    line.levels.windows(2).all(|w| w[1] <= w[0])
}

/// Distance from `p` to the nearest point of `line`.
fn line_distance(line: &FeatureLine, p: DVec2) -> f64 {
    line.points
        .windows(2)
        .map(|w| {
            let ab = w[1] - w[0];
            let t = ((p - w[0]).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
            p.distance(w[0] + ab * t)
        })
        .fold(f64::INFINITY, f64::min)
}

#[test]
fn every_river_runs_downhill() {
    for world in worlds() {
        for t in world.terrains() {
            for river in t.rivers() {
                assert_eq!(river.kind, FeatureKind::River);
                assert!(
                    falls(river),
                    "river in region {} rises: {:?}",
                    t.region().id.0,
                    river.levels
                );
            }
        }
    }
}

#[test]
fn rivers_cross_borders_and_keep_falling() {
    let mut crossed = 0;
    for world in worlds() {
        for (edge, port) in crossings(world) {
            let PortKind::River { from } = port.kind else {
                continue;
            };
            crossed += 1;
            let (upstream, downstream) = (
                terrain(world, from),
                terrain(world, other(world, &edge, from)),
            );
            let (out_end, in_end) = (
                end_inside(upstream, &edge, &port),
                end_inside(downstream, &edge, &port),
            );

            // Both sides carve the same piece across the border, running
            // downstream.
            let (piece_points, _) = port.piece(&edge);
            let is_piece = |r: &&FeatureLine| {
                let mut reversed = r.points.clone();
                reversed.reverse();
                r.points == piece_points || reversed == piece_points
            };
            let pieces: Vec<&FeatureLine> = [upstream, downstream]
                .iter()
                .map(|t| t.rivers().iter().find(is_piece).expect("crossing piece"))
                .collect();
            assert_eq!(pieces[0], pieces[1]);
            assert!(falls(pieces[0]));
            assert_eq!(pieces[0].points[0], out_end);
            assert_eq!(*pieces[0].points.last().unwrap(), in_end);

            // Upstream: a river arrives where the piece starts, at its level.
            let arriving = upstream
                .rivers()
                .iter()
                .filter(|r| !is_piece(r))
                .find(|r| r.points.last() == Some(&out_end))
                .expect("a river leads to the crossing");
            assert_eq!(arriving.levels.last(), pieces[0].levels.first());

            // Downstream: a river leaves where the piece ends, at its level,
            // and ends in a pond or joins another river.
            let leaving = downstream
                .rivers()
                .iter()
                .filter(|r| !is_piece(r))
                .find(|r| r.points.first() == Some(&in_end))
                .expect("a river leads on from the crossing");
            assert_eq!(leaving.levels.first(), pieces[0].levels.last());
            let last = *leaving.points.last().unwrap();
            let in_pond = downstream.ponds().iter().any(|p| p.centre == last);
            // A joining river ends inside another river's channel, no lower
            // than that river's water there.
            let joins = downstream.rivers().iter().any(|r| {
                !std::ptr::eq(r, leaving)
                    && line_distance(r, last)
                        <= f64::from(r.widths.iter().copied().fold(0.0, f32::max))
                    && downstream.water_level(last).is_some()
            });
            assert!(
                in_pond || joins,
                "river from the crossing ends nowhere at {last}: {} points, levels {:?}, \
                 nearest other river {:?} m, water {:?}, ponds {:?}, region {}",
                leaving.points.len(),
                &leaving.levels[leaving.levels.len().saturating_sub(3)..],
                downstream
                    .rivers()
                    .iter()
                    .filter(|r| !std::ptr::eq(*r, leaving))
                    .map(|r| line_distance(r, last))
                    .fold(f64::INFINITY, f64::min),
                downstream.water_level(last),
                downstream
                    .ponds()
                    .iter()
                    .map(|p| p.centre)
                    .collect::<Vec<_>>(),
                downstream.region().id.0
            );

            // Both sides hold the same water at the crossing.
            let on_border = port.point(&edge);
            assert_eq!(upstream.water_level(on_border), Some(port.level));
            assert_eq!(downstream.water_level(on_border), Some(port.level));
        }
    }
    assert!(crossed > 0, "the test worlds should have river crossings");
}

#[test]
fn neighbouring_mountain_rivers_both_cross_and_join_below() {
    // Plains and mountains with seed 217: two mountain rivers cross the
    // bent border 159 m apart, closer than crossings of different kinds may
    // be (rivers from one side may cross closer together). Both cross into
    // the plains, and neither ends in a pond: each joins a river there.
    let world = common::two_regions(
        DVec2::ZERO,
        common::preset(procgena_terrain::fixtures::plains_medium(), 217),
        common::preset(procgena_terrain::fixtures::mountains_medium(), 217),
        common::DEFAULT.convergence(),
        &procgena_terrain::Elevation::flat(),
    );
    let mountains = procgena_terrain::RegionId(2);
    let rivers: Vec<(RegionEdge, Port)> = crossings(&world)
        .into_iter()
        .filter(|(_, p)| p.kind == (PortKind::River { from: mountains }))
        .collect();
    // The closest pair, which must be under 3 × the feature clearance.
    let pair = rivers
        .iter()
        .flat_map(|a| rivers.iter().map(move |b| (a, b)))
        .filter(|(a, b)| a.1.along < b.1.along)
        .min_by(|(a, b), (c, d)| (b.1.along - a.1.along).total_cmp(&(d.1.along - c.1.along)))
        .expect("two mountain rivers cross");
    assert!(
        pair.1.1.along - pair.0.1.along < 3.0 * procgena_terrain::FEATURE_CLEARANCE,
        "the closest crossings are {} m apart",
        pair.1.1.along - pair.0.1.along
    );
    let mut joined = 0;
    for (edge, port) in [pair.0.clone(), pair.1.clone()] {
        let plains = terrain(&world, other(&world, &edge, mountains));
        let start = end_inside(plains, &edge, &port);
        let leaving = plains
            .rivers()
            .iter()
            .find(|r| r.points.first() == Some(&start))
            .expect("a river leads on from the crossing");
        let last = *leaving.points.last().unwrap();
        assert!(
            !plains.ponds().iter().any(|p| p.centre == last),
            "the river from the crossing at {} m ends in a pond",
            port.along
        );
        assert!(
            plains
                .rivers()
                .iter()
                .any(|r| !std::ptr::eq(r, leaving) && line_distance(r, last) < 1e-6),
            "the river from the crossing at {} m joins no river",
            port.along
        );
        joined += 1;
    }
    assert_eq!(joined, 2);
}

#[test]
fn every_river_fixture_has_a_river_crossing() {
    for config in CONFIGS {
        for seed in RIVER_SEEDS {
            let world = hills_into_plains(seed, config);
            let rivers = crossings(&world)
                .iter()
                .filter(|(_, p)| matches!(p.kind, PortKind::River { .. }))
                .count();
            assert!(
                rivers > 0,
                "seed {seed} has no river crossing with {config:?}"
            );
        }
    }
}

#[test]
fn roads_cross_borders_within_their_grade() {
    let mut crossed = 0;
    for world in worlds() {
        for (edge, port) in crossings(world) {
            if port.kind != PortKind::Road {
                continue;
            }
            crossed += 1;
            for t in world
                .terrains()
                .iter()
                .filter(|t| t.region().edges.iter().any(|e| e.id == edge.id))
            {
                let start = end_inside(t, &edge, &port);
                let road = t
                    .roads()
                    .iter()
                    .find(|r| r.points.first() == Some(&start))
                    .expect("a road leads on from the crossing");
                // It starts where the crossing piece ends, at its level.
                let (piece_points, piece_levels) = port.piece(&edge);
                let at = piece_points
                    .iter()
                    .position(|&p| p == start)
                    .expect("piece end");
                assert_eq!(road.levels[0], piece_levels[at]);
                // Each step within the steepest grade allowed where it lies.
                for (p, l) in road.points.windows(2).zip(road.levels.windows(2)) {
                    let middle = p[0].lerp(p[1], 0.5);
                    let max_grade = f64::from(t.generator().road_grades(middle).max_grade);
                    let grade = f64::from((l[1] - l[0]).abs()) / p[0].distance(p[1]);
                    assert!(
                        grade <= max_grade * 1.001,
                        "grade {grade} in region {}",
                        t.region().id.0
                    );
                }
            }
        }
    }
    assert!(crossed > 0, "the test worlds should have road crossings");
}

#[test]
fn no_cracks_around_crossings() {
    for world in worlds() {
        for (edge, port) in crossings(world) {
            let centre = port.point(&edge);
            for r in [3.0, 30.0, 70.0, 120.0] {
                let ring = |s: f64| centre + DVec2::from_angle(s * std::f64::consts::TAU) * r;
                assert_no_crack(
                    world,
                    ring,
                    std::f64::consts::TAU * r,
                    &format!("ring {r} m around crossing at {centre}"),
                );
            }
            // Straight across the border and along it.
            let (left, right) = port.ends(&edge);
            let across = right - left;
            assert_no_crack(
                world,
                |s| left - across + across * 3.0 * s,
                3.0 * across.length(),
                "across a crossing",
            );
        }
    }
}

/// The largest gaps between a line's levels and the uncarved ground at its
/// points: (ground above the level, level above the ground). Each point is
/// measured against the ground of the region it lies in, since crossing
/// pieces reach into the neighbour.
fn level_gaps(world: &StitchedWorld, line: &FeatureLine) -> (f32, f32) {
    line.points
        .iter()
        .zip(&line.levels)
        .map(|(&p, &level)| world.owner(p).expect("inside the world").uncarved_height(p) - level)
        .fold((0.0f32, 0.0f32), |(cut, fill), gap| {
            (cut.max(gap), fill.max(-gap))
        })
}

#[test]
fn rivers_and_roads_stay_close_to_the_ground() {
    for (w, world) in worlds().iter().enumerate() {
        for t in world.terrains() {
            // How deep a river may cut through a rise to escape a hollow
            // (the crate's rule), plus 1 m below the ground, plus 20 m for
            // relief finer than the 25 m planning grid.
            let base = &t.generator().options().base;
            let breach = 4.0 + 0.03 * (base.hills.height + base.mountains.height);
            let deepest_cut = breach + 1.0 + 20.0;
            for river in t.rivers() {
                let (cut, fill) = level_gaps(world, river);
                let at = format!(
                    "world {w}, region {}, river of {} points",
                    t.region().id.0,
                    river.points.len()
                );
                assert!(fill <= 3.0, "{at}: water {fill} m above the ground");
                assert!(
                    cut <= deepest_cut,
                    "{at}: cut {cut} m deep, over {deepest_cut}"
                );
            }
            // Roads may cut as deep as they need (the cutting steepens to a
            // cliff), but embankments stay at most 10 m high.
            for road in t.roads() {
                let (cut, fill) = level_gaps(world, road);
                let at = format!(
                    "world {w}, region {}, road of {} points",
                    t.region().id.0,
                    road.points.len()
                );
                assert!(fill <= 10.0, "{at}: fill {fill} m (cut {cut} m)");
            }
        }
    }
}

/// How far along `river` the point on it nearest `p` is, in metres.
fn along_river(river: &FeatureLine, p: DVec2) -> f64 {
    let (mut best, mut along, mut at) = (f64::INFINITY, 0.0, 0.0);
    for w in river.points.windows(2) {
        let ab = w[1] - w[0];
        let t = ((p - w[0]).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
        let d = p.distance(w[0] + ab * t);
        if d < best {
            (best, at) = (d, along + t * ab.length());
        }
        along += ab.length();
    }
    at
}

/// A road crosses one river twice only where going round instead would
/// mean following the river for at least two bridges' worth of road, 600 m,
/// between the crossings: as where a long river loops back and the road
/// crosses both arms of the loop.
#[test]
fn roads_cross_a_river_again_only_to_save_a_long_way_round() {
    let mut bridged = 0;
    let mut wasteful = Vec::new();
    for (w, world) in worlds().iter().enumerate() {
        for t in world.terrains() {
            // Each road's crossings of each river, by distance along the
            // river (the middle of the two banks).
            let network = t.road_network();
            let mut crossed: BTreeMap<(RoadNodeId, usize), Vec<f64>> = BTreeMap::new();
            for crossing in &network.crossings {
                let [a, b] = crossing
                    .banks
                    .map(|id| network.node(id).expect("bank").position);
                let along = along_river(&t.rivers()[crossing.river], a.lerp(b, 0.5));
                crossed
                    .entry((crossing.origin, crossing.river))
                    .or_default()
                    .push(along);
            }
            for ((road, river), mut at) in crossed {
                bridged += at.len();
                at.sort_by(f64::total_cmp);
                for pair in at.windows(2) {
                    if pair[1] - pair[0] < 600.0 {
                        wasteful.push(format!(
                            "world {w}, region {}: road {road:?} crosses river {river} at {} and \
                             again at {} m along it",
                            t.region().id.0,
                            pair[0],
                            pair[1]
                        ));
                    }
                }
            }
        }
    }
    assert!(wasteful.is_empty(), "{}", wasteful.join("\n"));
    assert!(
        bridged > 0,
        "the test worlds should have roads crossing rivers"
    );
}

/// Distance from `p` to the segment from `a` to `b`.
fn segment_distance(a: DVec2, b: DVec2, p: DVec2) -> f64 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Roads stop at both banks where they cross a river or the pond it ends
/// in: no carved road runs over a river or through a pond, and each
/// crossing's banks lie on either side of its river, or of its river's
/// pond.
#[test]
fn roads_leave_the_rivers_they_cross_open() {
    let mut crossings = 0;
    for (w, world) in worlds().iter().enumerate() {
        for t in world.terrains() {
            for road in t.roads() {
                for a in road.points.windows(2) {
                    for river in t.rivers() {
                        for b in river.points.windows(2) {
                            assert!(
                                !segments_cross(a[0], a[1], b[0], b[1]),
                                "world {w}, region {}: a road runs over a river near {}",
                                t.region().id.0,
                                a[0]
                            );
                        }
                    }
                    for pond in t.ponds() {
                        assert!(
                            segment_distance(a[0], a[1], pond.centre) >= pond.radius,
                            "world {w}, region {}: a road runs through the pond at {}",
                            t.region().id.0,
                            pond.centre
                        );
                    }
                }
            }
            let network = t.road_network();
            for crossing in &network.crossings {
                crossings += 1;
                let [a, b] = crossing
                    .banks
                    .map(|id| network.node(id).expect("bank").position);
                let river = &t.rivers()[crossing.river];
                let over_river = river
                    .points
                    .windows(2)
                    .any(|r| segments_cross(a, b, r[0], r[1]));
                // The pond the river ends in, if any.
                let mouth = *river.points.last().expect("rivers have points");
                let over_pond = t.ponds().iter().any(|pond| {
                    mouth.distance(pond.centre) < pond.radius
                        && segment_distance(a, b, pond.centre) < pond.radius
                });
                assert!(
                    over_river || over_pond,
                    "world {w}, region {}: banks {a} and {b} are on one side of river {} \
                     and its pond",
                    t.region().id.0,
                    crossing.river
                );
            }
        }
    }
    assert!(crossings > 0, "the test worlds should have river crossings");
}

/// The content hashes of every road network in `world`, by region.
fn network_hashes(world: &StitchedWorld) -> Vec<(u64, u64)> {
    world
        .terrains()
        .iter()
        .map(|t| (t.region().id.0, t.road_network().content_hash()))
        .collect()
}

/// A world's road networks hash the same however many threads build it.
/// Run with and without the `rayon` feature, `--nocapture` prints the
/// hashes, which must also match between the two builds.
#[test]
fn road_networks_are_the_same_on_any_number_of_threads() {
    let build = || {
        RIVER_SEEDS
            .iter()
            .flat_map(|&s| network_hashes(&hills_into_plains(s, WITH_LAND[0])))
            .collect::<Vec<_>>()
    };
    let hashes = build();
    println!("road network hashes: {hashes:x?}");
    assert!(
        RIVER_SEEDS.iter().any(|&s| {
            hills_into_plains(s, WITH_LAND[0])
                .terrains()
                .iter()
                .any(|t| !t.road_network().crossings.is_empty())
        }),
        "the river worlds should have road networks with crossings"
    );
    assert_eq!(build(), hashes, "a second build should hash the same");
    #[cfg(feature = "rayon")]
    for threads in [1, 3, 8] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .expect("a thread pool");
        assert_eq!(
            pool.install(build),
            hashes,
            "on {threads} threads the networks should hash the same"
        );
    }
}

/// The first pair of edges of the closed outline `outline` that cross, by
/// the index of each edge's first point; `None` if it never crosses itself.
fn first_crossing(outline: &[DVec2]) -> Option<(usize, usize)> {
    let n = outline.len();
    (0..n).find_map(|i| {
        (i + 2..n).find_map(|j| {
            let crosses = !(i == 0 && j == n - 1)
                && segments_cross(
                    outline[i],
                    outline[(i + 1) % n],
                    outline[j],
                    outline[(j + 1) % n],
                );
            crosses.then_some((i, j))
        })
    })
}

/// The first pair of edges of the open line `points` that cross, by the
/// index of each edge's first point; `None` if it never crosses itself.
fn first_tangle(points: &[DVec2]) -> Option<(usize, usize)> {
    let n = points.len();
    (0..n.saturating_sub(1)).find_map(|i| {
        (i + 2..n - 1).find_map(|j| {
            segments_cross(points[i], points[i + 1], points[j], points[j + 1]).then_some((i, j))
        })
    })
}

/// Road earthworks keep to their footprints: every segment's swept
/// footprint and every breakline is free of self-crossings, junction
/// footprints are convex and counter-clockwise, and between a crossing's
/// two bank nodes the roads leave the ground (the river's channel and
/// banks) exactly as it was.
#[test]
fn road_earthworks_are_valid_and_leave_crossings_untouched() {
    let (mut segments, mut junctions, mut crossings, mut checked) = (0, 0, 0, 0);
    for (w, world) in worlds().iter().enumerate() {
        for t in world.terrains() {
            let region = t.region().id.0;
            let earthworks = t.road_earthworks();
            for s in &earthworks.segments {
                segments += 1;
                let footprint = s.footprint();
                if let Some((i, j)) = first_crossing(&footprint) {
                    let n = footprint.len();
                    let near = |q: DVec2| {
                        s.stations
                            .iter()
                            .enumerate()
                            .filter(|(_, st)| st.position.distance(q) < 12.0)
                            .map(|(k, st)| (k, st.position, st.sides.map(|d| d.daylight)))
                            .collect::<Vec<_>>()
                    };
                    panic!(
                        "world {w}, region {region}: segment {:?}'s footprint crosses itself: \
                         edge {i} {:?}-{:?} and edge {j} {:?}-{:?} of {n}; stations near: {:?}",
                        s.segment,
                        footprint[i],
                        footprint[(i + 1) % n],
                        footprint[j],
                        footprint[(j + 1) % n],
                        near(footprint[i])
                    );
                }
            }
            for b in &earthworks.breaklines {
                let line: Vec<DVec2> = b.points.iter().map(|p| DVec2::new(p.x, p.z)).collect();
                if let Some((i, j)) = first_tangle(&line) {
                    panic!(
                        "world {w}, region {region}: a {:?} breakline of {:?} crosses itself: \
                         edge {i} {:?}-{:?} and edge {j} {:?}-{:?} of {}",
                        b.kind,
                        b.segment,
                        b.points[i],
                        b.points[i + 1],
                        b.points[j],
                        b.points[j + 1],
                        line.len()
                    );
                }
            }
            for j in &earthworks.junctions {
                junctions += 1;
                let n = j.outline.len();
                assert!(n >= 3);
                assert!((0..n).all(|i| {
                    let (a, b, c) = (j.outline[i], j.outline[(i + 1) % n], j.outline[(i + 2) % n]);
                    (b - a).perp_dot(c - b) > 0.0
                }));
            }

            // Between the banks, more than a few metres from each, the
            // ground is as it was before the roads.
            let network = t.road_network();
            for crossing in &network.crossings {
                crossings += 1;
                let [a, b] = crossing
                    .banks
                    .map(|id| network.node(id).expect("bank").position);
                let span = a.distance(b);
                let steps = (span / 0.5).ceil() as usize;
                for k in 0..=steps {
                    let along = span * k as f64 / steps as f64;
                    if along < 5.0 || along > span - 5.0 {
                        continue;
                    }
                    let p = a.lerp(b, along / span);
                    checked += 1;
                    assert_eq!(
                        t.height(p),
                        t.height_before_roads(p),
                        "world {w}, region {region}: a road changes the ground at {p}, \
                         between banks {a} and {b}"
                    );
                }
            }
        }
    }
    assert!(segments > 0 && junctions > 0 && crossings > 0 && checked > 0);
}
