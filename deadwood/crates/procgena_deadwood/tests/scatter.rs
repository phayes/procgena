//! Stick scatter over flat and shaped ground.

use glam::{DVec2, Vec2, Vec3};
use procgena_core::scatter::{Scatter, ScatterArea, ScatterInput};
use procgena_deadwood::{
    FlatGround, Ground, ScatterError, ScatterOptions, StickScatter, default_lod,
};
use procgena_specimen::{Part, Specimen, SurfaceMesh, TagFilter, tags};

/// Kindling, to scatter over [`small_area`].
fn small() -> ScatterOptions {
    ScatterOptions::kindling()
}

/// A 3 m square around the World origin, to keep the tests quick.
fn small_area() -> ScatterArea {
    ScatterArea::new(DVec2::ZERO, Vec2::splat(3.0))
}

/// Scatters `o` over `ground` in [`small_area`], with `o`'s default seed.
fn scatter(o: &ScatterOptions, ground: &dyn Ground) -> StickScatter {
    StickScatter::new(o, &ScatterInput::new(ground, small_area(), o.default_seed)).unwrap()
}

#[test]
fn scatter_is_repeatable_spaced_and_in_bounds() {
    let o = small();
    let a = scatter(&o, &FlatGround);
    let b = scatter(&o, &FlatGround);
    assert_eq!(a, b);
    assert!(!a.instances().is_empty());
    assert_eq!(a.variants().len(), o.variants as usize);

    let sites: Vec<Vec2> = a
        .instances()
        .iter()
        .map(|i| {
            assert!((i.variant as usize) < a.variants().len());
            assert!(i.transform.translation.is_finite());
            Vec2::new(i.position.x, i.position.z)
        })
        .collect();
    // The area holds the sticks whose lattice cell's corner it holds, so a
    // stick may stand up to one cell past its edge.
    let reach = 1.5 + a.lattice().cell_metres() + 1e-4;
    for (k, s) in sites.iter().enumerate() {
        assert!(s.x.abs() <= reach && s.y.abs() <= reach, "{s}");
        for t in &sites[k + 1..] {
            assert!(s.distance(*t) >= o.min_spacing - 1e-4);
        }
    }
}

#[test]
fn the_mask_thins_the_scatter() {
    let o = small();
    let full = scatter(&o, &FlatGround);
    let half_bare = |p: DVec2| if p.x < 0.0 { 0.0 } else { 1.0 };
    let input = ScatterInput::new(&FlatGround, small_area(), o.default_seed).with_mask(&half_bare);
    let thinned = StickScatter::new(&o, &input).unwrap();
    assert!(thinned.instances().len() < full.instances().len());
    for i in thinned.instances() {
        assert!(i.position.x >= 0.0, "{}", i.position);
    }
}

#[test]
fn sticks_follow_a_slope() {
    let o = small();
    let slope = |p: DVec2| 0.3 * p.x as f32;
    let s = scatter(&o, &slope);
    assert!(!s.instances().is_empty());
    for i in s.instances() {
        let p = i.position;
        let up = i.transform.matrix3 * glam::Vec3A::Y;
        // Tilted to follow the slope, with the reported position on the
        // ground at the stick's site.
        assert!(up.x < -0.1, "up {up}");
        assert!(
            (p.y - slope(Vec2::new(p.x, p.z).as_dvec2())).abs() < 1e-4,
            "site {p}"
        );
        let middle = i.transform.transform_point3(glam::Vec3::ZERO);
        assert!(middle.is_finite());
    }
}

#[test]
fn ground_steeper_than_the_limit_is_left_bare() {
    // 0.3 rises about 17°, 1.0 rises 45°: kindling allows up to 35°.
    let gentle = |p: DVec2| 0.3 * p.x as f32;
    let steep = |p: DVec2| p.x as f32;
    let o = small();
    assert!(!scatter(&o, &gentle).instances().is_empty());
    assert!(scatter(&o, &steep).instances().is_empty());
    // With no limit, the steep ground is covered too.
    let unlimited = ScatterOptions {
        max_slope: 90.0,
        ..small()
    };
    assert!(!scatter(&unlimited, &steep).instances().is_empty());
}

