//! Export to files for 3D tools: OBJ (with the `export-obj` feature) and
//! glTF (with the `export-gltf` feature).
//!
//! The functions return the file contents; your application saves them.
//! Pass one mesh, such as [`crate::Mushroom::mesh`], or both meshes from
//! [`crate::Mushroom::lod_meshes`]. Each mesh becomes a separate object
//! named `mushroom_LOD0`, `mushroom_LOD1` and so on, or just `mushroom` when
//! there is only one. Warts are painted into the vertex colours when the
//! mesh's detail has [`paint_warts`](crate::MushroomDetail::paint_warts) on.

#[cfg(feature = "export-gltf")]
pub mod gltf;
#[cfg(feature = "export-obj")]
pub mod obj;

/// Returns the object name for mesh `lod` of `count` exported meshes.
pub(crate) fn level_name(lod: usize, count: usize) -> String {
    if count > 1 {
        format!("mushroom_LOD{lod}")
    } else {
        "mushroom".to_owned()
    }
}
