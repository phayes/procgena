// The prepass twin of `fruit_impostor.wesl`: depth, normals and shadows of
// a cluster's impostor attached to a tree.

import bevy_pbr::{
    prepass::io::{Vertex, VertexOutput, decompress_vertex},
    render::mesh_view_bindings::globals,
};
import procgena_core_bevy::shaders::instanced::{copy_record, copy_dither};
import procgena_impostor::shaders::common::impostor;
import procgena_impostor::shaders::vertex_prepass::impostor_prepass_vertex;
import package::shaders::fruit_impostor_place::{Attached, ClusterPlace, place_cluster};

@vertex
fn vertex(in: Vertex, copy: Attached) -> VertexOutput {
    let record = copy_record(copy.instance);
    let vertex = decompress_vertex(in, record);
    let place: ClusterPlace = place_cluster(copy, record, globals.time);
    var out = impostor_prepass_vertex(
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
