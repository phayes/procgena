//! Create glTF 2.0 scenes for 3D viewers and modelling tools.
//!
//! [`to_glb`] returns binary `.glb` contents. [`to_gltf`] returns JSON `.gltf`
//! text and a separate `.bin` payload. Your application saves these files.
//! Both use one opaque `stone` material, with optional textures.
//!
//! Supply one mesh or several levels of detail (simpler versions for distant
//! viewing). Every level is included in the scene, so a viewer may display them
//! all together; choose a level or set up switching in the receiving tool.
//! Generation settings are saved in `asset.extras.stoneOptions` for later reuse.

use std::collections::BTreeMap;
use std::fmt;

use gltf_json as json;
use json::accessor::{ComponentType, GenericComponentType, Type};
use json::buffer::Target;
use json::extras::RawValue;
use json::material::{
    NormalTexture, OcclusionTexture, PbrBaseColorFactor, PbrMetallicRoughness, StrengthFactor,
};
use json::mesh::{Mode, Semantic};
use json::texture::{MagFilter, MinFilter, WrappingMode};
use json::validation::Checked::Valid;
use json::validation::USize64;
use json::{Index, Root};

use super::{InvalidTextureSize, UvMesh, box_projection, checked_texture_size};
use crate::{ImageMap, MaterialOptions, StoneMesh, StoneOptions};

/// Supplies the location of a PNG or JPEG for the requested [`ImageMap`].
/// Return `None` to omit that image.
///
/// Relative paths are resolved from the saved `.gltf` or `.glb` file. Make the
/// images available there yourself; the exporter neither copies nor checks them.
pub type TextureUriFn<'a> = &'a dyn Fn(ImageMap) -> Option<String>;

/// Supplies PNG or JPEG file bytes to include in the export for the requested
/// [`ImageMap`]. Return `None` to omit that image.
pub type TextureImageFn<'a> = &'a dyn Fn(ImageMap) -> Option<EmbeddedImage>;

/// Where the exported material gets its texture images.
#[derive(Clone, Copy, Default)]
pub enum GltfTextures<'a> {
    /// Uses only the material's colour and roughness values; the default.
    #[default]
    None,
    /// Links to PNG or JPEG images using the callback's URIs.
    /// Keep these images accessible when opening the exported scene.
    Uri(TextureUriFn<'a>),
    /// Stores the callback's images in the binary payload: inside a GLB, or
    /// in the separate `.bin` file returned by [`to_gltf`].
    Embed(TextureImageFn<'a>),
}

impl fmt::Debug for GltfTextures<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::None => "None",
            Self::Uri(_) => "Uri(<fn>)",
            Self::Embed(_) => "Embed(<fn>)",
        })
    }
}

/// An image file to include in the export without a separate image reference.
///
/// Bytes are copied as supplied. The exporter neither decodes the image nor
/// checks that its contents match the declared format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedImage {
    /// The entire encoded PNG or JPEG file, including its header, rather than raw pixels.
    pub bytes: Vec<u8>,
    /// The encoding used by [`Self::bytes`]; the caller must supply the correct format.
    pub format: ImageFormat,
}

/// Image file formats accepted for embedding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// PNG encoding, which preserves image values without lossy compression.
    Png,
    /// JPEG encoding, which uses lossy compression.
    Jpeg,
}

impl ImageFormat {
    /// Returns the MIME label that tells a loader how the image bytes are encoded.
    fn mime_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
        }
    }
}

/// Images for the material shared by all meshes in an export.
#[derive(Debug, Clone, Copy, Default)]
pub struct GltfOptions<'a> {
    /// How to obtain images. The default, [`GltfTextures::None`], uses no images.
    pub textures: GltfTextures<'a>,
}

