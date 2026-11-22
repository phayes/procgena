//! Export to files for 3D tools: OBJ (with the `export-obj` feature) and
//! glTF (with the `export-gltf` feature).
//!
//! The functions return the file contents; your application saves them.
//! Pass one mesh, or one per detail level from
//! [`crate::Mushroom::lod_meshes`]. Each mesh becomes a separate object
//! named `mushroom_LOD0`, `mushroom_LOD1` and so on, or just `mushroom` when
//! there is only one. A flat stem is exported as its two strips crossing at
//! right angles, because a file can't keep a strip turned toward the camera.

#[cfg(feature = "export-gltf")]
pub mod gltf;
#[cfg(feature = "export-obj")]
pub mod obj;

use crate::mesh::{MeshBuffers, MushroomMesh};

/// Returns the object name for mesh `lod` of `count` exported meshes.
pub(crate) fn level_name(lod: usize, count: usize) -> String {
    if count > 1 {
        format!("mushroom_LOD{lod}")
    } else {
        "mushroom".to_owned()
    }
}

/// Returns every triangle of `mesh` in one set of buffers, including both
/// strips of a flat stem in their resting positions.
pub(crate) fn static_buffers(mesh: &MushroomMesh) -> MeshBuffers {
    let mut out = mesh.surfaces.clone();
    if let Some(ribbon) = &mesh.stem_ribbon {
        append(&mut out, &ribbon.strip);
        append(&mut out, &ribbon.cull);
    }
    out
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
    out.zones.extend_from_slice(&extra.zones);
    out.indices.extend(extra.indices.iter().map(|i| i + base));
}
