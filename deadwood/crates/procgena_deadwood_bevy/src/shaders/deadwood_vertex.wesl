// Vertex shader for drawing dead wood on screen: Bevy's standard vertex
// work, plus turning flat stems toward the camera. Meshes without billboard
// data use the second entry point.

import bevy_pbr::render::{
    forward_io::{Vertex, VertexOutput, decompress_vertex},
    mesh_functions,
    view_transformations::position_world_to_clip,
};
@if(BILLBOARD)
import package::shaders::deadwood_billboard::{TurnedVertex, turn_billboard};

/// Billboard data that flat stems add to Bevy's vertex inputs.
@if(BILLBOARD)
struct DeadwoodVertex {
    @location(9) billboard_pivot: vec4<f32>,
    @location(10) billboard_axis: vec4<f32>,
}

/// Places a flat stem vertex turned toward the camera.
@if(BILLBOARD)
@vertex
fn vertex(in: Vertex, wood: DeadwoodVertex) -> VertexOutput {
    let local_from_world = mesh_functions::get_local_from_world(in.instance_index);
    let turned = turn_billboard(wood.billboard_pivot, wood.billboard_axis, local_from_world);
    return shade(in, turned.position, turned.normal);
}

/// Places an ordinary vertex where the mesh puts it.
@if(!BILLBOARD)
@vertex
fn vertex(in: Vertex) -> VertexOutput {
    let vertex = decompress_vertex(in, in.instance_index);
    return shade(in, vertex.position, vec3<f32>(0.0));
}

/// Returns Bevy's standard vertex outputs for a vertex at `position` in the
/// mesh's coordinates. For flat stems, `turned_normal` is used in place of
/// the mesh's normal; otherwise it is ignored.
fn shade(in: Vertex, position: vec3<f32>, turned_normal: vec3<f32>) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(in, in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(in.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(position, 1.0),
    );
    out.position = position_world_to_clip(out.world_position.xyz);

@if(BILLBOARD)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(turned_normal, in.instance_index);
@if(VERTEX_NORMALS && !BILLBOARD)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, in.instance_index);
@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;
@if(VERTEX_TANGENTS)
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vertex.tangent,
        in.instance_index,
    );
@if(VERTEX_COLORS)
    out.color = vertex.color;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = in.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        in.instance_index,
        world_from_local[3],
    );
    return out;
}
