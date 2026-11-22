//! Create Wavefront OBJ geometry and its companion MTL material file.
//!
//! Save both strings returned by [`to_obj`], and make any referenced texture
//! images available beside them or at the specified paths. Each call exports one
//! mesh; export detail levels separately, or use glTF to package them together.

use std::fmt::{self, Write};

use super::{InvalidTextureSize, box_projection, checked_texture_size};
use crate::{ImageMap, MaterialOptions, StoneMesh, StoneOptions};

/// Supplies an image path for the requested [`ImageMap`]. Return `None` to
/// omit that image.
///
/// Use simple paths relative to the saved MTL file and copy the images there
/// yourself. Paths are written unchanged, without escaping or validation.
pub type TexturePathFn<'a> = &'a dyn Fn(ImageMap) -> Option<String>;

/// File references and images to use when calling [`to_obj`].
#[derive(Clone, Copy)]
pub struct ObjOptions<'a> {
    /// Path placed in the OBJ's material-library reference; defaults to `stone.mtl`.
    /// Save [`ObjFiles::mtl`] at that location relative to the OBJ file.
    pub material_library: &'a str,
    /// Supplies paths for colour, normal and combined occlusion/roughness/metalness
    /// images. Defaults to `None` to use material values alone. The callback may
    /// return `None` for individual maps; see [`ImageMap`] for image requirements.
    pub textures: Option<TexturePathFn<'a>>,
}

impl Default for ObjOptions<'_> {
    fn default() -> Self {
        Self {
            material_library: "stone.mtl",
            textures: None,
        }
    }
}

impl fmt::Debug for ObjOptions<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObjOptions")
            .field("material_library", &self.material_library)
            .field("textures", &self.textures.map(|_| "<fn>"))
            .finish()
    }
}

/// The OBJ and MTL text produced by [`to_obj`]. No files have been written yet.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ObjFiles {
    /// Geometry text to write to a file with the `.obj` extension.
    pub obj: String,
    /// Material text to write at [`ObjOptions::material_library`].
    pub mtl: String,
}

/// Converts one mesh into OBJ geometry and MTL material text.
///
/// The material's colour, roughness, metalness and texture size come from
/// [`StoneOptions::material`]; other settings in `options` are not used.
/// The object and material are both named `stone`. Positions and supplied normals
/// are preserved, and [box projection](super) adds texture coordinates. An empty
/// mesh produces no faces. Indices are read in groups of three; an incomplete
/// final group is ignored.
///
/// Colour is written as linear RGB. Roughness (how matte the surface looks)
/// and metalness use the `Pr`, `map_Pr` and `Pm` extensions, which some importers
/// ignore. `map_Pr` requests roughness from the green channel of [`ImageMap::Orm`];
/// ambient occlusion is not exported. Normal images use `map_Bump`, which some
/// tools treat as a height map. You may need to set up these images manually
/// in your modelling tool. Material values other than texture size, and image
/// paths, are not validated.
///
/// # Errors
///
/// Returns [`InvalidTextureSize`] if the material's
/// [`texture_size`](MaterialOptions::texture_size) is not finite and
/// greater than zero.
///
/// # Panics
///
/// Panics if an index in a complete triangle refers to a missing position or normal.
pub fn to_obj(
    mesh: &StoneMesh,
    options: &StoneOptions,
    obj: &ObjOptions<'_>,
) -> Result<ObjFiles, InvalidTextureSize> {
    let material = &options.material;
    let out = box_projection(mesh, checked_texture_size(material)?);
    let mut text = String::new();
    text.push_str("# Stone exported by procgena_stone\n");
    let _ = writeln!(text, "mtllib {}", obj.material_library);
    text.push_str("o stone\n");

    // The three vertex lists have matching entries, so each face corner can
    // use the same index for its position, texture coordinate, and normal.
    for [x, y, z] in &out.positions {
        let _ = writeln!(text, "v {x} {y} {z}");
    }
    for [u, v] in &out.uvs {
        let _ = writeln!(text, "vt {u} {v}");
    }
    for [x, y, z] in &out.normals {
        let _ = writeln!(text, "vn {x} {y} {z}");
    }

    // OBJ indices start at 1. The smoothing group requests smooth surface shading.
    text.push_str("usemtl stone\ns 1\n");
    for t in out.indices.as_chunks::<3>().0 {
        let [a, b, c] = t.map(|i| i + 1);
        let _ = writeln!(text, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}");
    }

    Ok(ObjFiles {
        obj: text,
        mtl: material_library(material, obj.textures),
    })
}

/// Writes the `stone` material and adds an image reference for each supplied path.
fn material_library(material: &MaterialOptions, textures: Option<TexturePathFn<'_>>) -> String {
    let path = |map| textures.and_then(|f| f(map));
    let [r, g, b] = material.color;
    let mut out = String::from("# Materials exported by procgena_stone\n");
    let _ = write!(
        out,
        "\nnewmtl stone\nKd {r} {g} {b}\nKs 0 0 0\nPr {}\nPm {}\n",
        material.roughness, material.metallic
    );
    if let Some(p) = path(ImageMap::Color) {
        let _ = writeln!(out, "map_Kd {p}");
    }
    if let Some(p) = path(ImageMap::Normal) {
        let _ = writeln!(out, "map_Bump {p}");
    }
    // Ask the importer to read only the green (roughness) channel.
    if let Some(p) = path(ImageMap::Orm) {
        let _ = writeln!(out, "map_Pr -imfchan g {p}");
    }
    out
}
