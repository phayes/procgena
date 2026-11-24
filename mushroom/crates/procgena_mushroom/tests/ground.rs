//! Ground scatter: whole mushrooms spread over a `Ground`.

use std::collections::HashMap;

use glam::{DVec2, Vec2, Vec3};
use procgena_core::scatter::{Placement, Rest, ScatterArea, ScatterInput};
use procgena_core::{CopyAddress, FlatGround, Ground, SpecimenId};
use procgena_mushroom::{
    Mushroom, MushroomCopy, MushroomOptions, MushroomPreset, MushroomScatter, ScatterError,
    ScatterOptions,
};

/// Gently rolling ground, no steeper than about 15°.
fn rolling(p: DVec2) -> f32 {
    let (x, z) = (p.x as f32, p.y as f32);
    0.2 * (0.9 * x).sin() + 0.15 * (0.7 * z + 0.3 * x).cos()
}

/// Ground options for porcini at a lively density.
fn options() -> ScatterOptions {
    ScatterOptions {
        density: 3.0,
        min_spacing: 0.2,
        ..ScatterOptions::default()
    }
}

/// Scatters `mushroom` with `options` over `ground` in a square of `size`
/// metres around `centre`, seeded with `seed`.
fn scatter(
    mushroom: &MushroomOptions,
    options: &ScatterOptions,
    ground: &dyn Ground,
    centre: DVec2,
    size: f32,
    seed: u32,
) -> MushroomScatter {
    let input = ScatterInput::new(ground, ScatterArea::new(centre, Vec2::splat(size)), seed);
    MushroomScatter::new(mushroom, options, &input).unwrap()
}

/// The copies of `s` by address.
fn by_cell(s: &MushroomScatter) -> HashMap<CopyAddress, &MushroomCopy> {
    s.copies().iter().map(|c| (c.address, c)).collect()
}

/// Asserts that `a` and `b` are the same copy, wherever their instances
/// sit in their scatters' lists.
fn assert_same_copy(a: &MushroomCopy, b: Option<&&MushroomCopy>) {
    let b = b.unwrap_or_else(|| panic!("{:?} is missing", a.address));
    assert_eq!(
        (a.slot, a.translation, a.yaw, a.scale, &a.members),
        (b.slot, b.translation, b.yaw, b.scale, &b.members),
        "{:?}",
        a.address
    );
}

#[test]
fn copies_fill_the_area_at_about_the_density() {
    let porcini = MushroomPreset::porcini().mushroom.clone();
    let s = scatter(&porcini, &options(), &rolling, DVec2::ZERO, 10.0, 4);
    let n = s.copies().len() as f32;
    assert!((150.0..450.0).contains(&n), "{n} copies on 100 m²");
    // The area holds the copies whose cell's corner lies inside it, so a
    // copy may stand up to one 0.2 m cell past its edge.
    for c in s.copies() {
        assert!(c.translation.x.abs() <= 5.2 && c.translation.z.abs() <= 5.2);
        assert_eq!(c.instances.len(), 1, "a single mushroom has one member");
    }
}

#[test]
fn a_larger_area_keeps_the_copies_already_there() {
    let porcini = MushroomPreset::porcini().mushroom.clone();
    let small = scatter(&porcini, &options(), &rolling, DVec2::ZERO, 6.0, 4);
    let large = scatter(&porcini, &options(), &rolling, DVec2::ZERO, 12.0, 4);
    let large = by_cell(&large);
    assert!(!small.copies().is_empty());
    for c in small.copies() {
        assert_same_copy(c, large.get(&c.address));
    }
}

#[test]
fn less_density_or_a_mask_only_removes_copies() {
    let porcini = MushroomPreset::porcini().mushroom.clone();
    let full = scatter(&porcini, &options(), &rolling, DVec2::ZERO, 10.0, 4);
    let full_cells = by_cell(&full);

    let sparse = ScatterOptions {
        density: 1.0,
        ..options()
    };
    let sparse = scatter(&porcini, &sparse, &rolling, DVec2::ZERO, 10.0, 4);
    assert!(sparse.copies().len() < full.copies().len());
    for c in sparse.copies() {
        assert_same_copy(c, full_cells.get(&c.address));
    }

    let mask = |p: DVec2| if p.x < 0.0 { 0.0 } else { 0.5 };
    let area = ScatterArea::new(DVec2::ZERO, Vec2::splat(10.0));
    let input = ScatterInput::new(&rolling, area, 4).with_mask(&mask);
    let masked = MushroomScatter::new(&porcini, &options(), &input).unwrap();
    assert!(!masked.copies().is_empty());
    for c in masked.copies() {
        assert!(c.translation.x >= 0.0);
        assert_same_copy(c, full_cells.get(&c.address));
    }
}

