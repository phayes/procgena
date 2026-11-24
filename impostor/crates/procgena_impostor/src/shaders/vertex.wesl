// The forward pass's impostor vertex, for any material that draws an
// impostor: the built-in impostor material, or a host's material that
// supplies its own transform and offset.

import bevy_pbr::render::{forward_io::VertexOutput, view_transformations::position_world_to_clip};
import package::shaders::place::{ImpostorParams, impostor_corner};

/// The forward pass's output for quad corner `corner` (each of x and y is -1
/// or 1, the impostor quad mesh's own position) of the impostor `params`
/// describes, drawn with transform `world_from_local` and its inverse
/// `local_from_world`, moved by `world_offset` in world units (zero for
/// none).
///
/// `record` is the mesh record the fragment reads its mesh flags from,
/// written as the output's instance index. The visibility-range dither is
/// left for the caller to set.
fn impostor_vertex(
    params: ImpostorParams,
    corner: vec2<f32>,
    world_from_local: mat4x4<f32>,
    local_from_world: mat4x4<f32>,
    world_offset: vec3<f32>,
    record: u32,
) -> VertexOutput {
    let placed = impostor_corner(params, corner, local_from_world, world_offset);
    var out: VertexOutput;
    out.world_position = world_from_local * vec4<f32>(placed.position, 1.0)
        + vec4<f32>(world_offset, 0.0);
    out.position = position_world_to_clip(out.world_position.xyz);
    out.world_normal = normalize(transpose(mat3x3<f32>(
        local_from_world[0].xyz,
        local_from_world[1].xyz,
        local_from_world[2].xyz,
    )) * placed.basis[2]);
@if(VERTEX_UVS_A)
    out.uv = placed.uv;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = record;
    return out;
}
