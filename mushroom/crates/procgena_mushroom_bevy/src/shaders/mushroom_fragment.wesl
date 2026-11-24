// Colours and lights mushroom surfaces with Bevy's standard lighting, adding
// gill lines on the underside, and warts and a pattern on top of the cap.
// Used both when lighting directly (forward) and when storing surfaces to
// light later (deferred).

import bevy_pbr::{
    decal::clustered::apply_decals,
    render::{
        mesh_functions,
        mesh_view_bindings::view,
        pbr_fragment::pbr_input_from_standard_material,
        pbr_functions::alpha_discard,
    },
};
import package::shaders::mushroom_triplanar::{TriplanarSample, sample_flat, sample_triplanar};

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
import procgena_core_bevy::shaders::dither::dither_discard;

/// Pattern, gill and wart settings; must match the Rust `MushroomSurface`.
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
    /// Average linear colour of the stem colour image.
    stem_texture_average: vec3<f32>,
    /// 1 to add detail to the stem with the stem images, 0 for vertex
    /// colours only.
    stem_textured: u32,
    /// Mesh units of stem covered by one repeat of the stem images.
    stem_texture_size: f32,
    /// Typical width of a flat stem, in mesh units.
    stem_ribbon_width: f32,
    /// Wart colour in linear RGB, with 1 in alpha to draw warts, 0 for none.
    wart_colour: vec4<f32>,
    /// Typical distance between neighbouring wart centres, in mesh units.
    wart_spacing: f32,
    /// Radius of the largest warts, in mesh units.
    wart_radius: f32,
    /// How much smaller than `wart_radius` warts can be, from 0 to 1.
    wart_size_jitter: f32,
    /// How raised warts look, as a multiple of their radius.
    wart_height: f32,
    /// Distance from the cap's top centre beyond which no warts are drawn.
    wart_reach: f32,
    /// Length of the cap's top from rim to top centre, in mesh units.
    cap_top_length: f32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> surface: MushroomSurface;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var cap_color_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var cap_color_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var cap_normal_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var cap_normal_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var cap_orh_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var cap_orh_sampler: sampler;
// The three stem images share the colour image's sampler.
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var stem_color_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(108) var stem_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(109) var stem_normal_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(110) var stem_orh_texture: texture_2d<f32>;

const PI: f32 = 3.14159265;
// Zone numbers stored in the second texture coordinate (see the Rust
// `Zone::id`).
/// The cap's top.
const ZONE_CAP_TOP: f32 = 0.0;
/// The cap's rim band.
const ZONE_CAP_RIM: f32 = 1.0;
/// The cap's underside.
const ZONE_UNDERSIDE: f32 = 2.0;
/// The stem.
const ZONE_STEM: f32 = 3.0;

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

/// The wart under one point of the cap: how much wart colour shows there,
/// from 0 to 1, and which way to tilt the surface so the wart looks
/// raised.
struct WartSample {
    cover: f32,
    /// Tilt in map directions (see `wart_at`), as a fraction of the
    /// surface normal's length.
    tilt: vec2<f32>,
}

/// Returns the wart at `p`, a point on a flat map of the cap's top in mesh
/// units: the top centre is at the origin, and each point sits in its
/// direction around the cap at its distance from the top, measured along
/// the cap. The map keeps those distances true, so warts stay round
/// except near the rim of very rounded caps, where they widen a little
/// around the cap.
///
/// Warts sit one per cell of a square grid, `wart_spacing` wide, each at a
/// random spot far enough inside its cell that neighbours never touch.
fn wart_at(p: vec2<f32>) -> WartSample {
    var out: WartSample;
    let q = p / surface.wart_spacing;
    let margin = surface.wart_radius / surface.wart_spacing;
    let cell = floor(q);
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let c = cell + vec2<f32>(f32(x), f32(y));
            let h = hash3(vec3<f32>(c, 17.0));
            let centre = c + margin + h.xy * (1.0 - 2.0 * margin);
            // Only warts whose centre lies within reach of the top.
            if length(centre) * surface.wart_spacing > surface.wart_reach {
                continue;
            }
            let radius = margin * (1.0 - surface.wart_size_jitter * h.z);
            let d = (q - centre) / radius;
            let r2 = dot(d, d);
            if r2 < 1.0 {
                // A dome: its slope grows toward the edge.
                out.cover = 1.0 - smoothstep(0.8, 1.0, sqrt(r2));
                out.tilt = d * (surface.wart_height / max(sqrt(1.0 - r2), 0.25));
                return out;
            }
        }
    }
    return out;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    // While two detail levels blend, each skips a pattern of pixels so the
    // other shows through.
