//! Convert generated clump meshes into meshes Bevy can draw.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use procgena_clump::ClumpMesh;

/// Creates a Bevy triangle mesh from `mesh`, ready to draw with
/// [`ClumpMaterial`](crate::ClumpMaterial).
///
/// The mesh has positions, normals and indices, and no texture coordinates:
/// the material projects its textures onto the clump. Each vertex's
/// [`ClumpMesh::fade`] is stored as the alpha of a white vertex colour
/// ([`Mesh::ATTRIBUTE_COLOR`]), which the material uses to thin out the
/// clump's edges. Positions keep the units of `mesh`.
pub fn clump_mesh(mesh: &ClumpMesh) -> Mesh {
    let colors: Vec<[f32; 4]> = mesh.fade.iter().map(|&f| [1.0, 1.0, 1.0, f]).collect();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, mesh.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, mesh.normals.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(mesh.indices.clone()))
}
