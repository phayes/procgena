//! Ground scatter: whole mushrooms spread over a `Ground`.

use std::collections::HashMap;

use glam::{DVec2, IVec2, Vec2, Vec3};
use procgena_core::scatter::{ScatterArea, ScatterInput};
use procgena_core::{FlatGround, Ground, SpecimenId};
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

/// The copies of `s` by cell.
fn by_cell(s: &MushroomScatter) -> HashMap<IVec2, &MushroomCopy> {
    s.copies().iter().map(|c| (c.cell, c)).collect()
}

/// Asserts that `a` and `b` are the same copy, wherever their instances
/// sit in their scatters' lists.
fn assert_same_copy(a: &MushroomCopy, b: Option<&&MushroomCopy>) {
    let b = b.unwrap_or_else(|| panic!("cell {} is missing", a.cell));
    assert_eq!(
        (a.slot, a.translation, a.yaw, a.scale, &a.members),
        (b.slot, b.translation, b.yaw, b.scale, &b.members),
        "cell {}",
        a.cell
    );
}

#[test]
fn copies_fill_the_area_at_about_the_density() {
    let porcini = MushroomPreset::Porcini.options();
    let s = scatter(&porcini, &options(), &rolling, DVec2::ZERO, 10.0, 4);
    let n = s.copies().len() as f32;
    assert!((150.0..450.0).contains(&n), "{n} copies on 100 m²");
    for c in s.copies() {
        assert!(c.translation.x.abs() <= 5.0 && c.translation.z.abs() <= 5.0);
        assert_eq!(c.instances.len(), 1, "a single mushroom has one member");
    }
}

#[test]
fn a_larger_area_keeps_the_copies_already_there() {
    let porcini = MushroomPreset::Porcini.options();
    let small = scatter(&porcini, &options(), &rolling, DVec2::ZERO, 6.0, 4);
    let large = scatter(&porcini, &options(), &rolling, DVec2::ZERO, 12.0, 4);
    let large = by_cell(&large);
    assert!(!small.copies().is_empty());
    for c in small.copies() {
        assert_same_copy(c, large.get(&c.cell));
    }
}

#[test]
fn less_density_or_a_mask_only_removes_copies() {
    let porcini = MushroomPreset::Porcini.options();
    let full = scatter(&porcini, &options(), &rolling, DVec2::ZERO, 10.0, 4);
    let full_cells = by_cell(&full);

    let sparse = ScatterOptions {
        density: 1.0,
        ..options()
    };
    let sparse = scatter(&porcini, &sparse, &rolling, DVec2::ZERO, 10.0, 4);
    assert!(sparse.copies().len() < full.copies().len());
    for c in sparse.copies() {
        assert_same_copy(c, full_cells.get(&c.cell));
    }

    let mask = |p: DVec2| if p.x < 0.0 { 0.0 } else { 0.5 };
    let area = ScatterArea::new(DVec2::ZERO, Vec2::splat(10.0));
    let input = ScatterInput::new(&rolling, area, 4).with_mask(&mask);
    let masked = MushroomScatter::new(&porcini, &options(), &input).unwrap();
    assert!(!masked.copies().is_empty());
    for c in masked.copies() {
        assert!(c.translation.x >= 0.0);
        assert_same_copy(c, full_cells.get(&c.cell));
    }
}

#[test]
fn a_patchy_mask_only_removes_copies_even_when_crowded() {
    // Dense enough that neighbours often crowd each other, under a mask
    // that changes from cell to cell.
    let crowded = ScatterOptions {
        density: 20.0,
        ..options()
    };
    let mask = |p: DVec2| (0.5 + 0.5 * (7.0 * p.x).sin() * (5.0 * p.y).cos()) as f32;
    let porcini = MushroomPreset::Porcini.options();
    for seed in [1, 2, 3, 4, 5] {
        let full = scatter(&porcini, &crowded, &rolling, DVec2::ZERO, 8.0, seed);
        let full_cells = by_cell(&full);
        let area = ScatterArea::new(DVec2::ZERO, Vec2::splat(8.0));
        let input = ScatterInput::new(&rolling, area, seed).with_mask(&mask);
        let masked = MushroomScatter::new(&porcini, &crowded, &input).unwrap();
        assert!(masked.copies().len() < full.copies().len());
        for c in masked.copies() {
            assert_same_copy(c, full_cells.get(&c.cell));
        }
    }
}

