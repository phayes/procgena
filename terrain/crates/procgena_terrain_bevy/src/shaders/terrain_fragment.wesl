// Colours and lights ground with Bevy's standard lighting: each pixel mixes
// up to eight materials' colours and roughness by the weights in two
// textures, with patchy variation. Fades the ground out at the end of the
// view, and can tint chunk levels and draw chunk borders for debugging. Used
// both when lighting directly (forward) and when storing surfaces to light
// later (deferred).

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
    palette: array<vec4<f32>, 8>,
    texels: vec2<f32>,
    variation: f32,
    fade: vec4<f32>,
    tint: vec4<f32>,
    border: f32,
    overlay: f32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> surface: TerrainSurface;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var weights_low: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var weights_low_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var weights_high: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var weights_high_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var overlay: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var overlay_sampler: sampler;

/// A smooth value noise from 0 to 1, from world x and z.
fn hash2(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}

fn noise2(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2<f32>(1.0, 0.0));
    let c = hash2(i + vec2<f32>(0.0, 1.0));
    let d = hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    // Fade the ground out at the end of the view.
    let distance = length(in.world_position.xyz - view.world_position);
    dither_discard(in.position, dither_level(surface.fade, distance));

    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let position = in.world_position.xyz;

    // The weights here: the UVs run from the first texel's centre to the
    // last's.
    var uv = vec2<f32>(0.5);
@if(VERTEX_UVS_A)
    uv = in.uv;
    let at = (uv * (surface.texels - 1.0) + 0.5) / surface.texels;
    let low = textureSample(weights_low, weights_low_sampler, at);
    let high = textureSample(weights_high, weights_high_sampler, at);
    let total = max(dot(low, vec4<f32>(1.0)) + dot(high, vec4<f32>(1.0)), 1e-4);

    // Mix the palette's colours and roughness by the weights.
    var mixed = vec4<f32>(0.0);
    for (var k = 0; k < 4; k++) {
        mixed += surface.palette[k] * low[k];
        mixed += surface.palette[k + 4] * high[k];
    }
    mixed /= total;

    // Patchy variation, at two scales, so large areas aren't one flat colour.
    let patches = noise2(position.xz / 23.0) * 0.6 + noise2(position.xz / 5.0) * 0.4;
    let vary = 1.0 + surface.variation * (patches - 0.5) * 2.0;
    var colour = mixed.rgb * vary;

    // Debug: tint by chunk level, cover with the overlay image, and darken a
    // line along chunk borders. The overlay's UVs, like the weights', run
    // from its first texel's centre to its last's.
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
    pbr_input.material.perceptual_roughness = mixed.a;
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
