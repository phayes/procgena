// The rock material's textures, shared by the main, prepass and deferred rock shaders.

import bevy_pbr::render::mesh_functions;
import package::shaders::stone_triplanar::{TriplanarSample, sample_triplanar};

/// The material's texture settings, supplied by the Rust `TriplanarSettings`.
struct TriplanarSettings {
    /// Mesh units covered by one repeat of the textures.
    texture_size: f32,
    /// 1 when the images below are in use, 0 otherwise.
    textured: u32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> settings: TriplanarSettings;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var color_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var color_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var normal_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var normal_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var orh_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var orh_sampler: sampler;

/// Returns texture colour, roughness, occlusion and a world-space lighting
/// normal for a point on the rock. Pass its world position, unit world normal
/// and mesh instance. The textures stay fixed to the rock as it moves.
/// Without textures, returns white, roughness and occlusion of 1, and the
/// supplied normal, leaving the base material unchanged.
fn stone_surface(
    world_position: vec4<f32>,
    world_normal: vec3<f32>,
    instance_index: u32,
    mip_bias: f32,
) -> TriplanarSample {
    if settings.textured == 0u {
        return TriplanarSample(vec3<f32>(1.0), 1.0, 1.0, world_normal);
    }

    // Move the fragment into mesh coordinates.
    let world_from_local = mesh_functions::get_world_from_local(instance_index);
    let local_from_world = mesh_functions::get_local_from_world(instance_index);
    let position = (local_from_world * world_position).xyz;
    let linear = mat3x3<f32>(world_from_local[0].xyz, world_from_local[1].xyz, world_from_local[2].xyz);
    let normal = normalize(transpose(linear) * world_normal);

    var surface = sample_triplanar(
        color_texture,
        color_sampler,
        normal_texture,
        normal_sampler,
        orh_texture,
        orh_sampler,
        position,
        normal,
        settings.texture_size,
        mip_bias,
    );
    surface.normal = normalize(mesh_functions::mesh_normal_local_to_world(surface.normal, instance_index));
    return surface;
}