/// Scene text and binary contents to save as the two files requested by [`to_gltf`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GltfFiles {
    /// The scene description, ready to save with a `.gltf` extension.
    pub json: String,
    /// Geometry and any embedded images. Save these bytes at the `bin_uri`
    /// passed to [`to_gltf`]; a relative URI is relative to the `.gltf` file.
    pub bin: Vec<u8>,
}

/// An error reported while assembling or serializing an export.
///
/// These errors do not cover every invalid input: mesh indices, material values
/// other than the texture size, image contents, and URIs are not validated.
#[derive(Debug)]
#[non_exhaustive]
pub enum GltfError {
    /// No meshes were supplied. Export at least one mesh.
    NoMeshes,
    /// The mesh at this zero-based slice index has fewer than three indices.
    /// This exporter requires at least one triangle per mesh; a fully carved-away
    /// rock can produce an empty mesh.
    EmptyMesh(usize),
    /// The material's [`texture_size`](MaterialOptions::texture_size) is not
    /// finite and greater than zero.
    InvalidTextureSize(InvalidTextureSize),
    /// The padded GLB file length does not fit in its 32-bit size field.
    /// The limit is 4 GiB minus one byte; this check applies only to GLB output.
    TooLarge,
    /// Creating the scene JSON or its generation-settings metadata failed.
    Json(serde_json::Error),
}

impl fmt::Display for GltfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoMeshes => f.write_str("no meshes to export"),
            Self::EmptyMesh(lod) => write!(f, "mesh {lod} has no triangles"),
            Self::InvalidTextureSize(err) => err.fmt(f),
            Self::TooLarge => f.write_str("the exported data is larger than 4 GiB"),
            Self::Json(err) => write!(f, "could not write glTF JSON: {err}"),
        }
    }
}

impl std::error::Error for GltfError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(err) => Some(err),
            Self::InvalidTextureSize(err) => Some(err),
            _ => None,
        }
    }
}

/// Creates binary `.glb` contents from one or more rock meshes without saving a file.
///
/// Supply at least one mesh. For levels of detail, pass the most detailed first,
/// followed by simpler versions; [`Stone::levels`](crate::Stone::levels)
/// provides these. One mesh is named `stone`; multiple meshes are named
/// `stone_LOD0`, `stone_LOD1`, and so on, in input order. All are scene nodes:
/// these names do not set up automatic switching or hide the simpler versions.
///
/// Pass the rock's generation settings as `options` to store them as metadata;
/// they do not regenerate or modify the supplied meshes. The shared material
/// uses [`StoneOptions::material`]. See [`to_gltf`] for the
/// exported vertex data. Use [`GltfTextures::Embed`] to include images in the GLB;
/// URI-based images remain external references.
///
/// # Errors
///
/// Returns [`GltfError::NoMeshes`] for an empty slice, [`GltfError::EmptyMesh`]
/// when any mesh has fewer than three indices, [`GltfError::InvalidTextureSize`]
/// for a texture size that is not finite and greater than zero,
/// [`GltfError::TooLarge`] when the padded file length exceeds 4 GiB minus one
/// byte, or [`GltfError::Json`] if creating the JSON fails. Other material values
/// and images are not validated.
///
/// # Panics
///
/// Panics if an index in a complete triangle refers to a missing position or normal.
pub fn to_glb(
    meshes: &[StoneMesh],
    options: &StoneOptions,
    gltf: &GltfOptions<'_>,
) -> Result<Vec<u8>, GltfError> {
    let (root, bin) = build(meshes, options, gltf, None)?;
    let json = json::serialize::to_vec(&root).map_err(GltfError::Json)?;
    glb(json, bin)
}

