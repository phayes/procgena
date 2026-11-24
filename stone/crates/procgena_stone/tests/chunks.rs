//! Stone Scatters placed chunk by chunk on a World lattice: the same stones
//! with the same addresses in any order, lookups by address, and
//! footprints kept clear.

use glam::{DVec2, Vec2};
use procgena_core::scatter::conformance::{
    assert_chunks_match_area, assert_keeps_clear, assert_resolves,
};
use procgena_core::scatter::{ChunkGrid, Disc, Footprints, Scatter, ScatterArea, ScatterInput};
use procgena_core::{AddressPrefix, FlatGround, RegionId};
use procgena_stone::{StoneScatter, StoneScatterOptions};

fn rolling(p: DVec2) -> f32 {
    (p.x * 0.07).sin() as f32 + (p.y * 0.05).cos() as f32 * 1.5
}

fn boulder_field() -> StoneScatter {
    StoneScatter::prepare(&StoneScatterOptions::boulder_field(), 5).unwrap()
}

#[test]
fn chunks_in_any_order_give_the_stones_of_the_whole_area() {
    let scatter = boulder_field();
    let mask = |p: DVec2| (0.6 + 0.4 * (p.y * 0.05).sin()) as f32;
    let area = ScatterArea::new(DVec2::new(-700.0, 1234.5), Vec2::new(40.0, 30.0));
    let input = ScatterInput::new(&rolling, area, 2)
        .with_mask(&mask)
        .with_prefix(AddressPrefix {
            region: RegionId(9),
            ecotope: 1,
            scatter: 2,
        });
    // Chunks the lattice doesn't divide, so stones and clusters straddle
    // their edges.
    let count = assert_chunks_match_area(&scatter, &input, ChunkGrid::new(9.7));
    assert!(count > 50, "{count} stones");
    assert_chunks_match_area(&scatter, &input, ChunkGrid::new(16.0));
}

#[test]
fn stones_resolve_from_their_address() {
    let area = ScatterArea::new(DVec2::new(40.0, -12.0), Vec2::splat(12.0));
    assert_resolves(&boulder_field(), &ScatterInput::new(&rolling, area, 3));
}

#[test]
fn stones_keep_clear_of_the_trunks_above_them() {
    // Trunks 0.4 m across every 2 m.
    let trunks: Footprints = (-6..6)
        .flat_map(|i| (-6..6).map(move |j| DVec2::new(i as f64 * 2.0, j as f64 * 2.0)))
        .map(|p| Disc::new(p, 0.4))
        .collect();
    let area = ScatterArea::new(DVec2::ZERO, Vec2::splat(20.0));
    let (kept, free) = assert_keeps_clear(
        &boulder_field(),
        &ScatterInput::new(&FlatGround, area, 1),
        &trunks,
    );
    assert!(kept < free && kept > 0, "{kept} of {free}");
}

#[test]
fn the_scatter_reports_its_reach_and_slots() {
    let scatter = boulder_field();
    let reach = scatter.reach();
    assert_eq!(reach.rounds, 5);
    assert!(reach.spacing >= 6.0, "boulders are 6 m apart");
    assert!(reach.displacement >= scatter.lattice().cell_metres());
    assert!(reach.footprint > 0.0);
    let slots = scatter.slots(5);
    let options = scatter.options();
    for (class, c) in options.classes.iter().enumerate() {
        for v in 0..c.variants {
            let slot = slots[scatter.slot(class, v) as usize];
            assert_eq!(slot.entry as usize, class);
            assert_eq!(slot.specimen.seed(), scatter.variant(class, v).default_seed);
        }
    }
}
