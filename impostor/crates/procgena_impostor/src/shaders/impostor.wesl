// Draws an impostor in the forward pass: one quad facing the nearest baked
// view, showing the three nearest views blended (see `surface.wesl`), lit
// with their normals through Bevy's own lighting and written at their
// depth.
//
// The fragment shader also serves materials that place impostors with
// their own vertex shader (see `vertex.wesl`): it needs only the bindings in
// `common.wesl` and the forward pass's usual vertex output.

import bevy_pbr::render::{
    forward_io::{Vertex, VertexOutput, decompress_vertex},
    mesh_functions,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};
import package::shaders::common::{Instance, impostor};
import package::shaders::surface::{impostor_surface, view_depth};
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

/// The forward pass's output: lit colour, and the depth of the surface the
/// impostor shows rather than of its flat quad.
struct ImpostorFragmentOutput {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> ImpostorFragmentOutput {
    // Find the surface before discarding, while control flow is uniform.
    let surface = impostor_surface(in.world_position.xyz, in.uv);
@if(VISIBILITY_RANGE_DITHER)
    pbr_functions::visibility_range_dither(in.position, in.visibility_range_dither);
    if surface.coverage < 0.5 {
        discard;
    }

    // Light it as the material would be lit, with the atlas's colour, at
    // the surface rather than on the quad, so it is shadowed where the
    // object is.
    var at_surface = in;
    at_surface.world_position = vec4<f32>(surface.world_position, 1.0);
    at_surface.world_normal = surface.world_normal;
    var pbr_input = pbr_input_from_standard_material(at_surface, is_front);
    pbr_input.material.base_color =
        vec4<f32>(surface.color * pbr_input.material.base_color.rgb, 1.0);
    pbr_input.N = surface.world_normal;
    pbr_input.world_normal = surface.world_normal;
    var out: ImpostorFragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    out.depth = view_depth(surface.world_position);
    return out;
}
