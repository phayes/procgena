// Draws trees with wind movement while preserving Bevy's lighting and texture inputs.

import bevy_pbr::render::{
    forward_io::{Vertex, VertexOutput, decompress_vertex},
    mesh_functions,
    mesh_view_bindings::globals,
    view_transformations::position_world_to_clip,
};
import package::shaders::tree_billboard::{TurnedVertex, turn_billboard, turn_lit_billboard};
import package::shaders::tree_wind::tree_wind_offset;
@if(ATTACHED)
import procgena_core_bevy::shaders::instanced::{attached_position, attached_direction, attached_wind};

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
    var position = vertex.position;
    var normal = vec3<f32>(0.0, 1.0, 0.0);
@if(VERTEX_NORMALS)
    normal = vertex.normal;
    var wind = tree.wind;
@if(ATTACHED) {
    position = attached_position(tree.attached_place, tree.attached_turn, position);
    normal = attached_direction(tree.attached_turn, normal);
    wind = vec4<f32>(attached_wind(tree.attached_wind), tree.wind.w);
}

    // Rotate billboards toward this view before adding wind movement.
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
    position = turned.position;
    out.world_normal = mesh_functions::mesh_normal_local_to_world(turned.normal, index);
}

    // Apply wind locally, then use the entity transform to place the vertex in the world.
    position += tree_wind_offset(position, wind, world_from_local, globals.time);
    out.world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(position, 1.0),
    );
    out.position = position_world_to_clip(out.world_position.xyz);

@if(VERTEX_NORMALS && !BILLBOARD)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(normal, index);
@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;
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
@if(VERTEX_COLORS)
    out.color = vertex.color;
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
