//! Relief convergence: neighbours fade their relief towards an agreed
//! amplitude near their shared edges, and leave it alone elsewhere.

mod common;

use common::{CONFIGS, DEFAULT, JUNCTION, TWO_REGION_BORDER, preset, triple_junction, two_regions};
use glam::DVec2;
use procgena_terrain::{
    Convergence, DEFAULT_RELIEF_BLEND, StitchedWorld, Terrain, TerrainGenerator, TerrainPreset,
};

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

/// Points on a grid over the whole two-region world.
fn grid_points() -> Vec<DVec2> {
    (0..=40)
        .flat_map(|i| {
            (0..=30).map(move |j| {
                DVec2::new(
                    -2000.0 + 100.0 * f64::from(i),
                    -1500.0 + 100.0 * f64::from(j),
                )
            })
        })
        .collect()
}

/// The terrain's own height as if it had no neighbours.
fn unconverged(terrain: &Terrain) -> TerrainGenerator {
    TerrainGenerator::new(terrain.generator().options().clone()).expect("valid options")
}

/// The blends the suites check: none, the default, and only blending.
const BLENDS: [f32; 3] = [0.0, DEFAULT_RELIEF_BLEND, 1.0];

fn plains_and_mountains(meeting: f32, blend: f32) -> StitchedWorld {
    two_regions(
        DVec2::ZERO,
        preset(TerrainPreset::Plains, 1),
        preset(TerrainPreset::Mountains, 1),
        Convergence { meeting, blend },
    )
}

/// Plains and mountains converging by scaling their relief alone.
fn scaling(meeting: f32) -> StitchedWorld {
    plains_and_mountains(meeting, 0.0)
}

#[test]
fn alike_neighbours_already_match_at_their_border() {
    for blend in BLENDS {
        for p in TerrainPreset::ALL {
            let convergence = Convergence {
                blend,
                ..DEFAULT.convergence()
            };
            let world = two_regions(DVec2::ZERO, preset(p, 7), preset(p, 7), convergence);
            let [west, east] = [&world.terrains()[0], &world.terrains()[1]];
            for q in border_points(5.0) {
                let (a, b) = (
                    west.generator().natural_height(q),
                    east.generator().natural_height(q),
                );
                assert_eq!(
                    a,
                    b,
                    "{}, blend {blend}: own heights differ at {q}",
                    p.name()
                );
            }
        }
    }
}

#[test]
fn relief_deep_inside_a_region_is_unchanged() {
    for blend in BLENDS {
        let world = plains_and_mountains(DEFAULT.meeting, blend);
        let mountains = &world.terrains()[1];
        let own = unconverged(mountains);
        for z in [-1300.0, -400.0, 0.0, 600.0, 1300.0] {
            let p = DVec2::new(1950.0, z);
            assert_eq!(
                mountains.generator().natural_height(p).to_bits(),
                own.natural_height(p).to_bits(),
                "blend {blend}, at {p}"
            );
        }
    }
}

#[test]
fn relief_changes_near_the_border() {
    for blend in BLENDS {
        let world = plains_and_mountains(DEFAULT.meeting, blend);
        for t in world.terrains() {
            let own = unconverged(t);
            let changed = border_points(50.0)
                .into_iter()
                .filter(|&q| t.generator().natural_height(q) != own.natural_height(q))
                .count();
            assert!(
                changed > 0,
                "region {} doesn't converge at blend {blend}",
                t.region().id.0
            );
        }
    }
}

#[test]
fn only_the_rougher_side_changes_at_negative_infinity() {
    for blend in BLENDS {
        let world = plains_and_mountains(f32::NEG_INFINITY, blend);
        let plains = &world.terrains()[0];
        let own = unconverged(plains);
        for q in grid_points().into_iter().chain(border_points(5.0)) {
            assert_eq!(
                plains.generator().natural_height(q).to_bits(),
                own.natural_height(q).to_bits(),
                "plains changed at {q}, blend {blend}"
            );
        }
    }
}

