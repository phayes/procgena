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
use procgena_core_bevy::texture::TextureArrays;
use procgena_terrain::ChunkKey;
use procgena_terrain::ground::{
    GroundWeights, MaterialCatalogue, PALETTE_SIZE, Projection, ground_textures,
};

use crate::textures::{GroundTextureSettings, GroundTextures, texture_layer};

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
    /// The ground textures' colour array, or `None` for flat colours. Set
    /// with [`set_textures`](Self::set_textures).
    #[texture(105, dimension = "2d_array")]
    #[sampler(106)]
    pub colour: Option<Handle<Image>>,
    /// The ground textures' normal map array.
    #[texture(107, dimension = "2d_array")]
    pub normal: Option<Handle<Image>>,
    /// The ground textures' packed occlusion, roughness and height array.
    #[texture(108, dimension = "2d_array")]
    pub orh: Option<Handle<Image>>,
    /// An image mixed over the ground by
    /// [`TerrainSurface::overlay`], stretched across the mesh's UVs as the
    /// weights are. Set from [`TerrainOverlay`](crate::TerrainOverlay);
    /// the default image while there is none.
    #[texture(109)]
    #[sampler(110)]
    pub overlay: Handle<Image>,
}

impl TerrainExtension {
    /// The extension drawing `weights`, with their palette's colours and
    /// roughness from `catalogue` (the painter's), and the two weight
    /// images added to `images`. It draws textures once the app's
    /// [`GroundTextures`](crate::GroundTextures) have loaded, if
    /// `textures` is given and set.
    pub fn new(
        weights: &GroundWeights,
        catalogue: &MaterialCatalogue,
        textures: Option<(&GroundTextures, &GroundTextureSettings)>,
        images: &mut Assets<Image>,
    ) -> Self {
        let [low, high] = weight_images(weights);
        let mut extension = Self {
            surface: TerrainSurface::new(weights, catalogue),
            weights_low: images.add(low),
            weights_high: images.add(high),
            colour: None,
            normal: None,
            orh: None,
            overlay: Handle::default(),
        };
        if let Some((textures, settings)) = textures {
            extension.set_textures(textures.arrays(), settings, true);
        }
        extension
    }

