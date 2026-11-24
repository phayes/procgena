//! Ecotope mode: one Ecotope placed over a square patch of flat or rolling
//! ground, drawn with `procgena_biome_bevy`.

use std::sync::Arc;

use bevy::math::DVec2;
use bevy::prelude::*;
use procgena_biome_bevy::{EcotopeFinished, EcotopeParams, ground_mesh, rolling_hills};
use procgena_core::scatter::ScatterArea;
use procgena_core::{FlatGround, Ground, HeightGrid};

use super::environment::{EnvironmentSettings, SOIL};
use super::protocol::{EcotopeGround, EcotopeSettings, SceneMode, ViewerEvent};
use super::subject::SceneState;

/// Spacing of the ground's height samples and mesh vertices, in metres.
const GROUND_CELL: f64 = 0.5;

/// Ground drawn beyond the patch on each side, in metres, so copies at the
/// edge stand on something.
const GROUND_MARGIN: f64 = 4.0;

/// Draws the ground under the Ecotope shown and reports each placement.
pub(crate) struct EcotopeScenePlugin;

impl Plugin for EcotopeScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (draw_ground, show_ground).chain())
            .add_observer(report_ecotope);
    }
}

/// The Ecotope mode's settings, and the ground built for them.
///
/// The ground is kept while its choice and size stay the same, so an edit
/// to the Ecotope alone redraws only the Scatters it changes.
pub(crate) struct EcotopeState {
    settings: Arc<EcotopeSettings>,
    ground: Arc<dyn Ground>,
}

impl EcotopeState {
    /// The state for `settings`, with its ground built.
    pub(crate) fn new(settings: Arc<EcotopeSettings>) -> Self {
        let ground = build_ground(&settings);
        Self { settings, ground }
    }

    /// The settings shown.
    pub(crate) fn settings(&self) -> &EcotopeSettings {
        &self.settings
    }

    /// Replaces the settings, building new ground only if its choice or
    /// size changed.
    pub(crate) fn set(&mut self, settings: Arc<EcotopeSettings>) {
        if settings.ground != self.settings.ground || settings.size != self.settings.size {
            self.ground = build_ground(&settings);
        }
        self.settings = settings;
    }

    /// The component that places and draws the Ecotope.
    pub(crate) fn params(&self) -> EcotopeParams {
        let settings = &self.settings;
        EcotopeParams {
            ecotope: Arc::new(settings.ecotope.clone()),
            seed: settings.ecotope.default_seed,
            ground: self.ground.clone(),
            area: ScatterArea::new(DVec2::ZERO, Vec2::splat(settings.size)),
            coverage: None,
        }
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

/// Reports each placement of the Ecotope shown.
fn report_ecotope(
    finished: On<EcotopeFinished>,
    state: Res<SceneState>,
    mut events: MessageWriter<ViewerEvent>,
) {
    if state.mode() == SceneMode::Ecotope && finished.entity == state.subject() {
        events.write(ViewerEvent::EcotopeBuilt(finished.result.clone()));
    }
}