/// Creates a JSON `.gltf` scene and its separate binary contents without writing files.
///
/// Set `bin_uri` to the location where the binary data will be saved, usually a
/// relative filename such as `stone.bin`. Save [`GltfFiles::bin`] there, relative
/// to the `.gltf` file. Embedded images go in that binary file too. Mesh naming,
/// detail levels, and saved generation settings work as described in [`to_glb`].
///
/// Each mesh includes positions, supplied normals, [box-projected](super) texture
/// coordinates, and tangents (surface directions used to light normal-map bumps).
/// The exporter flips v to `1 - v` for glTF's top-left image origin while retaining
/// the tangent frame's image-up direction. Complete triangles are preserved;
/// any trailing one or two indices are ignored.
///
/// All meshes share an opaque material. Supply [`ImageMap::Orm`] to add shading
/// in cracks and hollows (ambient occlusion), vary how matte or polished the
/// surface looks (roughness) and mark metal areas (metalness, which multiplies
/// [`MaterialOptions::metallic`]). Only the material's texture size
/// is validated; other material values, images and URIs are not checked.
///
/// # Errors
///
/// Returns [`GltfError::NoMeshes`] for an empty slice, [`GltfError::EmptyMesh`]
/// when any mesh has fewer than three indices, [`GltfError::InvalidTextureSize`]
/// for a texture size that is not finite and greater than zero, or
/// [`GltfError::Json`] if creating the JSON fails. Unlike GLB output, this path has no 32-bit file-size check.
///
/// # Panics
///
/// Panics if an index in a complete triangle refers to a missing position or normal.
pub fn to_gltf(
    meshes: &[StoneMesh],
    options: &StoneOptions,
    gltf: &GltfOptions<'_>,
    bin_uri: &str,
) -> Result<GltfFiles, GltfError> {
    let (root, bin) = build(meshes, options, gltf, Some(bin_uri))?;
    let json = json::serialize::to_string_pretty(&root).map_err(GltfError::Json)?;
    Ok(GltfFiles { json, bin })
}

