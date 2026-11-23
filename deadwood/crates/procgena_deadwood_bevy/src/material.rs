//! The dead wood material: Bevy's standard lighting plus vertex colours,
//! growth rings and cracks on ends, grain on bare wood, bark images, and
//! thin stems drawn as flat strips that turn toward the camera.

use std::f32::consts::PI;

use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use procgena_deadwood::{
    DeadwoodOptions, END_TEXTURE_SETS, LeafMap, TextureCompression, TextureMap, TextureSize,
    bark_texture_path, end_texture_path, leaf_texture_path,
};

use crate::mesh::{ATTRIBUTE_BILLBOARD_AXIS, ATTRIBUTE_BILLBOARD_PIVOT};

/// Vertex shader for the main (visible) pass.
pub(crate) const VERTEX_SHADER: &str =
    "embedded://procgena_deadwood_bevy/shaders/deadwood_vertex.wesl";
/// Vertex shader for depth, shadow and deferred passes.
pub(crate) const PREPASS_SHADER: &str =
    "embedded://procgena_deadwood_bevy/shaders/deadwood_prepass.wesl";
/// Fragment shader for the main and deferred passes.
pub(crate) const FRAGMENT_SHADER: &str =
    "embedded://procgena_deadwood_bevy/shaders/deadwood_fragment.wesl";

/// Shader input slot for [`ATTRIBUTE_BILLBOARD_PIVOT`], after the slots
/// Bevy uses for its own vertex attributes.
const BILLBOARD_PIVOT_LOCATION: u32 = 9;
/// Shader input slot for [`ATTRIBUTE_BILLBOARD_AXIS`].
const BILLBOARD_AXIS_LOCATION: u32 = 10;

/// Bevy's [`StandardMaterial`] with dead wood surface details added.
///
/// Use it with meshes from [`deadwood_mesh`](crate::deadwood_mesh) or
/// [`ribbon_mesh`](crate::ribbon_mesh). Vertex colours multiply the base
/// colour. Cut and broken ends get growth rings and drying cracks, bare
/// wood gets fine grain, and bark can be drawn from bark images. Meshes
/// with both billboard attributes are turned toward whatever is looking at
/// them: the camera, or a light when its shadows are drawn. Create one with
/// [`deadwood_material`].
pub type DeadwoodMaterial = ExtendedMaterial<StandardMaterial, DeadwoodExtension>;

/// The part of a [`DeadwoodMaterial`] that adds dead wood details to the
/// standard material.
///
/// Leave the three bark images as `None`, with
/// [`DeadwoodSurface::bark_textured`] at 0, for vertex colours only. When
/// `bark_textured` is 1, supply all three, with mipmaps: the material reads
/// the colour image's smallest mip level as its average colour.
///
/// Likewise, supply both end-grain images when
/// [`DeadwoodSurface::end_textured`] is 1, and leave them `None` when it is
/// 0.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct DeadwoodExtension {
    /// Ring, crack, grain and bark settings.
    #[uniform(100)]
    pub surface: DeadwoodSurface,
    /// Bark colour image, loaded as sRGB.
    #[texture(101)]
    #[sampler(102)]
    pub bark_color: Option<Handle<Image>>,
    /// Bark normal map in the OpenGL (+Y) convention, loaded as linear data.
    #[texture(103)]
    #[sampler(104)]
    pub bark_normal: Option<Handle<Image>>,
    /// Bark roughness in green (and red and blue), loaded as linear data.
    #[texture(105)]
    #[sampler(106)]
    pub bark_roughness: Option<Handle<Image>>,
    /// End-grain photo, loaded as sRGB, drawn on sawn ends.
    #[texture(107)]
    #[sampler(108)]
    pub end_color: Option<Handle<Image>>,
    /// End-grain normal map in the OpenGL (+Y) convention, loaded as linear
    /// data.
    #[texture(109)]
    #[sampler(110)]
    pub end_normal: Option<Handle<Image>>,
}

