// Draws pooled rocks in Bevy's prepass and deferred G-buffer: depth always;
// normals, motion vectors and the G-buffer when the view asks for them.
// Group 0 is Bevy's prepass view, group 1 is empty and group 2 holds the pool.

import package::shaders::pool_bridge::{view_clip_from_world, view_mip_bias, prepass_normal};
import package::shaders::pool_draw::pool_vertex;
import procgena_core_bevy::shaders::dither::dither_discard;

@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS)
import package::shaders::pool_surface::pool_surface;
@if(MOTION_VECTOR_PREPASS)
import package::shaders::pool_bridge::prepass_motion_vector;
@if(DEFERRED_PREPASS)
import package::shaders::pool_bridge::{stone_pbr_input, stone_gbuffer, DEFERRED_LIGHTING_PASS_ID};

struct VertexOutput {
    // Invariant so the depth matches the main pass exactly.
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

@if(PREPASS_FRAGMENT)
struct FragmentOutput {
    @if(NORMAL_PREPASS)
    @location(0) normal: vec4<f32>,
    @if(MOTION_VECTOR_PREPASS)
    @location(1) motion_vector: vec2<f32>,
    @if(DEFERRED_PREPASS)
    @location(2) deferred: vec4<u32>,
    @if(DEFERRED_PREPASS)
    @location(3) deferred_lighting_pass_id: u32,
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

// Rocks fading between detail levels discard some pixels, so the prepass needs
// a fragment shader even when it writes depth alone.
@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    dither_discard(in.position, in.dither);
    var out: FragmentOutput;

@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS) {
    let surface = pool_surface(
        in.position,
        in.world_position,
        in.world_normal,
        in.local_position,
        in.local_normal,
        mat3x3<f32>(in.normal_x, in.normal_y, in.normal_z),
        view_mip_bias(),
    );
@if(NORMAL_PREPASS)
    out.normal = prepass_normal(surface.normal);
@if(DEFERRED_PREPASS) {
    out.deferred = stone_gbuffer(stone_pbr_input(surface));
    out.deferred_lighting_pass_id = DEFERRED_LIGHTING_PASS_ID;
}
}

@if(MOTION_VECTOR_PREPASS)
    out.motion_vector = prepass_motion_vector(in.world_position);
    return out;
}
@else
@fragment
fn fragment(in: VertexOutput) {
    dither_discard(in.position, in.dither);
}
