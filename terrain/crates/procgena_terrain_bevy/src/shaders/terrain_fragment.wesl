// Colours and lights ground with Bevy's standard lighting: each pixel mixes
// up to eight materials' colours and roughness by the weights in two
// textures, with patchy variation. Near the camera it reads each material's
// texture set from shared texture arrays instead (from above, or from the
// sides too on steep ground), blends them by height, and fades back into the
// flat colours with distance. Fades the ground out at the end of the
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
import procgena_core_bevy::shaders::triplanar::{
    triplanar_uvs, triplanar_weights, unpack_normal, triplanar_normal, top_normal,
};

/// Colouring settings; must match the Rust `TerrainSurface`.
struct TerrainSurface {
    palette: array<vec4<f32>, 8>,
    layers: array<vec4<f32>, 8>,
    tints: array<vec4<f32>, 8>,
    texture_fade: vec4<f32>,
    texels: vec2<f32>,
    variation: f32,
    fade: vec4<f32>,
    tint: vec4<f32>,
    border: f32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> surface: TerrainSurface;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var weights_low: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var weights_low_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var weights_high: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var weights_high_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var colour_array: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var array_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var normal_array: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(108) var orh_array: texture_2d_array<f32>;

/// Below this weight a material is not sampled.
const MIN_WEIGHT: f32 = 0.02;
/// Surfaces less upright than this (the normal's y) take triplanar
/// materials from the side as well as from above.
const TRIPLANAR_BELOW: f32 = 0.85;
/// The width of the height-blended edge between materials, in weight.
const HEIGHT_BLEND_DEPTH: f32 = 0.25;
/// Where the textures' occlusion and normal-map detail start to ease out,
/// as a share of the distance where textures start to fade; both are gone
/// by that distance.
const DETAIL_FADE_FROM: f32 = 0.4;

/// One material's texture read at one point.
struct GroundTexel {
    colour: vec3<f32>,
    normal: vec3<f32>,
    roughness: f32,
    occlusion: f32,
    height: f32,
}

/// Screen-space rates of change of the three projections' coordinates
/// (before dividing by a material's repeat), taken once in uniform control
/// flow so materials can be sampled inside branches.
struct Gradients {
    x_dx: vec2<f32>, x_dy: vec2<f32>,
    y_dx: vec2<f32>, y_dy: vec2<f32>,
    z_dx: vec2<f32>, z_dy: vec2<f32>,
}

fn sample_map(map: texture_2d_array<f32>, uv: vec2<f32>, layer: i32, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    return textureSampleGrad(map, array_sampler, uv, layer, dx, dy);
}

/// Reads material texture `layer` at `position`, repeating every
/// 1 / `scale` metres, from above only or, if `triplanar` and the ground is
/// steep, from all three sides. `n` is the unit surface normal.
fn sample_ground(layer: i32, scale: f32, triplanar: bool, position: vec3<f32>, n: vec3<f32>, g: Gradients) -> GroundTexel {
    let uvs = triplanar_uvs(position * scale);
    let y_orh = sample_map(orh_array, uvs.y, layer, g.y_dx * scale, g.y_dy * scale).rgb;
    let y_colour = sample_map(colour_array, uvs.y, layer, g.y_dx * scale, g.y_dy * scale).rgb;
    let y_normal = unpack_normal(sample_map(normal_array, uvs.y, layer, g.y_dx * scale, g.y_dy * scale));
    if !triplanar || n.y > TRIPLANAR_BELOW {
        return GroundTexel(y_colour, top_normal(y_normal, n), y_orh.g, y_orh.r, y_orh.b);
    }

    // Steep ground: add the side projections, weighted by direction and height.
    let x_orh = sample_map(orh_array, uvs.x, layer, g.x_dx * scale, g.x_dy * scale).rgb;
    let z_orh = sample_map(orh_array, uvs.z, layer, g.z_dx * scale, g.z_dy * scale).rgb;
    let w = triplanar_weights(n, vec3<f32>(x_orh.b, y_orh.b, z_orh.b));
    let colour = sample_map(colour_array, uvs.x, layer, g.x_dx * scale, g.x_dy * scale).rgb * w.x
        + y_colour * w.y
        + sample_map(colour_array, uvs.z, layer, g.z_dx * scale, g.z_dy * scale).rgb * w.z;
    let x_normal = unpack_normal(sample_map(normal_array, uvs.x, layer, g.x_dx * scale, g.x_dy * scale));
    let z_normal = unpack_normal(sample_map(normal_array, uvs.z, layer, g.z_dx * scale, g.z_dy * scale));
    let orh = x_orh * w.x + y_orh * w.y + z_orh * w.z;
    return GroundTexel(colour, triplanar_normal(x_normal, y_normal, z_normal, n, w), orh.g, orh.r, orh.b);
}

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

