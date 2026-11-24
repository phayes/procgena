//! The terrain material: Bevy's standard lighting, with the ground
//! coloured by ground weights.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat,
};
use bevy::shader::ShaderRef;
use procgena_terrain::ground::{GroundWeights, MaterialCatalogue, PALETTE_SIZE};

/// Fragment shader for the main and deferred passes.
pub(crate) const FRAGMENT_SHADER: &str =
    "embedded://procgena_terrain_bevy/shaders/terrain_fragment.wesl";

/// Bevy's [`StandardMaterial`] with the ground coloured by ground weights.
/// The standard material's base colour and roughness are replaced; its
/// other settings are kept.
///
/// The mesh needs UVs ([`Mesh::ATTRIBUTE_UV_0`]) running from 0 at the
/// weights' first texel to 1 at their last, along x and z, as
/// [`chunk_mesh`](crate::chunk_mesh) makes them.
pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExtension>;

/// The part of a [`TerrainMaterial`] that colours the ground. Make one
/// from ground weights with [`new`](Self::new).
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct TerrainExtension {
    /// The palette's colours, and drawing settings.
    #[uniform(100)]
    pub surface: TerrainSurface,
    /// The weights of palette slots 0 to 3, one per channel.
    #[texture(101)]
    #[sampler(102)]
    pub weights_low: Handle<Image>,
    /// The weights of palette slots 4 to 7, one per channel.
    #[texture(103)]
    #[sampler(104)]
    pub weights_high: Handle<Image>,
}

impl TerrainExtension {
    /// The extension drawing `weights`, with their palette's colours and
    /// roughness from `catalogue` (the painter's), and the two weight
    /// images added to `images`.
    pub fn new(
        weights: &GroundWeights,
        catalogue: &MaterialCatalogue,
        images: &mut Assets<Image>,
    ) -> Self {
        let [low, high] = weight_images(weights);
        Self {
            surface: TerrainSurface::new(weights, catalogue),
            weights_low: images.add(low),
            weights_high: images.add(high),
        }
    }
}

impl MaterialExtension for TerrainExtension {
    fn fragment_shader() -> ShaderRef {
        FRAGMENT_SHADER.into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        FRAGMENT_SHADER.into()
    }
}

/// How the terrain material colours the ground. Colours are linear RGB.
#[derive(ShaderType, Reflect, Debug, Clone, Copy, PartialEq)]
pub struct TerrainSurface {
    /// Each palette slot's material: its colour, and its roughness in `w`.
    /// Slots past the weights' palette are unused.
    pub palette: [Vec4; PALETTE_SIZE],
    /// The weights' texels along x and z.
    pub texels: Vec2,
    /// How much colours vary across the ground in patches, from 0 to 1.
    pub variation: f32,
    /// Where the ground fades out with distance from the camera, packed as
    /// [`LodRange::to_vec4`](procgena_core::LodRange::to_vec4) packs a
    /// level's range. Set by [`TerrainPlugin`](crate::TerrainPlugin) on
    /// its chunks.
    pub fade: Vec4,
    /// A colour mixed over the ground to show its chunk level, with its
    /// strength in `w`; 0 strength for none. Set by
    /// [`TerrainDebug`](crate::TerrainDebug) on terrain chunks.
    pub tint: Vec4,
    /// The width in metres of this material's chunk, for drawing chunk
    /// borders; 0 for none. Set by [`TerrainDebug`](crate::TerrainDebug)
    /// on terrain chunks.
    pub border: f32,
}

impl Default for TerrainSurface {
    /// A black palette, with the usual variation and no fade or debug
    /// drawing.
    fn default() -> Self {
        Self {
            palette: [Vec4::ZERO; PALETTE_SIZE],
            texels: Vec2::ONE,
            variation: DEFAULT_VARIATION,
            fade: Vec4::new(0.0, 0.0, f32::INFINITY, f32::INFINITY),
            tint: Vec4::ZERO,
            border: 0.0,
        }
    }
}

impl TerrainSurface {
    /// The surface drawing `weights`: their palette's colours and
    /// roughness from `catalogue` (the painter's) and their texel count,
    /// with the default variation and no fade or debug drawing.
    pub fn new(weights: &GroundWeights, catalogue: &MaterialCatalogue) -> Self {
        let mut palette = [Vec4::ZERO; PALETTE_SIZE];
        for (slot, &id) in weights.palette().iter().enumerate() {
            let material = catalogue.get(id).1;
            palette[slot] = Vec3::from(material.colour).extend(material.roughness);
        }
        Self {
            palette,
            texels: weights.size().as_vec2(),
            ..default()
        }
    }
}

/// How much colours vary across the ground by default.
pub const DEFAULT_VARIATION: f32 = 0.3;

/// The two weight images of `weights`, for
/// [`TerrainExtension::weights_low`] and
/// [`weights_high`](TerrainExtension::weights_high): one texel per weight
/// texel, linear, filtered and clamped at the edges.
pub(crate) fn weight_images(weights: &GroundWeights) -> [Image; 2] {
    let size = weights.size();
    let half = |offset: usize| {
        let data: Vec<u8> = weights
            .texels()
            .iter()
            .flat_map(|texel| [0, 1, 2, 3].map(|k| texel[offset + k]))
            .collect();
        let mut image = Image::new(
            Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
        image
    };
    [half(0), half(4)]
}

/// The debug tint of chunk level `level`: a colour that steps round the
/// hue circle, so neighbouring levels differ.
pub(crate) fn level_tint(level: u8) -> Vec3 {
    let hue = (f32::from(level) * 67.0) % 360.0;
    let colour = Color::hsl(hue, 0.85, 0.5).to_linear();
    Vec3::new(colour.red, colour.green, colour.blue)
}
