//! Export to files for 3D tools: OBJ (with the `export-obj` feature) and
//! glTF (with the `export-gltf` feature).
//!
//! The functions return the file contents; your application saves them.
//! Pass one mesh, or one per level of detail from
//! [`crate::Piece::lod_meshes`]. Each mesh becomes a separate object named
//! `deadwood_LOD0`, `deadwood_LOD1` and so on, or just `deadwood` when there
//! is only one.
//!
//! Strips ([`crate::Ribbons`]) are exported as their two fixed, crossing
//! strips, because a file can't keep a strip turned toward the camera.
//! Draw them two-sided, with back-face culling turned off.

#[cfg(feature = "export-gltf")]
pub mod gltf;
#[cfg(feature = "export-obj")]
pub mod obj;

use crate::mesh::{DeadwoodMesh, MeshBuffers};

/// Returns the object name for mesh `lod` of `count` exported meshes.
pub(crate) fn level_name(lod: usize, count: usize) -> String {
    if count > 1 {
        format!("deadwood_LOD{lod}")
    } else {
        "deadwood".to_owned()
    }
}

/// Returns both strips of `mesh`'s ribbons in one set of buffers, in their
/// resting positions, or `None` if it has none.
pub(crate) fn strip_buffers(mesh: &DeadwoodMesh) -> Option<MeshBuffers> {
    let ribbons = mesh.ribbons.as_ref()?;
    let mut out = ribbons.strip.clone();
    append(&mut out, &ribbons.cull);
    Some(out)
}

/// Appends `extra`'s vertices and triangles to `out`, renumbering the
/// triangles' vertices to match.
fn append(out: &mut MeshBuffers, extra: &MeshBuffers) {
    let base = out.positions.len() as u32;
    out.positions.extend_from_slice(&extra.positions);
    out.normals.extend_from_slice(&extra.normals);
    out.uvs.extend_from_slice(&extra.uvs);
    out.tangents.extend_from_slice(&extra.tangents);
    out.colors.extend_from_slice(&extra.colors);
    out.wood.extend_from_slice(&extra.wood);
    out.indices.extend(extra.indices.iter().map(|i| i + base));
}
