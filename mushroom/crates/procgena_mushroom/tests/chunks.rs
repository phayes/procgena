//! Ground Mushroom Scatters placed chunk by chunk on a World lattice: the
//! same copies with the same addresses in any order, lookups by address,
//! and footprints kept clear.

use glam::{DVec2, Vec2};
use procgena_core::scatter::conformance::{
    assert_chunks_match_area, assert_keeps_clear, assert_resolves,
};
use procgena_core::scatter::{ChunkGrid, Disc, Footprints, Scatter, ScatterArea, ScatterInput};
use procgena_core::{AddressPrefix, FlatGround, RegionId};
use procgena_mushroom::{MushroomPreset, MushroomScatter, ScatterError, ScatterOptions};

fn rolling(p: DVec2) -> f32 {
    let (x, z) = (p.x as f32, p.y as f32);
    0.2 * (0.9 * x).sin() + 0.15 * (0.7 * z + 0.3 * x).cos()
}

fn porcini() -> MushroomScatter {
    let options = ScatterOptions {
        density: 3.0,
        min_spacing: 0.2,
        ..ScatterOptions::default()
    };
    MushroomScatter::prepare(&MushroomPreset::porcini().mushroom, &options, 8).unwrap()
}

#[test]
fn chunks_in_any_order_give_the_copies_of_the_whole_area() {
    let scatter = porcini();
    let mask = |p: DVec2| (0.5 + 0.5 * (p.y * 1.3).sin()) as f32;
    let area = ScatterArea::new(DVec2::new(77.7, 3.3), Vec2::new(9.0, 7.0));
    let input = ScatterInput::new(&rolling, area, 5)
        .with_mask(&mask)
        .with_prefix(AddressPrefix {
            region: RegionId(1),
            ecotope: 5,
            scatter: 6,
        });
    // Chunks the 0.2 m lattice doesn't divide, so copies straddle edges.
    let count = assert_chunks_match_area(&scatter, &input, ChunkGrid::new(2.3));
    assert!(count > 50, "{count} copies");
    assert_chunks_match_area(&scatter, &input, ChunkGrid::new(4.0));
}

#[test]
fn fairy_rings_in_chunks_match_the_whole_area_too() {
    let options = ScatterOptions {
        density: 0.1,
        min_spacing: 2.0,
        ..ScatterOptions::default()
    };
    let rings =
        MushroomScatter::prepare(&MushroomPreset::fairy_ring().mushroom, &options, 3).unwrap();
    let area = ScatterArea::new(DVec2::ZERO, Vec2::splat(30.0));
    let count = assert_chunks_match_area(
        &rings,
        &ScatterInput::new(&rolling, area, 2),
        ChunkGrid::new(7.1),
    );
    assert!(count > 5, "{count} rings");
}

#[test]
fn copies_resolve_from_their_address() {
    let area = ScatterArea::new(DVec2::new(-2.0, 9.0), Vec2::splat(3.0));
    assert_resolves(&porcini(), &ScatterInput::new(&rolling, area, 4));
}

#[test]
fn copies_keep_clear_of_the_footprints_above_them() {
    let above: Footprints = (-3..3)
        .flat_map(|i| (-3..3).map(move |j| DVec2::new(i as f64 * 0.7, j as f64 * 0.7)))
        .map(|p| Disc::new(p, 0.2))
        .collect();
    let area = ScatterArea::new(DVec2::ZERO, Vec2::splat(4.0));
    let (kept, free) =
        assert_keeps_clear(&porcini(), &ScatterInput::new(&FlatGround, area, 3), &above);
    assert!(kept < free && kept > 0, "{kept} of {free}");
}

#[test]
fn a_scatter_grown_on_a_host_cannot_be_placed_on_the_ground() {
    let host = procgena_specimen::Specimen::single(vec![procgena_specimen::Part::new(
        [procgena_specimen::tags::GROUND],
        procgena_specimen::SurfaceMesh {
            positions: vec![[-1.0, 0.0, -1.0], [1.0, 0.0, -1.0], [0.0, 0.0, 1.0]],
            normals: vec![[0.0, 1.0, 0.0]; 3],
            indices: vec![0, 2, 1],
        },
    )]);
    let options = ScatterOptions::default();
    let grown = MushroomScatter::on(&host, &MushroomPreset::porcini().mushroom, &options).unwrap();
    let area = ScatterArea::new(DVec2::ZERO, Vec2::ONE);
    let error = grown
        .place(&ScatterInput::new(&FlatGround, area, 1))
        .unwrap_err();
    assert!(error.downcast_ref::<ScatterError>() == Some(&ScatterError::HostOnly));
}

#[test]
fn the_scatter_reports_its_reach_and_slots() {
    let scatter = porcini();
    let reach = scatter.reach();
    assert_eq!(reach.rounds, 5);
    assert_eq!(reach.spacing, 0.2);
    assert!(reach.footprint > 0.0);
    assert!(scatter.draw_range() > 1.0 && scatter.draw_range() < 1000.0);
    let slots = scatter.slots(8);
    assert_eq!(slots.len(), 3);
    for (slot, body) in slots.iter().zip(scatter.bodies()) {
        assert_eq!(slot.specimen.seed(), body.id().seed);
    }
}
