// Colours and lights mushroom surfaces with Bevy's standard lighting, adding
// gill lines on the underside and a pattern on top of the cap. Used both
// when lighting directly (forward) and when storing surfaces to light later
// (deferred).

import bevy_pbr::{
    decal::clustered::apply_decals,
    render::{
        mesh_functions,
        mesh_view_bindings::view,
        pbr_fragment::pbr_input_from_standard_material,
        pbr_functions::alpha_discard,
    },
};
import package::shaders::mushroom_triplanar::sample_triplanar;

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

/// Pattern and gill settings; must match the Rust `MushroomSurface`.
struct MushroomSurface {
    /// Pattern colour in linear RGB, with its strength (0 to 1) in alpha.
    pattern_colour: vec4<f32>,
    /// Which pattern: 0 none, 1 speckle, 2 cracks, 3 spots.
    pattern: u32,
    /// Pattern features per unit of the mesh.
    pattern_frequency: f32,
    /// Number of gill lines around the underside.
    gill_lines: f32,
    /// How much gill lines darken, from 0 to 1.
    gill_strength: f32,
    /// 1 to paint the cap's top with the cap images, 0 for vertex colours.
    cap_textured: u32,
    /// Mesh units covered by one repeat of the cap images.
    cap_texture_size: f32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> surface: MushroomSurface;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var cap_color_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var cap_color_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var cap_normal_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var cap_normal_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var cap_orh_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var cap_orh_sampler: sampler;

const PI: f32 = 3.14159265;
// Zone numbers stored in the second texture coordinate (see the Rust
// `Zone::id`).
/// The cap's top.
const ZONE_CAP_TOP: f32 = 0.0;
/// The cap's rim band.
const ZONE_CAP_RIM: f32 = 1.0;
/// The cap's underside.
const ZONE_UNDERSIDE: f32 = 2.0;

/// Returns three repeatable random numbers from 0 to 1 for the grid cell
/// with whole-number corner `cell`.
fn hash3(cell: vec3<f32>) -> vec3<f32> {
    var p = vec3<u32>(vec3<i32>(cell) + vec3<i32>(4096));
    p = p * 1664525u + 1013904223u;
    p.x += p.y * p.z;
    p.y += p.z * p.x;
    p.z += p.x * p.y;
    p ^= p >> vec3<u32>(16u);
    p.x += p.y * p.z;
    p.y += p.z * p.x;
    p.z += p.x * p.y;
    return vec3<f32>(p >> vec3<u32>(8u)) / 16777216.0;
}

/// Returns the distances from `p` to the nearest and second-nearest of a
/// set of random points, one per grid cell. Where the two are nearly equal,
/// `p` is on a boundary between cells, which draws cracks.
fn voronoi(p: vec3<f32>) -> vec2<f32> {
    let cell = floor(p);
    var d = vec2<f32>(10.0);
    for (var z = -1; z <= 1; z++) {
        for (var y = -1; y <= 1; y++) {
            for (var x = -1; x <= 1; x++) {
                let c = cell + vec3<f32>(f32(x), f32(y), f32(z));
                let dist = length(c + hash3(c) - p);
                if dist < d.x {
                    d = vec2<f32>(dist, d.x);
                } else if dist < d.y {
                    d.y = dist;
                }
            }
        }
    }
    return d;
}

/// Returns how much of the pattern colour shows at `p`, a position in the
/// mesh's coordinates, from 0 to 1. Using mesh coordinates keeps the
/// pattern fixed to the mushroom as it moves.
fn pattern_cover(p: vec3<f32>) -> f32 {
    let q = p * surface.pattern_frequency;
    switch surface.pattern {
        case 1u: {
            return step(0.88, hash3(floor(q * 4.0)).x);
        }
        case 2u: {
            let d = voronoi(q);
            return 1.0 - smoothstep(0.02, 0.06, d.y - d.x);
        }
        case 3u: {
            return 1.0 - smoothstep(0.24, 0.3, voronoi(q).x);
        }
        default: {
            return 0.0;
        }
    }
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    // While two detail levels blend, each skips a pattern of pixels so the
    // other shows through.
@if(VISIBILITY_RANGE_DITHER)
    visibility_range_dither(in.position, in.visibility_range_dither);

    var pbr_input = pbr_input_from_standard_material(in, is_front);
    var colour = pbr_input.material.base_color.rgb;

    // Gill lines on the underside, the pattern on the cap. Meshes without
    // zone numbers get neither.
@if(VERTEX_UVS_B) {
    let zone = round(in.uv_b.x);
    if zone == ZONE_UNDERSIDE && surface.gill_lines > 0.0 {
        let line = pow(abs(sin(in.uv.x * PI * surface.gill_lines)), 6.0);
        colour *= 1.0 - surface.gill_strength * line;
    }
    let cap_textured = zone == ZONE_CAP_TOP && surface.cap_textured != 0u;
    if (zone == ZONE_CAP_TOP || zone == ZONE_CAP_RIM) && surface.pattern != 0u && !cap_textured {
        let local_from_world = mesh_functions::get_local_from_world(in.instance_index);
        let p = (local_from_world * vec4<f32>(in.world_position.xyz, 1.0)).xyz;
        let cover = pattern_cover(p) * surface.pattern_colour.a;
        colour = mix(colour, surface.pattern_colour.rgb, cover);
    }

    // A textured cap's top takes its colour, roughness, shading in hollows
    // and bumps from the cap images, projected in the mushroom's own space
    // so they stay fixed to it.
    if cap_textured {
        let world_from_local = mesh_functions::get_world_from_local(in.instance_index);
        let local_from_world = mesh_functions::get_local_from_world(in.instance_index);
        let p = (local_from_world * vec4<f32>(in.world_position.xyz, 1.0)).xyz;
        let linear = mat3x3<f32>(world_from_local[0].xyz, world_from_local[1].xyz, world_from_local[2].xyz);
        let n = normalize(transpose(linear) * pbr_input.world_normal);
        let cap = sample_triplanar(
            cap_color_texture,
            cap_color_sampler,
            cap_normal_texture,
            cap_normal_sampler,
            cap_orh_texture,
            cap_orh_sampler,
            p,
            n,
            surface.cap_texture_size,
            view.mip_bias,
        );
        colour = cap.color;
        pbr_input.material.perceptual_roughness *= cap.roughness;
        pbr_input.diffuse_occlusion *= cap.occlusion;
        pbr_input.N = normalize(mesh_functions::mesh_normal_local_to_world(cap.normal, in.instance_index));
    }
}
    pbr_input.material.base_color = vec4<f32>(colour, pbr_input.material.base_color.a);

    // Apply the material's transparency setting and any decals.
    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color,
    );
    apply_decals(&pbr_input);

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
