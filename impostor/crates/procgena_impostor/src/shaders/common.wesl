// The impostor material's bindings, shared by the forward and prepass
// shaders. A material that draws impostors with its own vertex shader binds
// the same four at the same numbers; see `material.rs`.

import package::shaders::place::ImpostorParams;

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> impostor: ImpostorParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var albedo_atlas: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var atlas_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var normal_atlas: texture_2d<f32>;

/// The per-instance input of instanced pipelines: the instance's mesh
/// record index, and its dither level as the bits of an `i32`.
struct Instance {
    @location(12) instance: vec2<u32>,
}
