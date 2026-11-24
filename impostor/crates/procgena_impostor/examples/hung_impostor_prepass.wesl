// The prepass twin of `hung_impostor.wesl`, for depth and shadows.

import bevy_pbr::{
    prepass::io::{Vertex, VertexOutput, decompress_vertex},
    render::mesh_functions,
};
import procgena_impostor::shaders::common::impostor;
import procgena_impostor::shaders::vertex_prepass::impostor_prepass_vertex;

/// Where the impostor hangs within its entity, and how far it is moved.
struct Hang {
    entity_from_object: mat4x4<f32>,
    object_from_entity: mat4x4<f32>,
    world_offset: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(104) var<uniform> hang: Hang;

@vertex
fn vertex(in: Vertex) -> VertexOutput {
    let vertex = decompress_vertex(in, in.instance_index);
    let index = in.instance_index;
    return impostor_prepass_vertex(
        impostor,
        vertex.position.xy,
        mesh_functions::get_world_from_local(index) * hang.entity_from_object,
        hang.object_from_entity * mesh_functions::get_local_from_world(index),
        hang.world_offset.xyz,
        index,
    );
}
