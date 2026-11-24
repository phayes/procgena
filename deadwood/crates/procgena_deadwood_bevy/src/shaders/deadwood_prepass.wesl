// Vertex shader for the passes Bevy runs besides drawing on screen: depth,
// shadows and deferred rendering. It turns flat stems the same way as the
// main pass, so their shadows and depth match what is drawn. Copies drawn by
// the shared instanced renderer (`INSTANCED`) read their mesh record and
// dither from a per-copy input.

import bevy_pbr::{
    prepass::io::{Vertex, VertexOutput, decompress_vertex},
    render::{mesh_functions, view_transformations::position_world_to_clip},
};
@if(BILLBOARD)
import package::shaders::deadwood_billboard::{TurnedVertex, turn_billboard};
@if(INSTANCED)
import procgena_core_bevy::shaders::instanced::{copy_record, copy_dither};

/// Billboard data that flat stems, and the per-copy input that instanced
/// copies, add to Bevy's vertex inputs.
@if(BILLBOARD || INSTANCED)
struct DeadwoodVertex {
    @if(BILLBOARD)
    @location(9) billboard_pivot: vec4<f32>,
    @if(BILLBOARD)
    @location(10) billboard_axis: vec4<f32>,
    @if(INSTANCED)
    @location(12) instance: vec2<u32>,
}

/// Places a flat stem vertex turned toward this pass's viewer (the camera,
/// or the light when drawing shadows), or an instanced copy's vertex.
@if(BILLBOARD || INSTANCED)
@vertex
fn vertex(in: Vertex, wood: DeadwoodVertex) -> VertexOutput {
    var index = in.instance_index;
    var dither = 0;
@if(INSTANCED) {
    index = copy_record(wood.instance);
    dither = copy_dither(wood.instance);
}
@if(BILLBOARD) {
    let local_from_world = mesh_functions::get_local_from_world(index);
    let turned = turn_billboard(wood.billboard_pivot, wood.billboard_axis, local_from_world);
    return shade(in, index, dither, turned.position, turned.normal);
}
@if(!BILLBOARD) {
    let vertex = decompress_vertex(in, index);
    return shade(in, index, dither, vertex.position, vec3<f32>(0.0));
}
}

/// Places an ordinary vertex where the mesh puts it.
@if(!BILLBOARD && !INSTANCED)
@vertex
fn vertex(in: Vertex) -> VertexOutput {
    let vertex = decompress_vertex(in, in.instance_index);
    return shade(in, in.instance_index, 0, vertex.position, vec3<f32>(0.0));
}

/// Returns Bevy's prepass vertex outputs for a vertex at `position` in the
/// mesh's coordinates, drawn with mesh record `index`. For flat stems,
/// `turned_normal` is used in place of the mesh's normal; otherwise it is
/// ignored. `dither` is an instanced copy's dither level, ignored otherwise.
fn shade(
    in: Vertex,
    index: u32,
    dither: i32,
    position: vec3<f32>,
    turned_normal: vec3<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(in, index);
    let world_from_local = mesh_functions::get_world_from_local(index);
    out.world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(position, 1.0),
    );
    out.position = position_world_to_clip(out.world_position.xyz);
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION) {
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
}

@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS) {
@if(BILLBOARD)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(turned_normal, index);
@if(VERTEX_NORMALS && !BILLBOARD)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, index);
@if(VERTEX_TANGENTS)
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vertex.tangent,
        index,
    );
}
@if(VERTEX_COLORS)
    out.color = vertex.color;

    // Dead wood doesn't animate, so last frame's position differs only by
    // the entity's own movement. Used for motion blur and similar effects.
@if(MOTION_VECTOR_PREPASS) {
    let previous_world_from_local = mesh_functions::get_previous_world_from_local(index);
    out.previous_world_position = mesh_functions::mesh_position_local_to_world(
        previous_world_from_local,
        vec4<f32>(position, 1.0),
    );
}
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = index;
@if(VISIBILITY_RANGE_DITHER && INSTANCED)
    out.visibility_range_dither = dither;
@if(VISIBILITY_RANGE_DITHER && !INSTANCED)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        index,
        world_from_local[3],
    );
    return out;
}
