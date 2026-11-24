//! Wavefront OBJ files, a simple text format almost every 3D tool reads,
//! with their MTL material files.
//!
//! Vertex colours are written after each position (`v x y z r g b`), an
//! extension most tools understand, as linear values rather than sRGB.

use std::fmt::Write;

use super::level_name;
use crate::{FruitMaterial, FruitMesh, FruitOptions};

/// OBJ and MTL file contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjFiles {
    /// The `.obj` text.
    pub obj: String,
    /// The `.mtl` text.
    pub mtl: String,
}

/// Creates OBJ and MTL text for `meshes`, one object per mesh, without
/// writing any files. `options` chooses the fruit or flower material.
///
/// `material_library` is the file name the OBJ uses to find its material,
/// such as `"apple.mtl"`; save [`ObjFiles::mtl`] under that name next to
/// the OBJ.
pub fn to_obj(meshes: &[FruitMesh], options: &FruitOptions, material_library: &str) -> ObjFiles {
    let mut obj = String::new();
    let _ = writeln!(obj, "# procgena_fruit {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(obj, "mtllib {material_library}");

    // OBJ numbers vertices from 1, counting across all objects in the file.
    let mut base = 1u32;
    for (lod, mesh) in meshes.iter().enumerate() {
        let _ = writeln!(obj, "o {}", level_name(lod, meshes.len()));
        for (p, c) in mesh.positions.iter().zip(&mesh.colors) {
            let _ = writeln!(
                obj,
                "v {} {} {} {} {} {}",
                p[0], p[1], p[2], c[0], c[1], c[2]
            );
        }
        for t in &mesh.uvs {
            let _ = writeln!(obj, "vt {} {}", t[0], t[1]);
        }
        for n in &mesh.normals {
            let _ = writeln!(obj, "vn {} {} {}", n[0], n[1], n[2]);
        }
        let _ = writeln!(obj, "usemtl fruit");
        for t in mesh.indices.as_chunks::<3>().0 {
            let [x, y, z] = t.map(|i| i + base);
            let _ = writeln!(obj, "f {x}/{x}/{x} {y}/{y}/{y} {z}/{z}/{z}");
        }
        base += mesh.positions.len() as u32;
    }

    // A plain white material, so the vertex colours show unchanged.
    let material = FruitMaterial::for_item(&options.item);
    let mut mtl = String::new();
    let _ = writeln!(mtl, "newmtl fruit");
    let _ = writeln!(mtl, "Kd 1 1 1");
    let _ = writeln!(mtl, "Ks 0 0 0");
    let _ = writeln!(mtl, "Pr {}", material.roughness);
    let _ = writeln!(mtl, "Pm {}", material.metallic);
    ObjFiles { obj, mtl }
}