/// An even mask only removes copies, even where neighbours crowd each
/// other. (A mask that varies from place to place reranks candidates where
/// it varies, so it may also swap which of two crowded neighbours stands;
/// see "Masks and placement" in ARCHITECTURE.md.)
#[test]
fn an_even_mask_only_removes_copies_even_when_crowded() {
    let crowded = ScatterOptions {
        density: 20.0,
        ..options()
    };
    let mask = |_: DVec2| 0.45;
    let porcini = MushroomPreset::porcini().mushroom.clone();
    for seed in [1, 2, 3, 4, 5] {
        let full = scatter(&porcini, &crowded, &rolling, DVec2::ZERO, 8.0, seed);
        let full_cells = by_cell(&full);
        let area = ScatterArea::new(DVec2::ZERO, Vec2::splat(8.0));
        let input = ScatterInput::new(&rolling, area, seed).with_mask(&mask);
        let masked = MushroomScatter::new(&porcini, &crowded, &input).unwrap();
        assert!(masked.copies().len() < full.copies().len());
        for c in masked.copies() {
            assert_same_copy(c, full_cells.get(&c.address));
        }
    }
}

#[test]
fn the_species_placement_is_the_default_and_a_scatter_can_override_it() {
    let porcini = MushroomPreset::porcini().mushroom.clone();
    let default = porcini.ground_placement();
    assert_eq!(
        default,
        Placement {
            conform: 0.0,
            rest: Rest::Lowest,
            embed: 0.3,
            max_bend: None,
        }
    );

    // No override and the species' own placement give the same scatter.
    let own = ScatterOptions {
        placement: Some(default),
        ..options()
    };
    let plain = scatter(&porcini, &options(), &FlatGround, DVec2::ZERO, 4.0, 3);
    let explicit = scatter(&porcini, &own, &FlatGround, DVec2::ZERO, 4.0, 3);
    assert_eq!(plain.instances(), explicit.instances());

    // Sinking deeper lowers every mushroom by its stem radius times the
    // extra depth.
    let deep = ScatterOptions {
        placement: Some(Placement {
            embed: 1.0,
            ..default
        }),
        ..options()
    };
    let deep = scatter(&porcini, &deep, &FlatGround, DVec2::ZERO, 4.0, 3);
    assert!(!plain.instances().is_empty());
    for (a, b) in plain.instances().iter().zip(deep.instances()) {
        let radius = plain.bodies()[a.variant as usize].base_radius() * a.scale;
        assert!((a.translation.y - b.translation.y - 0.7 * radius).abs() < 1e-5);
    }

    // A placement out of range is refused.
    let bad = ScatterOptions {
        placement: Some(Placement {
            conform: 1.5,
            ..default
        }),
        ..options()
    };
    let input = ScatterInput::new(&FlatGround, ScatterArea::new(DVec2::ZERO, Vec2::ONE), 1);
    assert!(matches!(
        MushroomScatter::new(&porcini, &bad, &input),
        Err(ScatterError::Options(_))
    ));
}

#[test]
fn a_non_finite_area_is_refused() {
    let input = ScatterInput::new(
        &FlatGround,
        ScatterArea::new(DVec2::ZERO, Vec2::new(f32::INFINITY, 1.0)),
        1,
    );
    let err =
        MushroomScatter::new(&MushroomPreset::porcini().mushroom, &options(), &input).unwrap_err();
    assert!(matches!(err, ScatterError::TooLarge { .. }));
}

#[test]
fn steep_ground_is_left_bare() {
    // Flat for x < 0, a slope of about 63° for x > 0, steeper than the
    // default 50° limit.
    let ground = |p: DVec2| (2.0 * p.x.max(0.0)) as f32;
    let porcini = MushroomPreset::porcini().mushroom.clone();
    assert!(options().max_slope < 63.0);
    let s = scatter(&porcini, &options(), &ground, DVec2::ZERO, 10.0, 4);
    assert!(!s.copies().is_empty());
    assert!(s.copies().iter().all(|c| c.translation.x < 0.05));
}

/// Copies stand on a lattice fixed to the World: any area holding a spot,
/// far from the origin or not, gives it the same copy and address, with
/// positions relative to its own centre.
#[test]
fn the_same_world_spot_gives_the_same_copy_from_any_area() {
    let far = DVec2::new(1.0e6, -3.0e6);
    let porcini = MushroomPreset::porcini().mushroom.clone();
    let world = |s: &MushroomScatter, centre: DVec2| {
        s.copies()
            .iter()
            .map(|c| {
                let p = centre + DVec2::new(f64::from(c.translation.x), f64::from(c.translation.z));
                (c.address, (p, c.slot, c.yaw, c.scale))
            })
            .collect::<HashMap<_, _>>()
    };
    let shifted = far + DVec2::new(1.3, -0.7);
    let small = world(&scatter(&porcini, &options(), &rolling, far, 6.0, 4), far);
    let large = world(
        &scatter(&porcini, &options(), &rolling, shifted, 10.0, 4),
        shifted,
    );
    assert!(small.len() > 20);
    for (address, (p, slot, yaw, scale)) in &small {
        let (q, other_slot, other_yaw, other_scale) = large[address];
        assert!(p.distance(q) < 1e-3, "{p} vs {q}");
        assert_eq!((*slot, *yaw, *scale), (other_slot, other_yaw, other_scale));
    }
}

