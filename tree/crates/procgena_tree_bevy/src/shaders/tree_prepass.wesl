// Keeps tree depth, shadows and deferred rendering aligned with the visible wind animation.

import bevy_pbr::{
    prepass::io::{Vertex, VertexOutput, decompress_vertex},
    render::{mesh_functions, view_transformations::position_world_to_clip},
};
import bevy_render::globals::Globals;
import package::shaders::tree_billboard::{TurnedVertex, turn_billboard, turn_lit_billboard};
import package::shaders::tree_wind::tree_wind_offset;
@if(ATTACHED)
import procgena_core_bevy::shaders::instanced::{attached_position, attached_direction, attached_wind};

@group(0) @binding(1) var<uniform> globals: Globals;

/// Wind controls and optional billboard rotation data added to Bevy’s vertex inputs.
struct TreeVertex {
    @location(8) wind: vec4<f32>,
    @if(BILLBOARD)
    @location(9) billboard_pivot: vec4<f32>,
    @if(BILLBOARD)
    @location(10) billboard_axis: vec4<f32>,
    @if(BILLBOARD_NORMAL)
    @location(11) billboard_normal: vec3<f32>,
    /// For a grove tree: its index in the grove's mesh records, and its
    /// detail-level dither, as the bits of an `i32`. For fruit or flowers
    /// attached to a tree: the tree's record, and their own dither.
    @if(GROVE || ATTACHED)
    @location(12) grove: vec2<u32>,
    /// For attached fruit or flowers: their place, turn and branch sway on
    /// the tree (see `procgena_core_bevy::shaders::instanced`).
    @if(ATTACHED)
    @location(13) attached_place: vec4<f32>,
    @if(ATTACHED)
    @location(14) attached_turn: vec4<f32>,
    @if(ATTACHED)
    @location(15) attached_wind: vec4<f32>,
}

@vertex
fn vertex(in: Vertex, tree: TreeVertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(in, in.instance_index);

    // A grove draws many trees from one mesh; each names its own mesh record.
    var index = in.instance_index;
@if(GROVE || ATTACHED) {
    index = tree.grove.x;
}
    let world_from_local = mesh_functions::get_world_from_local(index);

    // Attached fruit and flowers move into the tree's coordinates, and sway
    // with their branch plus their own flutter.
    var rest = vertex.position;
    var normal = vec3<f32>(0.0, 1.0, 0.0);
@if(VERTEX_NORMALS)
    normal = vertex.normal;
    var wind = tree.wind;
@if(ATTACHED) {
    rest = attached_position(tree.attached_place, tree.attached_turn, rest);
    normal = attached_direction(tree.attached_turn, normal);
    wind = vec4<f32>(attached_wind(tree.attached_wind), tree.wind.w);
}

    // Rotate toward this pass’s view, including the light for a shadow pass,
    // then apply wind movement.
@if(BILLBOARD) {
    let local_from_world = mesh_functions::get_local_from_world(index);
    var turned: TurnedVertex;
@if(BILLBOARD_NORMAL) {
    turned = turn_lit_billboard(
        tree.billboard_pivot,
        tree.billboard_axis,
        tree.billboard_normal,
        local_from_world,
    );
}
@if(!BILLBOARD_NORMAL) {
    turned = turn_billboard(tree.billboard_pivot, tree.billboard_axis, local_from_world);
}
    rest = turned.position;
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(turned.normal, index);
}

    // Keep wind movement and world placement consistent with the visible render pass.
    let position = rest + tree_wind_offset(rest, wind, world_from_local, globals.time);
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
@if(VERTEX_NORMALS && !BILLBOARD)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(normal, index);
@if(VERTEX_TANGENTS) {
    var tangent = vertex.tangent;
@if(ATTACHED)
    tangent = vec4<f32>(attached_direction(tree.attached_turn, tangent.xyz), tangent.w);
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        tangent,
        index,
    );
}
}
@if(VERTEX_COLORS)
    out.color = vertex.color;

    // Use the previous wind time for motion vectors. Billboard orientation
    // still comes from the current frame, so these vectors omit its rotation.
@if(MOTION_VECTOR_PREPASS) {
    let previous_world_from_local = mesh_functions::get_previous_world_from_local(index);
    let previous = rest + tree_wind_offset(
        rest,
        wind,
        previous_world_from_local,
        globals.time - globals.delta_time,
    );
    out.previous_world_position = mesh_functions::mesh_position_local_to_world(
        previous_world_from_local,
        vec4<f32>(previous, 1.0),
    );
}
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = index;
@if(VISIBILITY_RANGE_DITHER && !GROVE && !ATTACHED)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        index,
        world_from_local[3],
    );
@if(VISIBILITY_RANGE_DITHER && (GROVE || ATTACHED))
    out.visibility_range_dither = bitcast<i32>(tree.grove.y);
    return out;
}
