//! Conversion of generated mushroom meshes into Bevy meshes.

use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::math::Vec3;
use bevy::mesh::{Indices, Mesh, MeshVertexAttribute, PrimitiveTopology, VertexFormat};
use procgena_mushroom::{MeshBuffers, StemRibbon};

/// Vertex attribute for flat stems: the point each vertex turns around in
/// `xyz`, and its sideways distance from it in `w`.
///
/// A mesh with both this and [`ATTRIBUTE_BILLBOARD_AXIS`] is turned toward
/// the camera by [`MushroomMaterial`](crate::MushroomMaterial).
/// [`ribbon_mesh`] fills both in from a
/// [`BillboardVertex`](procgena_mushroom::BillboardVertex).
pub const ATTRIBUTE_BILLBOARD_PIVOT: MeshVertexAttribute = MeshVertexAttribute::new(
    "ProcgenaMushroom_BillboardPivot",
    0x4d55_5348_524f_0001,
    VertexFormat::Float32x4,
);

/// Vertex attribute for flat stems: the unit axis each vertex turns around
/// in `xyz`, and its distance along that axis in `w`. Use it together with
/// [`ATTRIBUTE_BILLBOARD_PIVOT`].
pub const ATTRIBUTE_BILLBOARD_AXIS: MeshVertexAttribute = MeshVertexAttribute::new(
    "ProcgenaMushroom_BillboardAxis",
    0x4d55_5348_524f_0002,
    VertexFormat::Float32x4,
);

/// Creates a Bevy mesh from mushroom surfaces, such as
/// [`MushroomMesh::surfaces`](procgena_mushroom::MushroomMesh::surfaces).
///
/// The colours are in the vertices, so draw it with a white base colour, as
/// [`mushroom_material`](crate::mushroom_material) does. The second set of
/// texture coordinates (UV1) holds each vertex's
/// [`Zone`](procgena_mushroom::Zone) number in `x`; the material uses it to
/// put gill lines and cap patterns in the right places.
pub fn mushroom_mesh(buffers: &MeshBuffers) -> Mesh {
    base_mesh(buffers)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, buffers.normals.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, buffers.tangents.clone())
}

/// Creates a Bevy mesh for a flat stem that turns toward the camera around
/// the stem's axis.
///
/// Draw it with a two-sided [`MushroomMaterial`](crate::MushroomMaterial).
/// Bevy works out an entity's bounds from the resting strip, which can hide
/// the turned strip near the screen edge, so give the entity the bounds
/// from [`ribbon_aabb`] and Bevy's `NoAutoAabb` component.
pub fn ribbon_mesh(ribbon: &StemRibbon) -> Mesh {
    let (pivots, axes): (Vec<[f32; 4]>, Vec<[f32; 4]>) = ribbon
        .billboards
        .iter()
        .map(|b| {
            let [px, py, pz] = b.pivot;
            let [ax, ay, az] = b.axis;
            ([px, py, pz, b.offset[0]], [ax, ay, az, b.offset[1]])
        })
        .unzip();
    base_mesh(&ribbon.strip)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, ribbon.strip.normals.clone())
        .with_inserted_attribute(ATTRIBUTE_BILLBOARD_PIVOT, pivots)
        .with_inserted_attribute(ATTRIBUTE_BILLBOARD_AXIS, axes)
}

/// Returns a box that holds `ribbon`'s strip however it is turned, for use
/// as the entity's bounds.
pub fn ribbon_aabb(ribbon: &StemRibbon) -> Aabb {
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for b in &ribbon.billboards {
        let pivot = Vec3::from(b.pivot);
        let reach = b.offset[0].hypot(b.offset[1]);
        min = min.min(pivot - reach);
        max = max.max(pivot + reach);
    }
    Aabb::from_min_max(min, max)
}

/// Creates a Bevy mesh with the positions, texture coordinates, colours,
/// zones and triangles of `buffers`, but no normals or tangents.
fn base_mesh(buffers: &MeshBuffers) -> Mesh {
    // In Bevy, v = 0 is the top of an image, so flip v.
    let uvs: Vec<[f32; 2]> = buffers.uvs.iter().map(|&[u, v]| [u, 1.0 - v]).collect();
    let zones: Vec<[f32; 2]> = buffers.zones.iter().map(|z| [z.id() as f32, 0.0]).collect();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, buffers.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, zones)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, buffers.colors.clone())
    .with_inserted_indices(Indices::U32(buffers.indices.clone()))
}