/// Ring, crack, grain and bark settings, laid out as the shaders expect.
/// [`deadwood_material`] fills them in from the piece's options.
#[derive(ShaderType, Reflect, Debug, Clone, Copy, PartialEq, Default)]
pub struct DeadwoodSurface {
    /// 1 to draw bark with the extension's bark images, 0 to use vertex
    /// colours only.
    pub bark_textured: u32,
    /// How many times the bark images repeat per unit of the mesh's texture
    /// coordinates, both around and along each stem. Use a whole number so
    /// the bark joins up around the stem.
    pub bark_repeats: f32,
    /// Growth rings per metre of radius on cut and broken ends.
    pub ring_frequency: f32,
    /// How dark the growth rings are, from 0 to 1.
    pub ring_strength: f32,
    /// How many and how dark the drying cracks on ends are, from 0 (none) to
    /// 1.
    pub crack_strength: f32,
    /// How strong the grain streaks on bare wood are, from 0 to 1.
    pub grain_strength: f32,
    /// Any number; different values give different ring wobble and cracks.
    pub seed: f32,
    /// 1 to draw the end-grain photo on sawn ends, 0 for drawn rings only.
    pub end_textured: u32,
    /// Where the photo's pith lies in its texture coordinates; the middle
    /// of each sawn end is drawn from here.
    pub end_centre: Vec2,
    /// Radius around [`end_centre`](Self::end_centre), in texture
    /// coordinates, of a circle that lies within the photo's wood. Each
    /// sawn end's outline is drawn from this circle.
    pub end_radius: f32,
    /// The photo's average linear colour within that circle. Dividing by it
    /// keeps the vertex colour as the end's overall colour.
    pub end_average: Vec3,
}

impl MaterialExtension for DeadwoodExtension {
    fn vertex_shader() -> ShaderRef {
        VERTEX_SHADER.into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        PREPASS_SHADER.into()
    }

    fn deferred_vertex_shader() -> ShaderRef {
        PREPASS_SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        FRAGMENT_SHADER.into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        FRAGMENT_SHADER.into()
    }

    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // For meshes with billboard data, pass it to the shaders and turn on
        // the code that turns them toward the viewer.
        let layout = &layout.0;
        if layout.contains(ATTRIBUTE_BILLBOARD_PIVOT) && layout.contains(ATTRIBUTE_BILLBOARD_AXIS) {
            let billboard = layout.get_layout(&[
                ATTRIBUTE_BILLBOARD_PIVOT.at_shader_location(BILLBOARD_PIVOT_LOCATION),
                ATTRIBUTE_BILLBOARD_AXIS.at_shader_location(BILLBOARD_AXIS_LOCATION),
            ])?;
            descriptor.vertex.buffers[0]
                .attributes
                .extend(billboard.attributes);
            descriptor.vertex.shader_defs.push("BILLBOARD".into());
        }
        Ok(())
    }
}

/// The three images of one bark texture set, for [`deadwood_material`].
#[derive(Debug, Clone, PartialEq)]
pub struct BarkTextures {
    /// Colour image, loaded as sRGB.
    pub color: Handle<Image>,
    /// Normal map in the OpenGL (+Y) convention, loaded as linear data.
    pub normal: Handle<Image>,
    /// Roughness image, loaded as linear data.
    pub roughness: Handle<Image>,
}

impl BarkTextures {
    /// Starts loading bark texture set `set`, one of
    /// [`BARK_TEXTURE_SETS`](procgena_deadwood::BARK_TEXTURE_SETS), at
    /// `size`.
    ///
    /// `root` is a folder relative to Bevy's asset folder that holds a copy
    /// of the repository's `deadwood/assets/textures`; files are read from
    /// `<root>/<path>`, with paths from [`bark_texture_path`]. UASTC files
    /// need Bevy's `basis-universal` feature. The images repeat in both
    /// directions and load in the background.
    pub fn load(
        asset_server: &AssetServer,
        root: &str,
        set: &str,
        size: TextureSize,
        compression: TextureCompression,
    ) -> Self {
        let load = |map: TextureMap| {
            let path = format!("{root}/{}", bark_texture_path(set, size, map, compression));
            load_image(asset_server, path, map.is_srgb(), ImageAddressMode::Repeat)
        };
        Self {
            color: load(TextureMap::Color),
            normal: load(TextureMap::Normal),
            roughness: load(TextureMap::Roughness),
        }
    }
}

