// Bindings and vertex transform shared by every pooled-rock draw: the main
// pass, the prepass, the deferred G-buffer and the shadow pass.
//
// Each draw reads its vertices (position then normal, in the generated unit
// cube) from vertex buffer 0, and one visible-rock entry per instance from
// vertex buffer 1: the rock's index in `instances` and its fade dither level.

import package::shaders::pool_types::{PoolInstance, instance_linear};

/// The pool's surface. Mirrors `PoolMaterialUniform` in `src/pool/render/pools.rs`.
struct PoolMaterial {
    /// Linear base colour in `rgb`.
    base_color: vec4<f32>,
    perceptual_roughness: f32,
    /// 0 for non-metals such as stone, 1 for bare metal.
    metallic: f32,
    /// World width of the generated unit cube (`StoneOptions::scale`).
    scale: f32,
    /// Mesh units covered by one repeat of the textures.
    texture_size: f32,
    /// 1 when the textures below are in use, 0 otherwise.
    textured: u32,
}

@group(2) @binding(0) var<storage, read> instances: array<PoolInstance>;
@group(2) @binding(1) var<uniform> material: PoolMaterial;
@group(2) @binding(2) var color_texture: texture_2d<f32>;
@group(2) @binding(3) var color_sampler: sampler;
@group(2) @binding(4) var normal_texture: texture_2d<f32>;
@group(2) @binding(5) var normal_sampler: sampler;
/// Linear surface data: red shades hollows, green controls roughness and
/// blue supplies height for blending projected textures.
@group(2) @binding(6) var orh_texture: texture_2d<f32>;
@group(2) @binding(7) var orh_sampler: sampler;

/// One vertex of a pooled rock, placed in the world.
struct PoolVertex {
    world_position: vec3<f32>,
    /// Unit world-space normal.
    world_normal: vec3<f32>,
    /// Position in the rock's mesh units, for projecting textures.
    local_position: vec3<f32>,
    /// Unit normal in the rock's mesh units.
    local_normal: vec3<f32>,
    /// Turns mesh-unit normals into world-space normals (normalize after use).
    normal_matrix: mat3x3<f32>,
    /// Bevy's visibility-range dither level for the rock's fade.
    dither: i32,
}

/// Places generated vertex `position`, `normal` of visible rock `visible`.
fn pool_vertex(position: vec3<f32>, normal: vec3<f32>, visible: vec2<u32>) -> PoolVertex {
    let instance = instances[visible.x];
    let linear = instance_linear(instance);
    // The cofactor matrix turns normals like the inverse transpose does, up to
    // a positive scale once multiplied by the determinant's sign.
    let cofactor = mat3x3<f32>(
        cross(linear[1], linear[2]),
        cross(linear[2], linear[0]),
        cross(linear[0], linear[1]),
    );
    let normal_matrix = cofactor * sign(determinant(linear));

    var out: PoolVertex;
    out.local_position = position * material.scale;
    out.local_normal = normal;
    out.world_position = linear * out.local_position
        + vec3<f32>(instance.rows[0].w, instance.rows[1].w, instance.rows[2].w);
    out.world_normal = normalize(normal_matrix * normal);
    out.normal_matrix = normal_matrix;
    out.dither = bitcast<i32>(visible.y);
    return out;
}
