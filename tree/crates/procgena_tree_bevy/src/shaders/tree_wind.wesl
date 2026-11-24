// Wind displacement shared by the main, depth, shadow and deferred tree shaders.

import procgena_tree::wind::{WindParams, wind_offset_shifted};
import package::shaders::tree_wind_phase::wind_phase_shift;

/// The material's wind settings, supplied by the Rust `WindExtension`.
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> wind_params: WindParams;

/// Returns movement in tree coordinates to add to `position` at `time` seconds.
///
/// `wind` holds this vertex's animation weights and timing offset. Trees sway
/// with gusts that travel across the world by their world position, and each
/// is shifted a little by its own `wind_phase`, from 0 to 1, so that
/// neighbours don't move in perfect step. Pass the same `wind_phase` for every
/// part of one tree, in every pass.
fn tree_wind_offset(
    position: vec3<f32>,
    wind: vec4<f32>,
    world_from_local: mat4x4<f32>,
    wind_phase: f32,
    time: f32,
) -> vec3<f32> {
    let anchor = world_from_local[3].xyz;
    let shift = wind_phase_shift(wind_phase);
    return wind_offset_shifted(position + anchor, wind, anchor, shift, time, wind_params);
}
