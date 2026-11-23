// Colours and lights dead wood with Bevy's standard lighting, adding growth
// rings and drying cracks on cut and broken ends, fine grain on bare wood,
// and a bark texture where bark remains. Used both when lighting directly
// (forward) and when storing surfaces to light later (deferred).

import bevy_pbr::{
    decal::clustered::apply_decals,
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

@if(VISIBILITY_RANGE_DITHER)
import bevy_pbr::render::pbr_functions::visibility_range_dither;

/// Surface detail settings; must match the Rust `DeadwoodSurface`.
struct DeadwoodSurface {
    /// 1 to draw bark with the bark images, 0 for vertex colours only.
    bark_textured: u32,
    /// Bark image repeats per unit of texture coordinate.
    bark_repeats: f32,
    /// Growth rings per metre of radius on ends.
    ring_frequency: f32,
    /// How dark the rings are, from 0 to 1.
    ring_strength: f32,
    /// How many and how dark the drying cracks on ends are, from 0 to 1.
    crack_strength: f32,
    /// How strong the grain streaks on bare wood are, from 0 to 1.
    grain_strength: f32,
    /// Offset that makes each piece's noise different.
    seed: f32,
    /// 1 to draw the end-grain photo on sawn ends, 0 for drawn rings only.
    end_textured: u32,
    /// The photo's pith, in its texture coordinates.
    end_centre: vec2<f32>,
    /// Radius of a circle around the pith within the photo's wood.
    end_radius: f32,
    /// The photo's average linear colour within that circle.
    end_average: vec3<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> surface: DeadwoodSurface;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var bark_color_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var bark_color_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var bark_normal_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var bark_normal_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var bark_roughness_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var bark_roughness_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var end_color_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(108) var end_color_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(109) var end_normal_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(110) var end_normal_sampler: sampler;

const TAU: f32 = 6.28318531;
// Zone numbers packed into the second texture coordinate (see the Rust
// `Zone`).
/// The outside of a stem: bark, or bare wood.
const ZONE_SIDE: f32 = 0.0;
/// A broken or axe-cut end.
const ZONE_END: f32 = 1.0;
/// A saw or chainsaw cut.
const ZONE_CUT: f32 = 3.0;
/// Number of places around an end where a drying crack may run.
const CRACK_SECTORS: f32 = 14.0;

/// Returns a repeatable random number from 0 to 1 for `x`.
fn hash11(x: f32) -> f32 {
    var p = u32(i32(floor(x)) + 65536);
    p = p * 747796405u + 2891336453u;
    p = ((p >> ((p >> 28u) + 4u)) ^ p) * 277803737u;
    p = (p >> 22u) ^ p;
    return f32(p >> 8u) / 16777216.0;
}

/// Returns a repeatable random number from 0 to 1 for the grid cell `cell`.
fn hash21(cell: vec2<f32>) -> f32 {
    return hash11(cell.x + hash11(cell.y) * 4096.0);
}

/// Returns smooth noise from 0 to 1 that changes over about one unit of `p`.
fn noise2(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let s = f * f * (3.0 - 2.0 * f);
    let a = hash21(i);
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, s.x), mix(c, d, s.x), s.y);
}

