//! Where an impostor sits among an asset's levels of detail.

use procgena_core::{LevelForm, LodLevel};

use crate::key::BakeSettings;

/// An asset's impostor as its farthest level of detail: one quad showing a
/// baked view of the specimen, beyond its mesh and far-form levels.
///
/// Distances are in metres for the asset's reference size, like the
/// asset's other levels, so they scale with each copy's size and the
/// quality setting in the same way. Impostors don't sway in the wind and
/// show a fixed set of views, so keep `distance` well beyond where either
/// would be noticed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImpostorLevel {
    /// Camera distance where the impostor takes over from the farthest
    /// other level, in metres.
    pub distance: f32,
    /// Width of the crossfade with the farthest other level, as a fraction
    /// of `distance` on each side.
    pub hysteresis: f32,
    /// The level the impostor is baked from, counting from 0 for the
    /// nearest. It should be a level with real geometry, not camera-facing
    /// cards.
    pub baked_level: usize,
    /// How the impostor is baked: its view count and size.
    pub bake: BakeSettings,
}

impl ImpostorLevel {
    /// An impostor from `distance` metres, crossfading over `hysteresis`,
    /// baked from level `baked_level` with the default [`BakeSettings`].
    pub fn new(distance: f32, hysteresis: f32, baked_level: usize) -> Self {
        Self {
            distance,
            hysteresis,
            baked_level,
            bake: BakeSettings::default(),
        }
    }

    /// The impostor as a level of the shared level-of-detail type, with
    /// `detail` as its level data. Push it after every other level.
    pub fn level<D>(&self, detail: D) -> LodLevel<D> {
        LodLevel::new(self.distance, self.hysteresis, detail).with_form(LevelForm::Impostor)
    }
}

#[cfg(test)]
mod tests {
    use procgena_core::{FarForm, Lod, Quality};

    use super::*;

    /// A mesh, a far form, then the impostor: the impostor fades in where
    /// the far form fades out, and is drawn at every distance beyond.
    #[test]
    fn the_impostor_is_the_last_level() {
        let impostor = ImpostorLevel::new(100.0, 0.1, 0);
        let lod = Lod::new(
            2.0,
            vec![
                LodLevel::new(0.0, 0.0, ()),
                LodLevel::new(40.0, 0.1, ()).with_form(LevelForm::Far(FarForm::Card)),
                impostor.level(()),
            ],
        );
        lod.validate().unwrap();
        let ranges = lod.ranges(lod.factor(4.0, Quality::High));
        assert_eq!(ranges[1].fade_out, 180.0..220.0, "scaled by size");
        assert_eq!(ranges[2].fade_in, ranges[1].fade_out);
        assert!(ranges[2].fade_out.start.is_infinite());
    }
}
