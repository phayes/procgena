// Where a fruit or flower cluster's impostor sits on its tree, shared by the
// forward and prepass shaders of attached cluster impostors.

import bevy_pbr::render::mesh_functions;
import procgena_core_bevy::shaders::instanced::attached_wind;
import procgena_tree::wind::{WindParams, wind_offset};

/// The material's wind settings, the trees' own (binding 104, after the
/// impostor's bindings 100 to 103).
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var<uniform> wind_params: WindParams;

/// The per-copy input of an attached draw: the tree's record and the
/// cluster's dither, its place and scale on the tree, its turn, and its
/// branch's wind numbers.
struct Attached {
    @location(12) instance: vec2<u32>,
    @location(13) place: vec4<f32>,
    @location(14) turn: vec4<f32>,
    @location(15) wind: vec4<f32>,
}

/// A cluster's transform into the world, its inverse, and how far its
/// branch sways it, in world units.
struct ClusterPlace {
    world_from_cluster: mat4x4<f32>,
    cluster_from_world: mat4x4<f32>,
    sway: vec3<f32>,
}

/// The rotation matrix of unit quaternion `q`.
fn rotation(q: vec4<f32>) -> mat3x3<f32> {
    let x = q.x;
    let y = q.y;
    let z = q.z;
    let w = q.w;
    return mat3x3<f32>(
        vec3<f32>(1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + w * z), 2.0 * (x * z - w * y)),
        vec3<f32>(2.0 * (x * y - w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + w * x)),
        vec3<f32>(2.0 * (x * z + w * y), 2.0 * (y * z - w * x), 1.0 - 2.0 * (x * x + y * y)),
    );
}

/// Places the attached cluster `copy` on the tree whose mesh record is
/// `record`, at `time` seconds.
fn place_cluster(copy: Attached, record: u32, time: f32) -> ClusterPlace {
    let world_from_tree = mesh_functions::get_world_from_local(record);
    let tree_from_world = mesh_functions::get_local_from_world(record);

    // The cluster's own frame within the tree: moved, turned and scaled.
    let scale = copy.place.w;
    let turn = rotation(copy.turn);
    let tree_from_cluster = mat4x4<f32>(
        vec4<f32>(turn[0] * scale, 0.0),
        vec4<f32>(turn[1] * scale, 0.0),
        vec4<f32>(turn[2] * scale, 0.0),
        vec4<f32>(copy.place.xyz, 1.0),
    );
    let back = transpose(turn) * (1.0 / scale);
    let cluster_from_tree = mat4x4<f32>(
        vec4<f32>(back[0], 0.0),
        vec4<f32>(back[1], 0.0),
        vec4<f32>(back[2], 0.0),
        vec4<f32>(-(back * copy.place.xyz), 1.0),
    );

    // The branch's sway at the cluster, as the tree's meshes sway, without
    // the leaves' own flutter: the whole cluster moves as one.
    let anchor = world_from_tree[3].xyz;
    let wind = vec4<f32>(attached_wind(copy.wind), 0.0);
    let sway = wind_offset(copy.place.xyz + anchor, wind, anchor, time, wind_params);

    var out: ClusterPlace;
    out.world_from_cluster = world_from_tree * tree_from_cluster;
    out.cluster_from_world = cluster_from_tree * tree_from_world;
    out.sway = (world_from_tree * vec4<f32>(sway, 0.0)).xyz;
    return out;
}
