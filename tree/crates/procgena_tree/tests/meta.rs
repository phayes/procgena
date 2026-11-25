//! `tree.meta.yml` and `tree_scatter.meta.yml` are in step with the tree
//! and tree Scatter options.

use procgena_core::meta::{ChoiceLists, KindMeta, assert_in_step};
use procgena_core::preset::{Preset, to_value};
use procgena_tree::{
    TREE_META, TREE_SCATTER_META, TreeOptions, TreePreset, TreeScatter, TreeScatterPreset,
    meta_choices,
};

#[test]
fn the_tree_metadata_is_in_step_with_the_options() {
    let meta = KindMeta::parse(TREE_META).unwrap();
    let mut samples = vec![("default".to_owned(), to_value(&TreeOptions::default()))];
    samples.extend(TreePreset::all().map(|p| (p.key().to_owned(), to_value(p.options()))));
    assert_in_step(&meta, &samples, &meta_choices(), &["fruit", "flower"]);
}

#[test]
fn the_tree_scatter_metadata_is_in_step_with_the_options() {
    let meta = KindMeta::parse(TREE_SCATTER_META).unwrap();
    let mut samples = vec![("default".to_owned(), to_value(&TreeScatter::default()))];
    samples.extend(TreeScatterPreset::all().map(|p| (p.key().to_owned(), to_value(p.options()))));
    assert_in_step(&meta, &samples, &ChoiceLists::new(), &["tree"]);
}
