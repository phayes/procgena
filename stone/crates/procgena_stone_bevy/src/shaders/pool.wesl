// Draws pooled rocks in the main pass with projected textures and Bevy's lighting.

import package::shaders::pool_bridge::{stone_pbr_input, stone_lit, view_clip_from_world, view_mip_bias};
import package::shaders::pool_draw::pool_vertex;
import procgena_core_bevy::shaders::dither::dither_discard;
import package::shaders::pool_surface::pool_surface;

struct VertexOutput {
    // Invariant so the depth matches the prepass exactly.
    @builtin(position) @invariant position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) local_position: vec3<f32>,
    @location(3) local_normal: vec3<f32>,
    @location(4) @interpolate(flat) normal_x: vec3<f32>,
    @location(5) @interpolate(flat) normal_y: vec3<f32>,
    @location(6) @interpolate(flat) normal_z: vec3<f32>,
    @location(7) @interpolate(flat) dither: i32,
}

@vertex
fn vertex(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) visible: vec2<u32>,
) -> VertexOutput {
    let v = pool_vertex(position, normal, visible);
    var out: VertexOutput;
    out.world_position = vec4<f32>(v.world_position, 1.0);
    out.position = view_clip_from_world() * out.world_position;
    out.world_normal = v.world_normal;
    out.local_position = v.local_position;
    out.local_normal = v.local_normal;
    out.normal_x = v.normal_matrix[0];
    out.normal_y = v.normal_matrix[1];
    out.normal_z = v.normal_matrix[2];
    out.dither = v.dither;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    dither_discard(in.position, in.dither);
    let surface = pool_surface(
        in.position,
        in.world_position,
        in.world_normal,
        in.local_position,
        in.local_normal,
        mat3x3<f32>(in.normal_x, in.normal_y, in.normal_z),
        view_mip_bias(),
    );
    return stone_lit(stone_pbr_input(surface));
}
