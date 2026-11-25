//! `fruit.meta.yml` and `flower.meta.yml` are in step with the cluster
//! options.

use procgena_core::meta::{ChoiceLists, KindMeta, assert_in_step};
use procgena_core::preset::{Preset, to_value};
use procgena_fruit::{FLOWER_META, FRUIT_META, FlowerPreset, FruitOptions, FruitPreset};

#[test]
fn the_fruit_metadata_is_in_step_with_the_options() {
    let meta = KindMeta::parse(FRUIT_META).unwrap();
    let mut samples = vec![("default".to_owned(), to_value(&FruitOptions::default()))];
    samples.extend(FruitPreset::all().map(|p| (p.key().to_owned(), to_value(p.options()))));
    assert_in_step(&meta, &samples, &ChoiceLists::new(), &[]);
}

#[test]
fn the_flower_metadata_is_in_step_with_the_options() {
    let meta = KindMeta::parse(FLOWER_META).unwrap();
    let samples: Vec<_> = FlowerPreset::all()
        .map(|p| (p.key().to_owned(), to_value(p.options())))
        .collect();
    assert_in_step(&meta, &samples, &ChoiceLists::new(), &[]);
}
