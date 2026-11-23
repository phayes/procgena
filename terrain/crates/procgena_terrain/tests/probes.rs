//! Measurements of the test worlds, run by hand:
//! `cargo test -p procgena_terrain --release --test probes -- --ignored --nocapture`.

mod common;

use common::{CONFIGS, Config, TWO_REGION_BORDER, hills_into_plains, two_biomes};
use glam::DVec2;
use procgena_terrain::{FeatureLine, PortKind, StitchedWorld};

/// Points along the two-region border, at most `spacing` metres apart.
fn border_points(spacing: f64) -> Vec<DVec2> {
    let mut out = Vec::new();
    for w in TWO_REGION_BORDER.windows(2) {
        let (a, b) = (DVec2::from(w[0]), DVec2::from(w[1]));
        let count = (a.distance(b) / spacing).ceil() as usize;
        out.extend((0..count).map(|k| a.lerp(b, k as f64 / count as f64)));
    }
    out
}

/// A configuration as a short label.
fn label(config: Config) -> String {
    format!(
        "{:?} seeds, meeting {}, blend {}",
        config.seeds, config.meeting, config.blend
    )
}

/// How far the mountains' own ground lies above the plains' own ground
/// along the two-biome border (mean and largest, with the part of the mean
/// that comes from the presets' mean elevations), and how far the
/// negotiated border moves each side's own ground (mean).
#[test]
#[ignore]
fn border_gap() {
    for config in CONFIGS {
        let world = two_biomes(DVec2::ZERO, config);
        let [plains, mountains] = [0, 1].map(|i| &world.terrains()[i]);
        let points = border_points(5.0);
        let gaps: Vec<f32> = points
            .iter()
            .map(|&p| {
                mountains.generator().natural_height(p) - plains.generator().natural_height(p)
            })
            .collect();
        let mean = gaps.iter().sum::<f32>() / gaps.len() as f32;
        let largest = gaps.iter().fold(0.0f32, |m, g| m.max(g.abs()));
        let elevation = mountains.generator().options().base.elevation
            - plains.generator().options().base.elevation;
        let shift = |t: &procgena_terrain::Terrain| {
            points
                .iter()
                .map(|&p| (t.uncarved_height(p) - t.generator().natural_height(p)).abs())
                .sum::<f32>()
                / points.len() as f32
        };
        eprintln!(
            "{}: gap mean {mean:.0} m, largest {largest:.0} m, elevation {elevation:.0} m; \
             shift plains {:.0} m, mountains {:.0} m",
            label(config),
            shift(plains),
            shift(mountains),
        );
    }
}

/// The number of river crossings in `world`.
fn river_crossings(world: &StitchedWorld) -> usize {
    world
        .agreements()
        .iter()
        .flat_map(|a| &a.ports)
        .filter(|p| matches!(p.kind, PortKind::River { .. }))
        .count()
}

/// Which hill seeds from 0 to 299 give a river crossing in
/// `hills_into_plains`.
#[test]
#[ignore]
fn river_crossing_seeds() {
    for config in CONFIGS {
        let seeds: Vec<u32> = (0..300)
            .filter(|&s| river_crossings(&hills_into_plains(s, config)) > 0)
            .collect();
        eprintln!(
            "{}: {} of 300 give a river crossing: {seeds:?}",
            label(config),
            seeds.len()
        );
    }
}

/// Whether segments `a`–`b` and `c`–`d` cross (touching ends don't count).
fn segments_cross(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let side = |p: DVec2, q: DVec2, r: DVec2| (q - p).perp_dot(r - p);
    side(c, d, a) * side(c, d, b) < 0.0 && side(a, b, c) * side(a, b, d) < 0.0
}

/// The distance from `p` to the nearest point of `line`.
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

/// The largest gaps between a line's levels and the uncarved ground of the
/// region each point lies in: (ground above the level, level above the
/// ground).
fn level_gaps(world: &StitchedWorld, line: &FeatureLine) -> (f32, f32) {
    line.points
        .iter()
        .zip(&line.levels)
        .map(|(&p, &l)| world.owner(p).unwrap().uncarved_height(p) - l)
        .fold((0.0f32, 0.0f32), |(cut, fill), gap| {
            (cut.max(gap), fill.max(-gap))
        })
}

