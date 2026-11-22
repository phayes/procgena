//! Mesh detail levels for a rock: how finely each level is meshed, when a
//! renderer should switch to it, and which texture size suits it.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::material::TextureSize;

/// One level of detail: a mesh resolution, the camera distance where it starts,
/// and the texture size to draw it with.
///
/// Level of detail (LOD) lets an application draw simpler meshes for distant
/// rocks. This crate builds the meshes; the renderer decides when to switch.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DetailLevel {
    /// Number of sampling cells along each axis, from [`crate::MIN_RESOLUTION`]
    /// through [`crate::MAX_RESOLUTION`]. Doubling it needs about eight times
    /// the work and typically gives about four times as many triangles.
    pub resolution: u32,
    /// Camera distance in metres where this level starts to be shown.
    /// Must be finite and zero or more, and greater than the previous level's.
    /// Distances suit a rock with the default
    /// [`StoneOptions::scale`](crate::StoneOptions::scale) of 2.5 m; renderers
    /// may scale them in proportion to each rock's size.
    pub distance: f32,
    /// Width of the switching band as a fraction of `distance`, on each side
    /// of it, from 0 up to but not including 1. For example, 0.05 lets a
    /// renderer fade between `distance × 0.95` and `distance × 1.05`.
    pub hysteresis: f32,
    /// Texture image size suited to this level. Exporters use the first level's size.
    pub texture_size: TextureSize,
}

impl DetailLevel {
    /// Creates a level with a hysteresis of 0.05.
    pub const fn new(resolution: u32, distance: f32, texture_size: TextureSize) -> Self {
        Self {
            resolution,
            distance,
            hysteresis: 0.05,
            texture_size,
        }
    }
}

/// The detail levels to build for a rock, most detailed (nearest) first.
///
/// Use one of the presets ([`LOW`](Self::LOW), [`MEDIUM`](Self::MEDIUM),
/// [`HIGH`](Self::HIGH), [`ULTRA`](Self::ULTRA)) or define your own levels
/// with [`new`](Self::new). The default is [`MEDIUM`](Self::MEDIUM).
///
/// A set needs 1 through [`crate::MAX_DETAIL_LEVELS`] levels with increasing
/// distances; [`StoneOptions::validate`](crate::StoneOptions::validate) checks
/// this. A single level means no distance switching. In JSON, a detail set is
/// an array of levels.
///
/// ```
/// use procgena_stone::{DetailLevel, StoneDetail, TextureSize};
///
/// // A close-up mesh and one simpler mesh from 20 metres away.
/// let detail = StoneDetail::new(vec![
///     DetailLevel::new(128, 0.0, TextureSize::Size2k),
///     DetailLevel::new(32, 20.0, TextureSize::Size512),
/// ]);
/// assert_eq!(detail.levels().len(), 2);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StoneDetail(Cow<'static, [DetailLevel]>);

impl StoneDetail {
    /// Fastest to build: 48, 24 and 12 cells, all with 512 textures.
    pub const LOW: Self = Self::preset(&[
        DetailLevel::new(48, 0.0, TextureSize::Size512),
        DetailLevel::new(24, 15.0, TextureSize::Size512),
        DetailLevel::new(12, 40.0, TextureSize::Size512),
    ]);

    /// Balanced default: 96, 48 and 24 cells, with 1k, 512 and 512 textures.
    pub const MEDIUM: Self = Self::preset(&[
        DetailLevel::new(96, 0.0, TextureSize::Size1k),
        DetailLevel::new(48, 15.0, TextureSize::Size512),
        DetailLevel::new(24, 40.0, TextureSize::Size512),
    ]);

    /// For close-up views: 192, 96 and 48 cells, with 2k, 1k and 512 textures.
    pub const HIGH: Self = Self::preset(&[
        DetailLevel::new(192, 0.0, TextureSize::Size2k),
        DetailLevel::new(96, 15.0, TextureSize::Size1k),
        DetailLevel::new(48, 40.0, TextureSize::Size512),
    ]);

    /// Most detailed: 240, 120 and 60 cells, with 2k, 2k and 1k textures.
    /// Still within the GPU generator's resolution limit.
    pub const ULTRA: Self = Self::preset(&[
        DetailLevel::new(240, 0.0, TextureSize::Size2k),
        DetailLevel::new(120, 15.0, TextureSize::Size2k),
        DetailLevel::new(60, 40.0, TextureSize::Size1k),
    ]);

    /// Every preset with a display name, least detailed first.
    pub const PRESETS: [(&'static str, Self); 4] = [
        ("Low", Self::LOW),
        ("Medium", Self::MEDIUM),
        ("High", Self::HIGH),
        ("Ultra", Self::ULTRA),
    ];

    /// Creates a detail set from your own levels, most detailed (nearest) first.
    /// The levels are checked when the options are validated.
    pub fn new(levels: Vec<DetailLevel>) -> Self {
        Self(Cow::Owned(levels))
    }

    /// Creates a set with one level, for a single mesh without distance switching.
    pub fn single(resolution: u32, texture_size: TextureSize) -> Self {
        Self::new(vec![DetailLevel {
            resolution,
            distance: 0.0,
            hysteresis: 0.0,
            texture_size,
        }])
    }

    /// Wraps a fixed list of levels without copying it.
    const fn preset(levels: &'static [DetailLevel]) -> Self {
        Self(Cow::Borrowed(levels))
    }

    /// The levels, most detailed (nearest) first.
    pub fn levels(&self) -> &[DetailLevel] {
        &self.0
    }

    /// The most detailed level, or `None` for an empty (invalid) set.
    pub fn first(&self) -> Option<&DetailLevel> {
        self.0.first()
    }

    /// The display name of the preset equal to this set, or `None` for a custom set.
    pub fn preset_name(&self) -> Option<&'static str> {
        Self::PRESETS
            .iter()
            .find(|(_, preset)| preset == self)
            .map(|(name, _)| *name)
    }
}

impl Default for StoneDetail {
    fn default() -> Self {
        Self::MEDIUM
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_named_and_custom_sets_are_not() {
        assert_eq!(StoneDetail::HIGH.preset_name(), Some("High"));
        let copy = StoneDetail::new(StoneDetail::LOW.levels().to_vec());
        assert_eq!(copy.preset_name(), Some("Low"));
        let custom = StoneDetail::new(vec![DetailLevel::new(32, 0.0, TextureSize::Size512)]);
        assert_eq!(custom.preset_name(), None);
    }

    #[test]
    fn json_is_an_array_of_levels() {
        let json = serde_json::to_string(&StoneDetail::LOW).unwrap();
        assert!(json.starts_with('['), "{json}");
        assert!(json.contains(r#""texture_size":"512""#), "{json}");
        let back: StoneDetail = serde_json::from_str(&json).unwrap();
        assert_eq!(back, StoneDetail::LOW);
    }
}
