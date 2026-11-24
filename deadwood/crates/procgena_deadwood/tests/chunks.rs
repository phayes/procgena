//! Deadwood Scatters placed chunk by chunk on a World lattice: the same
//! copies with the same addresses in any order, lookups by address,
//! footprints kept clear, and logs' footprints along their length.

use glam::{DVec2, Vec2};
use procgena_core::scatter::conformance::{
    assert_chunks_match_area, assert_keeps_clear, assert_resolves,
};
use procgena_core::scatter::{ChunkGrid, Disc, Footprints, Scatter, ScatterArea, ScatterInput};
use procgena_core::{AddressPrefix, FlatGround, RegionId};
use procgena_deadwood::{ScatterOptions, StickScatter};

fn rolling(p: DVec2) -> f32 {
    (p.x * 0.3).sin() as f32 * 0.2 + (p.y * 0.2).cos() as f32 * 0.3
}

fn kindling() -> StickScatter {
    StickScatter::prepare(&ScatterOptions::kindling(), 4).unwrap()
}

#[test]
fn chunks_in_any_order_give_the_sticks_of_the_whole_area() {
    let scatter = kindling();
    let mask = |p: DVec2| (0.5 + 0.5 * (p.x * 0.7).sin()) as f32;
    let area = ScatterArea::new(DVec2::new(321.1, -45.0), Vec2::new(12.0, 9.0));
    let input = ScatterInput::new(&rolling, area, 6)
        .with_mask(&mask)
        .with_prefix(AddressPrefix {
            region: RegionId(2),
            ecotope: 3,
            scatter: 4,
        });
    // Chunks the 0.3 m lattice doesn't divide, so sticks straddle edges.
    let count = assert_chunks_match_area(&scatter, &input, ChunkGrid::new(2.9));
    assert!(count > 40, "{count} sticks");
    assert_chunks_match_area(&scatter, &input, ChunkGrid::new(4.0));
}

#[test]
fn logs_in_chunks_match_the_whole_area_too() {
    let logs = StickScatter::prepare(&ScatterOptions::fallen_logs(), 1).unwrap();
    let area = ScatterArea::new(DVec2::new(10.0, 10.0), Vec2::splat(60.0));
    let input = ScatterInput::new(&FlatGround, area, 2);
    let count = assert_chunks_match_area(&logs, &input, ChunkGrid::new(17.0));
    assert!(count > 20, "{count} logs");
}

#[test]
fn sticks_resolve_from_their_address() {
    let area = ScatterArea::new(DVec2::new(-5.0, 2.0), Vec2::splat(4.0));
    assert_resolves(&kindling(), &ScatterInput::new(&rolling, area, 1));
}

#[test]
fn sticks_keep_clear_of_the_footprints_above_them() {
    let above: Footprints = (-4..4)
        .flat_map(|i| (-4..4).map(move |j| DVec2::new(i as f64 * 0.8, j as f64 * 0.8)))
        .map(|p| Disc::new(p, 0.25))
        .collect();
    let area = ScatterArea::new(DVec2::ZERO, Vec2::splat(6.0));
    let (kept, free) = assert_keeps_clear(
        &kindling(),
        &ScatterInput::new(&FlatGround, area, 3),
        &above,
    );
    assert!(kept < free && kept > 0, "{kept} of {free}");
}

#[test]
fn a_log_takes_up_ground_along_its_length() {
    let logs = StickScatter::prepare(&ScatterOptions::fallen_logs(), 1).unwrap();
    let area = ScatterArea::new(DVec2::ZERO, Vec2::splat(60.0));
    let placed = logs
        .sticks(&ScatterInput::new(&FlatGround, area, 2))
        .unwrap();
    assert!(!placed.is_empty());
    for log in &placed {
        let ends = (
            log.footprint.first().unwrap(),
            log.footprint.last().unwrap(),
        );
        let length = ends.0.centre.distance_squared(ends.1.centre) as f64;
        // At least the shortest log's length, less its two end radii.
        assert!(
            length.sqrt() / 256.0 > 2.0 - 0.6,
            "{} m",
            length.sqrt() / 256.0
        );
    }
    assert!(logs.reach().footprint >= 1.0);
    assert!(logs.draw_range() > 0.0);
}
