//! Rivers and roads: running downhill, crossing borders, and ending safely.

mod common;

use common::{
    CONFIGS, JOIN_SEEDS, RIVER_SEEDS, WARPED, WITH_LAND, assert_no_crack, hills_into_plains,
    triple_junction, two_terrains,
};
use glam::DVec2;
use procgena_terrain::{
    FeatureKind, FeatureLine, Port, PortKind, RegionEdge, StitchedWorld, Terrain,
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
/// count).
fn segments_cross(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let side = |p: DVec2, q: DVec2, r: DVec2| (q - p).perp_dot(r - p);
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

/// Where `road` crosses `river`, as (distance along the road, distance
/// along the river). Crossings within 30 m of each other along the road
/// count as one.
fn bridges(road: &FeatureLine, river: &FeatureLine) -> Vec<(f64, f64)> {
    let mut found: Vec<(f64, f64)> = Vec::new();
    let mut along_road = 0.0;
    for a in road.points.windows(2) {
        let mut along_river = 0.0;
        for b in river.points.windows(2) {
            if segments_cross(a[0], a[1], b[0], b[1]) {
                // Where along each segment the two cross.
                let (d, e) = (a[1] - a[0], b[1] - b[0]);
                let t = (b[0] - a[0]).perp_dot(e) / d.perp_dot(e);
                let s = (b[0] - a[0]).perp_dot(d) / d.perp_dot(e);
                let at = (along_road + t * d.length(), along_river + s * e.length());
                if found.last().is_none_or(|last| at.0 - last.0 > 30.0) {
                    found.push(at);
                }
            }
            along_river += b[0].distance(b[1]);
        }
        along_road += a[0].distance(a[1]);
    }
    found
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
            for (i, road) in t.roads().iter().enumerate() {
                for (j, river) in t.rivers().iter().enumerate() {
                    let at = bridges(road, river);
                    bridged += at.len();
                    for pair in at.windows(2) {
                        if (pair[1].1 - pair[0].1).abs() < 600.0 {
                            wasteful.push(format!(
                                "world {w}, region {}: road {i} crosses river {j} at {:?} and \
                                 again at {:?} m (along road, along river)",
                                t.region().id.0,
                                pair[0],
                                pair[1]
                            ));
                        }
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
