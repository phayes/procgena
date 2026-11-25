//! Drawing the shared drawing chain ([`DistanceChain`]) through this
//! renderer's level table.

use procgena_core::chain::{DistanceChain, ViewScale};
use procgena_core::{LevelForm, Lod, LodLevel};

/// The level table an [`InstancedSlot`](super::InstancedSlot) needs to draw
/// `chain`: its steps as metre distances for a copy 1 m across, seen through
/// a [`ViewScale::DEFAULT`] camera at High quality.
///
/// Set each copy's [`InstancedCopy::size`](super::InstancedCopy::size) to
/// its bounding-sphere diameter in metres. The renderer's fade bands are
/// then exactly [`DistanceChain::ranges`] at every quality.
///
/// The table holds the game-level mesh (level 0), the lower mesh (level 1)
/// and, with `impostor`, the impostor (level 2, casting no shadows).
/// Without it, the lower mesh is drawn until the cull.
///
/// The renderer will take the chain directly in a later version, and this
/// function will go.
pub fn chain_lod(chain: &DistanceChain, impostor: bool) -> Lod {
    let distance = |threshold: f32| ViewScale::DEFAULT.get() / threshold;
    let mut levels = vec![
        LodLevel::new(0.0, chain.hysteresis, ()),
        LodLevel::new(distance(chain.lower_below), chain.hysteresis, ()),
    ];
    if impostor {
        let level = LodLevel::new(distance(chain.impostor_below), chain.hysteresis, ())
            .with_form(LevelForm::Impostor)
            .without_shadows();
        levels.push(level);
    }
    Lod::new(1.0, levels).with_cull(distance(chain.cull_below))
}

#[cfg(test)]
mod tests {
    use procgena_core::{LodRange, Quality};

    use super::*;

    /// The table, scaled as the renderer scales it (by a copy's size and
    /// the quality's distance scale), gives the chain's own fade bands at
    /// every quality, with and without the impostor.
    #[test]
    fn the_table_draws_the_chain() {
        let chain = DistanceChain::DEFAULT;
        let size = 2.7;
        let factor = DistanceChain::factor(size, ViewScale::DEFAULT);
        let close = |a: f32, b: f32| (a - b).abs() <= b.abs() * 1e-5 || a == b;
        let same = |a: &LodRange, b: &LodRange| {
            close(a.fade_in.start, b.fade_in.start)
                && close(a.fade_in.end, b.fade_in.end)
                && close(a.fade_out.start, b.fade_out.start)
                && close(a.fade_out.end, b.fade_out.end)
        };
        for impostor in [false, true] {
            let lod = chain_lod(&chain, impostor);
            lod.validate().unwrap();
            assert_eq!(lod.levels.len(), if impostor { 3 } else { 2 });
            assert!(lod.levels[..2].iter().all(|level| level.casts_shadows));
            for quality in Quality::ALL {
                let table = lod.ranges(size / lod.reference_size * quality.distance_scale());
                let [game, lower, last] = chain.ranges(factor, quality);
                let expected = if impostor {
                    vec![game, lower, last]
                } else {
                    let lower = LodRange {
                        fade_in: lower.fade_in,
                        fade_out: last.fade_out,
                    };
                    vec![game, lower]
                };
                assert_eq!(table.len(), expected.len());
                for (got, want) in table.iter().zip(&expected) {
                    assert!(same(got, want), "{quality:?}: {got:?} != {want:?}");
                }
            }
        }
        assert!(!chain_lod(&chain, true).levels[2].casts_shadows);
    }
}
