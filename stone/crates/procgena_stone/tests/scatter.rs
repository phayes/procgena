//! Stones spread over ground by `StoneScatter`.

use std::collections::HashMap;

use glam::{DVec2, Vec2, Vec3};
use procgena_core::scatter::{ScatterArea, ScatterInput};
use procgena_core::{FlatGround, Ground, HeightGrid};
use procgena_stone::{
    OptionsError, StoneClass, StoneInstance, StonePreset, StoneScatter, StoneScatterError,
    StoneScatterOptions,
};

/// Rocks and pebbles, two shapes each, evenly spread.
fn options() -> StoneScatterOptions {
    let mut rocks = StoneClass::new(StonePreset::RiverStone.options(), 0.1, 1.5);
    rocks.variants = 2;
    let mut pebbles = StoneClass::new(StonePreset::Pebble.options(), 1.0, 0.3);
    pebbles.variants = 2;
    StoneScatterOptions {
        seed: 0,
        classes: vec![rocks, pebbles],
    }
}

fn area(centre: DVec2, side: f32) -> ScatterArea {
    ScatterArea::new(centre, Vec2::splat(side))
}

fn scatter(options: &StoneScatterOptions, ground: &dyn Ground, side: f32) -> StoneScatter {
    let input = ScatterInput::new(ground, area(DVec2::ZERO, side), 7);
    StoneScatter::new(options, &input).unwrap()
}

/// What names a stone: its class, cell and index within the cell.
type Name = (usize, [i32; 2], u32);

fn name(s: &StoneInstance) -> Name {
    (s.class, s.cell, s.index)
}

fn by_cell(stones: &[StoneInstance]) -> HashMap<Name, StoneInstance> {
    stones.iter().map(|s| (name(s), *s)).collect()
}

#[test]
fn the_same_input_gives_the_same_stones_and_the_seed_changes_them() {
    let o = options();
    let a = scatter(&o, &FlatGround, 20.0);
    assert!(a.instances().len() > 100);
    assert_eq!(a, scatter(&o, &FlatGround, 20.0));

    let input = ScatterInput::new(&FlatGround, area(DVec2::ZERO, 20.0), 8);
    let other = StoneScatter::new(&o, &input).unwrap();
    assert_ne!(a.instances(), other.instances());
}

#[test]
fn both_classes_are_placed_near_their_densities() {
    let o = options();
    let stones = scatter(&o, &FlatGround, 30.0);
    let count = |class| {
        stones
            .instances()
            .iter()
            .filter(|s| s.class == class)
            .count()
    };
    // 900 m² at 0.1 rocks and 1 pebble per m². Spacing and rocks' footprints
    // remove some.
    assert!((40..=90).contains(&count(0)), "{} rocks", count(0));
    assert!((300..=900).contains(&count(1)), "{} pebbles", count(1));
}

#[test]
fn stones_of_a_class_keep_their_spacing() {
    let o = options();
    let stones = scatter(&o, &FlatGround, 30.0);
    let list = stones.instances();
    for (n, a) in list.iter().enumerate() {
        for b in &list[n + 1..] {
            if a.class != b.class {
                continue;
            }
            let d = (a.position - b.position).with_y(0.0).length();
            let spacing = o.classes[a.class].spacing;
            assert!(d >= spacing - 1e-4, "{d} m apart, need {spacing}");
        }
    }
}

/// Pebbles in clusters of 2 to 5 within 0.4 m, spots 1.5 m apart.
fn clustered() -> StoneScatterOptions {
    let mut pebbles = StoneClass::new(StonePreset::Pebble.options(), 0.2, 1.5);
    pebbles.variants = 2;
    pebbles.members = [2, 5];
    pebbles.cluster_radius = 0.4;
    StoneScatterOptions {
        seed: 0,
        classes: vec![pebbles],
    }
}

