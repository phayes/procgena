//! Create clump materials: textures projected without texture coordinates,
//! and edges that break up along the texture.

use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Face, ShaderType};
use bevy::shader::ShaderRef;
use procgena_clump::{MaterialOptions, TextureCompression, TextureMap, TextureSet, TextureSize};

/// Fragment shader for clumps in the main pass and in deferred rendering.
pub(crate) const CLUMP_SHADER: &str = "embedded://procgena_clump_bevy/shaders/clump.wesl";
/// Fragment shader for clump depth, shadows and normals in the prepass.
pub(crate) const CLUMP_PREPASS_SHADER: &str =
    "embedded://procgena_clump_bevy/shaders/clump_prepass.wesl";

/// Bevy's [`StandardMaterial`] with textures projected onto the clump along
/// its x, y and z axes, and edges that break up along the texture.
///
/// Meshes need positions and normals. A vertex colour's alpha
/// ([`Mesh::ATTRIBUTE_COLOR`]) sets how fully covered each vertex is, from 0
/// at the clump's edge to 1 at full thickness; [`clump_mesh`](crate::clump_mesh)
/// stores the clump's fade there. With textures, only the texture's raised
/// details are drawn near the edge, so the edge breaks up along the texture
/// instead of ending in a smooth line; shadows match. Without textures, or
/// without vertex colours, the whole mesh is drawn.
///
/// Textures are fixed to the clump, so they move, turn and scale with its
/// entity. The base material's colour and roughness multiply the textures;
/// its own texture slots are not used. Keep its alpha mode as made by
/// [`clump_material`]: thinning the edges needs [`AlphaMode::Mask`].
pub type ClumpMaterial = ExtendedMaterial<StandardMaterial, ClumpExtension>;

/// Adds projected textures and edge breakup to a [`ClumpMaterial`].
///
/// Leave all three images as `None`, with `settings.textured` set to 0, to
/// draw the base material alone. When `textured` is 1, supply all three images.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct ClumpExtension {
    /// Texture repeat size and whether to use the images.
    #[uniform(100)]
    pub settings: ClumpSettings,
    /// Colour image, loaded as sRGB.
    #[texture(101)]
    #[sampler(102)]
    pub color: Option<Handle<Image>>,
    /// Normal map using OpenGL directions, loaded as linear data.
    #[texture(103)]
    #[sampler(104)]
    pub normal: Option<Handle<Image>>,
    /// Combined occlusion, roughness and height image ([`TextureMap::Orh`]),
    /// loaded as linear data. Height also shapes where the edges break up.
    #[texture(105)]
    #[sampler(106)]
    pub orh: Option<Handle<Image>>,
}

/// Texture settings for a [`ClumpExtension`], in the layout the clump shaders use.
#[derive(ShaderType, Reflect, Debug, Clone, Copy, PartialEq)]
pub struct ClumpSettings {
    /// Units of the clump's mesh covered by one repeat of the textures; must
    /// be positive. See [`MaterialOptions::texture_size`].
    pub texture_size: f32,
    /// 1 to use the extension's images, 0 to ignore them.
    pub textured: u32,
}

impl Default for ClumpSettings {
    /// One repeat per unit, without textures.
    fn default() -> Self {
        Self {
            texture_size: 1.0,
            textured: 0,
        }
    }
}

impl MaterialExtension for ClumpExtension {
    fn fragment_shader() -> ShaderRef {
        CLUMP_SHADER.into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        CLUMP_PREPASS_SHADER.into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        CLUMP_SHADER.into()
    }
}

/// Colour, normal and occlusion/roughness/height images for one clump
/// texture set at one resolution. Use these with [`clump_material`].
#[derive(Debug, Clone, PartialEq)]
pub struct ClumpTextures {
    /// Colour image, interpreted as sRGB colour.
    pub color: Handle<Image>,
    /// Normal map using OpenGL directions, in linear colour space.
    pub normal: Handle<Image>,
    /// Combined occlusion, roughness and height image ([`TextureMap::Orh`]),
    /// in linear colour space.
    pub orh: Handle<Image>,
}

impl ClumpTextures {
    /// Requests the images of texture set `set` at `size`.
    ///
    /// `root` is a folder relative to Bevy's asset directory, such as a copy
    /// of the repository's `clump/assets/textures` folder. Files are loaded
    /// from `<root>/<path>`, where `path` comes from [`TextureSet::path`].
    /// UASTC files need Bevy's `basis-universal` feature. Images repeat and
    /// load in the background; the returned handles do not mean loading is
    /// complete.
    pub fn load(
        asset_server: &AssetServer,
        root: &str,
        set: &TextureSet,
        size: TextureSize,
        compression: TextureCompression,
    ) -> Self {
        let load = |map: TextureMap| {
            let path = set.path(size, map, compression);
            load_repeating(
                asset_server,
                format!("{root}/{path}"),
                map == TextureMap::Color,
            )
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

/// Creates a clump material from `material` and optional textures.
///
/// Supply the textures of [`MaterialOptions::texture`]. Without them the
/// clump uses [`MaterialOptions::color`] and [`MaterialOptions::roughness`]
/// alone. The texture size is measured in the units of the clump's mesh,
/// which are metres for meshes made by [`ClumpPlugin`](crate::ClumpPlugin).
pub fn clump_material(
    material: &MaterialOptions,
    textures: Option<&ClumpTextures>,
) -> ClumpMaterial {
    let [r, g, b] = material.color;
    let base = StandardMaterial {
        base_color: Color::linear_rgb(r, g, b),
        perceptual_roughness: material.roughness,
        cull_mode: Some(Face::Back),
        alpha_mode: AlphaMode::Mask(0.5),
        ..default()
    };
    let extension = ClumpExtension {
        settings: ClumpSettings {
            texture_size: material.texture_size,
            textured: u32::from(textures.is_some()),
        },
        color: textures.map(|t| t.color.clone()),
        normal: textures.map(|t| t.normal.clone()),
        orh: textures.map(|t| t.orh.clone()),
    };
    ClumpMaterial { base, extension }
}
