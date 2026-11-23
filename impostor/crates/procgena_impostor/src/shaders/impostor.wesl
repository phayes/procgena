// Draws an impostor in the forward pass: one quad facing the nearest baked
// view, coloured from the albedo atlas and lit with the normal atlas through
// Bevy's own lighting.

import bevy_pbr::render::{
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    mesh_functions,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    view_transformations::position_world_to_clip,
};
import package::shaders::common::{
    Instance, albedo_atlas, atlas_basis, atlas_sampler, impostor_corner, normal_atlas,
};

/// Places the quad's corner for this view, for the mesh record `index`.
fn place(in: Vertex, index: u32) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(in, in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(index);
    let local_from_world = mesh_functions::get_local_from_world(index);

    // The quad's corner for this view, in the object's coordinates.
    let corner = impostor_corner(vertex.position.xy, local_from_world);
    out.world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(corner.position, 1.0),
    );
    out.position = position_world_to_clip(out.world_position.xyz);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(corner.basis[2], index);
@if(VERTEX_UVS_A)
    out.uv = corner.uv;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = index;
    return out;
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
    // Sample both atlases before discarding, while control flow is uniform.
    let albedo = textureSample(albedo_atlas, atlas_sampler, in.uv);
    let baked_normal = textureSample(normal_atlas, atlas_sampler, in.uv).xyz * 2.0 - 1.0;
@if(VISIBILITY_RANGE_DITHER)
    pbr_functions::visibility_range_dither(in.position, in.visibility_range_dither);
    if albedo.a < 0.5 {
        discard;
    }

    // The baked normal is in its view's camera space; turn it back into the
    // object's coordinates, then into the world.
    let local_normal = atlas_basis(in.uv) * baked_normal;
    let normal = normalize(mesh_functions::mesh_normal_local_to_world(local_normal, in.instance_index));

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