#[test]
fn cluster_members_are_copies_numbered_within_their_cell() {
    let o = clustered();
    let stones = scatter(&o, &FlatGround, 30.0);
    let mut cells: HashMap<[i32; 2], Vec<StoneInstance>> = HashMap::new();
    for s in stones.instances() {
        cells.entry(s.cell).or_default().push(*s);
    }
    assert!(cells.len() > 50, "{} clusters", cells.len());

    // Names are unique, and no cell holds more than the most members.
    assert_eq!(by_cell(stones.instances()).len(), stones.instances().len());
    let mut several = 0;
    for members in cells.values() {
        assert!(members.len() <= 5);
        assert!(members.iter().all(|s| s.index < 5));
        several += usize::from(members.len() > 1);

        // Members lie near each other and never overlap.
        for (n, a) in members.iter().enumerate() {
            for b in &members[n + 1..] {
                let d = (a.position - b.position).with_y(0.0).length();
                assert!(d <= 0.8 + 1e-4, "members {d} m apart");
                assert!(d >= a.radius + b.radius - 1e-4, "members overlap");
            }
        }
    }
    assert!(
        several * 2 > cells.len(),
        "{several} of {} clusters",
        cells.len()
    );
}

#[test]
fn a_member_on_missing_ground_is_dropped_and_the_rest_stay() {
    let o = clustered();
    let full = by_cell(scatter(&o, &FlatGround, 30.0).instances());
    // No ground in a band along X = 0.
    let gap = |p: DVec2| if p.x.abs() < 0.3 { f32::NAN } else { 0.0 };
    let cut = scatter(&o, &gap, 30.0);
    let kept = by_cell(cut.instances());
    assert!(kept.len() < full.len());

    // No stone stands in the gap, and every stone found on both grounds is
    // the same: members don't move when a neighbour is dropped. (A stone
    // crowded out on whole ground may appear once its neighbour is gone.)
    for (key, stone) in &kept {
        assert!(stone.position.x.abs() >= 0.3, "{key:?} in the gap");
        if let Some(whole) = full.get(key) {
            assert_eq!(whole, stone);
        }
    }

    // Some clusters lose members to the gap while keeping others.
    let mut split = 0;
    for (key, stone) in &full {
        let (class, cell, _) = *key;
        let lost = !kept.contains_key(key);
        let others_stay = kept
            .keys()
            .any(|&(c, other, _)| c == class && other == cell);
        split += usize::from(lost && others_stay && stone.position.x.abs() < 1.0);
    }
    assert!(split > 0, "no cluster kept part of itself");
}

#[test]
fn lowering_a_density_only_removes_stones() {
    let full = options();
    let mut thin = options();
    thin.classes[1].density = 0.4;
    let many = by_cell(scatter(&full, &FlatGround, 30.0).instances());
    let few = scatter(&thin, &FlatGround, 30.0);
    assert!(few.instances().len() < many.len());
    for stone in few.instances() {
        assert_eq!(many.get(&name(stone)), Some(stone));
    }
}

#[test]
fn patchiness_only_removes_stones_and_leaves_bare_ground() {
    for seed in 0..4 {
        let place = |o: &StoneScatterOptions| {
            let input = ScatterInput::new(&FlatGround, area(DVec2::ZERO, 30.0), seed);
            StoneScatter::new(o, &input).unwrap()
        };
        let even = options();
        let mut patchy = options();
        patchy.classes[1].patchiness = 0.5;
        patchy.classes[1].patch_size = 4.0;
        let many = by_cell(place(&even).instances());
        let few = place(&patchy);
        let pebbles = |s: &StoneScatter| s.instances().iter().filter(|s| s.class == 1).count();
        let even_pebbles = many.values().filter(|s| s.class == 1).count();
        assert!(
            pebbles(&few) * 4 < even_pebbles * 3,
            "seed {seed}: {} vs {even_pebbles}",
            pebbles(&few)
        );
        for stone in few.instances() {
            assert_eq!(many.get(&name(stone)), Some(stone), "seed {seed}");
        }
    }
}

