// Per-copy input of the shared instanced renderer, for material vertex shaders.
//
// Import as `procgena_core_bevy::shaders::instanced` once `InstancedPlugin` is
// added. With the `INSTANCED` shader def, a material's vertex shader reads
// `@location(12) instance: vec2<u32>` (the per-copy vertex buffer) and passes
// it to the functions below. Only vertex shaders of Bevy mesh pipelines can
// use `copy_wind_phase`, which reads Bevy's mesh records.

import bevy_pbr::render::mesh_functions::get_tag;

/// The copy's index in Bevy's mesh records. Use it wherever a shader would
/// use `instance_index`: for the transform, the normal matrix, and the
/// `instance_index` vertex output.
fn copy_record(instance: vec2<u32>) -> u32 {
    return instance.x;
}

/// The copy's visibility-range dither level for this pass, from -16 to 16 as
/// Bevy's `visibility_range_dither` expects.
fn copy_dither(instance: vec2<u32>) -> i32 {
    return bitcast<i32>(instance.y);
}

/// The copy's wind phase, from 0 to 1, from the copy's mesh record. Offset
/// the copy's sway by it so copies of one specimen don't sway in step.
fn copy_wind_phase(record: u32) -> f32 {
    return bitcast<f32>(get_tag(record));
}
