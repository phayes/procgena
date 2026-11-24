// Draws a fruit or flower cluster's impostor attached to a tree, in the
// forward pass: one quad, turned and scaled into the cluster's place on its
// tree, swaying rigidly with its branch. The fragment shader is the impostor
// crate's own.

import bevy_pbr::render::{
    forward_io::{Vertex, VertexOutput, decompress_vertex},
    mesh_view_bindings::globals,
};
import procgena_core_bevy::shaders::instanced::{copy_record, copy_dither};
import procgena_impostor::shaders::common::impostor;
import procgena_impostor::shaders::vertex::impostor_vertex;
import package::shaders::fruit_impostor_place::{Attached, ClusterPlace, place_cluster};

@vertex
fn vertex(in: Vertex, copy: Attached) -> VertexOutput {
    let record = copy_record(copy.instance);
    let vertex = decompress_vertex(in, record);
    let place: ClusterPlace = place_cluster(copy, record, globals.time);
    var out = impostor_vertex(
        impostor,
        vertex.position.xy,
        place.world_from_cluster,
        place.cluster_from_world,
        place.sway,
        record,
    );
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = copy_dither(copy.instance);
    return out;
}
