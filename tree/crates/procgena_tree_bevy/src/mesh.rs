//! Convert generated bark, leaf, fruit and flower geometry into meshes Bevy can draw.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, MeshVertexAttribute, PrimitiveTopology, VertexFormat};
use procgena_tree::procgena_fruit::FruitMesh;
use procgena_tree::{BarkMaterial, BarkOptions, MeshBuffers};

/// Wind controls stored on each vertex: trunk bend, branch movement weight,
/// branch timing offset and leaf flutter. See [`MeshBuffers::wind`] for the layout.
/// Every mesh drawn with [`TreeMaterial`](crate::TreeMaterial) needs this attribute.
pub const ATTRIBUTE_WIND: MeshVertexAttribute = MeshVertexAttribute::new(
    "ProcgenaTree_Wind",
    0x5a45_5452_4545_0001,
    VertexFormat::Float32x4,
);

/// Converts bark buffers, such as [`TreeMesh::branches`](procgena_tree::TreeMesh::branches),
/// to a Bevy mesh using the supplied bark settings. Texture coordinates include
/// `bark`’s repeat settings and Bevy’s vertical image orientation.
///
/// Flat shading gives each triangle its own normal for a faceted appearance.
pub fn bark_mesh(buffers: &MeshBuffers, bark: &BarkOptions) -> Mesh {
    let [su, sv] = BarkMaterial::new(bark).uv_scale;
    let uvs = buffers
        .uvs
        .iter()
        .map(|&[u, v]| [u * su, 1.0 - v * sv])
        .collect();
    let mut mesh = base_mesh(buffers, uvs);
    if !buffers.tangents.is_empty() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, buffers.tangents.clone());
    }
    if bark.flat_shading {
        mesh.duplicate_vertices();
        mesh.compute_flat_normals();
        // Rebuild the texture directions to match the new flat surface normals.
        if mesh.contains_attribute(Mesh::ATTRIBUTE_TANGENT) {
            mesh.remove_attribute(Mesh::ATTRIBUTE_TANGENT);
            if let Err(error) = mesh.generate_tangents() {
                bevy::log::warn!("procgena_tree_bevy: no tangents for flat-shaded bark: {error}");
            }
        }
    }
    mesh
}

/// Creates a Bevy leaf mesh from buffers such as [`TreeMesh::leaves`](procgena_tree::TreeMesh::leaves).
///
/// Flips the vertical texture coordinate to place the top of the image at the leaf
/// tip. Preserves any supplied tangents, which orient normal-map lighting detail.
pub fn leaf_mesh(buffers: &MeshBuffers) -> Mesh {
    let uvs = buffers.uvs.iter().map(|&[u, v]| [u, 1.0 - v]).collect();
    let mut mesh = base_mesh(buffers, uvs);
    if !buffers.tangents.is_empty() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, buffers.tangents.clone());
    }
    mesh
}

/// Creates a Bevy mesh for fruit or flowers from buffers such as
/// [`TreeMesh::fruit`](procgena_tree::TreeMesh::fruit) or
/// [`TreeMesh::flowers`](procgena_tree::TreeMesh::flowers).
///
/// The surface colour comes from the per-vertex colours, so draw it with
/// [`fruit_material`](crate::fruit_material) or [`flower_material`](crate::flower_material),
/// or another material with a white base colour.
pub fn cluster_mesh(buffers: &MeshBuffers) -> Mesh {
    let uvs = buffers.uvs.iter().map(|&[u, v]| [u, 1.0 - v]).collect();
    base_mesh(buffers, uvs).with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, buffers.colors.clone())
}

/// Creates a Bevy mesh of one fruit or flower cluster specimen at one
/// detail level, in the cluster's own frame, for drawing attached to trees
/// with [`fruit_material`](crate::fruit_material) or
/// [`flower_material`](crate::flower_material). Its wind input holds only
/// each vertex's own flutter; the tree adds its branch's sway where the
/// cluster hangs.
pub fn attached_cluster_mesh(mesh: &FruitMesh) -> Mesh {
    cluster_mesh(&MeshBuffers {
        positions: mesh.positions.clone(),
        normals: mesh.normals.clone(),
        uvs: mesh.uvs.clone(),
        wind: mesh.flutter.iter().map(|&f| [0.0, 0.0, 0.0, f]).collect(),
        colors: mesh.colors.clone(),
        indices: mesh.indices.clone(),
        ..MeshBuffers::default()
    })
}

/// Converts positions, indices, wind controls and any normals into Bevy
/// attributes. Supply one texture-coordinate pair per position in `uvs`.
fn base_mesh(buffers: &MeshBuffers, uvs: Vec<[f32; 2]>) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, buffers.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(ATTRIBUTE_WIND, buffers.wind.clone())
    .with_inserted_indices(Indices::U32(buffers.indices.clone()));
    if !buffers.normals.is_empty() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, buffers.normals.clone());
    }
    mesh
}
