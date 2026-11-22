// Writes clump depth, shadows, textured normals and motion vectors in the
// prepass, with the same thinned edges as the main pass.

import bevy_pbr::{
    prepass::io::{VertexOutput, FragmentOutput},
    render::{mesh_view_bindings::view, pbr_functions, pbr_prepass_functions},
};
import package::shaders::clump_material::{clump_rim_discard, clump_surface};

@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    // Fade between detail levels by discarding a dither pattern.
@if(VISIBILITY_RANGE_DITHER)
    pbr_functions::visibility_range_dither(in.position, in.visibility_range_dither);

    // Break up the edge exactly as the main pass does.
    var fade = 1.0;
@if(VERTEX_COLORS)
    fade = in.color.a;
    clump_rim_discard(fade, in.world_position, in.instance_index, view.mip_bias);

    var out: FragmentOutput;
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION)
    out.frag_depth = in.unclipped_depth;

    // Store the same textured normal the main pass lights with.
@if(NORMAL_PREPASS) {
    let surface = clump_surface(in.world_position, normalize(in.world_normal), in.instance_index, view.mip_bias);
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
