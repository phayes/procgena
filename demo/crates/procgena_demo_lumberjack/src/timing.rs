//! How long placing one tile of the forest takes, Scatter by Scatter.

use std::time::{Duration, Instant};

use bevy::math::{DVec2, Vec2};
use procgena_biome::Ecotope;
use procgena_core::scatter::{ScatterArea, ScatterInput};

use crate::land::{Land, coverage};
use crate::settings::DemoOptions;

/// Places the tile at the start of the play area `runs` times over, with
/// the Scatters added one at a time in their order, and prints what each
/// Scatter adds to the time, with and without the play area's keep-off
/// mask. The first placement, which builds the land under the tile, is
/// timed apart.
///
/// # Panics
///
/// Panics if `runs` is 0, or if the forest can't be placed.
pub fn time_placing(land: &Land, options: &DemoOptions, runs: u32) {
    assert!(runs > 0, "time_placing needs at least one run");
    let area = &options.play_area;
    let [x, z] = area.centre;
    let tile = ScatterArea::new(DVec2::new(x, z), Vec2::splat(area.tile_size));
    let mask = coverage(land, area.keep_off);
    let place = |ecotope: &Ecotope, masked: bool| {
        let input = ScatterInput {
            mask: if masked { mask.as_deref() } else { None },
            ..ScatterInput::new(&*land.terrain, tile, land.forest.default_seed)
        };
        let started = Instant::now();
        let placement = ecotope.place(&input).expect("the forest places");
        let copies: usize = placement.layers().iter().map(|l| l.copies().len()).sum();
        (started.elapsed(), copies)
    };
    let fastest = |ecotope: &Ecotope, masked: bool| {
        (0..runs)
            .map(|_| place(ecotope, masked))
            .min_by_key(|(time, _)| *time)
            .expect("runs is above 0")
    };

    // The first placement builds the land the tile stands on.
    let (cold, copies) = place(&land.forest, area.keep_off);
    println!(
        "tile of {} m at ({x}, {z}), keep_off {}: first placement {cold:.2?} ({copies} copies), \
         fastest of {runs} after it:",
        area.tile_size, area.keep_off
    );

    // Each Scatter's share: the time with it, less the time without it.
    let order: Vec<_> = land
        .forest
        .order()
        .into_iter()
        .map(|(key, scatter)| (key.clone(), scatter.clone()))
        .collect();
    for masked in [false, true] {
        if masked && mask.is_none() {
            continue;
        }
        println!(
            "  {}",
            if masked {
                "with the keep-off mask"
            } else {
                "without a mask"
            }
        );
        let mut prefix = Ecotope {
            scatters: Default::default(),
            ..(*land.forest).clone()
        };
        let mut before = Duration::ZERO;
        let mut before_copies = 0;
        for (key, scatter) in &order {
            prefix.scatters.insert(key.clone(), scatter.clone());
            let (time, copies) = fastest(&prefix, masked);
            println!(
                "    {key:<14} {:>10.2?}  {:>6} copies",
                time.saturating_sub(before),
                copies - before_copies
            );
            before = time;
            before_copies = copies;
        }
        println!(
            "    {:<14} {before:>10.2?}  {before_copies:>6} copies",
            "all"
        );
    }
}
