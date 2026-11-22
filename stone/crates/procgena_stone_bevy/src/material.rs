//! Create rock materials that project textures onto rocks without texture coordinates.

use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Face, ShaderType};
use bevy::shader::ShaderRef;
use procgena_stone::{MaterialOptions, TextureCompression, TextureMap, TextureSize, texture_path};

/// Fragment shader for rocks in the main pass and in deferred rendering.
pub(crate) const STONE_SHADER: &str = "embedded://procgena_stone_bevy/shaders/stone.wesl";
/// Fragment shader that writes textured rock normals to the normal prepass.
pub(crate) const STONE_PREPASS_SHADER: &str =
    "embedded://procgena_stone_bevy/shaders/stone_prepass.wesl";

/// Bevy's [`StandardMaterial`] with textures projected onto the rock along its
/// x, y and z axes ("triplanar" texturing).
///
/// Meshes need positions and normals only, as made by
/// [`stone_mesh`](crate::stone_mesh). Textures are fixed to the rock, so they
/// move, turn and scale with its entity. The base material's colour and
/// roughness multiply the textures; its own texture slots are not used.
pub type StoneMaterial = ExtendedMaterial<StandardMaterial, TriplanarExtension>;

/// Adds surface colour, bumps, shading in hollows and varying roughness to a
/// [`StoneMaterial`] without requiring texture coordinates on the mesh.
///
/// Leave all three images as `None`, with `settings.textured` set to 0, to draw
/// the base material alone. When `textured` is 1, supply all three images.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct TriplanarExtension {
    /// Texture repeat size and whether to use the images.
    #[uniform(100)]
    pub settings: TriplanarSettings,
    /// Colour image, loaded as sRGB.
    #[texture(101)]
    #[sampler(102)]
    pub color: Option<Handle<Image>>,
    /// Normal map using OpenGL directions, loaded as linear data.
    #[texture(103)]
    #[sampler(104)]
    pub normal: Option<Handle<Image>>,
    /// Combined occlusion, roughness and height image ([`TextureMap::Orh`]).
    /// Supply these values in the red, green and blue channels and load as
    /// linear data. Height helps blend textures where projections meet;
    /// it does not change the mesh.
    #[texture(105)]
    #[sampler(106)]
    pub orh: Option<Handle<Image>>,
}

/// Texture settings for a [`TriplanarExtension`], in the layout the rock shaders use.
#[derive(ShaderType, Reflect, Debug, Clone, Copy, PartialEq)]
pub struct TriplanarSettings {
    /// Units of the rock's mesh covered by one repeat of the textures; must be
    /// positive. See [`MaterialOptions::texture_size`].
    pub texture_size: f32,
    /// 1 to use the extension's images, 0 to ignore them.
    pub textured: u32,
}

impl Default for TriplanarSettings {
    /// One repeat per unit, without textures.
    fn default() -> Self {
        Self {
            texture_size: 1.0,
            textured: 0,
        }
    }
}

impl MaterialExtension for TriplanarExtension {
    fn fragment_shader() -> ShaderRef {
        STONE_SHADER.into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        STONE_PREPASS_SHADER.into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        STONE_SHADER.into()
    }
}

/// Colour, normal and occlusion/roughness/height images for one rock texture
/// set at one resolution. Use these with [`stone_material`].
#[derive(Debug, Clone, PartialEq)]
pub struct StoneTextures {
    /// Colour image, interpreted as sRGB colour.
    pub color: Handle<Image>,
    /// Normal map using OpenGL directions, in linear colour space.
    pub normal: Handle<Image>,
    /// Combined occlusion, roughness and height image ([`TextureMap::Orh`]).
    /// Its red, green and blue channels must be loaded as linear data,
    /// without sRGB colour correction.
    pub orh: Handle<Image>,
}

impl StoneTextures {
    /// Requests the images of texture set `set` at `size`.
    ///
    /// `root` is a folder relative to Bevy's asset directory, such as a copy of
    /// the repository's `assets/textures` folder. Files are loaded from
    /// `<root>/<path>`, where `path` comes from [`texture_path`]. UASTC files
    /// need Bevy's `basis-universal` feature. Images repeat and load in the
    /// background; the returned handles do not mean loading is complete.
    pub fn load(
        asset_server: &AssetServer,
        root: &str,
        set: &str,
        size: TextureSize,
        compression: TextureCompression,
    ) -> Self {
        let load = |map: TextureMap| {
            let path = texture_path(set, size, map, compression);
            load_repeating(asset_server, format!("{root}/{path}"), map.is_srgb())
        };
        Self {
            color: load(TextureMap::Color),
            normal: load(TextureMap::Normal),
            orh: load(TextureMap::Orh),
        }
    }
}

/// Requests an image with linear filtering and repetition in both texture directions.
fn load_repeating(asset_server: &AssetServer, path: String, srgb: bool) -> Handle<Image> {
    asset_server
        .load_builder()
        .with_settings(move |settings: &mut ImageLoaderSettings| {
            settings.is_srgb = srgb;
            settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                address_mode_u: ImageAddressMode::Repeat,
                address_mode_v: ImageAddressMode::Repeat,
                ..ImageSamplerDescriptor::linear()
            });
        })
        .load(path)
}

/// Creates a rock material from `material` and optional textures.
///
/// Supply textures for [`MaterialOptions::texture`]. Without them the rock
/// uses [`MaterialOptions::color`], [`MaterialOptions::roughness`] and
/// [`MaterialOptions::metallic`] alone.
/// The texture size is measured in the units of the rock's mesh, which are
/// metres for meshes made by [`StonePlugin`](crate::StonePlugin).
pub fn stone_material(
    material: &MaterialOptions,
    textures: Option<&StoneTextures>,
) -> StoneMaterial {
    let [r, g, b] = material.color;
    let base = StandardMaterial {
        base_color: Color::linear_rgb(r, g, b),
        perceptual_roughness: material.roughness,
        metallic: material.metallic,
        cull_mode: Some(Face::Back),
        ..default()
    };
    let extension = TriplanarExtension {
        settings: TriplanarSettings {
            texture_size: material.texture_size,
            textured: u32::from(textures.is_some()),
        },
        color: textures.map(|t| t.color.clone()),
        normal: textures.map(|t| t.normal.clone()),
        orh: textures.map(|t| t.orh.clone()),
    };
    StoneMaterial { base, extension }
}
