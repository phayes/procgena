//! Convert generated rock meshes into meshes Bevy can draw.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use procgena_stone::StoneMesh;

/// Creates a Bevy triangle mesh with the positions, normals and indices of `mesh`.
///
/// The mesh has no texture coordinates; draw it with
/// [`StoneMaterial`](crate::StoneMaterial), which projects textures onto the
/// rock along its axes. Positions keep the units of `mesh`.
pub fn stone_mesh(mesh: &StoneMesh) -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, mesh.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, mesh.normals.clone())
    .with_inserted_indices(Indices::U32(mesh.indices.clone()))
}