/// The images of one end-grain texture set, for [`deadwood_material`].
#[derive(Debug, Clone, PartialEq)]
pub struct EndTextures {
    /// Colour photo, loaded as sRGB.
    pub color: Handle<Image>,
    /// Normal map in the OpenGL (+Y) convention, loaded as linear data.
    pub normal: Handle<Image>,
    /// Where the wood lies in the photo.
    photo: EndPhoto,
}

/// Where the wood lies in an end-grain photo, measured by hand.
#[derive(Debug, Clone, Copy, PartialEq)]
struct EndPhoto {
    /// The pith, in texture coordinates.
    centre: Vec2,
    /// Radius of a circle around the pith that stays within the wood.
    radius: f32,
    /// Average linear colour within that circle.
    average: Vec3,
}

/// Measurements for each of [`END_TEXTURE_SETS`], in the same order.
const END_PHOTOS: [EndPhoto; END_TEXTURE_SETS.len()] = [
    // TreeEnd003
    EndPhoto {
        centre: Vec2::new(0.565, 0.51),
        radius: 0.37,
        average: Vec3::new(0.4171, 0.2544, 0.1067),
    },
    // TreeEnd005
    EndPhoto {
        centre: Vec2::new(0.565, 0.47),
        radius: 0.37,
        average: Vec3::new(0.6024, 0.4168, 0.1808),
    },
];

impl EndTextures {
    /// Starts loading end-grain texture set `set` at `size`, from the same
    /// folder as [`BarkTextures::load`], with paths from
    /// [`end_texture_path`].
    ///
    /// # Panics
    ///
    /// Panics if `set` is not one of [`END_TEXTURE_SETS`].
    pub fn load(
        asset_server: &AssetServer,
        root: &str,
        set: &str,
        size: TextureSize,
        compression: TextureCompression,
    ) -> Self {
        let index = END_TEXTURE_SETS
            .iter()
            .position(|&s| s == set)
            .unwrap_or_else(|| panic!("{set:?} is not one of END_TEXTURE_SETS"));
        let load = |map: TextureMap| {
            let path = format!("{root}/{}", end_texture_path(set, size, map, compression));
            load_image(
                asset_server,
                path,
                map.is_srgb(),
                ImageAddressMode::ClampToEdge,
            )
        };
        Self {
            color: load(TextureMap::Color),
            normal: load(TextureMap::Normal),
            photo: END_PHOTOS[index],
        }
    }
}

/// The two images of one leaf card texture set, for [`leaf_material`].
#[derive(Debug, Clone, PartialEq)]
pub struct LeafTextures {
    /// Colour with transparency, loaded as sRGB.
    pub color: Handle<Image>,
    /// Normal map in the OpenGL (+Y) convention, loaded as linear data.
    pub normal: Handle<Image>,
}

impl LeafTextures {
    /// Starts loading leaf card texture set `set`, one of
    /// [`LEAF_TEXTURE_SETS`](procgena_deadwood::LEAF_TEXTURE_SETS), from
    /// the same folder as [`BarkTextures::load`], with paths from
    /// [`leaf_texture_path`]. Leaf cards come in one size only.
    pub fn load(
        asset_server: &AssetServer,
        root: &str,
        set: &str,
        compression: TextureCompression,
    ) -> Self {
        let load = |map: LeafMap| {
            let path = format!("{root}/{}", leaf_texture_path(set, map, compression));
            load_image(
                asset_server,
                path,
                map.is_srgb(),
                ImageAddressMode::ClampToEdge,
            )
        };
        Self {
            color: load(LeafMap::Color),
            normal: load(LeafMap::Normal),
        }
    }
}

/// Creates the material for leaf card meshes, from
/// [`DeadwoodMesh::leaves`](procgena_deadwood::DeadwoodMesh::leaves)
/// through [`deadwood_mesh`](crate::deadwood_mesh).
///
/// Both sides of each card are drawn, and the colour image's alpha cuts
/// out the leaves (alpha testing at 0.5). The vertex colours carry
/// [`LeafOptions::tint`](procgena_deadwood::LeafOptions::tint).
pub fn leaf_material(textures: &LeafTextures) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(textures.color.clone()),
        normal_map_texture: Some(textures.normal.clone()),
        alpha_mode: AlphaMode::Mask(0.5),
        perceptual_roughness: 0.7,
        metallic: 0.0,
        reflectance: 0.3,
        double_sided: true,
        cull_mode: None,
        ..default()
    }
}

