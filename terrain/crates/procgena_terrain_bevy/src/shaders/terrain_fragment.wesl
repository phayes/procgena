// Colours and lights terrain chunks with Bevy's standard lighting: grass on
// flat ground, rock on steep ground, snow up high. Fades the ground out at the
// end of the view, and can tint chunk levels and draw chunk borders for
// debugging. Used both when lighting directly (forward) and when storing
// surfaces to light later (deferred).

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
    grass: vec3<f32>,
    rock: vec3<f32>,
    snow: vec3<f32>,
    rock_slope: vec2<f32>,
    snow_height: vec2<f32>,
    variation: f32,
    fade: vec4<f32>,
    tint: vec4<f32>,
    border: f32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> surface: TerrainSurface;

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
    let normal = normalize(in.world_normal);

    // How steep the ground is: the sine of its angle from flat.
    let slope = sqrt(max(0.0, 1.0 - normal.y * normal.y));

    // Patchy variation, at two scales, so large areas aren't one flat colour.
    let patches = noise2(position.xz / 23.0) * 0.6 + noise2(position.xz / 5.0) * 0.4;
    let vary = 1.0 + surface.variation * (patches - 0.5) * 2.0;

    // Grass, turning to rock as the ground steepens, and to snow up high,
    // where the ground is flat enough to hold it.
    var colour = surface.grass * vary;
    let edge = (patches - 0.5) * 0.1;
    let rock = smoothstep(surface.rock_slope.x + edge, surface.rock_slope.y + edge, slope);
    colour = mix(colour, surface.rock * vary, rock);
    let snow_line = smoothstep(surface.snow_height.x, surface.snow_height.y, position.y + edge * 400.0);
    let snow = snow_line * (1.0 - smoothstep(0.55, 0.8, slope));
    colour = mix(colour, surface.snow, snow);

    // Debug: tint by chunk level, and darken a line along chunk borders.
    colour = mix(colour, surface.tint.rgb, surface.tint.w);
    if surface.border > 0.0 {
        let cell = fract(position.xz / surface.border + 0.5) - 0.5;
        let width = fwidth(position.xz / surface.border) * 1.5;
        let line = any(abs(cell) < width);
        if line {
            colour *= 0.15;
        }
    }

    pbr_input.material.base_color = vec4<f32>(colour, pbr_input.material.base_color.a);
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