/// Every way the rivers and roads of `world` break the feature suite's
/// limits, one line each.
fn feature_problems(world: &StitchedWorld) -> Vec<String> {
    let mut out = Vec::new();
    for t in world.terrains() {
        let region = t.region().id.0;
        let o = t.generator().options();

        // Roads: grade against the steepest allowed where each step lies,
        // and embankment height (cuttings may be any depth).
        for road in t.roads() {
            let (over, grade, allowed) = road
                .points
                .windows(2)
                .zip(road.levels.windows(2))
                .map(|(p, l)| {
                    let grade = f64::from((l[1] - l[0]).abs()) / p[0].distance(p[1]);
                    let middle = p[0].lerp(p[1], 0.5);
                    let allowed = f64::from(t.generator().road_grades(middle).max_grade);
                    (grade / allowed, grade, allowed)
                })
                .fold((0.0, 0.0, 0.0), |m, s| if s.0 > m.0 { s } else { m });
            if over > 1.001 {
                out.push(format!(
                    "region {region}: road grade {grade:.4} over {allowed:.4}"
                ));
            }
            let (cut, fill) = level_gaps(world, road);
            if fill > 10.0 {
                out.push(format!(
                    "region {region}: road of {} points, cut {cut:.1} m, fill {fill:.1} m",
                    road.points.len()
                ));
            }
        }

        // Rivers: distance from the ground.
        let allowed = 4.0 + 0.03 * (o.base.hills.height + o.base.mountains.height) + 21.0;
        let rivers = t.rivers();
        for river in rivers {
            let (cut, fill) = level_gaps(world, river);
            if cut > allowed || fill > 3.0 {
                out.push(format!(
                    "region {region}: river of {} points, cut {cut:.1} m (allowed {allowed:.1}), \
                     fill {fill:.1} m",
                    river.points.len()
                ));
            }
        }

        // Rivers: crossings.
        for (i, r) in rivers.iter().enumerate() {
            for (j, s) in rivers.iter().enumerate().skip(i) {
                for (ka, a) in r.points.windows(2).enumerate() {
                    for (kb, b) in s.points.windows(2).enumerate() {
                        if i == j && kb <= ka + 1 {
                            continue;
                        }
                        if segments_cross(a[0], a[1], b[0], b[1]) {
                            out.push(format!(
                                "region {region}: river {i} crosses river {j} near {}",
                                a[0]
                            ));
                        }
                    }
                }
            }
        }

        // Rivers: every one ends in a pond, at the start of another (a
        // crossing piece), across the border, or inside another river with
        // water there.
        for (i, r) in rivers.iter().enumerate() {
            let last = *r.points.last().unwrap();
            let ends_well = t.ponds().iter().any(|p| p.centre == last)
                || !t.region().boundary.contains(last)
                || rivers.iter().enumerate().any(|(j, s)| {
                    j != i
                        && (s.points[0] == last
                            || (line_distance(s, last) <= f64::from(s.width)
                                && t.water_level(last).is_some()))
                })
                || world
                    .terrains()
                    .iter()
                    .any(|u| u.rivers().iter().any(|s| s.points[0] == last));
            if !ends_well {
                let nearest = rivers
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .map(|(_, s)| line_distance(s, last))
                    .fold(f64::INFINITY, f64::min);
                out.push(format!(
                    "region {region}: river {i} ends nowhere at {last}, nearest river {nearest:.1} m"
                ));
            }
        }
    }
    out
}

/// TEMPORARY: writes the blend-1 two-biome world's carved ground around
/// the mountain road for a 3D viewer: a header line, then per sample a
/// little-endian f32 height and a u8 label (0 ground, 1 water, 2 bluff,
/// 3 cliff, 4 road), then the roads and points of interest as JSON.
#[test]
#[ignore]
fn scene_dump() {
    let world = two_biomes(DVec2::ZERO, CONFIGS[10]);
    let (x0, z0, step, n) = (0.0f64, -640.0f64, 2.5f64, 513usize);
    let roads: Vec<&FeatureLine> = world.terrains().iter().flat_map(|t| t.roads()).collect();
    let mut bytes = Vec::with_capacity(n * n * 5);
    let mut labels = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            let p = DVec2::new(x0 + step * i as f64, z0 + step * j as f64);
            let t = world.owner(p).unwrap();
            bytes.extend(t.height(p).to_le_bytes());
            // Label bits: 1 water, 2 road, 4 cut, 8 embankment,
            // 16 retaining wall, 32 bluff, 64 cliff.
            let tags = t.road_tags(p);
            let mut label = u8::from(t.water_level(p).is_some());
            for (bit, tag) in [
                (2u8, procgena_terrain::RoadTags::ROAD),
                (4, procgena_terrain::RoadTags::CUT),
                (8, procgena_terrain::RoadTags::EMBANKMENT),
                (16, procgena_terrain::RoadTags::RETAINING_WALL),
                (32, procgena_terrain::RoadTags::BLUFF),
                (64, procgena_terrain::RoadTags::CLIFF),
            ] {
                if tags.contains(tag) {
                    label |= bit;
                }
            }
            labels.push(label);
        }
    }
    bytes.extend(labels);
    let lines: Vec<String> = roads
        .iter()
        .map(|r| {
            let pts: Vec<String> = r
                .points
                .iter()
                .zip(&r.levels)
                .map(|(p, l)| format!("[{:.2},{:.2},{:.2}]", p.x, l, p.y))
                .collect();
            format!("{{\"width\":{},\"points\":[{}]}}", r.width, pts.join(","))
        })
        .collect();
    let pois: Vec<String> = world
        .terrains()
        .iter()
        .flat_map(|t| t.points_of_interest())
        .map(|p| format!("[{:.2},{:.2},{:.2}]", p.position.x, p.level, p.position.y))
        .collect();
    let dir = std::env::var("SCENE_DIR").expect("SCENE_DIR");
    std::fs::write(format!("{dir}/scene.bin"), bytes).unwrap();
    std::fs::write(
        format!("{dir}/scene.json"),
        format!(
            "{{\"x0\":{x0},\"z0\":{z0},\"step\":{step},\"n\":{n},\"roads\":[{}],\"pois\":[{}]}}",
            lines.join(","),
            pois.join(",")
        ),
    )
    .unwrap();
}

/// Every break of the feature suite's limits in every test world, and in
/// `hills_into_plains` with every seed from 0 to 299.
#[test]
#[ignore]
fn feature_limits() {
    for config in CONFIGS {
        let mut worlds = vec![
            ("two biomes".to_string(), two_biomes(DVec2::ZERO, config)),
            (
                "triple junction".to_string(),
                common::triple_junction(config),
            ),
        ];
        for s in 0..300 {
            worlds.push((
                format!("hills into plains {s}"),
                hills_into_plains(s, config),
            ));
        }
        let mut total = 0;
        for (name, world) in &worlds {
            for problem in feature_problems(world) {
                eprintln!("{}, {name}, {problem}", label(config));
                total += 1;
            }
        }
        eprintln!("{}: {total} problems", label(config));
    }
}
