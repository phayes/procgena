//! Tree Scatters placed chunk by chunk on a World lattice: the same trees
//! with the same addresses in any order, lookups by address, footprints and
//! masks for the Scatters below.

use glam::{DVec2, Vec2};
use procgena_core::chain::{DistanceChain, ViewScale};
use procgena_core::scatter::conformance::{
    assert_chunks_match_area, assert_keeps_clear, assert_resolves,
};
use procgena_core::scatter::{ChunkGrid, Disc, Footprints, MaskSource};
use procgena_core::{AddressPrefix, RegionId};
use procgena_tree::{
    FlatGround, Scatter, ScatterArea, ScatterInput, TreeScatter, TreeScatterPreset,
};

fn rolling(p: DVec2) -> f32 {
    (p.x * 0.05).sin() as f32 * 2.0 + (p.y * 0.03).cos() as f32 * 3.0
}

fn mixed() -> TreeScatter {
    TreeScatterPreset::mixed_deciduous().tree_scatter.clone()
}

#[test]
fn chunks_in_any_order_give_the_trees_of_the_whole_area() {
    let scatter = mixed();
    let mask = |p: DVec2| (0.5 + 0.5 * (p.x * 0.04).sin()) as f32;
    let area = ScatterArea::new(DVec2::new(5000.3, -1200.0), Vec2::new(160.0, 120.0));
    let input = ScatterInput::new(&rolling, area, 9)
        .with_mask(&mask)
        .with_prefix(AddressPrefix {
            region: RegionId(3),
            ecotope: 7,
            scatter: 11,
        });
    // Chunks the 0.5 m lattice doesn't divide, so trees straddle edges.
    let count = assert_chunks_match_area(&scatter, &input, ChunkGrid::new(23.3));
    assert!(count > 100, "{count} trees");
    assert_chunks_match_area(&scatter, &input, ChunkGrid::new(64.0));
}

#[test]
fn trees_resolve_from_their_address() {
    let area = ScatterArea::new(DVec2::new(-300.0, 40.0), Vec2::splat(40.0));
    assert_resolves(&mixed(), &ScatterInput::new(&rolling, area, 2));
}

#[test]
fn trees_keep_clear_of_the_footprints_above_them() {
    let area = ScatterArea::new(DVec2::ZERO, Vec2::splat(40.0));
    // A boulder field above the trees: 1.5 m discs every 5 m.
    let above: Footprints = (-5..5)
        .flat_map(|i| (-5..5).map(move |j| DVec2::new(i as f64 * 5.0, j as f64 * 5.0)))
        .map(|p| Disc::new(p, 1.5))
        .collect();
    let (kept, free) =
        assert_keeps_clear(&mixed(), &ScatterInput::new(&FlatGround, area, 4), &above);
    assert!(kept < free && kept > 0, "{kept} of {free}");
}

#[test]
fn the_mask_comes_from_trunks_without_drawing() {
    let scatter = mixed();
    assert_eq!(scatter.mask_source(), MaskSource::Copies);
    let area = ScatterArea::new(DVec2::new(10.0, 10.0), Vec2::splat(30.0));
    let input = ScatterInput::new(&FlatGround, area, 1);
    let footprints = scatter.footprints(&input).unwrap();
    let mask = scatter.mask(&input).unwrap();
    assert!(!footprints.is_empty());
    let cleared = footprints
        .discs()
        .iter()
        .filter(|d| mask.sample(d.centre) < 255)
        .count();
    assert!(cleared > 0);
}

#[test]
fn the_reach_and_slots_describe_the_scatter() {
    let scatter = mixed();
    let reach = scatter.reach();
    assert_eq!(reach.rounds, 5);
    let widest = scatter.trees.iter().map(|t| t.spacing).fold(0.0, f32::max);
    assert_eq!(reach.spacing, widest);
    assert_eq!(scatter.lattice().cell_metres(), 0.5);
    // The largest tree, bounded by a diameter of twice its tallest height at
    // its largest scale, is drawn until the default chain's cull fades it out.
    let tallest = scatter
        .trees
        .iter()
        .map(|t| t.tree.height_bound())
        .fold(0.0, f32::max);
    let largest = 1.0 + 3.0 * scatter.variation.scale_jitter;
    let factor = DistanceChain::factor(2.0 * tallest * largest, ViewScale::DEFAULT);
    let chain = DistanceChain::DEFAULT;
    let expected = factor / chain.cull_below * (1.0 + chain.hysteresis);
    assert!((scatter.draw_range() - expected).abs() < 1e-2 * expected);

    let slots = scatter.slots(0);
    let variants: u32 = scatter.trees.iter().map(|t| t.variants).sum();
    assert_eq!(slots.len() as u32, variants);
    for (i, tree) in scatter.trees.iter().enumerate() {
        for v in 0..tree.variants {
            let slot = slots[scatter.slot(i, v) as usize];
            assert_eq!(slot.entry as usize, i);
            assert_eq!(slot.specimen.seed() as i32, tree.variant(v).default_seed);
        }
    }
}

#[test]
fn renaming_a_species_moves_only_its_own_choices() {
    // Two species keyed differently: changing one key keeps every spot's
    // rank, so the set of spots is unchanged, but reassigns which spots the
    // renamed species takes.
    let scatter = mixed();
    let area = ScatterArea::new(DVec2::ZERO, Vec2::splat(60.0));
    let input = ScatterInput::new(&FlatGround, area, 5);
    let before = scatter.place(&input).unwrap();
    let mut renamed = scatter.clone();
    renamed.trees[0].key = procgena_tree::ChildKey::new("maple").unwrap();
    let after = renamed.place(&input).unwrap();
    let species = |copies: &[procgena_core::scatter::ScatterCopy], s: &TreeScatter| {
        copies
            .iter()
            .map(|c| (c.address, s.slots(0)[c.slot as usize].entry))
            .collect::<Vec<_>>()
    };
    assert_ne!(species(&before, &scatter), species(&after, &renamed));
}
