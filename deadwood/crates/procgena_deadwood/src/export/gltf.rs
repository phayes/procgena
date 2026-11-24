//! glTF 2.0 scenes, the common format for exchanging 3D models: a single
//! binary `.glb` file, or a `.gltf` text file with a separate `.bin` file.
//!
//! Each mesh keeps its positions, normals, texture coordinates, tangents
//! and vertex colours, plus the wood values of [`crate::MeshBuffers::wood`]
//! as a custom `_WOOD` attribute. Texture coordinates are flipped
//! vertically (`v` becomes `1 - v`), because glTF puts `v = 0` at the top
//! of an image.
//!
//! Round stems, strips and leaf cards are separate primitives (separately
//! drawn pieces of one mesh); the strips' and leaves' materials are marked
//! double-sided. Textures are not embedded: give the `deadwood_leaves`
//! material the leaf texture, with alpha testing, in your tool. The piece's
//! settings are stored in the file (under `asset.extras.deadwoodOptions`),
//! so the piece can be recreated later.

use std::collections::BTreeMap;
use std::fmt;

use gltf_json as json;
use json::accessor::{ComponentType, GenericComponentType, Type};
use json::buffer::Target;
use json::extras::RawValue;
use json::material::{PbrBaseColorFactor, PbrMetallicRoughness, StrengthFactor};
use json::mesh::{Mode, Semantic};
use json::validation::Checked::Valid;
use json::validation::USize64;
use json::{Index, Root};

use super::{level_name, mesh_has_leaves, strip_buffers};
use crate::mesh::{DeadwoodMesh, MeshBuffers};
use crate::options::DeadwoodOptions;

/// The contents of a `.gltf` file and of the `.bin` file it refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GltfFiles {
    /// The `.gltf` text.
    pub json: String,
    /// The `.bin` contents.
    pub bin: Vec<u8>,
}

/// Why a glTF export failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum GltfError {
    /// No meshes were given.
    NoMeshes,
    /// The mesh at this index has no triangles.
    EmptyMesh(usize),
    /// The GLB would be larger than 4 GiB.
    TooLarge,
    /// The scene description could not be written as JSON. This is not
    /// expected to happen.
    Json(serde_json::Error),
}

impl fmt::Display for GltfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GltfError::NoMeshes => write!(f, "no meshes to export"),
            GltfError::EmptyMesh(i) => write!(f, "mesh {i} has no triangles"),
            GltfError::TooLarge => write!(f, "the GLB file would exceed 4 GiB"),
            GltfError::Json(e) => write!(f, "could not write glTF JSON: {e}"),
        }
    }
}

impl std::error::Error for GltfError {}

/// Creates the contents of a `.glb` file with one object per mesh in
/// `meshes`, without writing it. `options` is stored in the file, and its
/// surface roughness is used for the materials. The materials are plain
/// white, so the vertex colours show unchanged.
///
/// # Errors
///
/// Returns [`GltfError::NoMeshes`] for an empty slice,
/// [`GltfError::EmptyMesh`] for a mesh without triangles,
/// [`GltfError::TooLarge`] if the file would exceed 4 GiB, and
/// [`GltfError::Json`] if the scene can't be written as JSON.
pub fn to_glb(meshes: &[DeadwoodMesh], options: &DeadwoodOptions) -> Result<Vec<u8>, GltfError> {
    let (root, bin) = build(meshes, options, None)?;
    let json = json::serialize::to_vec(&root).map_err(GltfError::Json)?;
    glb(json, bin)
}

/// Creates the contents of a `.gltf` file and its `.bin` file with one
/// object per mesh in `meshes`, without writing them. `options` is stored
/// in the file, and its surface roughness is used for the materials.
///
/// `bin_uri` is where the `.gltf` file will look for the binary data, such
/// as `"log.bin"`; save [`GltfFiles::bin`] there, relative to the `.gltf`
/// file.
///
/// # Errors
///
/// Returns [`GltfError::NoMeshes`] for an empty slice,
/// [`GltfError::EmptyMesh`] for a mesh without triangles, and
/// [`GltfError::Json`] if the scene can't be written as JSON.
pub fn to_gltf(
    meshes: &[DeadwoodMesh],
    options: &DeadwoodOptions,
    bin_uri: &str,
) -> Result<GltfFiles, GltfError> {
    let (root, bin) = build(meshes, options, Some(bin_uri))?;
    let json = json::serialize::to_string_pretty(&root).map_err(GltfError::Json)?;
    Ok(GltfFiles { json, bin })
}

