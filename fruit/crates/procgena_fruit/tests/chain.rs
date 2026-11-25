//! Check the cluster's two meshes on the drawing chain: the game level and
//! the coarser mesh derived from it.

use procgena_core::chain::{DistanceChain, LowerDetail, ViewScale};
use procgena_fruit::{
    ClusterDetail, FlowerPreset, Fruit, FruitOptions, FruitPreset, ItemDetail, cluster_presets,
};

#[test]
fn the_default_game_level_is_simple_and_its_lower_mesh_is_minimal() {
    assert_eq!(ClusterDetail::default(), ClusterDetail::SIMPLE);
    assert_eq!(FruitOptions::default().game_level, ClusterDetail::SIMPLE);
    let lower = ClusterDetail::SIMPLE.lower();
    assert_eq!(lower.item_detail, ItemDetail::Minimal);
    assert_eq!(lower.item_stride, 2);
    assert_eq!(lower.item_scale, 1.25);
    // The lower mesh adds no flat items the game level doesn't have.
    assert_eq!(lower.flat_item_size, 0.0);
    // Full detail steps down one level at a time.
    assert_eq!(ClusterDetail::FULL.lower().item_detail, ItemDetail::Simple);
    assert_eq!(lower.lower().item_detail, ItemDetail::Minimal);
}

#[test]
fn the_lower_mesh_uses_fewer_triangles() {
    for (key, options) in cluster_presets() {
        let fruit = Fruit::new(options.clone(), options.default_seed, 0).unwrap();
        let meshes = fruit.meshes();
        assert!(
            meshes.lower.triangle_count() < meshes.game.triangle_count(),
            "{key}: {} vs {}",
            meshes.lower.triangle_count(),
            meshes.game.triangle_count()
        );
    }
}

#[test]
fn the_game_level_is_not_part_of_the_cluster_name() {
    let options = FruitPreset::apple().fruit.clone();
    let full = FruitOptions {
        game_level: ClusterDetail::FULL,
        ..options.clone()
    };
    assert_eq!(full.pool_id(), options.pool_id());
    let simple = Fruit::new(options, 1, 0).unwrap();
    let full = Fruit::new(full, 1, 0).unwrap();
    assert_eq!(simple.id(), full.id());
    assert!(full.meshes().game.triangle_count() > simple.meshes().game.triangle_count());
}

#[test]
fn a_cluster_specimen_has_its_two_meshes() {
    let fruit = Fruit::from_flower_preset(FlowerPreset::dogwood_bloom()).unwrap();
    let specimen = fruit.specimen();
    assert_eq!(specimen.levels.len(), 2);
    assert!(specimen.levels.iter().all(|l| l.parts.len() == 1));
    let factor = DistanceChain::factor(fruit.size(), ViewScale::DEFAULT);
    let expected = factor / DistanceChain::DEFAULT.lower_below;
    assert!((specimen.levels[1].start - expected).abs() < 1e-6);
}
