//! Wavefront OBJ files, a simple text format almost every 3D tool reads,
//! with their MTL material files.
//!
//! Vertex colours are written after each position (`v x y z r g b`), an
//! extension most tools understand, as linear values rather than sRGB.
//! Levels with open surfaces use the material `mushroom_open`; draw it
//! two-sided in tools that hide back faces, or the cap will show holes.

use std::fmt::Write;

use super::level_name;
use crate::colour::linear_from_hex;
use crate::mesh::MushroomMesh;
use crate::options::MushroomOptions;

/// OBJ and MTL file contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjFiles {
    /// The `.obj` text.
    pub obj: String,
    /// The `.mtl` text.
    pub mtl: String,
}

/// Creates OBJ and MTL text for `meshes`, one object per mesh, without
/// writing any files. `options` supplies the surface roughness.
///
/// `material_library` is the file name the OBJ uses to find its materials,
/// such as `"mushroom.mtl"`; save [`ObjFiles::mtl`] under that name next to
/// the OBJ. Texture coordinates are written unchanged, with `v` increasing
/// up the stem and toward the cap's top.
pub fn to_obj(
    meshes: &[MushroomMesh],
    options: &MushroomOptions,
    material_library: &str,
) -> ObjFiles {
    let mut obj = String::new();
    let _ = writeln!(obj, "# procgena_mushroom {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(obj, "mtllib {material_library}");

    // OBJ numbers vertices from 1, counting across all objects in the file.
    let mut base = 1u32;
    for (lod, mesh) in meshes.iter().enumerate() {
        let b = &mesh.surfaces;
        let _ = writeln!(obj, "o {}", level_name(lod, meshes.len()));
        for (p, c) in b.positions.iter().zip(&b.colors) {
            let _ = writeln!(
                obj,
                "v {} {} {} {} {} {}",
                p[0], p[1], p[2], c[0], c[1], c[2]
            );
        }
        for t in &b.uvs {
            let _ = writeln!(obj, "vt {} {}", t[0], t[1]);
        }
        for n in &b.normals {
            let _ = writeln!(obj, "vn {} {} {}", n[0], n[1], n[2]);
        }
        let material = if mesh.double_sided {
            "mushroom_open"
        } else {
            "mushroom"
        };
        let _ = writeln!(obj, "usemtl {material}");
        for t in b.indices.as_chunks::<3>().0 {
            let [x, y, z] = t.map(|i| i + base);
            let _ = writeln!(obj, "f {x}/{x}/{x} {y}/{y}/{y} {z}/{z}/{z}");
        }
        base += b.positions.len() as u32;
    }

    // Plain white materials, so the vertex colours show unchanged.
    let roughness = options.material.roughness;
    let shininess = (1.0 - roughness) * (1.0 - roughness) * 250.0;
    let white = linear_from_hex("#ffffff").unwrap_or([1.0; 3]);
    let mut mtl = String::new();
    for name in ["mushroom", "mushroom_open"] {
        let _ = writeln!(mtl, "newmtl {name}");
        let _ = writeln!(mtl, "Kd {} {} {}", white[0], white[1], white[2]);
        let _ = writeln!(mtl, "Ks 0.04 0.04 0.04");
        let _ = writeln!(mtl, "Ns {shininess}");
        let _ = writeln!(mtl, "Pr {roughness}");
        let _ = writeln!(mtl);
    }
    ObjFiles { obj, mtl }
}
