// Draws an impostor into depth, normal and shadow passes: the same quad as
// the forward pass, cut out by the albedo atlas's alpha.

import bevy_pbr::{
    prepass::io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    render::{mesh_functions, pbr_functions, view_transformations::position_world_to_clip},
};
import package::shaders::common::{Instance, albedo_atlas, atlas_sampler, impostor_corner};

/// Places the quad's corner for this view (a light's view for shadows), for
/// the mesh record `index`.
fn place(in: Vertex, index: u32) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(in, in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(index);
    let local_from_world = mesh_functions::get_local_from_world(index);

    let corner = impostor_corner(vertex.position.xy, local_from_world);
    out.world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(corner.position, 1.0),
    );
    out.position = position_world_to_clip(out.world_position.xyz);
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION) {
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
}
@if(VERTEX_UVS_A)
    out.uv = corner.uv;
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(corner.basis[2], index);
@if(MOTION_VECTOR_PREPASS)
    out.previous_world_position = out.world_position;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = index;
    return out;
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

@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    let albedo = textureSample(albedo_atlas, atlas_sampler, in.uv);
@if(VISIBILITY_RANGE_DITHER)
    pbr_functions::visibility_range_dither(in.position, in.visibility_range_dither);
    if albedo.a < 0.5 {
        discard;
    }

    // The prepass normal is the quad's own facing; the forward pass lights
    // with the baked normals.
    var out: FragmentOutput;
@if(NORMAL_PREPASS)
    out.normal = vec4<f32>(normalize(in.world_normal) * 0.5 + 0.5, 1.0);
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION)
    out.frag_depth = in.unclipped_depth;
@if(MOTION_VECTOR_PREPASS)
    out.motion_vector = vec2<f32>(0.0);
    return out;
}

// Depth or shadow only: cut out by the atlas, with nothing else to write.
@else
@fragment
fn fragment(in: VertexOutput) {
    let albedo = textureSample(albedo_atlas, atlas_sampler, in.uv);
@if(VISIBILITY_RANGE_DITHER)
    pbr_functions::visibility_range_dither(in.position, in.visibility_range_dither);
    if albedo.a < 0.5 {
        discard;
    }
}