#[test]
fn a_non_finite_area_is_refused() {
    let input = ScatterInput::new(
        &FlatGround,
        ScatterArea::new(DVec2::ZERO, Vec2::new(f32::INFINITY, 1.0)),
        1,
    );
    let err =
        MushroomScatter::new(&MushroomPreset::Porcini.options(), &options(), &input).unwrap_err();
    assert!(matches!(err, ScatterError::TooLarge { .. }));
}

#[test]
fn steep_ground_is_left_bare() {
    // Flat for x < 0, a slope of about 63° for x > 0, steeper than the
    // default 50° limit.
    let ground = |p: DVec2| (2.0 * p.x.max(0.0)) as f32;
    let porcini = MushroomPreset::Porcini.options();
    assert!(options().max_slope < 63.0);
    let s = scatter(&porcini, &options(), &ground, DVec2::ZERO, 10.0, 4);
    assert!(!s.copies().is_empty());
    assert!(s.copies().iter().all(|c| c.translation.x < 0.05));
}

#[test]
fn a_far_centre_gives_the_same_layout() {
    // Ground that moves with the area, so both see the same shape.
    let far = DVec2::new(1.0e6, -3.0e6);
    let shifted = move |p: DVec2| rolling(p - far);
    let porcini = MushroomPreset::Porcini.options();
    let here = scatter(&porcini, &options(), &rolling, DVec2::ZERO, 8.0, 4);
    let there = scatter(&porcini, &options(), &shifted, far, 8.0, 4);
    assert_eq!(here.copies().len(), there.copies().len());
    for (a, b) in here.copies().iter().zip(there.copies()) {
        assert_eq!(a.cell, b.cell);
        assert!(a.translation.distance(b.translation) < 1e-3);
    }
}

#[test]
fn each_member_stands_on_its_own_ground() {
    let ring = MushroomPreset::FairyRing.options();
    let opts = ScatterOptions {
        density: 0.05,
        min_spacing: 3.0,
        sink: 0.0,
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
    let ring = MushroomPreset::FairyRing.options();
    let opts = ScatterOptions {
        density: 0.2,
        min_spacing: 1.0,
        ..options()
    };
    let flat = scatter(&ring, &opts, &FlatGround, DVec2::ZERO, 6.0, 9);
    let holed = scatter(&ring, &opts, &ground, DVec2::ZERO, 6.0, 9);
    let whole = by_cell(&flat);
    let mut partial = 0;
    for c in holed.copies() {
        let full = whole.get(&c.cell).expect("the same copies");
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
    assert!(partial > 0, "no ring crossed the edge of the ground");
}

#[test]
fn specimens_come_from_the_input_seed() {
    let porcini = MushroomPreset::Porcini.options();
    let opts = options();
    let s = scatter(&porcini, &opts, &rolling, DVec2::ZERO, 6.0, 11);
    assert_eq!(s.bodies().len(), opts.specimens as usize);
    for (slot, body) in s.bodies().iter().enumerate() {
        let seed = SpecimenId::new(11, porcini.pool(), slot as u32).seed();
        assert_eq!(body.id().seed, seed);
        // The same as that specimen grown on its own.
        let alone = Mushroom::new(MushroomOptions {
            seed,
            ..porcini.clone()
        })
        .unwrap();
        assert_eq!(alone.bodies()[0].options(), body.options());
    }
    // Neither the options' seed nor the mushroom's seed takes part.
    let mut other = porcini.clone();
    other.seed += 5;
    let reseeded = ScatterOptions {
        seed: opts.seed + 5,
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
        MushroomScatter::new(&MushroomPreset::Porcini.options(), &options(), &input).unwrap_err();
    assert!(matches!(err, ScatterError::TooLarge { .. }));
}
