// What a bake copy writes into the normal atlas. Shared by the crate's own
// bake shader and by generators whose shaders draw their bake copies
// themselves, so every atlas stores normals and depth the same way.

import bevy_pbr::render::mesh_functions;

/// The normal atlas texel for a bake copy's fragment: the surface normal in
/// the baked object's own coordinates, packed as `n * 0.5 + 0.5`, and the
/// surface's depth in alpha.
///
/// `world_normal` is the fragment's normal in world space (`pbr_input.N`),
/// `frag_depth` its `@builtin(position).z`, and `record` the mesh record of
/// the copy (`VertexOutput::instance_index`).
fn baked_normal(world_normal: vec3<f32>, frag_depth: f32, record: u32) -> vec4<f32> {
    // Each copy is the object turned into its view, so the copy's own
    // rotation turns the normal back into the object's coordinates. Every
    // view then stores normals the same way, and views blend simply.
    let world_from_local = mesh_functions::get_world_from_local(record);
    let rotation = mat3x3<f32>(
        world_from_local[0].xyz,
        world_from_local[1].xyz,
        world_from_local[2].xyz,
    );
    let normal = normalize(transpose(rotation) * world_normal);

    // The bake camera's depth range spans the bounding sphere and a margin
    // on each side, centred on the sphere. Without the margin, depth is the
    // surface's height towards the viewer: 0 at the sphere's far side, 0.5
    // at its centre, 1 at its near side.
    let depth = (frag_depth - 0.5) * (1.0 + DEPTH_MARGIN) + 0.5;
    return vec4<f32>(normal * 0.5 + 0.5, depth);
}

/// How far the bake cameras' depth range reaches past the bounding sphere on
/// each side, as a share of its radius; `DEPTH_MARGIN` in `bake.rs`.
const DEPTH_MARGIN: f32 = 1.0 / 256.0;
