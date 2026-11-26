//! The terrain material: Bevy's standard lighting over plain grey ground,
//! with the fade at the end of the view, debug tints and borders, and an
//! optional overlay image.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

/// Fragment shader for the main and deferred passes.
pub(crate) const FRAGMENT_SHADER: &str =
    "embedded://procgena_terrain_bevy/shaders/terrain_fragment.wesl";

/// The plain grey the ground is drawn in, in linear RGB.
pub const GROUND_GREY: Vec3 = Vec3::splat(0.25);

/// Bevy's [`StandardMaterial`] drawing the ground in
/// [`TerrainSurface::colour`]. The standard material's base colour and
/// roughness are replaced; its other settings are kept.
///
/// An [overlay image](crate::TerrainOverlay) is stretched over the mesh's
/// UVs ([`Mesh::ATTRIBUTE_UV_0`]), from 0 on one corner to 1 on the
/// opposite one, as terrain chunk meshes make them.
pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExtension>;

/// The part of a [`TerrainMaterial`] that colours the ground.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct TerrainExtension {
    /// The ground's colour and drawing settings.
    #[uniform(100)]
    pub surface: TerrainSurface,
    /// An image mixed over the ground by [`TerrainSurface::overlay`],
    /// stretched across the mesh's UVs. Set from
    /// [`TerrainOverlay`](crate::TerrainOverlay); the default image while
    /// there is none.
    #[texture(101)]
    #[sampler(102)]
    pub overlay: Handle<Image>,
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
    /// The ground's colour, and its roughness in `w`.
    pub colour: Vec4,
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
    /// [`GROUND_GREY`], rough, with no fade or debug drawing.
    fn default() -> Self {
        Self {
            colour: GROUND_GREY.extend(0.95),
            fade: Vec4::new(0.0, 0.0, f32::INFINITY, f32::INFINITY),
            tint: Vec4::ZERO,
            border: 0.0,
            overlay: 0.0,
        }
    }
}

/// The debug tint of chunk level `level`: a colour that steps round the
/// hue circle, so neighbouring levels differ.
pub(crate) fn level_tint(level: u8) -> Vec3 {
    let hue = (f32::from(level) * 67.0) % 360.0;
    let colour = Color::hsl(hue, 0.85, 0.5).to_linear();
    Vec3::new(colour.red, colour.green, colour.blue)
}
