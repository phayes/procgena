//! `clump.meta.yml` is in step with the clump options.

use procgena_clump::{CLUMP_META, ClumpOptions, ClumpPreset};
use procgena_core::meta::{ChoiceLists, KindMeta, assert_in_step};
use procgena_core::preset::{Preset, to_value};

#[test]
fn the_metadata_is_in_step_with_the_options() {
    let meta = KindMeta::parse(CLUMP_META).unwrap();
    let mut samples = vec![("default".to_owned(), to_value(&ClumpOptions::default()))];
    samples.extend(ClumpPreset::all().map(|p| (p.key().to_owned(), to_value(p.options()))));
    assert_in_step(&meta, &samples, &ChoiceLists::new(), &[]);
}
