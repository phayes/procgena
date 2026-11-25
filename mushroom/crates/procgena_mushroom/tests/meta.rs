//! `mushroom.meta.yml` is in step with the mushroom options.

use procgena_core::meta::{KindMeta, assert_in_step};
use procgena_core::preset::{Preset, to_value};
use procgena_mushroom::{MUSHROOM_META, MushroomOptions, MushroomPreset, meta_choices};

#[test]
fn the_metadata_is_in_step_with_the_options() {
    let meta = KindMeta::parse(MUSHROOM_META).unwrap();
    let mut samples = vec![("default".to_owned(), to_value(&MushroomOptions::default()))];
    samples.extend(MushroomPreset::all().map(|p| (p.key().to_owned(), to_value(p.options()))));
    assert_in_step(&meta, &samples, &meta_choices(), &[]);
}
