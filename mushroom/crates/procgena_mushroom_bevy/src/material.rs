//! The mushroom material: Bevy's standard lighting plus vertex colours,
//! gill lines, cap patterns, and flat stems that turn toward the camera.

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
use procgena_mushroom::{
    MushroomOptions, Pattern, TextureCompression, TextureMap, TexturePart, TextureSize,
    linear_from_hex, stem_texture_average, texture_path,
};

use crate::mesh::{ATTRIBUTE_BILLBOARD_AXIS, ATTRIBUTE_BILLBOARD_PIVOT};

/// Vertex shader for the main (visible) pass.
pub(crate) const VERTEX_SHADER: &str =
    "embedded://procgena_mushroom_bevy/shaders/mushroom_vertex.wesl";
/// Vertex shader for depth, shadow and deferred passes.
pub(crate) const PREPASS_SHADER: &str =
    "embedded://procgena_mushroom_bevy/shaders/mushroom_prepass.wesl";
/// Fragment shader for the main and deferred passes.
pub(crate) const FRAGMENT_SHADER: &str =
    "embedded://procgena_mushroom_bevy/shaders/mushroom_fragment.wesl";

/// Shader input slot for [`ATTRIBUTE_BILLBOARD_PIVOT`], after the slots
/// Bevy uses for its own vertex attributes.
const BILLBOARD_PIVOT_LOCATION: u32 = 9;
/// Shader input slot for [`ATTRIBUTE_BILLBOARD_AXIS`].
const BILLBOARD_AXIS_LOCATION: u32 = 10;

/// Bevy's [`StandardMaterial`] with mushroom surface details added.
///
/// Use it with meshes from [`mushroom_mesh`](crate::mushroom_mesh) or
/// [`ribbon_mesh`](crate::ribbon_mesh). Vertex colours multiply the base
/// colour, gill lines darken the underside, and a pattern is drawn on the
/// cap. Meshes with both billboard attributes are turned toward whatever is
/// looking at them: the camera, or a light when its shadows are drawn.
/// Create one with [`mushroom_material`].
pub type MushroomMaterial = ExtendedMaterial<StandardMaterial, MushroomExtension>;

/// The part of a [`MushroomMaterial`] that adds mushroom details to the
/// standard material.
///
/// Leave the three cap images as `None`, with
/// [`MushroomSurface::cap_textured`] at 0, for vertex colours only. When
/// `cap_textured` is 1, supply all three. The three stem images work the
/// same way with [`MushroomSurface::stem_textured`]; they share one sampler,
/// taken from the stem colour image.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct MushroomExtension {
    /// Pattern, gill and cap texture settings.
    #[uniform(100)]
    pub surface: MushroomSurface,
    /// Cap colour image, loaded as sRGB.
    #[texture(101)]
    #[sampler(102)]
    pub cap_color: Option<Handle<Image>>,
    /// Cap normal map in the OpenGL (+Y) convention, loaded as linear data.
    #[texture(103)]
    #[sampler(104)]
    pub cap_normal: Option<Handle<Image>>,
    /// Cap occlusion, roughness and height in red, green and blue, loaded as
    /// linear data.
    #[texture(105)]
    #[sampler(106)]
    pub cap_orh: Option<Handle<Image>>,
    /// Stem colour image, loaded as sRGB. Its sampler is used for all three
    /// stem images, so it must repeat in both directions.
    #[texture(107)]
    #[sampler(108)]
    pub stem_color: Option<Handle<Image>>,
    /// Stem normal map in the OpenGL (+Y) convention, loaded as linear data.
    #[texture(109)]
    pub stem_normal: Option<Handle<Image>>,
    /// Stem occlusion, roughness and height in red, green and blue, loaded
    /// as linear data.
    #[texture(110)]
    pub stem_orh: Option<Handle<Image>>,
}

/// Pattern and gill settings, laid out as the shaders expect.
/// [`mushroom_material`] fills them in from the mushroom's options.
#[derive(ShaderType, Reflect, Debug, Clone, Copy, PartialEq, Default)]
pub struct MushroomSurface {
    /// Pattern colour in linear RGB, with its strength (0 to 1) in alpha.
    pub pattern_colour: Vec4,
    /// Which pattern to draw: 0 none, 1 speckle, 2 cracks, 3 spots.
    pub pattern: u32,
    /// Pattern features per unit of the mesh, which is per metre for meshes
    /// from this crate.
    pub pattern_frequency: f32,
    /// Number of gill lines around the underside; 0 for none.
    pub gill_lines: f32,
    /// How much gill lines darken the underside, from 0 to 1.
    pub gill_strength: f32,
    /// 1 to paint the cap's top with the extension's cap images, 0 to use
    /// vertex colours only.
    pub cap_textured: u32,
    /// Width covered by one repeat of the cap images, in mesh units (metres
    /// for meshes from this crate). Must be positive when `cap_textured` is
    /// 1.
    pub cap_texture_size: f32,
    /// Average linear colour of the stem colour image. The stem image is
    /// divided by it and multiplied by the stem's vertex colour, so the
    /// texture adds detail without changing the stem's colours.
    pub stem_texture_average: Vec3,
    /// 1 to add detail to the stem with the extension's stem images, 0 to
    /// use vertex colours only.
    pub stem_textured: u32,
    /// Length of stem covered by one repeat of the stem images, in mesh
    /// units. Must be positive when `stem_textured` is 1.
    pub stem_texture_size: f32,
    /// Typical width of a flat stem, in mesh units, so the stem images keep
    /// roughly their proportions across it. Must be positive when
    /// `stem_textured` is 1.
    pub stem_ribbon_width: f32,
}

