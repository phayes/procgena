//! How a slot's drawing chain becomes the renderer's fade bands.

use procgena_core::chain::{ChainError, DistanceChain, Step, ViewScale};
use procgena_core::{LodRange, Quality};

use super::InstancedSlot;

/// One step a slot draws: its fade bands for a copy 1 m across, seen
/// through a [`ViewScale::DEFAULT`] camera at High quality, and whether it
/// draws into shadow maps.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SlotStep {
    pub(super) range: LodRange,
    pub(super) casts_shadows: bool,
}

/// The steps `slot` draws, nearest first, indexed by [`Step::index`].
///
/// The game level and lower mesh are always drawn. The impostor is drawn
/// when the slot has an [`impostor`](InstancedSlot::impostor) step;
/// without one, the lower mesh is drawn until the cull. A slot that
/// doesn't [`cull`](InstancedSlot::cull) draws its last step at every
/// distance beyond.
///
/// The bands are linear in a copy's size, the quality's distance scale and
/// the view's scale, so the renderer multiplies them by all three on the
/// GPU and they stay exactly [`DistanceChain::ranges`].
pub(super) fn slot_steps(slot: &InstancedSlot) -> Result<Vec<SlotStep>, ChainError> {
    let chain = &slot.chain;
    chain.validate()?;
    let factor = DistanceChain::factor(1.0, ViewScale::DEFAULT);
    let [game, lower, impostor] = chain.ranges(factor, Quality::High);
    let step = |range, step: Step| SlotStep {
        range,
        casts_shadows: step.casts_shadows(),
    };
    let mut steps = if slot.impostor {
        vec![
            step(game, Step::Game),
            step(lower, Step::Lower),
            step(impostor, Step::Impostor),
        ]
    } else {
        let lower = LodRange {
            fade_in: lower.fade_in,
            fade_out: impostor.fade_out,
        };
        vec![step(game, Step::Game), step(lower, Step::Lower)]
    };
    if !slot.cull
        && let Some(last) = steps.last_mut()
    {
        last.range.fade_out = f32::INFINITY..f32::INFINITY;
    }
    Ok(steps)
}

/// How much larger things look through a view with `clip_from_view` than
/// through a [`ViewScale::DEFAULT`] camera: what the renderer multiplies
/// every band by for that view.
///
/// Orthographic views draw at the default scale.
pub(super) fn view_scale_ratio(clip_from_view: &bevy::math::Mat4) -> f32 {
    // A perspective projection has 1 / tan(fov_y / 2) on its diagonal's
    // second entry and 0 in its last; an orthographic one has 1 there.
    let perspective = clip_from_view.w_axis.w == 0.0;
    let focal = clip_from_view.y_axis.y;
    if !perspective || !focal.is_finite() || focal <= 0.0 {
        return 1.0;
    }
    (focal * 0.5) / ViewScale::DEFAULT.get()
}

#[cfg(test)]
mod tests {
    use bevy::camera::{CameraProjection, OrthographicProjection, PerspectiveProjection};
    use bevy::math::Mat4;
    use procgena_core::{GeneratorKind, OptionsHash, PoolId, SpecimenId};

    /// The projection Bevy gives a perspective camera with `fov`.
    fn perspective(fov: f32) -> Mat4 {
        PerspectiveProjection {
            fov,
            ..Default::default()
        }
        .get_clip_from_view()
    }

    use super::super::{InstancedPart, PartMesh};
    use super::*;

    fn slot(impostor: bool) -> InstancedSlot {
        let part = |step| InstancedPart {
            step,
            mesh: PartMesh::Mesh(Default::default()),
            material: bevy::asset::Handle::<bevy::pbr::StandardMaterial>::default().untyped(),
        };
        let mut parts = vec![part(Step::Game), part(Step::Lower)];
        if impostor {
            parts.push(part(Step::Impostor));
        }
        InstancedSlot {
            specimen: SpecimenId::new(
                7,
                PoolId::new(
                    GeneratorKind::from_key("procgena_core_bevy.test.kind"),
                    OptionsHash(2),
                ),
                0,
            ),
            piece: 0,
            chain: DistanceChain::DEFAULT,
            cull: true,
            impostor,
            parts,
            bounds: (bevy::math::Vec3::ZERO, 1.0),
        }
    }

    #[test]
    fn a_slot_without_its_own_cull_draws_its_last_step_at_any_distance() {
        for impostor in [false, true] {
            let culled = slot_steps(&slot(impostor)).unwrap();
            let mut uncut = slot(impostor);
            uncut.cull = false;
            let uncut = slot_steps(&uncut).unwrap();
            let last = uncut.len() - 1;
            assert_eq!(uncut[..last], culled[..last], "nearer steps are unchanged");
            assert_eq!(uncut[last].range.fade_in, culled[last].range.fade_in);
            assert!(uncut[last].range.is_drawn(1e9));
            assert!(!culled[last].range.is_drawn(1e9));
        }
    }

    /// The bands, scaled as the renderer scales them (by a copy's size, the
    /// quality's distance scale and the view's scale), are the chain's own
    /// fade bands at every quality and through every lens, with and
    /// without the impostor.
    #[test]
    fn the_bands_are_the_chains_own() {
        let chain = DistanceChain::DEFAULT;
        let size = 2.7;
        let close = |a: f32, b: f32| (a - b).abs() <= b.abs() * 1e-5 || a == b;
        let same = |a: &LodRange, b: &LodRange| {
            close(a.fade_in.start, b.fade_in.start)
                && close(a.fade_in.end, b.fade_in.end)
                && close(a.fade_out.start, b.fade_out.start)
                && close(a.fade_out.end, b.fade_out.end)
        };
        for fov in [0.4_f32, std::f32::consts::FRAC_PI_4, 1.4] {
            let view = ViewScale::perspective(fov);
            let ratio = view_scale_ratio(&perspective(fov));
            let factor = DistanceChain::factor(size, view);
            for impostor in [false, true] {
                let steps = slot_steps(&slot(impostor)).unwrap();
                assert_eq!(steps.len(), if impostor { 3 } else { 2 });
                for quality in Quality::ALL {
                    let scale = size * quality.distance_scale() * ratio;
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
                    for (got, want) in steps.iter().zip(&expected) {
                        let got = LodRange {
                            fade_in: got.range.fade_in.start * scale..got.range.fade_in.end * scale,
                            fade_out: got.range.fade_out.start * scale
                                ..got.range.fade_out.end * scale,
                        };
                        assert!(same(&got, want), "{fov} {quality:?}: {got:?} != {want:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn only_the_mesh_steps_cast_shadows() {
        let steps = slot_steps(&slot(true)).unwrap();
        let shadows: Vec<bool> = steps.iter().map(|step| step.casts_shadows).collect();
        assert_eq!(shadows, [true, true, false]);
    }

    #[test]
    fn an_invalid_chain_is_refused() {
        let mut slot = slot(true);
        slot.chain.impostor_below = slot.chain.lower_below * 2.0;
        assert!(slot_steps(&slot).is_err());
    }

    #[test]
    fn orthographic_views_draw_at_the_default_scale() {
        let ortho = OrthographicProjection::default_3d().get_clip_from_view();
        assert_eq!(view_scale_ratio(&ortho), 1.0);
        let default = PerspectiveProjection::default().get_clip_from_view();
        assert!((view_scale_ratio(&default) - 1.0).abs() < 1e-5);
    }
}