#[test]
fn an_area_away_from_the_origin_reads_the_ground_there() {
    // A step at x = 1000: the area around (1000.5, 0) has ground only on
    // its high side, and positions stay relative to the area's centre.
    let o = small();
    let step = |p: DVec2| if p.x < 1000.0 { f32::NAN } else { 2.0 };
    let area = ScatterArea::new(DVec2::new(1000.5, 0.0), Vec2::splat(3.0));
    let s = StickScatter::new(&o, &ScatterInput::new(&step, area, o.default_seed)).unwrap();
    assert!(!s.instances().is_empty());
    for i in s.instances() {
        assert!(i.position.x >= -0.5, "{}", i.position);
        assert!(
            i.position.x <= 1.5 + s.lattice().cell_metres() + 1e-4,
            "{}",
            i.position
        );
        assert_eq!(i.position.y, 2.0);
    }
}

#[test]
fn no_ground_drops_sticks() {
    let o = small();
    let hole = |p: DVec2| if p.x < 0.0 { f32::NAN } else { 0.0 };
    let s = scatter(&o, &hole);
    for i in s.instances() {
        assert!(i.position.x >= 0.0);
    }
}

#[test]
fn height_grid_from_a_specimen_follows_its_top() {
    // A tilted square, tagged as ground.
    let mesh = SurfaceMesh {
        positions: vec![
            [-2.0, -0.4, -2.0],
            [2.0, 0.4, -2.0],
            [2.0, 0.4, 2.0],
            [-2.0, -0.4, 2.0],
        ],
        normals: vec![[0.0, 1.0, 0.0]; 4],
        indices: vec![0, 2, 1, 0, 3, 2],
    };
    let specimen = Specimen::single(vec![Part::new([tags::GROUND], mesh)]);
    let grid = specimen
        .height_grid(0, &TagFilter::any_of([tags::GROUND]), 0.1)
        .unwrap();
    assert!((grid.height(DVec2::new(1.0, 0.5)) - 0.2).abs() < 1e-3);
    let s = scatter(&small(), &grid);
    assert!(!s.instances().is_empty());
}

#[test]
fn too_many_sticks_is_an_error() {
    let o = ScatterOptions {
        density: 10.0,
        ..ScatterOptions::twig_litter()
    };
    let area = ScatterArea::new(DVec2::ZERO, Vec2::splat(1000.0));
    assert!(matches!(
        StickScatter::new(&o, &ScatterInput::new(&FlatGround, area, o.default_seed)),
        Err(ScatterError::TooMany { .. })
    ));
}

#[test]
fn an_empty_area_is_an_error() {
    let o = small();
    let area = ScatterArea::new(DVec2::ZERO, Vec2::new(3.0, 0.0));
    assert!(matches!(
        StickScatter::new(&o, &ScatterInput::new(&FlatGround, area, o.default_seed)),
        Err(ScatterError::Options(_))
    ));
}

#[test]
fn fallen_logs_are_logs() {
    let o = ScatterOptions::fallen_logs();
    let area = ScatterArea::new(DVec2::ZERO, Vec2::splat(20.0));
    let s = StickScatter::new(&o, &ScatterInput::new(&FlatGround, area, o.default_seed)).unwrap();
    assert!(!s.instances().is_empty());
    for v in s.variants() {
        assert_eq!(v.options().kind, procgena_deadwood::PieceKind::Log);
        assert!(v.options().diameter >= 0.2 && v.options().diameter <= 0.5);
    }
}

#[test]
fn baked_specimen_is_valid() {
    let s = scatter(&small(), &FlatGround);
    let specimen = s.specimen(&default_lod()).unwrap();
    specimen.validate().unwrap();
    let part = &specimen.levels[0].parts[0];
    assert!(part.closed);
    let centre: Vec3 = part
        .mesh
        .positions
        .iter()
        .map(|p| Vec3::from(*p))
        .sum::<Vec3>()
        / part.mesh.positions.len() as f32;
    assert!(centre.y > -0.1);
}

