//! The terrain material: Bevy's standard lighting, with the ground
//! coloured by its slope and height.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

/// Fragment shader for the main and deferred passes.
pub(crate) const FRAGMENT_SHADER: &str =
    "embedded://procgena_terrain_bevy/shaders/terrain_fragment.wesl";

/// Bevy's [`StandardMaterial`] with the ground coloured by slope and
/// height. The standard material's base colour is replaced; its roughness
/// and other settings are kept.
pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExtension>;

/// The part of a [`TerrainMaterial`] that colours the ground.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct TerrainExtension {
    /// Colours and where they apply.
    #[uniform(100)]
    pub surface: TerrainSurface,
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
///
/// Flat ground gets `grass`, which turns to `rock` as the ground steepens
/// and to `snow` above the snow line. The material also fades the ground
/// out at the end of the view, and can tint each chunk level for
/// debugging.
#[derive(ShaderType, Reflect, Debug, Clone, Copy, PartialEq)]
pub struct TerrainSurface {
    /// Colour of flat and gently sloping ground.
    pub grass: Vec3,
    /// Colour of steep ground.
    pub rock: Vec3,
    /// Colour above the snow line.
    pub snow: Vec3,
    /// Slopes, as the sine of the ground's angle from flat, over which
    /// grass turns to rock: from `x` to `y`.
    pub rock_slope: Vec2,
    /// Heights in metres over which snow comes in: from `x` to `y`.
    pub snow_height: Vec2,
    /// How much each colour varies across the ground, from 0 to 1.
    pub variation: f32,
    /// Where the ground fades out with distance from the camera, packed as
    /// [`LodRange::to_vec4`](procgena_core::LodRange::to_vec4) packs a
    /// level's range. Set by [`TerrainPlugin`](crate::TerrainPlugin).
    pub fade: Vec4,
    /// A colour mixed over the ground to show its chunk level, with its
    /// strength in `w`; 0 strength for none. Set by
    /// [`TerrainDebug`](crate::TerrainDebug).
    pub tint: Vec4,
    /// The width in metres of this material's chunks, for drawing chunk
    /// borders; 0 for none. Set by [`TerrainDebug`](crate::TerrainDebug).
    pub border: f32,
}

impl Default for TerrainSurface {
    fn default() -> Self {
        Self {
            grass: Vec3::new(0.14, 0.22, 0.06),
            rock: Vec3::new(0.24, 0.22, 0.2),
            snow: Vec3::new(0.85, 0.87, 0.9),
            rock_slope: Vec2::new(0.45, 0.65),
            snow_height: Vec2::new(900.0, 1100.0),
            variation: 0.3,
            fade: Vec4::new(0.0, 0.0, f32::INFINITY, f32::INFINITY),
            tint: Vec4::ZERO,
            border: 0.0,
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