#[test]
fn the_rougher_side_flattens_towards_its_border() {
    // Spread of the mountains' own heights along the border, before and
    // after converging towards the plains.
    let spread = |heights: Vec<f32>| {
        let n = heights.len() as f32;
        let mean = heights.iter().sum::<f32>() / n;
        (heights.iter().map(|h| (h - mean).powi(2)).sum::<f32>() / n).sqrt()
    };
    let world = scaling(DEFAULT.meeting);
    let mountains = &world.terrains()[1];
    let own = unconverged(mountains);
    let points = border_points(5.0);
    let before = spread(points.iter().map(|&q| own.natural_height(q)).collect());
    let after = spread(
        points
            .iter()
            .map(|&q| mountains.generator().natural_height(q))
            .collect(),
    );
    assert!(
        after < 0.2 * before,
        "spread {before} m before, {after} m after"
    );
}

#[test]
fn stated_relief_amplitude_matches_the_ground() {
    for p in TerrainPreset::ALL {
        let generator = TerrainGenerator::new(preset(p, 3)).expect("valid preset");
        let heights: Vec<f64> = (0..40_000)
            .map(|i| {
                let i = f64::from(i);
                let q = DVec2::new((i * 61.803) % 60_000.0, (i * 1.37) % 60_000.0);
                f64::from(generator.natural_height(q))
            })
            .collect();
        let n = heights.len() as f64;
        let mean = heights.iter().sum::<f64>() / n;
        let measured = (heights.iter().map(|h| (h - mean).powi(2)).sum::<f64>() / n).sqrt();
        let stated = f64::from(generator.relief_amplitude());
        assert!(
            (stated / measured - 1.0).abs() < 0.3,
            "{}: stated {stated} m, measured {measured} m",
            p.name()
        );
    }
}

#[test]
fn bad_relief_meetings_are_refused() {
    for meeting in [1.5, f32::NAN, f32::INFINITY] {
        let specs = vec![procgena_terrain::RegionSpec {
            id: procgena_terrain::RegionId(1),
            polygon: vec![DVec2::ZERO, DVec2::X * 100.0, DVec2::ONE * 100.0],
            options: preset(TerrainPreset::Plains, 1),
        }];
        let convergence = Convergence {
            meeting,
            ..Convergence::default()
        };
        assert!(matches!(
            StitchedWorld::build(specs, &convergence),
            Err(procgena_terrain::StitchError::ReliefMeeting(_))
        ));
    }
}

#[test]
fn bad_relief_blends_are_refused() {
    for blend in [-0.1, 1.5, f32::NAN] {
        let specs = vec![procgena_terrain::RegionSpec {
            id: procgena_terrain::RegionId(1),
            polygon: vec![DVec2::ZERO, DVec2::X * 100.0, DVec2::ONE * 100.0],
            options: preset(TerrainPreset::Plains, 1),
        }];
        let convergence = Convergence {
            blend,
            ..Convergence::default()
        };
        assert!(matches!(
            StitchedWorld::build(specs, &convergence),
            Err(procgena_terrain::StitchError::ReliefBlend(_))
        ));
    }
}

#[test]
fn road_grades_rise_to_the_steeper_neighbours_at_the_border() {
    let world = plains_and_mountains(DEFAULT.meeting, DEFAULT.blend);
    let [plains, mountains] = [&world.terrains()[0], &world.terrains()[1]];
    let own = |t: &Terrain| t.generator().options().features.roads.grades();
    let steeper = own(plains).steepest(own(mountains));
    assert_eq!(steeper, own(mountains));

    // On the border both sides allow the mountains' grades.
    for q in border_points(25.0) {
        assert_eq!(plains.generator().road_grades(q), steeper, "plains at {q}");
        assert_eq!(
            mountains.generator().road_grades(q),
            steeper,
            "mountains at {q}"
        );
    }
    // The mountains keep their own everywhere; the plains change gradually
    // and keep their own deep inside.
    let mut last = steeper;
    for k in 0..=39 {
        let q = DVec2::new(-50.0 * f64::from(k), 300.0);
        if mountains.region().boundary.contains(q) {
            continue;
        }
        let here = plains.generator().road_grades(q);
        assert!(here.grade <= last.grade && here.grade >= own(plains).grade);
        last = here;
    }
    assert_eq!(
        plains.generator().road_grades(DVec2::new(-1950.0, 0.0)),
        own(plains)
    );
    for q in grid_points() {
        if mountains.region().boundary.contains(q) {
            assert_eq!(mountains.generator().road_grades(q), own(mountains));
        }
    }
}