/// Starts loading texture image `path` with linear filtering and
/// `address_mode` in both directions, as sRGB colour when `srgb` is set.
fn load_image(
    asset_server: &AssetServer,
    path: String,
    srgb: bool,
    address_mode: ImageAddressMode,
) -> Handle<Image> {
    asset_server
        .load_builder()
        .with_settings(move |settings: &mut ImageLoaderSettings| {
            settings.is_srgb = srgb;
            settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                address_mode_u: address_mode,
                address_mode_v: address_mode,
                ..ImageSamplerDescriptor::linear()
            });
        })
        .load(path)
}

/// Creates a material for the meshes of a piece made from `options`.
///
/// Set `ribbon` for meshes from [`ribbon_mesh`](crate::ribbon_mesh): both
/// sides of each triangle are then drawn, and `bark` is ignored because the
/// strips can't show bark images. Otherwise only front faces are drawn.
///
/// `bark` paints bark from a texture set, usually the one named by
/// [`MaterialOptions::bark_texture`]. The vertex colours still set the
/// bark's overall colour; the images add its light and dark detail, bumps
/// and roughness. With `None`, vertex colours are used everywhere.
///
/// Ends get growth rings and drying cracks; the cracks grow with decay and
/// soften again, with the rings, as the wood rots.
///
/// `end` draws an end-grain photo, usually the set named by
/// [`MaterialOptions::end_texture`], on sawn ends, stretched to fit each
/// one. As with bark, the vertex colours keep the end's overall colour.
/// Strips ignore it.
///
/// [`MaterialOptions::bark_texture`]: procgena_deadwood::MaterialOptions::bark_texture
/// [`MaterialOptions::end_texture`]: procgena_deadwood::MaterialOptions::end_texture
pub fn deadwood_material(
    options: &DeadwoodOptions,
    bark: Option<&BarkTextures>,
    end: Option<&EndTextures>,
    ribbon: bool,
) -> DeadwoodMaterial {
    let base = StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: options.material.roughness,
        metallic: 0.0,
        reflectance: 0.35,
        double_sided: ribbon,
        cull_mode: if ribbon { None } else { Some(Face::Back) },
        ..default()
    };

    // Detail strengths by decay: cracks open as the wood dries out, then
    // close up and the rings fade as it rots.
    let bark = bark.filter(|_| !ribbon);
    let end = end.filter(|_| !ribbon);
    let photo = end.map(|e| e.photo);
    let decay = options.decay;
    let rotten = smoothstep(3.0, 5.0, decay);
    let circumference = PI * options.diameter;
    let extension = DeadwoodExtension {
        surface: DeadwoodSurface {
            bark_textured: u32::from(bark.is_some()),
            bark_repeats: (circumference / options.material.bark_texture_size)
                .round()
                .max(1.0),
            ring_frequency: 220.0,
            ring_strength: 1.0 - 0.7 * rotten,
            crack_strength: smoothstep(0.3, 2.5, decay) * (1.0 - 0.6 * rotten),
            grain_strength: 1.0 - 0.5 * rotten,
            seed: (options.seed % 1000) as f32 * 7.31,
            end_textured: u32::from(photo.is_some()),
            end_centre: photo.map_or(Vec2::splat(0.5), |p| p.centre),
            end_radius: photo.map_or(0.5, |p| p.radius),
            end_average: photo.map_or(Vec3::ONE, |p| p.average),
        },
        bark_color: bark.map(|b| b.color.clone()),
        bark_normal: bark.map(|b| b.normal.clone()),
        bark_roughness: bark.map(|b| b.roughness.clone()),
        end_color: end.map(|e| e.color.clone()),
        end_normal: end.map(|e| e.normal.clone()),
    };
    DeadwoodMaterial { base, extension }
}

/// Returns 0 below `a`, 1 above `b`, and a smooth blend between.
fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
