// Lights ground in one plain colour with Bevy's standard lighting. Fades the
// ground out at the end of the view, can mix an overlay image over it, and
// can tint chunk levels and draw chunk borders for debugging. Used both
// when lighting directly (forward) and when storing surfaces to light later
// (deferred).

import bevy_pbr::{
    render::{
        mesh_view_bindings::view,
        pbr_fragment::pbr_input_from_standard_material,
        pbr_functions::alpha_discard,
    },
};

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

import procgena_core_bevy::shaders::dither::{dither_discard, dither_level};

/// Colouring settings; must match the Rust `TerrainSurface`.
struct TerrainSurface {
    colour: vec4<f32>,
    fade: vec4<f32>,
    tint: vec4<f32>,
    border: f32,
    overlay: f32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> surface: TerrainSurface;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var overlay: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var overlay_sampler: sampler;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    // Fade the ground out at the end of the view.
    let distance = length(in.world_position.xyz - view.world_position);
    dither_discard(in.position, dither_level(surface.fade, distance));

    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let position = in.world_position.xyz;
    var colour = surface.colour.rgb;

    // Debug: tint by chunk level, cover with the overlay image, and darken a
    // line along chunk borders. The overlay's UVs run from its first
    // texel's centre to its last's.
    var uv = vec2<f32>(0.5);
@if(VERTEX_UVS_A)
    uv = in.uv;
    colour = mix(colour, surface.tint.rgb, surface.tint.w);
    let overlay_size = vec2<f32>(textureDimensions(overlay));
    let overlay_at = (uv * (overlay_size - 1.0) + 0.5) / overlay_size;
    let cover = textureSample(overlay, overlay_sampler, overlay_at);
    colour = mix(colour, cover.rgb, surface.overlay * cover.a);
    if surface.border > 0.0 {
        let cell = fract(position.xz / surface.border + 0.5) - 0.5;
        let width = fwidth(position.xz / surface.border) * 1.5;
        let line = any(abs(cell) < width);
        if line {
            colour *= 0.15;
        }
    }

    pbr_input.material.base_color = vec4<f32>(colour, pbr_input.material.base_color.a);
    pbr_input.material.perceptual_roughness = surface.colour.a;
    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color,
    );

    // Deferred rendering stores the surface to light later; forward
    // rendering lights it now, unless the material is unlit.
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
