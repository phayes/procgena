// Draws an impostor into depth, normal and shadow passes: the same quad and
// surface as the forward pass (see `surface.wesl`), cut out by the blended
// views' coverage and written at their depth, so impostors meet the ground
// and each other where their objects would, and cast shadows shaped like
// their objects rather than like flat cards.
//
// The fragment shader also serves materials that place impostors with
// their own vertex shader (see `vertex_prepass.wesl`).

import bevy_pbr::{
    prepass::io::{Vertex, VertexOutput, decompress_vertex},
    render::{mesh_functions, pbr_functions},
};
import package::shaders::common::{Instance, impostor};
import package::shaders::surface::{impostor_surface, surface_depth};
import package::shaders::vertex_prepass::impostor_prepass_vertex;

/// Places the quad's corner for this view (a light's view for shadows), for
/// the mesh record `index`.
fn place(in: Vertex, index: u32) -> VertexOutput {
    let vertex = decompress_vertex(in, in.instance_index);
    return impostor_prepass_vertex(
        impostor,
        vertex.position.xy,
        mesh_functions::get_world_from_local(index),
        mesh_functions::get_local_from_world(index),
        vec3<f32>(0.0),
        index,
    );
}

// One impostor per entity, faded by its own visibility range.
@if(!INSTANCED)
@vertex
fn vertex(in: Vertex) -> VertexOutput {
    var out = place(in, in.instance_index);
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        in.instance_index,
        mesh_functions::get_world_from_local(in.instance_index)[3],
    );
    return out;
}

// Many impostors in one draw, each naming its mesh record and dither level.
@else
@vertex
fn vertex(in: Vertex, instance: Instance) -> VertexOutput {
    var out = place(in, instance.instance.x);
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = bitcast<i32>(instance.instance.y);
    return out;
}

/// The prepass's output: Bevy's own targets, and the depth of the surface
/// the impostor shows rather than of its flat quad.
@if(PREPASS_FRAGMENT)
struct ImpostorPrepassOutput {
    @if(NORMAL_PREPASS)
    @location(0) normal: vec4<f32>,
    @if(MOTION_VECTOR_PREPASS)
    @location(1) motion_vector: vec2<f32>,
    @builtin(frag_depth) depth: f32,
}

@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput) -> ImpostorPrepassOutput {
    // Find the surface before discarding, while control flow is uniform.
    let surface = impostor_surface(in.world_position.xyz, in.uv);
@if(VISIBILITY_RANGE_DITHER)
    pbr_functions::visibility_range_dither(in.position, in.visibility_range_dither);
    if surface.coverage < 0.5 {
        discard;
    }

    var out: ImpostorPrepassOutput;
@if(NORMAL_PREPASS)
    out.normal = vec4<f32>(surface.world_normal * 0.5 + 0.5, 1.0);
@if(MOTION_VECTOR_PREPASS)
    out.motion_vector = vec2<f32>(0.0);
    out.depth = surface_depth(surface);
    return out;
}

// Depth or shadow only: cut out by the atlas, at the surface's depth.
@else
@fragment
fn fragment(in: VertexOutput) -> @builtin(frag_depth) f32 {
    let surface = impostor_surface(in.world_position.xyz, in.uv);
@if(VISIBILITY_RANGE_DITHER)
    pbr_functions::visibility_range_dither(in.position, in.visibility_range_dither);
    if surface.coverage < 0.5 {
        discard;
    }
    return surface_depth(surface);
}
