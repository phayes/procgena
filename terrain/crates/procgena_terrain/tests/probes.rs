//! Measurements of the test worlds, run by hand:
//! `cargo test -p procgena_terrain --release --test probes -- --ignored --nocapture`.

mod common;

use common::{CONFIGS, Config, TWO_REGION_BORDER, hills_into_plains, two_terrains};
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
/// along the two-terrain border (mean and largest, with the part of the mean
/// that comes from the presets' mean elevations), and how far the
/// negotiated border moves each side's own ground (mean).
#[test]
#[ignore]
fn border_gap() {
    for config in CONFIGS {
        let world = two_terrains(DVec2::ZERO, config);
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
                            || (line_distance(s, last) <= f64::from(widest(s))
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

/// Writes the carved ground of the test worlds for the 3D viewer, into the
/// directory named by `SCENE_DIR`: per scene, `<key>.bin` holds an f32
/// height per sample (row by row, NaN outside every region), then a label
/// byte per sample (bits: 1 water, 2 road, 4 cut, 8 embankment,
/// 16 retaining wall, 32 bluff, 64 cliff), then the owning region's index
/// per sample (255 outside); `<key>.json` holds the grid, the regions,
/// rivers, roads and points of interest; `scenes.json` lists the scenes.
#[test]
#[ignore]
fn scene_dump() {
    let dir = std::env::var("SCENE_DIR").expect("SCENE_DIR");
    let default = common::DEFAULT;
    let blend_1 = Config {
        blend: 1.0,
        ..default
    };
    let scenes = [
        ("two_terrains", "Two terrains: plains and mountains", two_terrains(DVec2::ZERO, default), 5.0, None),
        ("two_terrains_blend_1", "Two terrains, relief blend 1", two_terrains(DVec2::ZERO, blend_1), 5.0, None),
        (
            "mountain_road",
            "Mountain road close-up (blend 1)",
            two_terrains(DVec2::ZERO, blend_1),
            2.5,
            Some((DVec2::new(0.0, -640.0), DVec2::new(1280.0, 640.0))),
        ),
        ("triple_junction", "Triple junction", common::triple_junction(default), 5.0, None),
        (
            "hills_into_plains_81",
            "Hills into plains, seed 81 (river crossing)",
            hills_into_plains(81, default),
            5.0,
            None,
        ),
        (
            "hills_into_plains_64",
            "Hills into plains, seed 64 (river joins)",
            hills_into_plains(64, default),
            5.0,
            None,
        ),
    ];
    let mut list = Vec::new();
    for (key, title, world, step, window) in &scenes {
        // The window: the whole world unless given.
        let (lo, hi) = window.unwrap_or_else(|| {
            world
                .terrains()
                .iter()
                .flat_map(polygon)
                .fold((DVec2::INFINITY, DVec2::NEG_INFINITY), |(lo, hi), p| {
                    (lo.min(p), hi.max(p))
                })
        });
        let nx = ((hi.x - lo.x) / step).floor() as usize + 1;
        let nz = ((hi.y - lo.y) / step).floor() as usize + 1;

        // Samples.
        let mut heights = Vec::with_capacity(nx * nz * 4);
        let mut labels = Vec::with_capacity(nx * nz);
        let mut owners = Vec::with_capacity(nx * nz);
        for j in 0..nz {
            for i in 0..nx {
                let p = lo + DVec2::new(*step * i as f64, *step * j as f64);
                let Some(t) = world.owner(p) else {
                    heights.extend(f32::NAN.to_le_bytes());
                    labels.push(0);
                    owners.push(255);
                    continue;
                };
                heights.extend(t.height(p).to_le_bytes());
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
                owners.push(
                    world
                        .terrains()
                        .iter()
                        .position(|u| std::ptr::eq(u, t))
                        .unwrap() as u8,
                );
            }
        }
        heights.extend(labels);
        heights.extend(owners);
        std::fs::write(format!("{dir}/{key}.bin"), heights).unwrap();

        // Lines, points and regions.
        let line = |l: &FeatureLine| {
            let pts: Vec<String> = l
                .points
                .iter()
                .zip(&l.levels)
                .map(|(p, h)| format!("[{:.2},{:.2},{:.2}]", p.x, h, p.y))
                .collect();
            format!("{{\"width\":{},\"points\":[{}]}}", widest(l), pts.join(","))
        };
        let terrains = world.terrains();
        let roads: Vec<String> = terrains.iter().flat_map(|t| t.roads()).map(line).collect();
        let rivers: Vec<String> = terrains.iter().flat_map(|t| t.rivers()).map(line).collect();
        let pois: Vec<String> = terrains
            .iter()
            .flat_map(|t| t.points_of_interest())
            .map(|p| format!("[{:.2},{:.2},{:.2}]", p.position.x, p.level, p.position.y))
            .collect();
        let regions: Vec<String> = terrains
            .iter()
            .map(|t| {
                let pts: Vec<String> =
                    polygon(t).iter().map(|p| format!("[{:.2},{:.2}]", p.x, p.y)).collect();
                format!(
                    "{{\"id\":{},\"seed\":{},\"polygon\":[{}]}}",
                    t.region().id.0,
                    t.generator().options().seed,
                    pts.join(",")
                )
            })
            .collect();
        std::fs::write(
            format!("{dir}/{key}.json"),
            format!(
                "{{\"title\":\"{title}\",\"x0\":{},\"z0\":{},\"step\":{step},\"nx\":{nx},\"nz\":{nz},\
                 \"regions\":[{}],\"roads\":[{}],\"rivers\":[{}],\"pois\":[{}]}}",
                lo.x,
                lo.y,
                regions.join(","),
                roads.join(","),
                rivers.join(","),
                pois.join(",")
            ),
        )
        .unwrap();
        list.push(format!("{{\"key\":\"{key}\",\"title\":\"{title}\"}}"));
    }
    std::fs::write(format!("{dir}/scenes.json"), format!("[{}]", list.join(","))).unwrap();
}

/// The widest a feature line gets, in metres.
fn widest(line: &FeatureLine) -> f32 {
    line.widths.iter().copied().fold(0.0, f32::max)
}

/// The polygon of a terrain's region.
fn polygon(t: &procgena_terrain::Terrain) -> Vec<DVec2> {
    match &t.region().boundary {
        procgena_terrain::Boundary::Polygon(p) => p.clone(),
        _ => Vec::new(),
    }
}

/// Every break of the feature suite's limits in every test world, and in
/// `hills_into_plains` with every seed from 0 to 299.
#[test]
#[ignore]
fn feature_limits() {
    for config in CONFIGS {
        let mut worlds = vec![
            ("two terrains".to_string(), two_terrains(DVec2::ZERO, config)),
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