/// Builds the scene description and its binary data. With `bin_uri`, the
/// scene refers to a separate binary file there; with `None`, the data is
/// meant to go inside a `.glb`.
fn build(
    meshes: &[DeadwoodMesh],
    options: &DeadwoodOptions,
    bin_uri: Option<&str>,
) -> Result<(Root, Vec<u8>), GltfError> {
    if meshes.is_empty() {
        return Err(GltfError::NoMeshes);
    }
    if let Some(i) = meshes.iter().position(|m| m.triangle_count() == 0) {
        return Err(GltfError::EmptyMesh(i));
    }
    let mut b = Builder {
        root: Root::default(),
        bin: Vec::new(),
    };
    let options_json = serde_json::to_string(options).map_err(GltfError::Json)?;
    let extras = format!("{{\"deadwoodOptions\":{options_json}}}");
    b.root.asset = json::Asset {
        generator: Some(format!("procgena_deadwood {}", env!("CARGO_PKG_VERSION"))),
        extras: Some(RawValue::from_string(extras).map_err(GltfError::Json)?),
        ..Default::default()
    };
    b.root.buffers.push(json::Buffer {
        byte_length: USize64(0),
        name: None,
        uri: bin_uri.map(str::to_owned),
        extensions: None,
        extras: None,
    });

    // One object per level: round stems one-sided, strips two-sided.
    let solid = b.material("deadwood", options.material.roughness, false);
    let strip = b.material("deadwood_strip", options.material.roughness, true);
    let leaf = mesh_has_leaves(meshes).then(|| b.material("deadwood_leaves", 0.6, true));
    let mut nodes = Vec::new();
    for (lod, mesh) in meshes.iter().enumerate() {
        let name = level_name(lod, meshes.len());
        let strips = strip_buffers(mesh);
        let mut primitives = Vec::new();
        if mesh.surfaces.triangle_count() > 0 {
            primitives.push(b.primitive(&mesh.surfaces, solid));
        }
        if let Some(s) = &strips {
            primitives.push(b.primitive(s, strip));
        }
        if let (Some(l), Some(material)) = (&mesh.leaves, leaf) {
            primitives.push(b.primitive(l, material));
        }
        let index = b.root.push(json::Mesh {
            name: Some(name.clone()),
            primitives,
            weights: None,
            extensions: None,
            extras: None,
        });
        nodes.push(b.root.push(json::Node {
            name: Some(name),
            mesh: Some(index),
            ..Default::default()
        }));
    }
    let scene = b.root.push(json::Scene {
        name: Some("deadwood".to_owned()),
        nodes,
        extensions: None,
        extras: None,
    });
    b.root.scene = Some(scene);

    pad(&mut b.bin, 0);
    b.root.buffers[0].byte_length = USize64(b.bin.len() as u64);
    Ok((b.root, b.bin))
}

/// Packs the scene JSON and binary data into one `.glb` file.
///
/// Returns [`GltfError::TooLarge`] if the file would exceed 4 GiB, the
/// format's limit.
fn glb(mut json: Vec<u8>, mut bin: Vec<u8>) -> Result<Vec<u8>, GltfError> {
    pad(&mut json, b' ');
    pad(&mut bin, 0);
    let total =
        u32::try_from(12 + 8 + json.len() + 8 + bin.len()).map_err(|_| GltfError::TooLarge)?;
    let mut out = Vec::with_capacity(total as usize);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&total.to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    Ok(out)
}

/// Adds `fill` bytes until the length is a multiple of four, as glTF
/// requires.
fn pad(bytes: &mut Vec<u8>, fill: u8) {
    bytes.resize(bytes.len().next_multiple_of(4), fill);
}

/// Collects the scene description and its binary data as meshes are added.
struct Builder {
    /// The scene description.
    root: Root,
    /// The binary data that the scene's buffer 0 refers to.
    bin: Vec<u8>,
}

