//! Check the cluster levels of detail: valid, sized by the cluster, and
//! simpler and cheaper with distance.

use procgena_core::{LevelForm, Quality};
use procgena_fruit::{
    FRUIT_LOD_REFERENCE_SIZE, FlowerPreset, Fruit, FruitPreset, ItemDetail, cluster_presets,
    default_lod,
};

#[test]
fn the_default_levels_are_valid_mesh_levels() {
    let lod = default_lod();
    lod.validate().unwrap();
    assert_eq!(lod.reference_size, FRUIT_LOD_REFERENCE_SIZE);
    assert!(lod.levels.iter().all(|l| l.form == LevelForm::Mesh));
    let details: Vec<ItemDetail> = lod.levels.iter().map(|l| l.detail.item_detail).collect();
    assert_eq!(
        details,
        [ItemDetail::Full, ItemDetail::Simple, ItemDetail::Minimal]
    );
    // The farthest level is small and casts no shadows; the others do.
    let shadows: Vec<bool> = lod.levels.iter().map(|l| l.casts_shadows).collect();
    assert_eq!(shadows, [true, true, false]);
    // The host ends the chain, so the clusters' own levels never cull.
    assert_eq!(lod.cull_distance, None);
}

#[test]
fn distances_scale_with_the_cluster_size() {
    let lod = default_lod();
    let apple = Fruit::from_preset(FruitPreset::apple()).unwrap();
    let catkins = Fruit::from_preset(FruitPreset::pussy_willow_catkin()).unwrap();
    let factor = |fruit: &Fruit, scale| fruit.lod_factor(&lod, scale, Quality::High);

    assert!((factor(&apple, 1.0) - apple.size() / FRUIT_LOD_REFERENCE_SIZE).abs() < 1e-6);
    assert!((factor(&apple, 2.0) - 2.0 * factor(&apple, 1.0)).abs() < 1e-6);

    // At one distance, a smaller cluster is drawn at a simpler level.
    let (small, large) = if apple.size() < catkins.size() {
        (&apple, &catkins)
    } else {
        (&catkins, &apple)
    };
    let distance = 5.0 * large.size() / FRUIT_LOD_REFERENCE_SIZE;
    let level = |fruit: &Fruit| lod.level_at(distance, factor(fruit, 1.0)).unwrap();
    assert!(
        level(small) > level(large),
        "{} vs {}",
        level(small),
        level(large)
    );
}

#[test]
fn farther_levels_use_fewer_triangles() {
    let lod = default_lod();
    for (key, options) in cluster_presets() {
        let fruit = Fruit::new(options.clone(), options.default_seed, 0).unwrap();
        let counts: Vec<usize> = fruit
            .lod_meshes(&lod)
            .iter()
            .map(|(_, mesh)| mesh.triangle_count())
            .collect();
        assert!(counts.windows(2).all(|w| w[1] < w[0]), "{key}: {counts:?}");
    }
}

#[test]
fn a_cluster_specimen_has_one_part_per_level() {
    let lod = default_lod();
    let fruit = Fruit::from_flower_preset(FlowerPreset::dogwood_bloom()).unwrap();
    let specimen = fruit.specimen(&fruit.lod_meshes(&lod));
    assert_eq!(specimen.levels.len(), lod.levels.len());
    assert!(specimen.levels.iter().all(|l| l.parts.len() == 1));
    assert_eq!(specimen.levels[1].start, lod.levels[1].distance);
}
