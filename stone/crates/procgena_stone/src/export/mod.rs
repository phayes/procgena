//! Export rock meshes for use in 3D viewers and modelling tools.
//!
//! Enable `export-obj` for OBJ geometry with an MTL material file, or
//! `export-gltf` for glTF 2.0 as a `.gltf`/`.bin` pair or a binary `.glb` file.
//! The exporters return file contents; your application saves them.
//!
//! Both formats add texture coordinates using box projection: imagine projecting
//! an image onto the rock from each of six cube faces. Each triangle uses the
//! face closest to its own facing direction. This suits repeating textures, but
//! seams and some stretching can remain. Vertices shared across projection
//! boundaries are copied so each side can have different texture coordinates.
//! Complete triangles keep their original positions, normals, and order.
//!
//! The exported material uses the rock's [`MaterialOptions`]: its colour,
//! roughness and texture size. No texture images are included with the crate.
//! Supply texture images through a callback, using [`ImageMap`](crate::ImageMap)
//! to identify each image's purpose, or use the material colour alone. OBJ
//! references image paths; glTF can reference images or embed their bytes.

#[cfg(feature = "export-gltf")]
pub mod gltf;
#[cfg(feature = "export-obj")]
pub mod obj;

use std::collections::HashMap;

use glam::Vec3;

use crate::{MaterialOptions, StoneMesh};

/// Returns `material.texture_size` if it is finite and greater than zero.
pub(crate) fn checked_texture_size(material: &MaterialOptions) -> Result<f32, InvalidTextureSize> {
    let size = material.texture_size;
    if size.is_finite() && size > 0.0 {
        Ok(size)
    } else {
        Err(InvalidTextureSize(size))
    }
}

/// A [`MaterialOptions::texture_size`] that is zero, negative, NaN, or infinite.
/// Holds the rejected value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InvalidTextureSize(pub f32);

impl std::fmt::Display for InvalidTextureSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "texture size must be finite and greater than zero, got {}",
            self.0
        )
    }
}

impl std::error::Error for InvalidTextureSize {}

/// Vertex data with texture coordinates and lighting directions added for export.
///
/// Each vertex belongs to one box-projection face. Shared vertices are copied
/// when faces need different coordinates. Triangle order is preserved; unused
/// vertices are omitted and the rest are ordered by first use.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct UvMesh {
    /// World-space positions, including copies at texture seams.
    pub(crate) positions: Vec<[f32; 3]>,
    /// Original vertex normals, copied along with their positions.
    pub(crate) normals: Vec<[f32; 3]>,
    /// Image coordinates: u increases rightwards and v upwards before any
    /// format-specific conversion. Values outside `0..=1` allow texture repeats.
    pub(crate) uvs: Vec<[f32; 2]>,
    /// A tangent (xyz) is a unit lighting direction along the surface, derived
    /// from the projection's u axis. The sign w makes `cross(normal, tangent) * w`
    /// point into the same half-plane as the v axis. This second direction is
    /// called the bitangent; smooth vertex normals can tilt both directions.
    pub(crate) tangents: Vec<[f32; 4]>,
    /// Triangle corners indexing all four vertex-data arrays.
    pub(crate) indices: Vec<u32>,
}

/// A cube face that determines which world axes supply texture coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Projection {
    axis: usize,
    positive: bool,
}

impl Projection {
    /// Chooses the axis with the largest absolute component of `direction`.
    /// Ties prefer x, then y, then z. A zero vector selects the positive x face.
    fn facing(direction: Vec3) -> Self {
        let a = direction.abs();
        let axis = if a.x >= a.y && a.x >= a.z {
            0
        } else if a.y >= a.z {
            1
        } else {
            2
        };
        Self {
            axis,
            positive: direction[axis] >= 0.0,
        }
    }

    /// Returns a distinct lookup key in `0..6` for this cube face.
    fn id(self) -> usize {
        self.axis * 2 + usize::from(self.positive)
    }

