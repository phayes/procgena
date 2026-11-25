//! Conversion of generated mushroom meshes into Bevy meshes.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use procgena_mushroom::MeshBuffers;

/// Creates a Bevy mesh from mushroom surfaces, such as
/// [`MushroomMesh::surfaces`](procgena_mushroom::MushroomMesh::surfaces).
///
/// The colours are in the vertices, so draw it with a white base colour, as
/// [`mushroom_material`](crate::mushroom_material) does. The second set of
/// texture coordinates (UV1) holds each vertex's
/// [`Zone`](procgena_mushroom::Zone) number in `x`; the material uses it to
/// put gill lines and cap patterns in the right places.
pub fn mushroom_mesh(buffers: &MeshBuffers) -> Mesh {
    // In Bevy, v = 0 is the top of an image, so flip v.
    let uvs: Vec<[f32; 2]> = buffers.uvs.iter().map(|&[u, v]| [u, 1.0 - v]).collect();
    let zones: Vec<[f32; 2]> = buffers.zones.iter().map(|z| [z.id() as f32, 0.0]).collect();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, buffers.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, buffers.normals.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, buffers.tangents.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, zones)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, buffers.colors.clone())
    .with_inserted_indices(Indices::U32(buffers.indices.clone()))
}