#[test]
fn stones_of_different_classes_never_overlap_at_the_ground() {
    let o = options();
    let stones = scatter(&o, &FlatGround, 30.0);
    let list = stones.instances();
    let mut close_calls = 0;
    for (n, a) in list.iter().enumerate() {
        for b in &list[n + 1..] {
            if a.class == b.class {
                continue;
            }
            let d = (a.position - b.position).with_y(0.0).length();
            assert!(d >= a.radius + b.radius - 1e-4, "{d} m apart");
            close_calls += usize::from(d < 2.0 * (a.radius + b.radius));
        }
    }
    // Pebbles do crowd up to the rocks, so the rule was tested.
    assert!(close_calls > 0);
}

#[test]
fn a_height_grid_with_a_mask_and_a_slope_limit() {
    // A grid exactly covering a 20 m area far from the origin: flat for
    // local X below 2 m, then rising at 45 degrees.
    let centre = DVec2::new(1000.0, 500.0);
    let ground = HeightGrid::from_fn(centre - 10.0, centre + 10.0, 0.5, |p| {
        (p.x - centre.x - 2.0).max(0.0) as f32
    });
    // No stones where local Z is below -5 m.
    let south = move |p: DVec2| if p.y < centre.y - 5.0 { 0.0 } else { 1.0 };
    let input = ScatterInput::new(&ground, area(centre, 20.0), 3).with_mask(&south);
    let stones = StoneScatter::new(&options(), &input).unwrap();
    let list = stones.instances();
    assert!(!list.is_empty());
    for s in list {
        assert!(s.position.z >= -5.0, "masked out: {}", s.position);
        assert!(
            s.position.x <= 2.6,
            "on the 45 degree slope: {}",
            s.position
        );
    }
    // The slope is known up to the grid's edge, so stones reach it.
    assert!(list.iter().any(|s| s.position.x < -9.5));
    assert!(list.iter().any(|s| s.position.z > 9.5));
}

#[test]
fn the_mask_is_read_at_world_positions() {
    let o = options();
    let centre = DVec2::new(250_000.0, -40_000.0);
    // Nothing west of the area's centre.
    let east = |p: DVec2| if p.x >= 250_000.0 { 1.0 } else { 0.0 };
    let input = ScatterInput::new(&FlatGround, area(centre, 20.0), 7).with_mask(&east);
    let stones = StoneScatter::new(&o, &input).unwrap();
    assert!(!stones.instances().is_empty());
    assert!(stones.instances().iter().all(|s| s.position.x >= 0.0));
}

#[test]
fn even_stones_on_flat_ground_look_the_same_anywhere() {
    let o = options();
    let near = scatter(&o, &FlatGround, 16.0);
    let input = ScatterInput::new(&FlatGround, area(DVec2::new(1.0e6, 3.0e5), 16.0), 7);
    let far = StoneScatter::new(&o, &input).unwrap();
    assert_eq!(near, far);
}

#[test]
fn steep_ground_beyond_the_limit_has_no_stones() {
    // A 45 degree ramp.
    let ramp = |p: DVec2| p.x as f32;
    let mut o = options();
    for class in &mut o.classes {
        class.max_slope = 40.0;
    }
    assert!(scatter(&o, &ramp, 20.0).instances().is_empty());
    for class in &mut o.classes {
        class.max_slope = 50.0;
    }
    assert!(!scatter(&o, &ramp, 20.0).instances().is_empty());
}