/// Points along the segment from `a` to `b`, at most `spacing` metres
/// apart, ends included.
fn segment_points(a: (f64, f64), b: (f64, f64), spacing: f64) -> Vec<DVec2> {
    let (a, b) = (DVec2::from(a), DVec2::from(b));
    let count = (a.distance(b) / spacing).ceil() as usize;
    (0..=count)
        .map(|k| a.lerp(b, k as f64 / count as f64))
        .collect()
}

/// The largest difference in own height, and in own slope across the
/// border, between `a` and `b` at `points` (slopes by central differences
/// along `normal`).
fn largest_differences(a: &Terrain, b: &Terrain, points: &[DVec2], normal: DVec2) -> (f32, f32) {
    let step = 0.5;
    let slope = |t: &Terrain, q: DVec2| {
        let g = t.generator();
        (g.natural_height(q + normal * step) - g.natural_height(q - normal * step))
            / (2.0 * step as f32)
    };
    points.iter().fold((0.0f32, 0.0f32), |(h, s), &q| {
        let dh = (a.generator().natural_height(q) - b.generator().natural_height(q)).abs();
        let ds = (slope(a, q) - slope(b, q)).abs();
        (h.max(dh), s.max(ds))
    })
}

#[test]
fn fully_blended_neighbours_share_their_ground_on_the_border() {
    for config in CONFIGS {
        let config = common::Config {
            blend: 1.0,
            ..config
        };
        let world = common::two_terrains(DVec2::ZERO, config);
        let [plains, mountains] = [&world.terrains()[0], &world.terrains()[1]];
        for q in border_points(5.0) {
            assert_eq!(
                plains.generator().natural_height(q).to_bits(),
                mountains.generator().natural_height(q).to_bits(),
                "{config:?}: own heights differ at {q}"
            );
        }
    }
}

#[test]
fn fully_blended_neighbours_share_their_slope_across_the_border() {
    for config in CONFIGS {
        let config = common::Config {
            blend: 1.0,
            ..config
        };
        let world = common::two_terrains(DVec2::ZERO, config);
        let [plains, mountains] = [&world.terrains()[0], &world.terrains()[1]];
        for w in TWO_REGION_BORDER.windows(2) {
            let (a, b) = (DVec2::from(w[0]), DVec2::from(w[1]));
            let normal = (b - a).perp().normalize();
            // Away from the bends, where the slope across has no one direction.
            let points: Vec<DVec2> = segment_points(w[0], w[1], 10.0)
                .into_iter()
                .filter(|q| q.distance(a) > 20.0 && q.distance(b) > 20.0)
                .collect();
            let (_, slope) = largest_differences(plains, mountains, &points, normal);
            assert!(
                slope < 0.02,
                "{config:?}: slopes differ by {slope} near {a}"
            );
        }
    }
}

#[test]
fn fully_blended_triple_junction_shares_its_ground() {
    for config in CONFIGS {
        let config = common::Config {
            blend: 1.0,
            ..config
        };
        let world = triple_junction(config);
        let t = |i: usize| &world.terrains()[i];
        let edges = [
            ((-800.0, -500.0), 0, 2),
            ((-50.0, 900.0), 0, 1),
            ((700.0, -600.0), 1, 2),
        ];
        for (end, i, j) in edges {
            let points = segment_points(JUNCTION, end, 5.0);
            let (a, b) = (DVec2::from(JUNCTION), DVec2::from(end));
            let normal = (b - a).perp().normalize();
            let (height, _) = largest_differences(t(i), t(j), &points, normal);
            assert!(
                height < 1e-3,
                "{config:?}: regions {} and {} differ by {height} m towards {end:?}",
                t(i).region().id.0,
                t(j).region().id.0
            );
        }
    }
}
