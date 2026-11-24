// Bevy's standard main-pass vertex shader, without skinning or morph targets,
// that also draws copies of the shared instanced renderer (`INSTANCED`) and
// attached copies (`ATTACHED`).

import bevy_pbr::render::{
    forward_io::{Vertex, VertexOutput, decompress_vertex},
    mesh_functions,
    view_transformations::position_world_to_clip,
};
@if(INSTANCED)
import procgena_core_bevy::shaders::instanced::{copy_record, copy_dither};
@if(ATTACHED)
import procgena_core_bevy::shaders::instanced::{attached_position, attached_direction};

/// The per-copy input of an instanced draw.
@if(INSTANCED && !ATTACHED)
struct Copy {
    @location(12) instance: vec2<u32>,
}

/// The per-copy input of an attached draw.
@if(ATTACHED)
struct Attached {
    @location(12) instance: vec2<u32>,
    @location(13) place: vec4<f32>,
    @location(14) turn: vec4<f32>,
}

@if(INSTANCED && !ATTACHED)
@vertex
fn vertex(in: Vertex, copy: Copy) -> VertexOutput {
    return shade(in, copy_record(copy.instance), copy_dither(copy.instance), PLACE, TURN);
}

@if(ATTACHED)
@vertex
fn vertex(in: Vertex, copy: Attached) -> VertexOutput {
    return shade(in, copy_record(copy.instance), copy_dither(copy.instance), copy.place, copy.turn);
}

@if(!INSTANCED)
@vertex
fn vertex(in: Vertex) -> VertexOutput {
    return shade(in, in.instance_index, 0, PLACE, TURN);
}

/// No offset and no turn, for draws that are not attached.
const PLACE: vec4<f32> = vec4<f32>(0.0, 0.0, 0.0, 1.0);
const TURN: vec4<f32> = vec4<f32>(0.0, 0.0, 0.0, 1.0);

/// Bevy's standard vertex outputs for mesh record `index`. `dither` is the
/// copy's dither level when instanced, and ignored otherwise. With
/// `ATTACHED`, `place` and `turn` move each vertex into the host's
/// coordinates first; otherwise they are ignored.
fn shade(in: Vertex, index: u32, dither: i32, place: vec4<f32>, turn: vec4<f32>) -> VertexOutput {
    var out: VertexOutput;
    var vertex = decompress_vertex(in, index);
@if(ATTACHED) {
    vertex.position = attached_position(place, turn, vertex.position);
@if(VERTEX_NORMALS)
    vertex.normal = attached_direction(turn, vertex.normal);
@if(VERTEX_TANGENTS)
    vertex.tangent = vec4<f32>(attached_direction(turn, vertex.tangent.xyz), vertex.tangent.w);
}
    let world_from_local = mesh_functions::get_world_from_local(index);
    out.world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0),
    );
    out.position = position_world_to_clip(out.world_position.xyz);
@if(VERTEX_NORMALS)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, index);
@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;
@if(VERTEX_TANGENTS)
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vertex.tangent,
        index,
    );
@if(VERTEX_COLORS)
    out.color = vertex.color;
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
