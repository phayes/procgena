use glam::{DVec2, Vec2};
use procgena_core::FlatGround;
use procgena_core::scatter::{Disc, Footprints, ScatterArea, ScatterInput};

use super::*;

/// A 4 m by 2 m area around World position (10, -6).
fn area() -> ScatterArea {
    ScatterArea::new(DVec2::new(10.0, -6.0), Vec2::new(4.0, 2.0))
}

#[test]
fn flat_open_ground_grows_everywhere_at_its_height() {
    let bake = PlantScatterOptions::default().bake(&ScatterInput::new(&FlatGround, area(), 1), 0.5);
    assert_eq!(bake.texels, [8, 4]);
    assert_eq!(bake.size, Vec2::new(4.0, 2.0));
    assert!(bake.heights.iter().all(|&h| h == 0.0));
    assert!(bake.density.iter().all(|&d| d == 1.0));
    // The first texel is at the −X, −Z corner.
    assert_eq!(bake.texel_centre(0, 0), Vec2::new(-1.75, -0.75));
}

#[test]
fn the_grid_has_at_least_one_and_at_most_the_limit_of_texels() {
    let options = PlantScatterOptions::default();
    let tiny = ScatterArea::new(DVec2::ZERO, Vec2::splat(0.1));
    assert_eq!(
        options
            .bake(&ScatterInput::new(&FlatGround, tiny, 0), 0.5)
            .texels,
        [1, 1]
    );
    let huge = ScatterArea::new(DVec2::ZERO, Vec2::new(5000.0, 1.0));
    let bake = options.bake(&ScatterInput::new(&FlatGround, huge, 0), 0.5);
    assert_eq!(bake.texels, [MAX_BAKE_TEXELS, 2]);
}

#[test]
fn holes_have_no_height_and_grow_nothing() {
    // No ground west of x = 10.
    let ground = |p: DVec2| if p.x < 10.0 { f32::NAN } else { 2.0 };
    let bake = PlantScatterOptions::default().bake(&ScatterInput::new(&ground, area(), 1), 0.5);
    for z in 0..bake.texels[1] {
        for x in 0..bake.texels[0] {
            let i = (z * bake.texels[0] + x) as usize;
            let west = bake.texel_centre(x, z).x < 0.0;
            assert_eq!(bake.heights[i].is_nan(), west, "texel {x}, {z}");
            assert_eq!(bake.density[i], if west { 0.0 } else { 1.0 });
        }
    }
}

#[test]
fn ground_steeper_than_the_limit_grows_nothing() {
    // Rises half a metre per metre: about 26.6°.
    let ground = |p: DVec2| 0.5 * p.x as f32;
    let input = ScatterInput::new(&ground, area(), 1);
    let at = |max_slope: f32| {
        let options = PlantScatterOptions {
            max_slope,
            ..PlantScatterOptions::default()
        };
        options.bake(&input, 0.5).density
    };
    assert!(at(20.0).iter().all(|&d| d == 0.0));
    assert!(at(30.0).iter().all(|&d| d == 1.0));
    assert!(at(90.0).iter().all(|&d| d == 1.0));
}

#[test]
fn the_mask_sets_the_density_at_each_texel_centre() {
    let mask = |p: DVec2| ((p.x - 8.0) / 4.0) as f32;
    let input = ScatterInput {
        mask: Some(&mask),
        ..ScatterInput::new(&FlatGround, area(), 1)
    };
    let bake = PlantScatterOptions::default().bake(&input, 0.5);
    for x in 0..bake.texels[0] {
        let expected = (bake.texel_centre(x, 0).x + 2.0) / 4.0;
        assert!((bake.density[x as usize] - expected).abs() < 1e-6);
    }
}

#[test]
fn a_footprint_clears_every_texel_it_touches_and_no_other() {
    // A thin trunk, 0.2 m across, whose disc covers no texel centre at all.
    let trunk = Disc::new(DVec2::new(9.5, -6.0), 0.1);
    let exclude: Footprints = [trunk].into_iter().collect();
    let input = ScatterInput {
        exclude: Some(&exclude),
        ..ScatterInput::new(&FlatGround, area(), 1)
    };
    let bake = PlantScatterOptions::default().bake(&input, 0.5);
    let cleared: Vec<(u32, u32)> = (0..bake.texels[1])
        .flat_map(|z| (0..bake.texels[0]).map(move |x| (x, z)))
        .filter(|&(x, z)| bake.density[(z * bake.texels[0] + x) as usize] == 0.0)
        .collect();
    // The trunk stands on the corner of four texels, and touches each.
    assert_eq!(cleared, vec![(2, 1), (3, 1), (2, 2), (3, 2)]);
}
