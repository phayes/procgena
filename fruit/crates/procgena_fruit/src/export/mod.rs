//! Export one cluster to files for 3D tools: OBJ (with the `export-obj`
//! feature) and glTF (with the `export-gltf` feature).
//!
//! The functions return the file contents; your application saves them.
//! Pass one mesh, or one per detail level from [`crate::Fruit::lod_meshes`].
//! Each mesh becomes a separate object named `fruit_LOD0`, `fruit_LOD1` and
//! so on, or just `fruit` when there is only one. Surfaces are drawn from
//! both sides, since petals, stalk strips and flat fruit have no thickness.

#[cfg(feature = "export-gltf")]
pub mod gltf;
#[cfg(feature = "export-obj")]
pub mod obj;

/// The object name for mesh `lod` of `count` exported meshes.
fn level_name(lod: usize, count: usize) -> String {
    if count > 1 {
        format!("fruit_LOD{lod}")
    } else {
        "fruit".to_owned()
    }
}
