//! Ecotope subject: one Ecotope placed over a square patch of flat or
//! rolling ground, drawn with `procgena_biome_bevy`.

use std::sync::Arc;

use bevy::math::DVec2;
use bevy::prelude::*;
use procgena_biome::{Ecotope, EcotopePreset};
use procgena_biome_bevy::{
    EcotopeFinished, EcotopeGroundPatch, EcotopeGroundWeights, EcotopeParams, EcotopePlacing,
    GroundPatch, rolling_hills,
};
use procgena_core::scatter::ScatterArea;
use procgena_core::{FlatGround, Ground, HeightGrid};
use procgena_terrain::ground::{
    GroundMix, GroundPainter, GroundSite, GroundSource, GroundStack, MaterialCatalogue, OnGround,
};
use procgena_terrain::{FeatureTier, Measures, RegionId};

use super::environment::EnvironmentSettings;
use super::orbit::{OrbitCamera, OrbitLimits};
use super::overlay::{Overlay, OverlayArea, OverlayGround, OverlaySource};
use super::protocol::ViewerEvent;
use super::subject::{Detail, SceneState, Subject};
use super::viewport::ViewportActivity;
use super::{AddSubject, Shown};

/// Spacing of the ground's height samples and mesh vertices, in metres.
const GROUND_CELL: f64 = 0.5;

/// Ground drawn beyond the patch on each side, in metres, so copies at the
/// edge stand on something.
const GROUND_MARGIN: f64 = 4.0;

/// What the Ecotope subject shows: an Ecotope over a square of ground.
#[derive(Debug, Clone, PartialEq)]
pub struct EcotopeSettings {
    /// The Ecotope, placed with its own
    /// [`default_seed`](Ecotope::default_seed).
    pub ecotope: Ecotope,
    /// The ground it grows on.
    pub ground: EcotopeGround,
    /// Side of the square patch it covers, in metres, centred on the
    /// origin. The ground reaches a few metres beyond it.
    pub size: f32,
}

impl Default for EcotopeSettings {
    /// The pine forest floor over 40 metres of rolling hills.
    fn default() -> Self {
        Self {
            ecotope: EcotopePreset::pine_forest_floor().ecotope.clone(),
            ground: EcotopeGround::default(),
            size: 40.0,
        }
    }
}

/// The ground an Ecotope is shown on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EcotopeGround {
    /// Level ground at height 0.
    Flat,
    /// Long swells about 2 metres high with a shallower ripple across them.
    #[default]
    RollingHills,
}

/// An Ecotope's Scatters over a square of ground, from [`EcotopeSettings`].
///
/// A changed Ecotope is placed again in the background and only the
/// Scatters the change can move are drawn again; a
/// [`ViewerEvent::EcotopeBuilt`] follows. If it can't be placed, the last
/// drawing stays. The Ecotope draws its own ground, and each copy shows the
/// detail level that suits its distance, whatever detail is chosen.
pub struct EcotopeSubject;

impl Subject for EcotopeSubject {
    type Settings = EcotopeSettings;
    const NAME: &'static str = "Ecotope";
    const OWN_GROUND: bool = true;

    fn spawn(world: &mut World, settings: &EcotopeSettings) -> Entity {
        let ground = build_ground(settings);
        world.spawn(params(settings, ground, base(settings))).id()
    }

    fn update(world: &mut World, shown: &mut Shown, old: &EcotopeSettings, new: &EcotopeSettings) {
        // Keep the ground while its choice and size stay the same, so an
        // edit to the Ecotope alone redraws only the Scatters it changes,
        // and its base look while its seed stays the same.
        let current = world.get::<EcotopeParams>(shown.entity);
        let ground = current
            .filter(|_| old.ground == new.ground && old.size == new.size)
            .map(|params| params.ground.clone())
            .unwrap_or_else(|| build_ground(new));
        let base = current
            .and_then(|params| params.patch.as_ref())
            .filter(|_| old.ecotope.default_seed == new.ecotope.default_seed)
            .map(|patch| patch.base.clone())
            .unwrap_or_else(|| base(new));
        shown.start_build(world);
        world
            .entity_mut(shown.entity)
            .insert(params(new, ground, base));
    }

    fn view(_: &World, settings: &EcotopeSettings) -> OrbitCamera {
        // Look down over the patch from beyond its corner.
        OrbitCamera {
            focus: Vec3::new(0.0, 1.0, 0.0),
            yaw: 0.6,
            pitch: 0.5,
            distance: 1.1 * settings.size.max(5.0),
            limits: OrbitLimits {
                distance: (1.0, (3.0 * settings.size).max(60.0)),
                pitch: (-0.05, 1.5),
            },
        }
    }

    fn detail(_: Detail) -> Detail {
        Detail::default()
    }
}

/// The component that places and draws the Ecotope `settings` on `ground`,
/// with its ground layers over `base`.
fn params(
    settings: &EcotopeSettings,
    ground: Arc<dyn Ground>,
    base: Arc<GroundStack>,
) -> EcotopeParams {
    EcotopeParams {
        ecotope: Arc::new(settings.ecotope.clone()),
        seed: settings.ecotope.default_seed,
        ground,
        area: ScatterArea::new(DVec2::ZERO, Vec2::splat(settings.size)),
        coverage: None,
        patch: Some(GroundPatch {
            base,
            margin: GROUND_MARGIN,
            cell: GROUND_CELL,
        }),
    }
}

