// Draws an impostor in the forward pass: one quad facing the nearest baked
// view, coloured from the albedo atlas and lit with the normal atlas through
// Bevy's own lighting.
//
// The fragment shader also serves materials that place impostors with
// their own vertex shader (see `vertex.wesl`): it needs only the bindings in
// `common.wesl` and the forward pass's usual vertex output.

import bevy_pbr::render::{
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    mesh_functions,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};
import package::shaders::common::{Instance, albedo_atlas, atlas_sampler, impostor, normal_atlas};
import package::shaders::place::world_view_basis;
import package::shaders::vertex::impostor_vertex;

/// Places the quad's corner for this view, for the mesh record `index`.
fn place(in: Vertex, index: u32) -> VertexOutput {
    let vertex = decompress_vertex(in, in.instance_index);
    return impostor_vertex(
        impostor,
        vertex.position.xy,
        mesh_functions::get_world_from_local(index),
        mesh_functions::get_local_from_world(index),
        vec3<f32>(0.0),
        index,
    );
}

// One impostor per entity, faded by its own visibility range.
@if(!INSTANCED)
@vertex
fn vertex(in: Vertex) -> VertexOutput {
    var out = place(in, in.instance_index);
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        in.instance_index,
        mesh_functions::get_world_from_local(in.instance_index)[3],
    );
    return out;
}

// Many impostors in one draw, each naming its mesh record and dither level.
@else
@vertex
fn vertex(in: Vertex, instance: Instance) -> VertexOutput {
    var out = place(in, instance.instance.x);
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = bitcast<i32>(instance.instance.y);
    return out;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    // Sample both atlases and find the view's axes before discarding, while
    // control flow is uniform.
    let albedo = textureSample(albedo_atlas, atlas_sampler, in.uv);
    let baked_normal = textureSample(normal_atlas, atlas_sampler, in.uv).xyz * 2.0 - 1.0;
    let basis = world_view_basis(in.world_position.xyz, in.uv);
@if(VISIBILITY_RANGE_DITHER)
    pbr_functions::visibility_range_dither(in.position, in.visibility_range_dither);
    if albedo.a < 0.5 {
        discard;
    }

    // The baked normal is in its view's camera space; turn it into the
    // world with the view's axes.
    let normal = normalize(basis * baked_normal);

    // Light it as the material would be lit, with the atlas's colour.
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = vec4<f32>(albedo.rgb * pbr_input.material.base_color.rgb, 1.0);
    pbr_input.N = normal;
    pbr_input.world_normal = normal;
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
