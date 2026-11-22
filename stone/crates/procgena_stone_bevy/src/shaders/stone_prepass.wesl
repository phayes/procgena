// Writes rock depth, textured normals and motion vectors in the prepass.

import bevy_pbr::{
    prepass::io::{VertexOutput, FragmentOutput},
    render::{mesh_view_bindings::view, pbr_functions, pbr_prepass_functions},
};
import package::shaders::stone_material::stone_surface;

@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    // Fade between detail levels by discarding a dither pattern.
@if(VISIBILITY_RANGE_DITHER)
    pbr_functions::visibility_range_dither(in.position, in.visibility_range_dither);

    var out: FragmentOutput;
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION)
    out.frag_depth = in.unclipped_depth;

    // Store the same textured normal the main pass lights with.
@if(NORMAL_PREPASS) {
    let surface = stone_surface(in.world_position, normalize(in.world_normal), in.instance_index, view.mip_bias);
    out.normal = vec4<f32>(surface.normal * 0.5 + vec3<f32>(0.5), 1.0);
}

@if(MOTION_VECTOR_PREPASS)
    out.motion_vector = pbr_prepass_functions::calculate_motion_vector(in.world_position, in.previous_world_position);
    return out;
}
@else
@fragment
fn fragment(in: VertexOutput) {
}