#[test]
fn stones_tilt_to_the_slope_and_rest_on_it() {
    // A gentle ramp rising 0.3 m per metre along X.
    let ramp = |p: DVec2| (0.3 * p.x) as f32;
    let mut o = options();
    for class in &mut o.classes {
        class.burial = [0.0, 0.0];
    }
    let stones = scatter(&o, &ramp, 20.0);
    assert!(!stones.instances().is_empty());
    let normal = Vec3::new(-0.3, 1.0, 0.0).normalize();
    for s in stones.instances() {
        let up = s.transform.transform_vector3(Vec3::Y).normalize();
        assert!(up.dot(normal) > 0.999, "up {up}");
        // Unburied, the stone's origin sits on the ground or just above it.
        let ground = ramp(DVec2::new(f64::from(s.position.x), f64::from(s.position.z)));
        assert!(
            s.position.y >= ground - 1e-3,
            "{} below {ground}",
            s.position.y
        );
        assert!(
            s.position.y <= ground + 0.5 * s.scale,
            "{} above {ground}",
            s.position.y
        );
    }
}

#[test]
fn burial_sinks_stones_into_the_ground() {
    let mut resting = options();
    let mut buried = options();
    for class in &mut resting.classes {
        class.burial = [0.0, 0.0];
    }
    for class in &mut buried.classes {
        class.burial = [1.0, 1.0];
    }
    let resting = scatter(&resting, &FlatGround, 12.0);
    let buried = by_cell(scatter(&buried, &FlatGround, 12.0).instances());
    for stone in resting.instances() {
        assert!(stone.position.y.abs() < 1e-4);
        let sunk = buried[&name(stone)];
        assert!(sunk.position.y < -1e-4, "not sunk: {}", sunk.position.y);
    }
}

#[test]
fn stones_stay_off_missing_ground() {
    // No ground within 4 m of the centre.
    let holed = |p: DVec2| if p.length() < 4.0 { f32::NAN } else { 0.0 };
    let stones = scatter(&options(), &holed, 20.0);
    assert!(!stones.instances().is_empty());
    for s in stones.instances() {
        assert!(Vec2::new(s.position.x, s.position.z).length() >= 4.0);
    }
}

#[test]
fn variants_match_the_class_settings() {
    let mut o = options();
    o.classes[0].stretch_jitter = 0.3;
    let stones = scatter(&o, &FlatGround, 8.0);
    for v in 0..2 {
        assert_eq!(*stones.variant(0, v), o.classes[0].stone.variant(v, 0.3));
    }
    assert_ne!(stones.variant(0, 0).stretch, stones.variant(0, 1).stretch);
}

#[test]
fn bad_settings_are_named() {
    let mut o = options();
    o.classes[1].density = 20.0;
    let input = ScatterInput::new(&FlatGround, area(DVec2::ZERO, 8.0), 0);
    match StoneScatter::new(&o, &input) {
        Err(StoneScatterError::Class {
            index: 1,
            error: OptionsError::FloatOutOfRange { field, .. },
        }) => assert_eq!(field, "class.density"),
        other => panic!("unexpected {other:?}"),
    }

    let mut o = options();
    o.classes[0].max_slope = 91.0;
    assert!(StoneScatter::new(&o, &input).is_err());

    let o = StoneScatterOptions {
        seed: 0,
        classes: vec![options().classes[0].clone(); 9],
    };
    assert_eq!(
        StoneScatter::new(&o, &input),
        Err(StoneScatterError::TooManyClasses { count: 9 })
    );
}

#[test]
fn huge_areas_are_refused() {
    let input = ScatterInput::new(&FlatGround, area(DVec2::ZERO, 4000.0), 0);
    assert!(matches!(
        StoneScatter::new(&options(), &input),
        Err(StoneScatterError::TooMany { .. })
    ));
}

#[test]
fn the_boulder_field_preset_is_valid_and_places_every_class() {
    let o = StoneScatterOptions::boulder_field();
    o.validate().unwrap();
    let stones = scatter(&o, &FlatGround, 60.0);
    for class in 0..o.classes.len() {
        assert!(
            stones.instances().iter().any(|s| s.class == class),
            "class {class} is empty"
        );
    }
}