/// Builds the shared scene and binary data for both export formats.
/// A `bin_uri` links the scene to a separate file; `None` keeps the data in a GLB.
/// Rejects missing meshes or meshes with no complete index triple before building.
fn build(
    meshes: &[StoneMesh],
    options: &StoneOptions,
    gltf: &GltfOptions<'_>,
    bin_uri: Option<&str>,
) -> Result<(Root, Vec<u8>), GltfError> {
    if meshes.is_empty() {
        return Err(GltfError::NoMeshes);
    }
    if let Some(lod) = meshes.iter().position(|m| m.triangle_count() == 0) {
        return Err(GltfError::EmptyMesh(lod));
    }
    let texture_size =
        checked_texture_size(&options.material).map_err(GltfError::InvalidTextureSize)?;
    let mut b = Builder {
        root: Root::default(),
        bin: Vec::new(),
        textures: gltf.textures,
    };

    // Store the supplied generation settings so applications can recover them later.
    let options_json = serde_json::to_string(options).map_err(GltfError::Json)?;
    let extras = format!("{{\"stoneOptions\":{options_json}}}");
    b.root.asset = json::Asset {
        generator: Some(format!("procgena_stone {}", env!("CARGO_PKG_VERSION"))),
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

    // Add every detail level to the scene; choosing which to show is the viewer's job.
    let material = b.material(&options.material);
    let mut nodes = Vec::new();
    for (lod, mesh) in meshes.iter().enumerate() {
        let name = if meshes.len() > 1 {
            format!("stone_LOD{lod}")
        } else {
            "stone".to_owned()
        };
        let uv_mesh = box_projection(mesh, texture_size);
        let mesh_index = b.mesh(&name, &uv_mesh, material);
        nodes.push(b.root.push(json::Node {
            name: Some(name),
            mesh: Some(mesh_index),
            ..Default::default()
        }));
    }
    let scene = b.root.push(json::Scene {
        name: Some("stone".to_owned()),
        nodes,
        extensions: None,
        extras: None,
    });
    b.root.scene = Some(scene);

    pad(&mut b.bin, 0);
    b.root.buffers[0].byte_length = USize64(b.bin.len() as u64);
    Ok((b.root, b.bin))
}

/// Wraps JSON and binary data in GLB headers and checks that the padded total fits u32.
fn glb(mut json: Vec<u8>, mut bin: Vec<u8>) -> Result<Vec<u8>, GltfError> {
    // GLB chunks must end on four-byte boundaries. JSON permits trailing spaces;
    // unused binary padding is filled with zeros.
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

/// Adds up to three `fill` bytes so the next data starts on a four-byte boundary.
fn pad(bytes: &mut Vec<u8>, fill: u8) {
    bytes.resize(bytes.len().next_multiple_of(4), fill);
}

/// Collects the scene description and binary payload as meshes and images are added.
struct Builder<'a> {
    root: Root,
    bin: Vec<u8>,
    textures: GltfTextures<'a>,
}

impl Builder<'_> {
    /// Stores bytes and adds a buffer view: a byte range within the shared binary payload.
    /// The range begins on a four-byte boundary. `target` identifies vertex or index
    /// data when applicable; image data has no such target.
    fn view(&mut self, bytes: &[u8], target: Option<Target>) -> Index<json::buffer::View> {
        pad(&mut self.bin, 0);
        let offset = self.bin.len();
        self.bin.extend_from_slice(bytes);
        self.root.push(json::buffer::View {
            buffer: Index::new(0),
            byte_length: USize64(bytes.len() as u64),
            byte_offset: Some(USize64(offset as u64)),
            byte_stride: None,
            name: None,
            target: target.map(Valid),
            extensions: None,
            extras: None,
        })
    }

    /// Stores bytes and describes how a loader should read them using a glTF accessor.
    /// An accessor specifies the number of elements, numeric encoding, and shape
    /// (such as a three-component vector). The caller must supply matching bytes;
    /// this method does not validate their length.
    fn accessor(
        &mut self,
        bytes: &[u8],
        count: usize,
        component: ComponentType,
        type_: Type,
        target: Option<Target>,
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

    /// Encodes floating-point vectors in glTF's little-endian byte order.
    /// The caller must choose a vector shape `type_` that matches the `N` components.
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
            Some(Target::ArrayBuffer),
        )
    }

    /// Stores positions and their per-axis bounds so loaders can find the mesh's extent.
    /// Expects nonempty, finite position data; it does not validate that requirement.
    fn positions(&mut self, data: &[[f32; 3]]) -> Index<json::Accessor> {
        let index = self.floats(data, Type::Vec3);
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for p in data {
            for axis in 0..3 {
                min[axis] = min[axis].min(p[axis]);
                max[axis] = max[axis].max(p[axis]);
            }
        }
        let accessor = &mut self.root.accessors[index.value()];
        accessor.min = Some(json::Value::from(min.to_vec()));
        accessor.max = Some(json::Value::from(max.to_vec()));
        index
    }

    /// Adds a triangle mesh with positions, normals, texture coordinates, and tangents.
    /// Expects nonempty vertex data and valid indices, and assigns the given material.
    fn mesh(
        &mut self,
        name: &str,
        mesh: &UvMesh,
        material: Index<json::Material>,
    ) -> Index<json::Mesh> {
        // Convert upward-increasing v to glTF's downward-increasing image coordinate.
        // Keep the tangent frame unchanged: its bitangent represents image-up for
        // OpenGL-style normal maps, which is decreasing v after this conversion.
        let uvs: Vec<[f32; 2]> = mesh.uvs.iter().map(|&[u, v]| [u, 1.0 - v]).collect();
        let mut attributes = BTreeMap::new();
        attributes.insert(Valid(Semantic::Positions), self.positions(&mesh.positions));
        attributes.insert(
            Valid(Semantic::Normals),
            self.floats(&mesh.normals, Type::Vec3),
        );
        attributes.insert(Valid(Semantic::TexCoords(0)), self.floats(&uvs, Type::Vec2));
        attributes.insert(
            Valid(Semantic::Tangents),
            self.floats(&mesh.tangents, Type::Vec4),
        );

        let indices: Vec<u8> = mesh.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let indices = self.accessor(
            &indices,
            mesh.indices.len(),
            ComponentType::U32,
            Type::Scalar,
            Some(Target::ElementArrayBuffer),
        );
        self.root.push(json::Mesh {
            name: Some(name.to_owned()),
            primitives: vec![json::mesh::Primitive {
                attributes,
                indices: Some(indices),
                material: Some(material),
                mode: Valid(Mode::Triangles),
                targets: None,
                extensions: None,
                extras: None,
            }],
            weights: None,
            extensions: None,
            extras: None,
        })
    }

    /// Creates the opaque `stone` material and attaches available images.
    /// All detail levels use this same material.
    fn material(&mut self, material: &MaterialOptions) -> Index<json::Material> {
        // Use the same image for occlusion in red and roughness in green.
        let color = self.texture(ImageMap::Color);
        let orm = self.texture(ImageMap::Orm);
        let normal = self.texture(ImageMap::Normal);

        let [r, g, b] = material.color;
        self.root.push(json::Material {
            name: Some("stone".to_owned()),
            pbr_metallic_roughness: PbrMetallicRoughness {
                base_color_factor: PbrBaseColorFactor([r, g, b, 1.0]),
                base_color_texture: color.map(info),
                metallic_factor: StrengthFactor(material.metallic),
                roughness_factor: StrengthFactor(material.roughness),
                metallic_roughness_texture: orm.map(info),
                extensions: None,
                extras: None,
            },
            occlusion_texture: orm.map(|index| OcclusionTexture {
                index,
                strength: StrengthFactor(1.0),
                tex_coord: 0,
                extensions: None,
                extras: None,
            }),
            normal_texture: normal.map(|index| NormalTexture {
                index,
                scale: 1.0,
                tex_coord: 0,
                extensions: None,
                extras: None,
            }),
            ..Default::default()
        })
    }

    /// Adds an image and a texture that repeats it in both directions.
    /// Returns `None` when textures are disabled or the callback omits this map.
    /// The sampler requests smooth filtering and mipmaps for distant views.
    fn texture(&mut self, map: ImageMap) -> Option<Index<json::Texture>> {
        let image = match self.textures {
            GltfTextures::None => return None,
            GltfTextures::Uri(uri) => json::Image {
                uri: Some(uri(map)?),
                buffer_view: None,
                mime_type: None,
                name: Some(texture_name(map)),
                extensions: None,
                extras: None,
            },
            GltfTextures::Embed(embed) => {
                let image = embed(map)?;
                json::Image {
                    uri: None,
                    buffer_view: Some(self.view(&image.bytes, None)),
                    mime_type: Some(json::image::MimeType(image.format.mime_type().to_owned())),
                    name: Some(texture_name(map)),
                    extensions: None,
                    extras: None,
                }
            }
        };
        let source = self.root.push(image);
        let sampler = self.root.push(json::texture::Sampler {
            mag_filter: Some(Valid(MagFilter::Linear)),
            min_filter: Some(Valid(MinFilter::LinearMipmapLinear)),
            wrap_s: Valid(WrappingMode::Repeat),
            wrap_t: Valid(WrappingMode::Repeat),
            name: None,
            extensions: None,
            extras: None,
        });
        Some(self.root.push(json::Texture {
            name: Some(texture_name(map)),
            sampler: Some(sampler),
            source,
            extensions: None,
            extras: None,
        }))
    }
}

/// Assigns a texture to a material input using texture-coordinate set zero.
fn info(index: Index<json::Texture>) -> json::texture::Info {
    json::texture::Info {
        index,
        tex_coord: 0,
        extensions: None,
        extras: None,
    }
}

/// Names a texture and its image by their role, for example `stone_normal`.
fn texture_name(map: ImageMap) -> String {
    format!("stone_{}", map.as_str())
}