#[test]
fn more_variants_keep_the_existing_ones() {
    // Variant `v` is slot `v` of the variant pool, and the number of
    // variants is not part of the pool, so raising it only adds variants.
    let few = ScatterOptions {
        variants: 3,
        ..small()
    };
    let more = ScatterOptions {
        variants: 5,
        ..small()
    };
    let a = scatter(&few, &FlatGround);
    let b = scatter(&more, &FlatGround);
    assert_eq!(a.variants(), &b.variants()[..3]);
}

#[test]
fn each_input_seed_grows_its_own_variants() {
    let o = small();
    let a = scatter(&o, &FlatGround);
    let input = ScatterInput::new(&FlatGround, small_area(), o.default_seed + 1);
    let b = StickScatter::new(&o, &input).unwrap();
    for (va, vb) in a.variants().iter().zip(b.variants()) {
        assert_ne!(va, vb);
    }
}

#[test]
fn the_input_seed_wins_over_the_options_seed() {
    let o = small();
    let other = ScatterOptions {
        default_seed: o.default_seed + 7,
        ..small()
    };
    let input = ScatterInput::new(&FlatGround, small_area(), o.default_seed);
    let (a, b) = (
        StickScatter::new(&o, &input).unwrap(),
        StickScatter::new(&other, &input).unwrap(),
    );
    assert_eq!(a.variants(), b.variants());
    assert_eq!(a.instances(), b.instances());
}

/// A few stumps over a 6 m square of `ground`, allowed on any slope.
fn stumps(ground: &dyn Ground) -> StickScatter {
    let o = ScatterOptions {
        max_slope: 90.0,
        density: 0.5,
        min_spacing: 1.5,
        variants: 2,
        decay: [0.5, 1.5],
        diameter: [0.3, 0.4],
        length: [0.4, 0.6],
        piece: procgena_deadwood::DeadwoodPreset::felled_stump()
            .deadwood
            .clone(),
        ..small()
    };
    let area = ScatterArea::new(DVec2::ZERO, Vec2::splat(6.0));
    StickScatter::new(&o, &ScatterInput::new(ground, area, o.default_seed)).unwrap()
}

#[test]
fn stumps_stand_upright_on_a_slope() {
    let slope = |p: DVec2| 0.4 * p.x as f32;
    let s = stumps(&slope);
    assert!(!s.instances().is_empty());
    for i in s.instances() {
        // Turned about Y only.
        let up = i.transform.matrix3 * glam::Vec3A::Y;
        assert!((up - glam::Vec3A::Y).length() < 1e-5, "up {up}");
    }
}

#[test]
fn stumps_sink_to_the_low_side_of_a_slope() {
    // On flat ground the base rests at y = 0. On a slope the same stump
    // rests where the lowest ground around its base is, below the ground
    // at its middle.
    let flat = stumps(&FlatGround);
    let slope = |p: DVec2| 0.4 * p.x as f32;
    let tilted = stumps(&slope);
    for i in flat.instances() {
        assert!(i.transform.translation.y.abs() < 1e-4, "{}", i.position);
    }
    for i in tilted.instances() {
        let base = i.transform.translation.y;
        assert!(
            base < i.position.y - 1e-3,
            "base {base}, ground {}",
            i.position.y
        );
    }
}

#[test]
fn logs_lie_along_a_slope() {
    let o = ScatterOptions {
        max_slope: 90.0,
        ..ScatterOptions::fallen_logs()
    };
    let slope = |p: DVec2| 0.4 * p.x as f32;
    let area = ScatterArea::new(DVec2::ZERO, Vec2::splat(20.0));
    let s = StickScatter::new(&o, &ScatterInput::new(&slope, area, o.default_seed)).unwrap();
    assert!(!s.instances().is_empty());
    let normal = glam::Vec3A::new(-0.4, 1.0, 0.0).normalize();
    for i in s.instances() {
        let up = i.transform.matrix3 * glam::Vec3A::Y;
        assert!((up - normal).length() < 1e-3, "up {up}");
    }
}
