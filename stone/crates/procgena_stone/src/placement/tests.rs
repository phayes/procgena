use glam::Vec3;

use super::*;
use crate::{Mesher, StoneOptions, StonePreset, mesh_field};

/// Creates settings for a smooth, uncut sphere with unscaled radius 0.4.
fn sphere_options() -> StoneOptions {
    let mut options = StoneOptions::default();
    options.carve.steps = 0;
    options.carve.base_radius = 0.8;
    options.surface.displacement_scale = 0.0;
    options
}

/// Creates placement settings with the requested settling mode and no burial cap.
fn settling(settle: bool) -> PlacementOptions {
    PlacementOptions {
        settle,
        ..PlacementOptions::default()
    }
}

/// Estimates placement for valid rock settings; panics if they are invalid.
fn placement(options: &StoneOptions) -> StonePlacement {
    StonePlacement::new(&StoneField::new(options).unwrap(), &options.placement)
}

/// Generates a grounded, unscaled mesh using the supplied settings and detail.
fn placed_mesh(options: &StoneOptions, mesher: Mesher, resolution: u32) -> StoneMesh {
    let field = StoneField::new(options).unwrap();
    let mut mesh = mesh_field(&field, mesher, resolution).unwrap();
    placement(options).apply(&mut mesh);
    mesh
}

#[test]
fn every_mesh_rests_on_the_ground() {
    // Both surface-building methods should reach ground level within half a
    // cell, even when mesh detail differs from placement detail.
    for settle in [false, true] {
        let options = StoneOptions {
            placement: settling(settle),
            ..StoneOptions::default()
        };
        for mesher in [Mesher::MarchingCubes, Mesher::DualContouring] {
            for resolution in [24, 48, 96] {
                let (min, _) = placed_mesh(&options, mesher, resolution).bounds().unwrap();
                let cell = 1.0 / resolution.min(ANALYSIS_RESOLUTION) as f32;
                assert!(
                    min[1].abs() < 0.5 * cell,
                    "settle {settle}, {mesher:?} at {resolution}: lowest point {}",
                    min[1]
                );
            }
        }
    }
}

#[test]
fn centre_of_mass_is_above_the_origin() {
    let options = StoneOptions::default();
    let field = StoneField::new(&options).unwrap();
    let grid = SampleGrid::sample(&field, ANALYSIS_RESOLUTION);
    let placement = StonePlacement::from_grid(&grid, &settling(true));
    let com = center_of_mass(&grid).unwrap();
    let placed = placement.rotation() * com + placement.translation();
    assert!(placed.x.abs() < 1e-5 && placed.z.abs() < 1e-5, "{placed}");
    assert!(placed.y > 0.1, "{placed}");
}

#[test]
fn unsettled_rocks_keep_their_orientation() {
    let options = StoneOptions {
        placement: settling(false),
        ..StoneOptions::default()
    };
    assert_eq!(placement(&options).rotation(), Mat3::IDENTITY);
}

#[test]
fn a_slab_settles_onto_its_broad_face() {
    // A slab that starts thin along z should settle onto its broad face.
    let options = StoneOptions {
        stretch: [1.0, 0.8, 0.3],
        ..StoneOptions::default()
    };
    let (min, max) = placed_mesh(&options, Mesher::MarchingCubes, 48)
        .bounds()
        .unwrap();
    let size = Vec3::from(max) - Vec3::from(min);
    assert!(
        size.y < 0.5 * size.x.min(size.z),
        "slab should lie flat, placed size {size}"
    );
}

#[test]
fn settling_ends_at_a_local_minimum() {
    // Small tested tilts should not noticeably lower the rock's balance point.
    for seed in [1, 7, 880, 4242] {
        let options = StoneOptions {
            seed,
            ..StoneOptions::default()
        };
        let field = StoneField::new(&options).unwrap();
        let grid = SampleGrid::sample(&field, ANALYSIS_RESOLUTION);
        let com = center_of_mass(&grid).unwrap();
        let offsets: Vec<Vec3> = crossing_edges(&grid)
            .iter()
            .map(|&(s, axis)| crossing_position(&grid, s, axis) - com)
            .collect();
        let (down, height) = settle_direction(&offsets);
        for j in 1..REFINE_CANDIDATES {
            let tilted = refine_direction(down, 0.05, j);
            let tilted_height = support_height(&offsets, tilted);
            assert!(
                tilted_height > height - 1e-3,
                "seed {seed}: tilt {j} lowers the centre of mass from {height} to {tilted_height}"
            );
        }
    }
}

#[test]
fn a_sphere_sinks_to_just_below_its_middle() {
    // A sphere's slices stay within 90% of its widest area down to about
    // 0.32 radii below the middle, so it sinks to about 0.34 of its height.
    let expected = 0.5 * (1.0 - (0.1f32).sqrt());
    for settle in [false, true] {
        let options = StoneOptions {
            placement: settling(settle),
            ..sphere_options()
        };
        let (_, max) = placed_mesh(&options, Mesher::MarchingCubes, 64)
            .bounds()
            .unwrap();
        let embed = placement(&options).embed_depth();
        let cell = 1.0 / ANALYSIS_RESOLUTION as f32;
        assert!(
            (embed - expected * max[1]).abs() < 2.0 * cell && embed < 0.5 * max[1],
            "embed {embed} for a sphere of height {}",
            max[1]
        );
    }
}

/// A rounded box: a sphere too large for its containing cube, clipped flat on
/// every side, kept upright so its sides are straight.
fn box_options() -> StoneOptions {
    let mut options = sphere_options();
    options.carve.base_radius = 1.6;
    options.placement.settle = false;
    options
}

