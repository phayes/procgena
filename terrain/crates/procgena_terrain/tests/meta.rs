//! `terrain.meta.yml` is in step with the Terrain options.

use procgena_core::meta::{ChoiceLists, KindMeta, assert_in_step};
use procgena_core::preset::{Preset, to_value};
use procgena_terrain::{TERRAIN_META, TerrainOptions, TerrainPreset};

#[test]
fn the_metadata_is_in_step_with_the_options() {
    let meta = KindMeta::parse(TERRAIN_META).unwrap();
    let mut samples = vec![("default".to_owned(), to_value(&TerrainOptions::default()))];
    samples.extend(TerrainPreset::all().map(|p| (p.key().to_owned(), to_value(p.options()))));
    assert_in_step(&meta, &samples, &ChoiceLists::new(), &[]);
}
