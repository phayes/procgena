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

// Attached copies (`ATTACHED`, see `InstancedAttached`) also read, per copy,
// `@location(13) attached_place: vec4<f32>`, `@location(14) attached_turn:
// vec4<f32>` and `@location(15) attached_wind: vec4<f32>`. `instance` names
// the host copy's mesh record, so `copy_record`, `copy_dither` and
// `copy_wind_phase` work on it as for any copy. Move each vertex into the
// host's own coordinates with the functions below before applying the
// record's transform.

/// A vertex position of an attached copy, moved from the copy's own
/// coordinates into its host's. `place` and `turn` are the copy's
/// `attached_place` and `attached_turn` inputs.
fn attached_position(place: vec4<f32>, turn: vec4<f32>, position: vec3<f32>) -> vec3<f32> {
    return place.xyz + attached_direction(turn, position * place.w);
}

/// A normal, tangent or other direction of an attached copy, turned from
/// the copy's own coordinates into its host's. `turn` is the copy's
/// `attached_turn` input. Lengths are kept.
fn attached_direction(turn: vec4<f32>, direction: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * cross(turn.xyz, direction);
    return direction + turn.w * t + cross(turn.xyz, t);
}

/// The attached copy's three wind numbers (`AttachedRow::wind`), from its
/// `attached_wind` input.
fn attached_wind(wind: vec4<f32>) -> vec3<f32> {
    return wind.xyz;
}
