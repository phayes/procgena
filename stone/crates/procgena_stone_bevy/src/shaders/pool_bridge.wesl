// The only pooled-rock shader that imports Bevy's shader modules. The other
// pool shaders reach Bevy's view, lighting, prepass and G-buffer through the
// functions here, so a Bevy upgrade that changes those modules changes this file.
//
// The view is Bevy's mesh view in the main pass and prepasses, and the light's
// view in the shadow pass (`POOL_SHADOW_PASS`).

@if(POOL_SHADOW_PASS)
import bevy_render::view::View;
@if(!POOL_SHADOW_PASS)
import bevy_pbr::render::mesh_view_bindings::view;
@if(!POOL_SHADOW_PASS)
import bevy_pbr::render::{
    pbr_types::{PbrInput, pbr_input_new, STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT},
    pbr_functions::calculate_view,
    mesh_types::MESH_FLAGS_SHADOW_RECEIVER_BIT,
};
@if(!PREPASS_PIPELINE && !POOL_SHADOW_PASS)
import bevy_pbr::render::pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing};
@if(MOTION_VECTOR_PREPASS)
import bevy_pbr::prepass::bindings::previous_view_uniforms;
@if(DEFERRED_PREPASS)
import bevy_pbr::deferred::functions::deferred_gbuffer_from_pbr_input;

/// The light's view in the shadow pass. Group 1 is empty, so the pool's
/// bindings are group 2 in every pooled-rock pipeline.
@if(POOL_SHADOW_PASS)
@group(0) @binding(0) var<uniform> view: View;

fn view_clip_from_world() -> mat4x4<f32> {
    return view.clip_from_world;
}

/// Bevy's texture mipmap bias for the view.
@if(!POOL_SHADOW_PASS)
fn view_mip_bias() -> f32 {
    return view.mip_bias;
}

/// A rock surface at one fragment, in world space.
struct StoneSurface {
    /// Linear colour.
    base_color: vec3<f32>,
    perceptual_roughness: f32,
    /// 0 for non-metals such as stone, 1 for bare metal.
    metallic: f32,
    /// Ambient occlusion: 0 blocks indirect diffuse light, 1 leaves it
    /// unchanged. Lower values shade cracks and hollows.
    occlusion: f32,
    /// Unit normal of the mesh, before normal mapping.
    geometric_normal: vec3<f32>,
    /// Unit normal to light with.
    normal: vec3<f32>,
    frag_coord: vec4<f32>,
    world_position: vec4<f32>,
}

/// `surface` as Bevy's `PbrInput`: an opaque surface that receives shadows
/// and fog.
@if(!POOL_SHADOW_PASS)
fn stone_pbr_input(surface: StoneSurface) -> PbrInput {
    let is_orthographic = view.clip_from_view[3].w == 1.0;
    var pbr_input = pbr_input_new();
    pbr_input.material.base_color = vec4<f32>(surface.base_color, 1.0);
    pbr_input.material.perceptual_roughness = surface.perceptual_roughness;
    pbr_input.material.metallic = surface.metallic;
    pbr_input.material.flags |= STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT;
    pbr_input.diffuse_occlusion = vec3<f32>(surface.occlusion);
    pbr_input.world_normal = surface.geometric_normal;
    pbr_input.N = surface.normal;
    pbr_input.frag_coord = surface.frag_coord;
    pbr_input.world_position = surface.world_position;
    pbr_input.is_orthographic = is_orthographic;
    pbr_input.V = calculate_view(surface.world_position, is_orthographic);
    pbr_input.flags = MESH_FLAGS_SHADOW_RECEIVER_BIT;
    return pbr_input;
}

/// The colour of `pbr_input` lit by every light, shadow and environment map in
/// the view, then fogged and post-processed for the view's settings.
@if(!PREPASS_PIPELINE && !POOL_SHADOW_PASS)
fn stone_lit(pbr_input: PbrInput) -> vec4<f32> {
    return main_pass_post_lighting_processing(pbr_input, apply_pbr_lighting(pbr_input));
}

/// `normal` as Bevy's normal prepass stores it.
fn prepass_normal(normal: vec3<f32>) -> vec4<f32> {
    return vec4<f32>(normal * 0.5 + vec3<f32>(0.5), 1.0);
}

/// The motion vector of a still point at `world_position`, from the camera's
/// own movement, as Bevy's prepass stores it: a UV-space offset, y down,
/// without jitter.
@if(MOTION_VECTOR_PREPASS)
fn prepass_motion_vector(world_position: vec4<f32>) -> vec2<f32> {
    let clip = view.unjittered_clip_from_world * world_position;
    let previous_clip = previous_view_uniforms.unjittered_clip_from_world * world_position;
    return (clip.xy / clip.w - previous_clip.xy / previous_clip.w) * vec2<f32>(0.5, -0.5);
}

/// `pbr_input` packed into Bevy's G-buffer.
@if(DEFERRED_PREPASS)
fn stone_gbuffer(pbr_input: PbrInput) -> vec4<u32> {
    return deferred_gbuffer_from_pbr_input(pbr_input);
}

/// Bevy's own deferred lighting pass, the one `StandardMaterial` uses.
const DEFERRED_LIGHTING_PASS_ID: u32 = 1u;