impl MaterialExtension for MushroomExtension {
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
            if let Some(fragment) = descriptor.fragment.as_mut() {
                fragment.shader_defs.push("BILLBOARD".into());
            }
        }
        Ok(())
    }
}

/// The three images of one cap or stem texture set, for
/// [`mushroom_material`].
#[derive(Debug, Clone, PartialEq)]
pub struct TextureSet {
    /// Colour image, loaded as sRGB.
    pub color: Handle<Image>,
    /// Normal map in the OpenGL (+Y) convention, loaded as linear data.
    pub normal: Handle<Image>,
    /// Occlusion, roughness and height image, loaded as linear data.
    pub orh: Handle<Image>,
}

impl TextureSet {
    /// Starts loading texture set `set`, made for `part` (one of
    /// [`TexturePart::sets`]), at `size`.
    ///
    /// `root` is a folder relative to Bevy's asset folder that holds a copy
    /// of the repository's `mushroom/assets/textures`; files are read from
    /// `<root>/<path>`, with paths from [`texture_path`]. UASTC files need
    /// Bevy's `basis-universal` feature. The images repeat in both
    /// directions and load in the background.
    pub fn load(
        asset_server: &AssetServer,
        root: &str,
        part: TexturePart,
        set: &str,
        size: TextureSize,
        compression: TextureCompression,
    ) -> Self {
        let load = |map: TextureMap| {
            let path = format!("{root}/{}", texture_path(part, set, size, map, compression));
            let srgb = map.is_srgb();
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
        };
        Self {
            color: load(TextureMap::Color),
            normal: load(TextureMap::Normal),
            orh: load(TextureMap::Orh),
        }
    }
}

/// Creates a material for the meshes of a mushroom made from `options`.
///
/// Set `double_sided` to draw both sides of every triangle. Use it for
/// levels with open surfaces (see
/// [`MushroomMesh::double_sided`](procgena_mushroom::MushroomMesh::double_sided))
/// and for flat stems.
///
/// `cap` paints the top of the cap with a texture set, usually the one
/// named by [`MaterialOptions::cap_texture`]; its colours replace the cap's
/// vertex colours and pattern there. The images are projected from three
/// directions in the mushroom's own space, one repeat per
/// [`MaterialOptions::cap_texture_size`], so they don't stretch or pinch.
/// With `None`, vertex colours are used everywhere.
///
/// `stem` adds detail to the stem with the texture set named by
/// [`MaterialOptions::stem_texture`], tinted to the stem's vertex colours
/// and repeating every [`MaterialOptions::stem_texture_size`]. It is ignored
/// when `stem_texture` doesn't name a stem set, since the tint needs that
/// set's average colour.
///
/// [`MaterialOptions::cap_texture`]: procgena_mushroom::MaterialOptions::cap_texture
/// [`MaterialOptions::cap_texture_size`]: procgena_mushroom::MaterialOptions::cap_texture_size
/// [`MaterialOptions::stem_texture`]: procgena_mushroom::MaterialOptions::stem_texture
/// [`MaterialOptions::stem_texture_size`]: procgena_mushroom::MaterialOptions::stem_texture_size
pub fn mushroom_material(
    options: &MushroomOptions,
    double_sided: bool,
    cap: Option<&TextureSet>,
    stem: Option<&TextureSet>,
) -> MushroomMaterial {
    let m = &options.material;
    // The stem texture needs its set's average colour for the tint.
    let stem_average = m.stem_texture.as_deref().and_then(stem_texture_average);
    let stem = stem.filter(|_| stem_average.is_some());
    let [r, g, b] = linear_from_hex(&m.pattern_colour).unwrap_or([1.0; 3]);
    let base = StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: m.roughness,
        metallic: 0.0,
        reflectance: 0.35,
        double_sided,
        cull_mode: if double_sided { None } else { Some(Face::Back) },
        ..default()
    };
    let pattern = match m.pattern {
        Pattern::None => 0,
        Pattern::Speckle => 1,
        Pattern::Cracks => 2,
        Pattern::Spots => 3,
    };
    let extension = MushroomExtension {
        surface: MushroomSurface {
            pattern_colour: Vec4::new(r, g, b, m.pattern_strength),
            pattern,
            pattern_frequency: m.pattern_scale / options.size,
            gill_lines: m.gill_lines as f32,
            gill_strength: 0.35,
            cap_textured: u32::from(cap.is_some()),
            cap_texture_size: m.cap_texture_size,
            stem_texture_average: Vec3::from(stem_average.unwrap_or([1.0; 3])),
            stem_textured: u32::from(stem.is_some()),
            stem_texture_size: m.stem_texture_size,
            // Mean of the top and base diameters, in metres.
            stem_ribbon_width: options.stem.radius * options.size * (1.0 + options.stem.base),
        },
        cap_color: cap.map(|c| c.color.clone()),
        cap_normal: cap.map(|c| c.normal.clone()),
        cap_orh: cap.map(|c| c.orh.clone()),
        stem_color: stem.map(|s| s.color.clone()),
        stem_normal: stem.map(|s| s.normal.clone()),
        stem_orh: stem.map(|s| s.orh.clone()),
    };
    MushroomMaterial { base, extension }
}