#[test]
fn each_member_stands_on_its_own_ground() {
    let ring = MushroomPreset::fairy_ring().mushroom.clone();
    let opts = ScatterOptions {
        density: 0.05,
        min_spacing: 3.0,
        placement: Some(Placement {
            embed: 0.0,
            ..ring.ground_placement()
        }),
        ..options()
    };
    let s = scatter(&ring, &opts, &rolling, DVec2::ZERO, 30.0, 2);
    let copy = s
        .copies()
        .iter()
        .find(|c| c.instances.len() > 10)
        .expect("a fairy ring with members");
    // Members sit on the ground under their own stems, at different heights.
    let heights: Vec<f32> = s.instances()[copy.instances.clone()]
        .iter()
        .map(|m| {
            let ground = rolling(DVec2::new(m.translation.x.into(), m.translation.z.into()));
            assert!(
                m.translation.y <= ground + 1e-4 && m.translation.y > ground - 0.05,
                "member at {} over ground {ground}",
                m.translation
            );
            m.translation.y
        })
        .collect();
    let spread = heights.iter().cloned().fold(f32::MIN, f32::max)
        - heights.iter().cloned().fold(f32::MAX, f32::min);
    assert!(spread > 0.01, "every member at the same height");
    // Members stay upright: only the cluster's own lean tips them.
    for m in &s.instances()[copy.instances.clone()] {
        assert!((m.rotation * Vec3::Y).y > 0.5);
    }
}

#[test]
fn a_member_without_ground_is_dropped_alone() {
    // No ground for x > 1: a ring centred near there loses part of itself.
    let ground = |p: DVec2| if p.x > 1.0 { f32::NAN } else { 0.0 };
    let ring = MushroomPreset::fairy_ring().mushroom.clone();
    let opts = ScatterOptions {
        density: 0.2,
        min_spacing: 1.0,
        ..options()
    };
    let mut partial = 0;
    for seed in 9..21 {
        let flat = scatter(&ring, &opts, &FlatGround, DVec2::ZERO, 6.0, seed);
        let holed = scatter(&ring, &opts, &ground, DVec2::ZERO, 6.0, seed);
        let whole = by_cell(&flat);
        for c in holed.copies() {
            // A copy with no member on ground is gone before spacing, so a
            // neighbour it crowded out on whole ground may appear; compare the
            // copies both grounds grow.
            let Some(full) = whole.get(&c.address) else {
                continue;
            };
            // The members kept are a subset, with their own indices.
            assert!(c.members.iter().all(|m| full.members.contains(m)));
            assert!(c.members.windows(2).all(|w| w[0] < w[1]));
            if c.members.len() < full.members.len() {
                partial += 1;
                assert!(!c.members.is_empty());
            }
            for m in &holed.instances()[c.instances.clone()] {
                assert!(m.translation.x <= 1.0);
            }
        }
    }
    assert!(partial > 0, "no ring crossed the edge of the ground");
}

#[test]
fn specimens_come_from_the_input_seed() {
    let porcini = MushroomPreset::porcini().mushroom.clone();
    let opts = options();
    let s = scatter(&porcini, &opts, &rolling, DVec2::ZERO, 6.0, 11);
    assert_eq!(s.bodies().len(), opts.specimens as usize);
    for (slot, body) in s.bodies().iter().enumerate() {
        let seed = SpecimenId::new(11, porcini.pool(), slot as u32).seed();
        assert_eq!(body.id().seed, seed);
        // The same as that specimen grown on its own.
        let alone = Mushroom::new(MushroomOptions {
            default_seed: seed,
            ..porcini.clone()
        })
        .unwrap();
        assert_eq!(alone.bodies()[0].options(), body.options());
    }
    // Neither the options' seed nor the mushroom's seed takes part.
    let mut other = porcini.clone();
    other.default_seed += 5;
    let reseeded = ScatterOptions {
        default_seed: opts.default_seed + 5,
        ..opts.clone()
    };
    let again = scatter(&other, &reseeded, &rolling, DVec2::ZERO, 6.0, 11);
    assert_eq!(again.copies(), s.copies());
    assert_eq!(again.instances(), s.instances());
}

#[test]
fn an_oversized_area_is_refused() {
    let input = ScatterInput::new(
        &FlatGround,
        ScatterArea::new(DVec2::ZERO, Vec2::splat(10_000.0)),
        1,
    );
    let err =
        MushroomScatter::new(&MushroomPreset::porcini().mushroom, &options(), &input).unwrap_err();
    assert!(matches!(err, ScatterError::TooLarge { .. }));
}
