//! Ecotope subject: one Ecotope placed over a square patch of flat or
//! rolling ground, drawn with `procgena_biome_bevy`.

use std::sync::Arc;

use bevy::math::DVec2;
use bevy::prelude::*;
use procgena_biome::{Ecotope, EcotopePreset};
use procgena_biome_bevy::{
    EcotopeFinished, EcotopeParams, EcotopePlacing, ground_mesh, rolling_hills,
};
use procgena_core::scatter::ScatterArea;
use procgena_core::{FlatGround, Ground, HeightGrid};

use super::environment::{EnvironmentSettings, SOIL};
use super::orbit::{OrbitCamera, OrbitLimits};
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
        world.spawn(params(settings, ground)).id()
    }

    fn update(world: &mut World, shown: &mut Shown, old: &EcotopeSettings, new: &EcotopeSettings) {
        // Keep the ground while its choice and size stay the same, so an
        // edit to the Ecotope alone redraws only the Scatters it changes.
        let kept = world
            .get::<EcotopeParams>(shown.entity)
            .filter(|_| old.ground == new.ground && old.size == new.size)
            .map(|params| params.ground.clone());
        let ground = kept.unwrap_or_else(|| build_ground(new));
        shown.start_build(world);
        world.entity_mut(shown.entity).insert(params(new, ground));
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

/// The component that places and draws the Ecotope `settings` on `ground`.
fn params(settings: &EcotopeSettings, ground: Arc<dyn Ground>) -> EcotopeParams {
    EcotopeParams {
        ecotope: Arc::new(settings.ecotope.clone()),
        seed: settings.ecotope.default_seed,
        ground,
        area: ScatterArea::new(DVec2::ZERO, Vec2::splat(settings.size)),
        coverage: None,
    }
}

/// Adds the Ecotope subject, the ground drawn under it, and its reports.
pub(crate) fn plugin(app: &mut App) {
    app.add_subject::<EcotopeSubject>()
        .add_systems(
            Update,
            ((draw_ground, show_ground).chain(), render_while_placing),
        )
        .add_observer(report_ecotope);
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

/// Marks the ground mesh drawn under an Ecotope, with the ground it shows.
#[derive(Component)]
struct EcotopeGroundMesh(Arc<dyn Ground>);

/// Gives each shown Ecotope a mesh of its ground, and rebuilds it when the
/// ground changes.
fn draw_ground(
    mut commands: Commands,
    ecotopes: Query<(Entity, &EcotopeParams, Option<&Children>), Changed<EcotopeParams>>,
    drawn: Query<&EcotopeGroundMesh>,
    environment: Res<EnvironmentSettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (entity, params, children) in &ecotopes {
        // Keep the mesh while the ground is the same.
        let old = children
            .into_iter()
            .flatten()
            .find(|child| drawn.contains(**child));
        if let Some(old) = old {
            if drawn
                .get(*old)
                .is_ok_and(|mesh| Arc::ptr_eq(&mesh.0, &params.ground))
            {
                continue;
            }
            commands.entity(*old).despawn();
        }

        // A mesh of the ground under the patch and a margin around it.
        let half = ground_half(params.area.size.max_element());
        let centre = params.area.centre;
        let mesh = ground_mesh(
            &*params.ground,
            centre - DVec2::splat(half),
            centre + DVec2::splat(half),
            GROUND_CELL,
        );
        commands.spawn((
            EcotopeGroundMesh(params.ground.clone()),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: SOIL,
                perceptual_roughness: 1.0,
                ..default()
            })),
            ground_visibility(&environment),
            ChildOf(entity),
        ));
    }
}

/// Shows or hides the Ecotope's ground with the environment's Ground switch.
fn show_ground(
    environment: Res<EnvironmentSettings>,
    mut grounds: Query<&mut Visibility, With<EcotopeGroundMesh>>,
) {
    if !environment.is_changed() {
        return;
    }
    for mut visibility in &mut grounds {
        visibility.set_if_neq(ground_visibility(&environment));
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
