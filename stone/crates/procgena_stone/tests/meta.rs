//! `stone.meta.yml` is in step with the stone options.

use procgena_core::meta::{KindMeta, assert_in_step};
use procgena_core::preset::{Preset, to_value};
use procgena_stone::{STONE_META, StoneOptions, StonePreset, meta_choices};

#[test]
fn the_metadata_is_in_step_with_the_options() {
    let meta = KindMeta::parse(STONE_META).unwrap();
    let mut samples = vec![("default".to_owned(), to_value(&StoneOptions::default()))];
    samples.extend(StonePreset::all().map(|p| (p.key().to_owned(), to_value(p.options()))));
    assert_in_step(&meta, &samples, &meta_choices(), &[]);
}
