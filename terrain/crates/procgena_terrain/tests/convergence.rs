//! Relief convergence: neighbours fade their relief towards an agreed
//! amplitude near their shared edges, and leave it alone elsewhere.

mod common;

use common::{DEFAULT, TWO_REGION_BORDER, preset, two_regions};
use glam::DVec2;
use procgena_terrain::{StitchedWorld, Terrain, TerrainGenerator, TerrainPreset};

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

fn plains_and_mountains(meeting: f32) -> StitchedWorld {
    two_regions(
        DVec2::ZERO,
        preset(TerrainPreset::Plains, 1),
        preset(TerrainPreset::Mountains, 1),
        meeting,
    )
}

#[test]
fn alike_neighbours_already_match_at_their_border() {
    for p in TerrainPreset::ALL {
        let world = two_regions(DVec2::ZERO, preset(p, 7), preset(p, 7), DEFAULT.meeting);
        let [west, east] = [&world.terrains()[0], &world.terrains()[1]];
        for q in border_points(5.0) {
            let (a, b) = (
                west.generator().natural_height(q),
                east.generator().natural_height(q),
            );
            assert_eq!(a, b, "{}: own heights differ at {q}", p.name());
        }
    }
}

#[test]
fn relief_deep_inside_a_region_is_unchanged() {
    let world = plains_and_mountains(DEFAULT.meeting);
    let mountains = &world.terrains()[1];
    let own = unconverged(mountains);
    for z in [-1300.0, -400.0, 0.0, 600.0, 1300.0] {
        let p = DVec2::new(1950.0, z);
        assert_eq!(
            mountains.generator().natural_height(p).to_bits(),
            own.natural_height(p).to_bits(),
            "at {p}"
        );
    }
}

#[test]
fn relief_changes_near_the_border() {
    let world = plains_and_mountains(DEFAULT.meeting);
    for t in world.terrains() {
        let own = unconverged(t);
        let changed = border_points(50.0)
            .into_iter()
            .filter(|&q| t.generator().natural_height(q) != own.natural_height(q))
            .count();
        assert!(changed > 0, "region {} doesn't converge", t.region().id.0);
    }
}

#[test]
fn only_the_rougher_side_changes_at_negative_infinity() {
    let world = plains_and_mountains(f32::NEG_INFINITY);
    let plains = &world.terrains()[0];
    let own = unconverged(plains);
    for q in grid_points().into_iter().chain(border_points(5.0)) {
        assert_eq!(
            plains.generator().natural_height(q).to_bits(),
            own.natural_height(q).to_bits(),
            "plains changed at {q}"
        );
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
    let world = plains_and_mountains(DEFAULT.meeting);
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
        assert!(matches!(
            StitchedWorld::build(specs, meeting),
            Err(procgena_terrain::StitchError::ReliefMeeting(_))
        ));
    }
}
