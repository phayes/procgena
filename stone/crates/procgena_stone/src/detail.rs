//! How finely a rock is built: its game-level mesh detail, and the coarser
//! mesh a renderer draws when the rock is small on screen.

use procgena_core::chain::{Lod, LowerDetail};
use serde::{Deserialize, Serialize};

use crate::material::TextureSize;
use crate::options::MIN_RESOLUTION;

/// How finely one rock mesh is built, and the texture size to draw it with.
///
/// A preset's [`MeshOptions::game_level`](crate::MeshOptions::game_level) is
/// one of these: the rock's real shape, which the game uses for collisions,
/// picking and its impostor. Renderers also draw a coarser mesh when the
/// rock is small on screen; [`lod`](Self::lod) gives both.
///
/// Pick one of the named details ([`ULTRA_LOW`](Self::ULTRA_LOW) through
/// [`ULTRA`](Self::ULTRA)) or set your own. The default is
/// [`MEDIUM_LOW`](Self::MEDIUM_LOW).
///
/// ```
/// use procgena_core::chain::Step;
/// use procgena_stone::{StoneDetail, TextureSize};
///
/// let lod = StoneDetail::MEDIUM.lod();
/// assert_eq!(lod.game, StoneDetail::MEDIUM);
/// // The coarser mesh has half the cells and a smaller texture.
/// assert_eq!(lod.detail(Step::Lower), Some(&StoneDetail::new(48, TextureSize::Size512)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoneDetail {
    /// Number of sampling cells along each axis, from [`crate::MIN_RESOLUTION`]
    /// through [`crate::MAX_RESOLUTION`]. Doubling it needs about eight times
    /// the work and typically gives about four times as many triangles.
    pub resolution: u32,
    /// Texture image size to draw the mesh with.
    pub texture_size: TextureSize,
}

impl StoneDetail {
    /// Fastest to build, for rocks seen only from afar: 16 cells, 512
    /// textures. Close up, rocks look visibly faceted.
    pub const ULTRA_LOW: Self = Self::new(16, TextureSize::Size512);
    /// 32 cells, 512 textures.
    pub const LOW: Self = Self::new(32, TextureSize::Size512);
    /// 48 cells, 512 textures. The default.
    pub const MEDIUM_LOW: Self = Self::new(48, TextureSize::Size512);
    /// 96 cells, 1k textures.
    pub const MEDIUM: Self = Self::new(96, TextureSize::Size1k);
    /// For close-up views: 192 cells, 2k textures.
    pub const HIGH: Self = Self::new(192, TextureSize::Size2k);
    /// Most detailed: 240 cells, 2k textures. Still within the GPU
    /// generator's resolution limit.
    pub const ULTRA: Self = Self::new(240, TextureSize::Size2k);

    /// Every named detail with its display name, least detailed first.
    pub const PRESETS: [(&'static str, Self); 6] = [
        ("Ultra Low", Self::ULTRA_LOW),
        ("Low", Self::LOW),
        ("Medium Low", Self::MEDIUM_LOW),
        ("Medium", Self::MEDIUM),
        ("High", Self::HIGH),
        ("Ultra", Self::ULTRA),
    ];

    /// A detail of `resolution` cells per axis, drawn with `texture_size`
    /// textures. The resolution is checked when the options are validated.
    pub const fn new(resolution: u32, texture_size: TextureSize) -> Self {
        Self {
            resolution,
            texture_size,
        }
    }

    /// The display name of the named detail equal to this one, or `None` for
    /// a custom detail.
    pub fn preset_name(&self) -> Option<&'static str> {
        Self::PRESETS
            .iter()
            .find(|(_, preset)| preset == self)
            .map(|(name, _)| *name)
    }

    /// This detail as the game level, and the coarser mesh after it (see
    /// [`LowerDetail`]).
    pub fn lod(&self) -> Lod<Self> {
        Lod::from_game(*self)
    }
}

/// Half the cells (at least [`MIN_RESOLUTION`]) and the next smaller texture
/// size (at least 512).
impl LowerDetail for StoneDetail {
    fn lower(&self) -> Self {
        let texture_size = match self.texture_size {
            TextureSize::Size2k => TextureSize::Size1k,
            TextureSize::Size1k | TextureSize::Size512 => TextureSize::Size512,
        };
        Self::new((self.resolution / 2).max(MIN_RESOLUTION), texture_size)
    }
}

impl Default for StoneDetail {
    fn default() -> Self {
        Self::MEDIUM_LOW
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_named_and_custom_details_are_not() {
        assert_eq!(StoneDetail::HIGH.preset_name(), Some("High"));
        let copy = StoneDetail::new(32, TextureSize::Size512);
        assert_eq!(copy.preset_name(), Some("Low"));
        let custom = StoneDetail::new(33, TextureSize::Size512);
        assert_eq!(custom.preset_name(), None);
    }

    #[test]
    fn the_lower_mesh_halves_the_cells_and_shrinks_the_texture() {
        let lower = |detail: StoneDetail| detail.lod().lower;
        assert_eq!(
            lower(StoneDetail::ULTRA),
            StoneDetail::new(120, TextureSize::Size1k)
        );
        assert_eq!(
            lower(StoneDetail::LOW),
            StoneDetail::new(16, TextureSize::Size512)
        );
        let smallest = StoneDetail::new(MIN_RESOLUTION, TextureSize::Size512);
        assert_eq!(lower(smallest), smallest);
    }

    #[test]
    fn json_is_a_resolution_and_a_texture_size() {
        let json = serde_json::to_string(&StoneDetail::LOW).unwrap();
        assert_eq!(json, r#"{"resolution":32,"texture_size":"512"}"#);
        let back: StoneDetail = serde_json::from_str(&json).unwrap();
        assert_eq!(back, StoneDetail::LOW);
    }
}
