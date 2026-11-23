// Draws a part for an impostor bake: its material's base colour, or its
// normal in the bake view's camera space, instead of the lit colour.
//
// A generator's own material switches its forward fragment shader to this
// one for bake copies, adding `BAKE_NORMAL` for the normal atlas.

import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
};

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    // Read the material as the lit shader does, and drop cut-out texels.
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color,
    );

    // Covered texels are fully opaque; uncovered ones keep the clear colour.
    var out: FragmentOutput;
@if(BAKE_NORMAL)
    out.color = vec4<f32>(normalize(pbr_input.N) * 0.5 + 0.5, 1.0);
@if(!BAKE_NORMAL)
    out.color = vec4<f32>(pbr_input.material.base_color.rgb, 1.0);
    return out;
}