/// Returns how bright end grain is at a point, as a multiplier: dark
/// latewood lines at each growth ring, a dark pith at the middle, and
/// drying cracks running in from the rim.
///
/// `uv` is the end's texture coordinate, 0.5 at the stem's centre line and
/// 0 or 1 at its outline, `radius` the point's distance from that line in
/// metres, and `rings` scales how dark the rings are.
fn end_grain(uv: vec2<f32>, radius: f32, rings: f32) -> f32 {
    let d = uv - 0.5;
    let reach = length(d) * 2.0;
    let angle = atan2(d.y, d.x);
    let p = d / max(reach, 1e-4) * 2.0 * radius;
    // The end's outline radius in metres; near the middle, where it can't
    // be measured, cracks are left out anyway.
    let rim = select(1e3, radius / reach, reach > 0.05);

    // Rings, wobbled a little so they aren't perfect circles. Each ring
    // fades from pale earlywood to a thin dark latewood edge.
    let wobble = (noise2(p * 18.0 + surface.seed) - 0.5) * 0.006
        + (noise2(vec2<f32>(angle * 1.5, surface.seed)) - 0.5) * 0.01;
    let ring = fract(max(radius + wobble, 0.0) * surface.ring_frequency);
    var bright = 1.0 - rings * surface.ring_strength * 0.35 * pow(ring, 6.0);

    // The pith: a small dark dot at the middle.
    bright *= mix(0.7, 1.0, smoothstep(0.002, 0.006, radius));

    // Drying cracks: each sector may hold one thin crack, at a random
    // angle, starting a random way out and running to the rim.
    let sector = floor((angle / TAU + 0.5) * CRACK_SECTORS);
    let present = step(hash11(sector * 3.0 + surface.seed), surface.crack_strength * 0.8);
    let centre = (sector + 0.2 + 0.6 * hash11(sector * 3.0 + 1.0 + surface.seed))
        / CRACK_SECTORS * TAU - TAU * 0.5;
    var gap = abs(angle - centre);
    gap = min(gap, TAU - gap) * radius;
    let width = 0.0008 + 0.0015 * hash11(sector * 3.0 + 2.0 + surface.seed);
    let start = rim * (0.3 + 0.5 * hash11(sector * 7.0 + surface.seed));
    let crack = (1.0 - smoothstep(width * 0.5, width, gap)) * smoothstep(start, start + 0.02, radius);
    bright *= 1.0 - 0.65 * crack * present;
    return bright;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    // While two detail levels blend, each skips a pattern of pixels so the
    // other shows through.
@if(VISIBILITY_RANGE_DITHER)
    visibility_range_dither(in.position, in.visibility_range_dither);

    var pbr_input = pbr_input_from_standard_material(in, is_front);
    var colour = pbr_input.material.base_color.rgb;

    // Meshes without wood values (or texture coordinates) keep their vertex
    // colours.
@if(VERTEX_UVS_A && VERTEX_UVS_B) {
    let zone = floor(in.uv_b.x / 4.0 + 0.01);
    let cover = clamp(in.uv_b.x - zone * 4.0, 0.0, 1.0);
    let radius = in.uv_b.y;

    // Sample the bark images everywhere, so the samples stay in uniform
    // control flow, then use them only where bark covers a stem's side.
    var bark_detail = vec3<f32>(1.0);
    var bark_normal = vec3<f32>(0.0, 0.0, 1.0);
    var bark_roughness = pbr_input.material.perceptual_roughness;
    if surface.bark_textured != 0u {
        let uv = in.uv * surface.bark_repeats;
        let texel = textureSampleBias(bark_color_texture, bark_color_sampler, uv, view.mip_bias).rgb;
        // The smallest mip level holds the image's average colour. Dividing
        // by it keeps the vertex colour as the bark's overall colour, with
        // the image adding only its light and dark detail.
        let last = f32(textureNumLevels(bark_color_texture) - 1u);
        let average = textureSampleLevel(bark_color_texture, bark_color_sampler, vec2<f32>(0.5), last).rgb;
        bark_detail = clamp(texel / max(average, vec3<f32>(1e-3)), vec3<f32>(0.0), vec3<f32>(4.0));
        bark_normal = textureSampleBias(bark_normal_texture, bark_normal_sampler, uv, view.mip_bias).rgb * 2.0 - 1.0;
        bark_roughness = textureSampleBias(bark_roughness_texture, bark_roughness_sampler, uv, view.mip_bias).g;
    }

    // Likewise the end-grain photo, placed so the end's middle falls on
    // the photo's pith and its outline on a circle within the photo's wood.
    // Past that circle the photo fades out, so its background never shows.
    var end_detail = vec3<f32>(1.0);
    var end_normal = vec3<f32>(0.0, 0.0, 1.0);
    var end_weight = 0.0;
    if surface.end_textured != 0u {
        let uv = surface.end_centre + (in.uv - 0.5) * 2.0 * surface.end_radius;
        let texel = textureSampleBias(end_color_texture, end_color_sampler, uv, view.mip_bias).rgb;
        end_detail = clamp(texel / max(surface.end_average, vec3<f32>(1e-3)), vec3<f32>(0.0), vec3<f32>(3.0));
        end_normal = textureSampleBias(end_normal_texture, end_normal_sampler, uv, view.mip_bias).rgb * 2.0 - 1.0;
        end_weight = 1.0 - smoothstep(1.0, 1.15, length(in.uv - 0.5) * 2.0);
    }

    // Which normal map to apply, and how strongly.
    var detail_normal = vec3<f32>(0.0, 0.0, 1.0);
    var detail_weight = 0.0;

    if zone == ZONE_SIDE {
        // Fine streaks along the grain where the wood is bare.
        let streak = noise2(vec2<f32>(in.uv.x * 160.0 * surface.bark_repeats, in.uv.y * 6.0 + surface.seed));
        colour *= 1.0 - surface.grain_strength * 0.3 * streak * (1.0 - cover);

        // Bark colour, bumps and roughness where bark remains.
        if surface.bark_textured != 0u {
            colour *= mix(vec3<f32>(1.0), bark_detail, cover);
            pbr_input.material.perceptual_roughness = mix(
                pbr_input.material.perceptual_roughness,
                bark_roughness,
                cover,
            );
            detail_normal = bark_normal;
            detail_weight = cover;
        }
    } else if zone == ZONE_END {
        // Rings and cracks on the bare wood of an end; its bark rim keeps
        // its colour.
        colour *= mix(end_grain(in.uv, radius, 1.0), 1.0, cover);
    } else if zone == ZONE_CUT {
        // A sawn end: the photo, with fainter drawn rings under it (the
        // photo has its own) and the drawn cracks, which follow decay.
        let photo = end_weight * (1.0 - cover);
        let grain = end_grain(in.uv, radius, 1.0 - 0.75 * photo);
        let wood = grain * mix(vec3<f32>(1.0), end_detail, photo);
        colour *= mix(wood, vec3<f32>(1.0), cover);
        detail_normal = end_normal;
        detail_weight = photo;
    }

@if(VERTEX_TANGENTS) {
    if detail_weight > 0.0 {
        let n = normalize(pbr_input.world_normal);
        let t = normalize(in.world_tangent.xyz - n * dot(in.world_tangent.xyz, n));
        let b = cross(n, t) * sign(in.world_tangent.w);
        let bumped = normalize(t * detail_normal.x + b * detail_normal.y + n * detail_normal.z);
        pbr_input.N = normalize(mix(pbr_input.N, bumped, detail_weight));
    }
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