    /// Returns unit world vectors for increasing u and v.
    ///
    /// Viewed from outside a face, u runs right and v runs up. Side faces use
    /// +y for v. The +y face uses -z for v, and the -y face uses +z, keeping
    /// `u.cross(v)` pointed outwards on every face.
    fn directions(self) -> (Vec3, Vec3) {
        let s = if self.positive { 1.0 } else { -1.0 };
        match self.axis {
            0 => (Vec3::new(0.0, 0.0, -s), Vec3::Y),
            1 => (Vec3::X, Vec3::new(0.0, 0.0, -s)),
            _ => (Vec3::new(s, 0.0, 0.0), Vec3::Y),
        }
    }
}

/// Projects each triangle onto a cube face to add texture coordinates and tangents.
///
/// Supply a finite, positive `texture_size` (see
/// [`checked_texture_size`]), measured in mesh units (metres for a
/// [`crate::Stone`] mesh) per repeat
/// along the projection axes, and unit vertex normals. Normals are not validated.
/// A triangle chooses its projection from its geometric normal. Vertices shared
/// across projection faces are copied without changing their positions or normals.
/// Any final one or two indices that do not form a triangle are ignored.
///
/// # Panics
///
/// Panics if an index in a complete triangle refers to a missing position or normal.
pub(crate) fn box_projection(mesh: &StoneMesh, texture_size: f32) -> UvMesh {
    let mut out = UvMesh::default();
    // Remember the exported vertex for each source vertex and projection face.
    let mut remap: HashMap<(u32, usize), u32> = HashMap::new();
    out.indices.reserve(mesh.indices.len());

    for t in mesh.indices.as_chunks::<3>().0 {
        // Use the triangle's facing direction. For a triangle with zero area,
        // use the sum of its vertex normals instead.
        let p = t.map(|i| Vec3::from(mesh.positions[i as usize]));
        let face = (p[1] - p[0]).cross(p[2] - p[0]);
        let face = if face == Vec3::ZERO {
            t.iter()
                .map(|&i| Vec3::from(mesh.normals[i as usize]))
                .sum()
        } else {
            face
        };
        let projection = Projection::facing(face);
        let (u_dir, v_dir) = projection.directions();

        // Triangles using the same source vertex and projection can share the copy.
        for &i in t {
            let next = out.positions.len() as u32;
            let index = *remap.entry((i, projection.id())).or_insert(next);
            if index == next {
                let position = Vec3::from(mesh.positions[i as usize]);
                let normal = Vec3::from(mesh.normals[i as usize]);
                out.positions.push(position.to_array());
                out.normals.push(normal.to_array());
                out.uvs.push([
                    position.dot(u_dir) / texture_size,
                    position.dot(v_dir) / texture_size,
                ]);
                out.tangents.push(tangent(normal, u_dir, v_dir));
            }
            out.indices.push(index);
        }
    }
    out
}

