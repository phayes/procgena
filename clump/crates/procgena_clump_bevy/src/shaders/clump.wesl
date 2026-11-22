// Draws clumps with projected textures, thinned edges and Bevy's lighting,
// in forward and deferred rendering.

import bevy_pbr::{
    decal::clustered::apply_decals,
    render::{
        mesh_view_bindings::view,
        pbr_fragment::pbr_input_from_standard_material,
    },
};
import package::shaders::clump_material::{clump_rim_discard, clump_surface};

@if(PREPASS_PIPELINE)
import bevy_pbr::{
    prepass::io::{VertexOutput, FragmentOutput},
    deferred::functions::deferred_output,
};
@else
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
};

@if(VISIBILITY_RANGE_DITHER)
import bevy_pbr::render::pbr_functions::visibility_range_dither;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    // Fade between detail levels by discarding a dither pattern.
@if(VISIBILITY_RANGE_DITHER)
    visibility_range_dither(in.position, in.visibility_range_dither);

    // Break up the clump's edge. The vertex colour's alpha carries its
    // coverage; meshes without vertex colours are fully covered.
    var fade = 1.0;
@if(VERTEX_COLORS)
    fade = in.color.a;
    clump_rim_discard(fade, in.world_position, in.instance_index, view.mip_bias);
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let surface = clump_surface(in.world_position, pbr_input.world_normal, in.instance_index, view.mip_bias);

    // Multiply the base colour and roughness by the texture values and use
    // the textured normal for lighting. Texture occlusion adds shading in
    // hollows by multiplying any existing screen-space occlusion. The
    // coverage was folded into alpha by Bevy; the kept pixels are opaque.
    pbr_input.material.base_color = vec4<f32>(pbr_input.material.base_color.rgb * surface.color, 1.0);
    pbr_input.material.perceptual_roughness *= surface.roughness;
    pbr_input.diffuse_occlusion *= surface.occlusion;
    pbr_input.N = surface.normal;
    apply_decals(&pbr_input);

    // Deferred rendering stores the surface for a later lighting pass; forward
    // rendering lights it now, unless the base material is unlit.
@if(PREPASS_PIPELINE)
    let out = deferred_output(in, pbr_input);
@if(!PREPASS_PIPELINE)
    var out: FragmentOutput;
@if(!PREPASS_PIPELINE) {
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
}
    return out;
}
