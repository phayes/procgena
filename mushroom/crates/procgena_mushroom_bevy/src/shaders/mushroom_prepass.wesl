// Vertex shader for the passes Bevy runs besides drawing on screen: depth,
// shadows and deferred rendering. It turns flat stems the same way as the
// main pass, so their shadows and depth match what is drawn.

import bevy_pbr::{
    prepass::io::{Vertex, VertexOutput, decompress_vertex},
    render::{mesh_functions, view_transformations::position_world_to_clip},
};
@if(BILLBOARD)
import package::shaders::mushroom_billboard::{TurnedVertex, turn_billboard};

/// Billboard data that flat stems add to Bevy's vertex inputs.
@if(BILLBOARD)
struct MushroomVertex {
    @location(9) billboard_pivot: vec4<f32>,
    @location(10) billboard_axis: vec4<f32>,
}

/// Places a flat stem vertex turned toward this pass's viewer: the camera,
/// or the light when drawing shadows.
@if(BILLBOARD)
@vertex
fn vertex(in: Vertex, mushroom: MushroomVertex) -> VertexOutput {
    let local_from_world = mesh_functions::get_local_from_world(in.instance_index);
    let turned = turn_billboard(mushroom.billboard_pivot, mushroom.billboard_axis, local_from_world);
    return shade(in, turned.position, turned.normal);
}

/// Places an ordinary vertex where the mesh puts it.
@if(!BILLBOARD)
@vertex
fn vertex(in: Vertex) -> VertexOutput {
    let vertex = decompress_vertex(in, in.instance_index);
    return shade(in, vertex.position, vec3<f32>(0.0));
}

/// Returns Bevy's prepass vertex outputs for a vertex at `position` in the
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
    out.world_normal = mesh_functions::mesh_normal_local_to_world(turned_normal, in.instance_index);
@if(VERTEX_NORMALS && !BILLBOARD)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, in.instance_index);
@if(VERTEX_TANGENTS)
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vertex.tangent,
        in.instance_index,
    );
}
@if(VERTEX_COLORS)
    out.color = vertex.color;

    // Mushrooms don't animate, so last frame's position differs only by
    // the entity's own movement. Used for motion blur and similar effects.
@if(MOTION_VECTOR_PREPASS) {
    let previous_world_from_local = mesh_functions::get_previous_world_from_local(in.instance_index);
    out.previous_world_position = mesh_functions::mesh_position_local_to_world(
        previous_world_from_local,
        vec4<f32>(position, 1.0),
    );
}
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = in.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        in.instance_index,
        world_from_local[3],
    );
    return out;
}