#[test]
fn straight_sides_sink_by_the_minimum() {
    // Every slice of a box is about as wide as the widest, so the lowest one
    // is chosen and the minimum depth decides how far it sinks.
    let mut options = box_options();
    let (_, max) = placed_mesh(&options, Mesher::MarchingCubes, 64)
        .bounds()
        .unwrap();
    let embed = placement(&options).embed_depth();
    assert!(
        embed < 0.12 * max[1],
        "embed {embed} for a box of height {}",
        max[1]
    );

    // A larger minimum sinks it further.
    options.placement.min_embed_fraction = 0.3;
    let embed = placement(&options).embed_depth();
    let cell = 1.0 / ANALYSIS_RESOLUTION as f32;
    assert!(
        (embed - 0.3 * max[1]).abs() < cell,
        "embed {embed} for a box of height {}",
        max[1]
    );
}

#[test]
fn the_minimum_cannot_exceed_the_maximum() {
    let mut options = StoneOptions::default();
    options.placement.max_embed_fraction = 0.2;
    options.placement.min_embed_fraction = 0.3;
    assert!(matches!(
        options.validate(),
        Err(crate::OptionsError::FloatOutOfRange {
            field: "placement.min_embed_fraction",
            ..
        })
    ));
    options.placement.min_embed_fraction = 0.2;
    assert_eq!(options.validate(), Ok(()));
}

#[test]
fn the_embed_limit_caps_the_search() {
    // The sphere is widest halfway up, but the cap allows burial only within
    // the lowest quarter of its height.
    let mut options = sphere_options();
    options.placement.max_embed_fraction = 0.25;
    let (_, max) = placed_mesh(&options, Mesher::MarchingCubes, 64)
        .bounds()
        .unwrap();
    let embed = placement(&options).embed_depth();
    let cell = 1.0 / ANALYSIS_RESOLUTION as f32;
    assert!(
        embed <= 0.25 * max[1] + cell && embed > 0.25 * max[1] - 2.0 * cell,
        "embed {embed} for a sphere of height {}",
        max[1]
    );

    // A zero cap and minimum must suggest zero burial.
    options.placement.max_embed_fraction = 0.0;
    options.placement.min_embed_fraction = 0.0;
    assert_eq!(placement(&options).embed_depth(), 0.0);
}

#[test]
fn every_preset_sinks_partway() {
    // Each preset should suggest partial burial within its configured cap,
    // allowing for the difference between mesh and placement estimates.
    for preset in StonePreset::ALL {
        let options = preset.options();
        let (_, max) = placed_mesh(&options, Mesher::MarchingCubes, 48)
            .bounds()
            .unwrap();
        let embed = placement(&options).embed_depth();
        let limit = options.placement.max_embed_fraction * max[1];
        assert!(
            embed > 0.0 && embed < max[1] && embed <= limit + 1.0 / 48.0,
            "{}: embed {embed}, height {}",
            preset.name(),
            max[1]
        );
    }
}

#[test]
fn placed_vertices_lie_on_the_field_surface() {
    let options = StoneOptions::default();
    let field = StoneField::new(&options).unwrap();
    let placement = placement(&options);
    let mut mesh = mesh_field(&field, Mesher::MarchingCubes, 32).unwrap();
    let original = mesh.positions.clone();
    placement.apply(&mut mesh);
    for (placed, original) in mesh.positions.iter().zip(&original) {
        let back = placement.field_position(Vec3::from(*placed));
        assert!(back.distance(Vec3::from(*original)) < 1e-5);
    }
    assert!(
        mesh.positions
            .iter()
            .all(|&p| Vec3::from(p).length() <= placement.bounding_radius())
    );
}

#[test]
fn rotations_turn_the_down_direction_to_negative_y() {
    let mut directions: Vec<Vec3> = (0..DIRECTION_COUNT).map(sphere_direction).collect();
    directions.extend([
        Vec3::Y,
        Vec3::NEG_Y,
        Vec3::X,
        Vec3::new(1e-4, 1.0, 0.0).normalize(),
    ]);
    for d in directions {
        let r = rotation_to_down(d);
        assert!((r * d).distance(Vec3::NEG_Y) < 1e-4, "{d}");
        assert!((r * r.transpose()).abs_diff_eq(Mat3::IDENTITY, 1e-4), "{d}");
        assert!((r.determinant() - 1.0).abs() < 1e-4, "{d}");
    }
}

#[test]
fn spread_directions_are_unit_length_and_end_near_negative_y() {
    for i in 0..DIRECTION_COUNT {
        assert!((sphere_direction(i).length() - 1.0).abs() < 1e-5);
    }
    assert!(sphere_direction(DIRECTION_COUNT - 1).y < -0.99);
}

#[test]
fn empty_rocks_get_no_placement() {
    let mut options = StoneOptions::default();
    options.carve.depth = 50.0;
    assert_eq!(placement(&options), StonePlacement::EMPTY);
}


#[test]
fn blocky_rocks_sink_only_a_little() {
    // Box-like rocks have nearly straight sides, so a low slice is already
    // nearly as wide as the widest one.
    for seed in [3, 8, 13] {
        let mut options = StoneOptions {
            seed,
            ..StoneOptions::default()
        };
        options.carve.blockiness = 1.0;
        let (_, max) = placed_mesh(&options, Mesher::MarchingCubes, 48)
            .bounds()
            .unwrap();
        let embed = placement(&options).embed_depth();
        assert!(
            embed < 0.25 * max[1],
            "seed {seed}: embed {embed} for a blocky rock of height {}",
            max[1]
        );
    }
}