    /// Draws the ground with `arrays` within `settings`' texture distance,
    /// if `textured`, or in flat colours otherwise or without arrays.
    pub fn set_textures(
        &mut self,
        arrays: Option<&TextureArrays>,
        settings: &GroundTextureSettings,
        textured: bool,
    ) {
        let arrays = arrays.filter(|_| textured);
        self.colour = arrays.map(|a| a.colour.clone());
        self.normal = arrays.map(|a| a.normal.clone());
        self.orh = arrays.map(|a| a.surface.clone());
        let (start, end) = settings.distance;
        self.surface.texture_fade = Vec4::new(
            start,
            end,
            if arrays.is_some() { 1.0 } else { 0.0 },
            if textured { 1.0 } else { 0.0 },
        );
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
    /// Each palette slot's texture: its layer in the texture arrays (−1
    /// for none), metres per repeat inverted, 1 for triplanar projection
    /// (0 for top), and the height blend from 0 to 1.
    pub layers: [Vec4; PALETTE_SIZE],
    /// Each palette slot's tint in `xyz`: its colour divided by its
    /// texture's average colour, so the texture averages to the slot's
    /// colour.
    pub tints: [Vec4; PALETTE_SIZE],
    /// Where textures stop: the fade's start and end in metres, then 1 if
    /// the texture arrays are bound, and 1 if this material may draw
    /// textures at all.
    pub texture_fade: Vec4,
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
    /// How strongly the overlay image covers the ground's colour, from 0
    /// (not at all) to 1, times the image's alpha. Set from
    /// [`TerrainOverlay`](crate::TerrainOverlay).
    pub overlay: f32,
}

impl Default for TerrainSurface {
    /// A black palette, with the usual variation and no fade or debug
    /// drawing.
    fn default() -> Self {
        Self {
            palette: [Vec4::ZERO; PALETTE_SIZE],
            layers: [NO_TEXTURE; PALETTE_SIZE],
            tints: [Vec4::ONE; PALETTE_SIZE],
            texture_fade: Vec4::ZERO,
            texels: Vec2::ONE,
            variation: DEFAULT_VARIATION,
            fade: Vec4::new(0.0, 0.0, f32::INFINITY, f32::INFINITY),
            tint: Vec4::ZERO,
            border: 0.0,
            overlay: 0.0,
        }
    }
}

impl TerrainSurface {
    /// The surface drawing `weights`: their palette's colours, roughness
    /// and textures from `catalogue` (the painter's) and their texel
    /// count, with the default variation, no fade or debug drawing, and no
    /// textures until [`TerrainExtension::set_textures`] gives some.
    pub fn new(weights: &GroundWeights, catalogue: &MaterialCatalogue) -> Self {
        let list = ground_textures();
        let mut surface = Self {
            texels: weights.size().as_vec2(),
            ..default()
        };
        for (slot, &id) in weights.palette().iter().enumerate() {
            let material = catalogue.get(id).1;
            surface.palette[slot] = Vec3::from(material.colour).extend(material.roughness);

            // The texture, tinted so it averages to the material's colour.
            let Some(texture) = material.texture.as_deref() else {
                continue;
            };
            let (Some(layer), Some(average)) = (
                texture_layer(list, texture),
                list.set(texture).and_then(|set| set.average),
            ) else {
                continue;
            };
            let triplanar = match material.projection {
                Projection::Top => 0.0,
                Projection::Triplanar => 1.0,
            };
            surface.layers[slot] = Vec4::new(
                layer as f32,
                1.0 / material.tile,
                triplanar,
                material.blend_height,
            );
            let average = Vec3::from(average).max(Vec3::splat(1e-3));
            surface.tints[slot] = (Vec3::from(material.colour) / average).extend(1.0);
        }
        surface
    }
}

/// A palette slot without a texture.
const NO_TEXTURE: Vec4 = Vec4::new(-1.0, 1.0, 0.0, 0.0);

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

/// The part of `image`, one of the weight images of chunk `from`, that
/// covers chunk `key` inside it, at the same number of texels: each texel
/// blended from the four of `image` around it, as the GPU filters them.
///
/// # Panics
///
/// If `image` has no pixels on the CPU.
pub(crate) fn weights_part(image: &Image, from: ChunkKey, key: ChunkKey) -> Image {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let data = image
        .data
        .as_ref()
        .expect("weight images keep their pixels");

    // Where `key`'s texels fall in `from`'s, in texels.
    let scale = (key.size() / from.size()) as f32;
    let offset = ((key.min_corner() - from.min_corner()) / from.size()).as_vec2();
    let texel =
        |t: usize, n: usize, o: f32| (o + scale * t as f32 / (n - 1) as f32) * (n - 1) as f32;

    // Each texel, blended from its four neighbours, channel by channel.
    let mut part = Vec::with_capacity(data.len());
    for j in 0..h {
        let y = texel(j, h, offset.y);
        let (y0, fy) = (y.floor() as usize, y.fract());
        let y1 = (y0 + 1).min(h - 1);
        for i in 0..w {
            let x = texel(i, w, offset.x);
            let (x0, fx) = (x.floor() as usize, x.fract());
            let x1 = (x0 + 1).min(w - 1);
            for c in 0..4 {
                let at = |x: usize, y: usize| f32::from(data[(y * w + x) * 4 + c]);
                let top = at(x0, y0) * (1.0 - fx) + at(x1, y0) * fx;
                let bottom = at(x0, y1) * (1.0 - fx) + at(x1, y1) * fx;
                part.push((top * (1.0 - fy) + bottom * fy).round() as u8);
            }
        }
    }
    let mut out = image.clone();
    out.data = Some(part);
    out
}

/// The debug tint of chunk level `level`: a colour that steps round the
/// hue circle, so neighbouring levels differ.
pub(crate) fn level_tint(level: u8) -> Vec3 {
    let hue = (f32::from(level) * 67.0) % 360.0;
    let colour = Color::hsl(hue, 0.85, 0.5).to_linear();
    Vec3::new(colour.red, colour.green, colour.blue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_weights_part_is_the_parents_texels_stretched() {
        // A parent whose first channel counts texels along x, and second
        // along z.
        let n = 129;
        let data: Vec<u8> = (0..n * n)
            .flat_map(|t| [(t % n) as u8, (t / n) as u8, 0, 255])
            .collect();
        let image = Image::new(
            Extent3d {
                width: n as u32,
                height: n as u32,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::RENDER_WORLD,
        );

        // The child at the parent's larger x and smaller z: its texel
        // `(i, j)` lies at the parent's `(64 + i / 2, j / 2)`.
        let (parent, child) = (ChunkKey::new(0, 0, 1), ChunkKey::new(1, 0, 0));
        let part = weights_part(&image, parent, child);
        let pixels = part.data.as_ref().unwrap();
        for (i, j) in [(0, 0), (1, 0), (128, 0), (0, 7), (37, 128), (128, 128)] {
            let texel = &pixels[(j * n + i) * 4..][..4];
            let expected = |x: f32| x.round() as u8;
            assert_eq!(texel[0], expected(64.0 + i as f32 / 2.0), "x at ({i}, {j})");
            assert_eq!(texel[1], expected(j as f32 / 2.0), "z at ({i}, {j})");
            assert_eq!(texel[3], 255);
        }
    }
}
