// The clump material's textures and edge thinning, shared by the main,
// prepass and deferred clump shaders.

import bevy_pbr::render::mesh_functions;
import package::shaders::clump_triplanar::{TriplanarSample, sample_triplanar, triplanar_weights};

/// The material's texture settings, supplied by the Rust `ClumpSettings`.
struct ClumpSettings {
    /// Mesh units covered by one repeat of the textures.
    texture_size: f32,
    /// 1 when the images below are in use, 0 otherwise.
    textured: u32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> settings: ClumpSettings;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var color_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var color_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var normal_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var normal_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var orh_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var orh_sampler: sampler;

/// The part of the edge, as a share of full coverage, where the clump breaks
/// up along its texture. Coverage above this is always drawn.
const RIM_BAND: f32 = 0.5;

/// Returns texture colour, roughness, occlusion and a world-space lighting
/// normal for a point on the clump. Pass its world position, unit world
/// normal and mesh instance. The textures stay fixed to the clump as it
/// moves. Without textures, returns white, roughness and occlusion of 1 and
/// the supplied normal, leaving the base material unchanged.
fn clump_surface(
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

/// Discards the fragment where the clump's edge breaks up. Pass its coverage
/// `fade`, from 0 at the edge to 1 at full thickness, its world position and
/// its mesh instance. Near the edge only the texture's raised details are
/// kept, so the edge follows the texture instead of ending in a smooth line.
/// Without textures nothing is discarded, and the clump tapers into the
/// surface below.
///
/// The test depends only on the point on the clump, not on the view, so
/// every render pass, shadows included, removes the same parts: the texture
/// height is read with the triangle's flat normal, which each pass can work
/// out itself.
fn clump_rim_discard(
    fade: f32,
    world_position: vec4<f32>,
    instance_index: u32,
    mip_bias: f32,
) {
    let height = rim_height(world_position, instance_index, mip_bias);
    if fade < RIM_BAND * (1.0 - height) {
        discard;
    }
}

/// Returns the blended texture height from 0 to 1 at a point on the clump,
/// using the triangle's flat normal. Returns 1 without textures.
fn rim_height(world_position: vec4<f32>, instance_index: u32, mip_bias: f32) -> f32 {
    if settings.textured == 0u {
        return 1.0;
    }

    // Work in mesh coordinates, as the colour textures do. The flat normal's
    // sign doesn't matter for choosing projections.
    let local_from_world = mesh_functions::get_local_from_world(instance_index);
    let position = (local_from_world * world_position).xyz;
    let flat_normal = normalize(cross(dpdx(position), dpdy(position)));

    // Blend the three projections' heights as `sample_triplanar` does.
    let p = position / settings.texture_size;
    let h_x = textureSampleBias(orh_texture, orh_sampler, vec2<f32>(p.z, -p.y), mip_bias).b;
    let h_y = textureSampleBias(orh_texture, orh_sampler, vec2<f32>(p.x, -p.z), mip_bias).b;
    let h_z = textureSampleBias(orh_texture, orh_sampler, vec2<f32>(p.x, -p.y), mip_bias).b;
    let heights = vec3<f32>(h_x, h_y, h_z);
    let w = triplanar_weights(flat_normal, heights);
    return dot(w, heights);
}
