//! Conversion of generated dead wood meshes into Bevy meshes.

use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::math::Vec3;
use bevy::mesh::{Indices, Mesh, MeshVertexAttribute, PrimitiveTopology, VertexFormat};
use procgena_deadwood::{MeshBuffers, Ribbons};

/// Vertex attribute for flat strips: the point each vertex turns around in
/// `xyz`, and its sideways distance from it in `w`.
///
/// A mesh with both this and [`ATTRIBUTE_BILLBOARD_AXIS`] is turned toward
/// the camera by [`DeadwoodMaterial`](crate::DeadwoodMaterial).
/// [`ribbon_mesh`] fills both in from a
/// [`BillboardVertex`](procgena_deadwood::BillboardVertex).
pub const ATTRIBUTE_BILLBOARD_PIVOT: MeshVertexAttribute = MeshVertexAttribute::new(
    "ProcgenaDeadwood_BillboardPivot",
    0x4445_4144_574f_0001,
    VertexFormat::Float32x4,
);

/// Vertex attribute for flat strips: the unit axis each vertex turns around
/// in `xyz`, and its distance along that axis in `w`. Use it together with
/// [`ATTRIBUTE_BILLBOARD_PIVOT`].
pub const ATTRIBUTE_BILLBOARD_AXIS: MeshVertexAttribute = MeshVertexAttribute::new(
    "ProcgenaDeadwood_BillboardAxis",
    0x4445_4144_574f_0002,
    VertexFormat::Float32x4,
);

/// Creates a Bevy mesh from dead wood surfaces, such as
/// [`DeadwoodMesh::surfaces`](procgena_deadwood::DeadwoodMesh::surfaces).
///
/// The colours are in the vertices, so draw it with a white base colour, as
/// [`deadwood_material`](crate::deadwood_material) does. The second set of
/// texture coordinates (UV1) holds what the material needs to tell bark,
/// end grain and hollows apart: in `x`, the vertex's
/// [`Zone`](procgena_deadwood::Zone) number times 4 plus its bark cover
/// (0 to 1); in `y`, its distance from the stem's centre line in metres.
pub fn deadwood_mesh(buffers: &MeshBuffers) -> Mesh {
    base_mesh(buffers)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, buffers.normals.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, buffers.tangents.clone())
}

/// Creates a Bevy mesh for thin stems drawn as flat strips that turn toward
/// the camera around each stem's axis.
///
/// Draw it with a two-sided [`DeadwoodMaterial`](crate::DeadwoodMaterial)
/// without bark images, such as one from
/// [`deadwood_material`](crate::deadwood_material) with `ribbon` set: the
/// strips have no tangents for a normal map. Bevy works out an entity's
/// bounds from the resting strips, which can hide turned strips near the
/// screen edge, so give the entity the bounds from [`ribbon_aabb`] and
/// Bevy's `NoAutoAabb` component. [`Ribbons::cull`] is not used.
pub fn ribbon_mesh(ribbons: &Ribbons) -> Mesh {
    let (pivots, axes): (Vec<[f32; 4]>, Vec<[f32; 4]>) = ribbons
        .billboards
        .iter()
        .map(|b| {
            let [px, py, pz] = b.pivot;
            let [ax, ay, az] = b.axis;
            ([px, py, pz, b.offset[0]], [ax, ay, az, b.offset[1]])
        })
        .unzip();
    base_mesh(&ribbons.strip)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, ribbons.strip.normals.clone())
        .with_inserted_attribute(ATTRIBUTE_BILLBOARD_PIVOT, pivots)
        .with_inserted_attribute(ATTRIBUTE_BILLBOARD_AXIS, axes)
}

/// Returns a box that holds `ribbons`' strips however they are turned, for
/// use as the entity's bounds.
pub fn ribbon_aabb(ribbons: &Ribbons) -> Aabb {
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for b in &ribbons.billboards {
        let pivot = Vec3::from(b.pivot);
        let reach = b.offset[0].hypot(b.offset[1]);
        min = min.min(pivot - reach);
        max = max.max(pivot + reach);
    }
    Aabb::from_min_max(min, max)
}

/// Creates a Bevy mesh with the positions, texture coordinates, colours,
/// wood values and triangles of `buffers`, but no normals or tangents.
fn base_mesh(buffers: &MeshBuffers) -> Mesh {
    // In Bevy, v = 0 is the top of an image, so flip v. Tangents keep their
    // sign: it gives the direction of increasing v before the flip, which is
    // "up" in the image, as Bevy's OpenGL-style normal maps expect.
    let uvs: Vec<[f32; 2]> = buffers.uvs.iter().map(|&[u, v]| [u, 1.0 - v]).collect();
    let wood: Vec<[f32; 2]> = buffers
        .wood
        .iter()
        .map(|&[zone, radius, bark, _rot]| [zone * 4.0 + bark.clamp(0.0, 1.0), radius])
        .collect();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, buffers.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, wood)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, buffers.colors.clone())
    .with_inserted_indices(Indices::U32(buffers.indices.clone()))
}

#[cfg(test)]
mod tests {
    use bevy::mesh::VertexAttributeValues;
    use procgena_deadwood::{DeadwoodPreset, Piece};

    use super::*;

    #[test]
    fn wood_values_pack_zone_and_bark_into_uv1() {
        let mesh = Piece::from_preset(DeadwoodPreset::fresh_cut_log())
            .unwrap()
            .mesh();
        let bevy = deadwood_mesh(&mesh.surfaces);
        let Some(VertexAttributeValues::Float32x2(uv1)) = bevy.attribute(Mesh::ATTRIBUTE_UV_1)
        else {
            panic!("no UV1");
        };
        for (packed, wood) in uv1.iter().zip(&mesh.surfaces.wood) {
            // The zone comes back from the integer part, the bark cover from
            // the rest, even at full cover.
            assert_eq!((packed[0] / 4.0 + 0.01).floor(), wood[0]);
            assert!((packed[0] - wood[0] * 4.0 - wood[2]).abs() < 1e-5);
            assert_eq!(packed[1], wood[1]);
        }
    }

    #[test]
    fn ribbon_bounds_hold_every_turned_vertex() {
        let piece = Piece::from_preset(DeadwoodPreset::dead_branch()).unwrap();
        let meshes = piece.lod_meshes();
        let ribbons = meshes
            .iter()
            .find_map(|(_, m)| m.ribbons.as_ref())
            .expect("a dead branch has strips at some level");
        let aabb = ribbon_aabb(ribbons);
        let (min, max) = (Vec3::from(aabb.min()), Vec3::from(aabb.max()));
        for b in &ribbons.billboards {
            let reach = b.offset[0].hypot(b.offset[1]);
            let pivot = Vec3::from(b.pivot);
            assert!((pivot - reach).cmpge(min - 1e-5).all());
            assert!((pivot + reach).cmple(max + 1e-5).all());
        }
        assert!(ribbon_mesh(ribbons).contains_attribute(ATTRIBUTE_BILLBOARD_AXIS));
    }
}