impl Builder {
    /// Appends `bytes` to the binary data, starting on a four-byte boundary,
    /// and records the range as a glTF buffer view.
    fn view(&mut self, bytes: &[u8], target: Target) -> Index<json::buffer::View> {
        pad(&mut self.bin, 0);
        let offset = self.bin.len();
        self.bin.extend_from_slice(bytes);
        self.root.push(json::buffer::View {
            buffer: Index::new(0),
            byte_length: USize64(bytes.len() as u64),
            byte_offset: Some(USize64(offset as u64)),
            byte_stride: None,
            name: None,
            target: Some(Valid(target)),
            extensions: None,
            extras: None,
        })
    }

    /// Stores `bytes` and adds a glTF accessor that says how to read them:
    /// `count` elements of the given number type and shape.
    fn accessor(
        &mut self,
        bytes: &[u8],
        count: usize,
        component: ComponentType,
        type_: Type,
        target: Target,
    ) -> Index<json::Accessor> {
        let view = self.view(bytes, target);
        self.root.push(json::Accessor {
            buffer_view: Some(view),
            byte_offset: None,
            count: USize64(count as u64),
            component_type: Valid(GenericComponentType(component)),
            extensions: None,
            extras: None,
            type_: Valid(type_),
            min: None,
            max: None,
            name: None,
            normalized: false,
            sparse: None,
        })
    }

    /// Stores a list of `N`-component vectors; `type_` must match `N`.
    fn floats<const N: usize>(&mut self, data: &[[f32; N]], type_: Type) -> Index<json::Accessor> {
        let bytes: Vec<u8> = data
            .iter()
            .flatten()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        self.accessor(
            &bytes,
            data.len(),
            ComponentType::F32,
            type_,
            Target::ArrayBuffer,
        )
    }

    /// Returns a primitive with every per-vertex value from `b`, drawn with
    /// `material`.
    fn primitive(
        &mut self,
        b: &MeshBuffers,
        material: Index<json::Material>,
    ) -> json::mesh::Primitive {
        // glTF requires the smallest and largest position on each axis.
        let positions = self.floats(&b.positions, Type::Vec3);
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for p in &b.positions {
            for axis in 0..3 {
                min[axis] = min[axis].min(p[axis]);
                max[axis] = max[axis].max(p[axis]);
            }
        }
        let accessor = &mut self.root.accessors[positions.value()];
        accessor.min = Some(json::Value::from(min.to_vec()));
        accessor.max = Some(json::Value::from(max.to_vec()));

        // In glTF, v = 0 is the top of an image, so flip v.
        let uvs: Vec<[f32; 2]> = b.uvs.iter().map(|&[u, v]| [u, 1.0 - v]).collect();
        let mut attributes = BTreeMap::new();
        attributes.insert(Valid(Semantic::Positions), positions);
        attributes.insert(
            Valid(Semantic::Normals),
            self.floats(&b.normals, Type::Vec3),
        );
        attributes.insert(Valid(Semantic::TexCoords(0)), self.floats(&uvs, Type::Vec2));
        attributes.insert(
            Valid(Semantic::Tangents),
            self.floats(&b.tangents, Type::Vec4),
        );
        attributes.insert(
            Valid(Semantic::Colors(0)),
            self.floats(&b.colors, Type::Vec4),
        );
        attributes.insert(
            Valid(Semantic::Extras("WOOD".to_owned())),
            self.floats(&b.wood, Type::Vec4),
        );

        let indices: Vec<u8> = b.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let indices = self.accessor(
            &indices,
            b.indices.len(),
            ComponentType::U32,
            Type::Scalar,
            Target::ElementArrayBuffer,
        );
        json::mesh::Primitive {
            attributes,
            indices: Some(indices),
            material: Some(material),
            mode: Valid(Mode::Triangles),
            targets: None,
            extensions: None,
            extras: None,
        }
    }

    /// Adds a white, non-metallic material, so the vertex colours show
    /// unchanged.
    fn material(
        &mut self,
        name: &str,
        roughness: f32,
        double_sided: bool,
    ) -> Index<json::Material> {
        self.root.push(json::Material {
            name: Some(name.to_owned()),
            double_sided,
            pbr_metallic_roughness: PbrMetallicRoughness {
                base_color_factor: PbrBaseColorFactor([1.0; 4]),
                metallic_factor: StrengthFactor(0.0),
                roughness_factor: StrengthFactor(roughness),
                ..Default::default()
            },
            ..Default::default()
        })
    }
}
