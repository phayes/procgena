//! Converting cluster meshes and their material for Bevy.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use procgena_fruit::{FruitMaterial, FruitMesh, FruitOptions};

/// A Bevy mesh with the positions, normals, texture coordinates and vertex
/// colours of `mesh`. Draw it with [`fruit_material`], which takes its
/// colour from the vertices and shows both sides.
pub fn fruit_mesh(mesh: &FruitMesh) -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, mesh.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, mesh.normals.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, mesh.uvs.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, mesh.colors.clone())
    .with_inserted_indices(Indices::U32(mesh.indices.clone()))
}

/// A white, two-sided material whose colour comes from the mesh's vertex
/// colours, with the roughness of the cluster's fruit or flowers.
pub fn fruit_material(options: &FruitOptions) -> StandardMaterial {
    let settings = FruitMaterial::for_item(&options.item);
    StandardMaterial {
        base_color: Color::WHITE,
        metallic: settings.metallic,
        perceptual_roughness: settings.roughness,
        cull_mode: None,
        double_sided: settings.double_sided,
        ..default()
    }
}