    // Mix the palette's colours and roughness by the weights: the whole
    // look far away, where no textures are read.
    var weights: array<f32, 8>;
    var mixed = vec4<f32>(0.0);
    for (var k = 0; k < 4; k++) {
        weights[k] = low[k] / total;
        weights[k + 4] = high[k] / total;
    }
    for (var k = 0; k < 8; k++) {
        mixed += surface.palette[k] * weights[k];
    }

    // Patchy variation, at two scales, so large areas aren't one flat colour.
    let patches = noise2(position.xz / 23.0) * 0.6 + noise2(position.xz / 5.0) * 0.4;
    let vary = 1.0 + surface.variation * (patches - 0.5) * 2.0;
    var colour = mixed.rgb * vary;
    var roughness = mixed.a;
    var occlusion = 1.0;
    var normal = pbr_input.N;

    // Near the camera, read each material's texture instead, tinted to its
    // colour, and fade into the flat mix towards the texture distance.
    let uvs = triplanar_uvs(position);
    let g = Gradients(dpdx(uvs.x), dpdy(uvs.x), dpdx(uvs.y), dpdy(uvs.y), dpdx(uvs.z), dpdy(uvs.z));
    let near = 1.0 - smoothstep(surface.texture_fade.x, surface.texture_fade.y, distance);
    if surface.texture_fade.z > 0.5 && near > 0.0 {
        // Read every material with weight, and score it by weight and, for
        // materials that blend by height, by how raised its texture is.
        var texels: array<GroundTexel, 8>;
        var scores: array<f32, 8>;
        var best = 0.0;
        var blend = 0.0;
        for (var k = 0; k < 8; k++) {
            if weights[k] < MIN_WEIGHT {
                continue;
            }
            let layer = surface.layers[k];
            if layer.x < 0.0 {
                texels[k] = GroundTexel(surface.palette[k].rgb, pbr_input.N, surface.palette[k].a, 1.0, 0.5);
            } else {
                texels[k] = sample_ground(i32(layer.x), layer.y, layer.z > 0.5, position, pbr_input.N, g);
                texels[k].colour *= surface.tints[k].rgb;
            }
            scores[k] = weights[k] + layer.w * texels[k].height;
            best = max(best, scores[k]);
            blend = max(blend, layer.w);
        }

        // Height blending: only the materials scoring near the best show,
        // so raised leaves and stones poke through at the edges.
        var raised: array<f32, 8>;
        var raised_total = 0.0;
        for (var k = 0; k < 8; k++) {
            if weights[k] >= MIN_WEIGHT {
                raised[k] = max(scores[k] - best + HEIGHT_BLEND_DEPTH, 0.0);
                raised_total += raised[k];
            }
        }

        // Mix the textures by the blended weights.
        var t_colour = vec3<f32>(0.0);
        var t_normal = vec3<f32>(0.0);
        var t_roughness = 0.0;
        var t_occlusion = 0.0;
        var used = 0.0;
        for (var k = 0; k < 8; k++) {
            if weights[k] < MIN_WEIGHT {
                continue;
            }
            let w = mix(weights[k], raised[k] / max(raised_total, 1e-4), blend);
            t_colour += texels[k].colour * w;
            t_normal += texels[k].normal * w;
            t_roughness += texels[k].roughness * w;
            t_occlusion += texels[k].occlusion * w;
            used += w;
        }
        used = max(used, 1e-4);
        colour = mix(colour, t_colour / used * vary, near);
        roughness = mix(roughness, t_roughness / used, near);
        // The textures' occlusion and normal-map bumps darken them below
        // their average colour, so both ease out before the textures fade
        // into the flat colour.
        let detail_near = 1.0 - smoothstep(
            surface.texture_fade.x * DETAIL_FADE_FROM,
            surface.texture_fade.x,
            distance,
        );
        occlusion = mix(1.0, t_occlusion / used, detail_near);
        normal = normalize(mix(normal, normalize(t_normal), detail_near));
    }

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
    pbr_input.material.perceptual_roughness = roughness;
    pbr_input.N = normal;
    pbr_input.diffuse_occlusion *= vec3<f32>(occlusion);
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
