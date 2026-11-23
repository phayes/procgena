//! Wavefront OBJ files, a simple text format almost every 3D tool reads,
//! with their MTL material files.
//!
//! Vertex colours are written after each position (`v x y z r g b`). This
//! is an extension to the format that most tools understand. The colours
//! are linear values, not sRGB, so a tool that expects sRGB shows them
//! darker than intended.
//!
//! Round stems use the material `deadwood`; strips use `deadwood_strip`,
//! which should be drawn two-sided. Leaf cards use `deadwood_leaves`, which
//! should be drawn two-sided with the leaf texture and alpha testing. OBJ
//! has no way to say this, so set it in your tool.

use std::fmt::Write;

use super::{level_name, mesh_has_leaves, strip_buffers};
use crate::mesh::{DeadwoodMesh, MeshBuffers};
use crate::options::DeadwoodOptions;

/// OBJ and MTL file contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjFiles {
    /// The `.obj` text.
    pub obj: String,
    /// The `.mtl` text.
    pub mtl: String,
}

/// Creates OBJ and MTL text for `meshes`, one object per mesh, without
/// writing any files. Only the surface roughness is taken from `options`.
/// The materials are plain white, so the vertex colours show unchanged.
///
/// `material_library` is the file name the OBJ uses to find its materials,
/// such as `"log.mtl"`; save [`ObjFiles::mtl`] under that name next to the
/// OBJ.
pub fn to_obj(
    meshes: &[DeadwoodMesh],
    options: &DeadwoodOptions,
    material_library: &str,
) -> ObjFiles {
    let mut obj = String::new();
    let _ = writeln!(obj, "# procgena_deadwood {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(obj, "mtllib {material_library}");

    // OBJ numbers vertices from 1, counting across all objects in the file.
    let mut base = 1u32;
    for (lod, mesh) in meshes.iter().enumerate() {
        let _ = writeln!(obj, "o {}", level_name(lod, meshes.len()));
        let strips = strip_buffers(mesh);
        for (buffers, material) in std::iter::once((&mesh.surfaces, "deadwood"))
            .chain(strips.as_ref().map(|s| (s, "deadwood_strip")))
            .chain(mesh.leaves.as_ref().map(|l| (l, "deadwood_leaves")))
        {
            write_group(&mut obj, buffers, material, &mut base);
        }
    }

    // Plain white materials, so the vertex colours show unchanged.
    let roughness = options.material.roughness;
    let shininess = (1.0 - roughness) * (1.0 - roughness) * 250.0;
    let mut mtl = String::new();
    let leaves = mesh_has_leaves(meshes).then_some("deadwood_leaves");
    for name in ["deadwood", "deadwood_strip"].into_iter().chain(leaves) {
        let _ = writeln!(mtl, "newmtl {name}");
        let _ = writeln!(mtl, "Kd 1 1 1");
        let _ = writeln!(mtl, "Ks 0.04 0.04 0.04");
        let _ = writeln!(mtl, "Ns {shininess}");
        let _ = writeln!(mtl, "Pr {roughness}");
        let _ = writeln!(mtl);
    }
    ObjFiles { obj, mtl }
}

/// Writes `b`'s vertices and its triangles with `material`, numbering
/// vertices from `base` and advancing it.
fn write_group(obj: &mut String, b: &MeshBuffers, material: &str, base: &mut u32) {
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
    let _ = writeln!(obj, "usemtl {material}");
    for t in b.indices.chunks(3) {
        let [x, y, z] = [t[0], t[1], t[2]].map(|i| i + *base);
        let _ = writeln!(obj, "f {x}/{x}/{x} {y}/{y}/{y} {z}/{z}/{z}");
    }
    *base += b.positions.len() as u32;
}