/// Builds the lighting directions used to interpret a normal map.
///
/// Expects a unit `normal` and perpendicular unit projection axes. The tangent
/// follows u after removing its component along the normal; w selects the sign
/// of the bitangent so it agrees with v. These are shading directions, not
/// necessarily the exact directions of texture-coordinate changes on a triangle.
fn tangent(normal: Vec3, u_dir: Vec3, v_dir: Vec3) -> [f32; 4] {
    // Keep the tangent perpendicular to the normal. When u is parallel to the
    // normal, try a direction perpendicular to v and the normal, then fall back to u.
    let t = (u_dir - normal * normal.dot(u_dir))
        .try_normalize()
        .unwrap_or_else(|| v_dir.cross(normal).normalize_or(u_dir));
    let w = if normal.cross(t).dot(v_dir) < 0.0 {
        -1.0
    } else {
        1.0
    };
    [t.x, t.y, t.z, w]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Stone, StoneOptions};

    fn stone_mesh(mesher: crate::Mesher) -> StoneMesh {
        let mut options = StoneOptions::default();
        options.mesh.mesher = mesher;
        options.mesh.game_level = crate::StoneDetail::new(32, crate::TextureSize::Size512);
        Stone::new(options).unwrap().mesh().clone()
    }

    #[test]
    fn projection_keeps_triangles_and_vertex_data() {
        for mesher in [crate::Mesher::MarchingCubes, crate::Mesher::DualContouring] {
            let mesh = stone_mesh(mesher);
            let out = box_projection(&mesh, 1.0);
            assert_eq!(out.indices.len(), mesh.indices.len());
            assert!(out.positions.len() > mesh.positions.len());
            assert_eq!(out.normals.len(), out.positions.len());
            assert_eq!(out.uvs.len(), out.positions.len());
            assert_eq!(out.tangents.len(), out.positions.len());

            // Splitting vertices at texture seams must preserve each triangle corner's data.
            for (a, b) in out.indices.iter().zip(&mesh.indices) {
                assert_eq!(out.positions[*a as usize], mesh.positions[*b as usize]);
                assert_eq!(out.normals[*a as usize], mesh.normals[*b as usize]);
            }
        }
    }

    #[test]
    fn texture_is_not_mirrored() {
        // Projection should keep counter-clockwise triangle winding in UV space,
        // so viewing a face from outside does not mirror its texture.
        let out = box_projection(&stone_mesh(crate::Mesher::MarchingCubes), 1.0);
        let uv = |i: u32| glam::Vec2::from(out.uvs[i as usize]);
        let mut clockwise = 0;
        for t in out.indices.as_chunks::<3>().0 {
            let area = (uv(t[1]) - uv(t[0])).perp_dot(uv(t[2]) - uv(t[0]));
            if area < 0.0 {
                clockwise += 1;
            }
        }
        assert_eq!(clockwise, 0);
    }

    #[test]
    fn projection_follows_the_face_direction() {
        // This triangle faces mostly +x, with a small -y component: u uses -z and v uses +y.
        let mesh = StoneMesh {
            positions: vec![[1.0, 0.0, 0.0], [1.0, 0.0, -2.0], [1.1, 4.0, 0.0]],
            normals: vec![[1.0, 0.0, 0.0]; 3],
            indices: vec![0, 1, 2],
        };
        let out = box_projection(&mesh, 2.0);
        assert_eq!(out.uvs, [[0.0, 0.0], [1.0, 0.0], [0.0, 2.0]]);
        for t in &out.tangents {
            assert_eq!(*t, [0.0, 0.0, -1.0, 1.0]);
        }
    }

    #[test]
    fn tangents_point_along_u_and_bitangents_along_v() {
        for mesher in [crate::Mesher::MarchingCubes, crate::Mesher::DualContouring] {
            let out = box_projection(&stone_mesh(mesher), 1.0);
            let (mut vertices, mut flipped) = (0, 0);
            for tri in out.indices.as_chunks::<3>().0 {
                // Reconstruct the triangle's projection axes from its positions.
                let p = tri.map(|i| Vec3::from(out.positions[i as usize]));
                let face = (p[1] - p[0]).cross(p[2] - p[0]);
                let (u_dir, v_dir) = Projection::facing(face).directions();

                for &i in tri {
                    let n = Vec3::from(out.normals[i as usize]);
                    let [x, y, z, w] = out.tangents[i as usize];
                    let t = Vec3::new(x, y, z);
                    assert!((t.length() - 1.0).abs() < 1e-4);
                    assert!(t.dot(n).abs() < 1e-4);
                    assert!(w == 1.0 || w == -1.0);
                    assert!(t.dot(u_dir) > 0.0, "{mesher:?}: tangent opposes u");
                    assert!(n.cross(t).dot(v_dir) * w >= 0.0, "bitangent opposes v");
                    vertices += 1;
                    if w < 0.0 {
                        flipped += 1;
                    }
                }
            }
            // Negative w corrects the bitangent when a smooth vertex normal opposes
            // its projection face. For these sample rocks that should be rare.
            assert!(
                flipped * 100 < vertices,
                "{mesher:?}: {flipped} of {vertices}"
            );
        }
    }

    #[test]
    fn empty_mesh_gives_empty_output() {
        assert_eq!(
            box_projection(&StoneMesh::default(), 1.0),
            UvMesh::default()
        );
    }
}