@if(VISIBILITY_RANGE_DITHER)
    dither_discard(in.position, in.visibility_range_dither);

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

    // Warts on the cap's top, over any texture. They are placed on a flat
    // map of the cap: the texture coordinates give each point's direction
    // around the cap (u) and how far down from the top it is (v, which is 0
    // at the top here). Raised warts tilt the surface outward from their
    // centres, using the mesh's tangents to turn map directions into
    // directions on the surface.
@if(VERTEX_TANGENTS) {
    if zone == ZONE_CAP_TOP && surface.wart_colour.a > 0.0 {
        let angle = in.uv.x * 2.0 * PI;
        let outward = vec2<f32>(cos(angle), sin(angle));
        let wart = wart_at(outward * (in.uv.y * surface.cap_top_length));
        if wart.cover > 0.0 {
            colour = mix(colour, surface.wart_colour.rgb, wart.cover);
            let n = pbr_input.N;
            let around = normalize(in.world_tangent.xyz - n * dot(n, in.world_tangent.xyz));
            let toward_top = cross(n, around) * in.world_tangent.w;
            let sideways = vec2<f32>(-outward.y, outward.x);
            let tilt = around * dot(wart.tilt, sideways) - toward_top * dot(wart.tilt, outward);
            pbr_input.N = normalize(n + tilt * wart.cover);
        }
    }
}

    // A textured stem keeps its own colours: the stem images are tinted so
    // their average colour matches the vertex colour, adding detail only.
    if zone == ZONE_STEM && surface.stem_textured != 0u {
        let world_from_local = mesh_functions::get_world_from_local(in.instance_index);
        let local_from_world = mesh_functions::get_local_from_world(in.instance_index);
        let p = (local_from_world * vec4<f32>(in.world_position.xyz, 1.0)).xyz;
        var stem: TriplanarSample;
@if(BILLBOARD) {
        // A flat stem turns to face the camera, so project along it: across
        // its width and up the mushroom's height, which stay put as it turns.
        let uv = vec2<f32>((in.uv.x - 0.5) * surface.stem_ribbon_width, -p.y) / surface.stem_texture_size;
        stem = sample_flat(
            stem_color_texture,
            stem_sampler,
            stem_normal_texture,
            stem_sampler,
            stem_orh_texture,
            stem_sampler,
            uv,
            view.mip_bias,
        );
        // Bend the turned surface direction by the normal map, taking the
        // image's top as the mushroom's up.
        let n = normalize(pbr_input.world_normal);
        let up = normalize(world_from_local[1].xyz);
        let right = normalize(cross(up, n));
        let top = cross(n, right);
        pbr_input.N = normalize(right * stem.normal.x + top * stem.normal.y + n * stem.normal.z);
}
@if(!BILLBOARD) {
        let linear = mat3x3<f32>(world_from_local[0].xyz, world_from_local[1].xyz, world_from_local[2].xyz);
        let n = normalize(transpose(linear) * pbr_input.world_normal);
        stem = sample_triplanar(
            stem_color_texture,
            stem_sampler,
            stem_normal_texture,
            stem_sampler,
            stem_orh_texture,
            stem_sampler,
            p,
            n,
            surface.stem_texture_size,
            view.mip_bias,
        );
        pbr_input.N = normalize(mesh_functions::mesh_normal_local_to_world(stem.normal, in.instance_index));
}
        colour = clamp(colour * stem.color / surface.stem_texture_average, vec3<f32>(0.0), vec3<f32>(1.0));
        pbr_input.material.perceptual_roughness *= stem.roughness;
        pbr_input.diffuse_occlusion *= stem.occlusion;
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