/// The ground the Ecotope's layers lie over: bare terrain's look, seeded
/// with the Ecotope's own seed.
fn base(settings: &EcotopeSettings) -> Arc<GroundStack> {
    Arc::new(GroundStack::bare(settings.ecotope.default_seed, 1.0))
}

/// Adds the Ecotope subject, the ground drawn under it, and its reports.
pub(crate) fn plugin(app: &mut App) {
    app.add_subject::<EcotopeSubject>()
        .add_systems(Update, (show_ground, render_while_placing, offer_overlay))
        .add_observer(report_ecotope);
}

/// The overlays an Ecotope offers: its ground has one region and no water
/// or roads, so only what it is made of.
const ECOTOPE_OVERLAYS: [Overlay; 1] = [Overlay::Ground];

/// Offers the ground overlay over each newly painted ground patch, read
/// from the weights the patch is drawn with.
fn offer_overlay(
    mut commands: Commands,
    patches: Query<(Entity, &EcotopeGroundWeights, &ChildOf), Changed<EcotopeGroundWeights>>,
    params: Query<&EcotopeParams>,
) {
    for (patch, painted, parent) in &patches {
        let Ok(params) = params.get(parent.parent()) else {
            continue;
        };
        let ground = Arc::new(PatchGround(params.ground.clone()));
        let painter = Arc::new(PatchPainter(painted.clone()));
        commands.entity(parent.parent()).insert(OverlaySource::new(
            ground,
            painter,
            &ECOTOPE_OVERLAYS,
        ));
        commands.entity(patch).insert(OverlayArea {
            min: painted.min,
            side: (painted.max - painted.min).max_element(),
            tier: FeatureTier::Fine,
        });
    }
}

/// The ground under an Ecotope, for the overlay: one region, no water.
struct PatchGround(Arc<dyn Ground>);

impl GroundSource for PatchGround {
    fn site(&self, p: DVec2, tier: FeatureTier) -> GroundSite {
        OnGround(&*self.0).site(p, tier)
    }
}

impl OverlayGround for PatchGround {
    fn region(&self, _: DVec2) -> RegionId {
        RegionId(0)
    }

    fn height(&self, p: DVec2, _: FeatureTier) -> f32 {
        self.0.height(p)
    }

    /// None: the Ecotope's ground has no water or roads to measure from.
    fn measures(&self, _: DVec2, _: FeatureTier) -> Option<Measures> {
        None
    }
}

/// The patch's painted ground as a painter: at each point, all of the
/// material with the most weight at the nearest texel.
struct PatchPainter(EcotopeGroundWeights);

impl GroundPainter for PatchPainter {
    fn catalogue(&self) -> &MaterialCatalogue {
        &self.0.catalogue
    }

    fn paint(&self, p: DVec2, _: &GroundSite) -> GroundMix {
        let weights = &self.0.weights;
        let last = (weights.size() - 1).as_dvec2();
        let t = ((p - self.0.min) / (self.0.max - self.0.min)).clamp(DVec2::ZERO, DVec2::ONE);
        let texel = (t * last).round();
        let at = weights.texel(texel.x as u32, texel.y as u32);
        let slot = (0..weights.palette().len())
            .max_by_key(|&k| (at[k], std::cmp::Reverse(k)))
            .expect("painted ground has a material");
        GroundMix::single(weights.palette()[slot])
    }
}

/// The ground `settings` asks for, a little wider than its patch.
fn build_ground(settings: &EcotopeSettings) -> Arc<dyn Ground> {
    match settings.ground {
        EcotopeGround::Flat => Arc::new(FlatGround),
        EcotopeGround::RollingHills => {
            let half = ground_half(settings.size);
            Arc::new(HeightGrid::from_fn(
                DVec2::splat(-half),
                DVec2::splat(half),
                GROUND_CELL,
                rolling_hills,
            ))
        }
    }
}

/// Half the side of the square of ground drawn around a patch `size`
/// metres across.
fn ground_half(size: f32) -> f64 {
    f64::from(size.max(1.0)) * 0.5 + GROUND_MARGIN
}

/// Shows or hides the Ecotope's ground with the environment's Ground
/// switch, as it is drawn and whenever the switch changes.
fn show_ground(
    environment: Res<EnvironmentSettings>,
    mut grounds: Query<(&mut Visibility, Ref<EcotopeGroundPatch>)>,
) {
    for (mut visibility, patch) in &mut grounds {
        if environment.is_changed() || patch.is_added() {
            visibility.set_if_neq(ground_visibility(&environment));
        }
    }
}

/// The ground's visibility for the environment's Ground switch.
fn ground_visibility(environment: &EnvironmentSettings) -> Visibility {
    if environment.show_ground {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    }
}

/// Requests more frames while an Ecotope is being placed.
fn render_while_placing(
    placing: Query<(), With<EcotopePlacing>>,
    mut activity: ResMut<ViewportActivity>,
) {
    if !placing.is_empty() {
        activity.request_frames(ViewportActivity::SETTLE_FRAMES);
    }
}

/// Reports each placement of the Ecotope shown.
fn report_ecotope(
    finished: On<EcotopeFinished>,
    state: Res<SceneState>,
    mut events: MessageWriter<ViewerEvent>,
) {
    if state
        .get::<EcotopeSubject>()
        .is_some_and(|(shown, _)| shown.entity == finished.entity)
    {
        events.write(ViewerEvent::EcotopeBuilt(finished.result.clone()));
    }
}
