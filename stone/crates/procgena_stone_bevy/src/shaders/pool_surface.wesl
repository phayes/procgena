// The textured surface of a pooled rock at one fragment, shared by the main
// pass, the normal prepass and the deferred G-buffer.

import package::shaders::pool_bridge::StoneSurface;
import package::shaders::pool_draw::{
    material, color_texture, color_sampler, normal_texture, normal_sampler, orh_texture,
    orh_sampler,
};
import package::shaders::stone_triplanar::sample_triplanar;

/// Returns the pool material with texture colour, roughness, shading in
/// hollows and normal-map bumps applied. Pass `local_position` and
/// `local_normal` in mesh space, and a `normal_matrix` that transforms mesh
/// normals into world space. Metalness always comes from the material.
/// Without textures, returns the material's colour and roughness, occlusion
/// of 1 and the normalised world normal.
fn pool_surface(
    frag_coord: vec4<f32>,
    world_position: vec4<f32>,
    world_normal: vec3<f32>,
    local_position: vec3<f32>,
    local_normal: vec3<f32>,
    normal_matrix: mat3x3<f32>,
    mip_bias: f32,
) -> StoneSurface {
    let geometric_normal = normalize(world_normal);
    var surface = StoneSurface(
        material.base_color.rgb,
        material.perceptual_roughness,
        material.metallic,
        1.0,
        geometric_normal,
        geometric_normal,
        frag_coord,
        world_position,
    );
    if material.textured == 1u {
        let sample = sample_triplanar(
            color_texture,
            color_sampler,
            normal_texture,
            normal_sampler,
            orh_texture,
            orh_sampler,
            local_position,
            normalize(local_normal),
            material.texture_size,
            mip_bias,
        );
        surface.base_color *= sample.color;
        surface.perceptual_roughness *= sample.roughness;
        surface.occlusion = sample.occlusion;
        surface.normal = normalize(normal_matrix * sample.normal);
    }
    return surface;
}
