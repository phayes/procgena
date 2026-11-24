//! Fruit subject: a single fruit or flower cluster, hanging from or standing
//! on its attachment point.

use std::sync::Arc;

use bevy::prelude::*;
use procgena_fruit::{Facing, Fruit, FruitOptions};
use procgena_fruit_bevy::{FruitFinished, FruitParams};

use super::orbit::{OrbitCamera, OrbitLimits};
use super::protocol::{BuildStats, MeshSize, ViewerEvent};
use super::subject::{SceneState, Subject};
use super::{AddSubject, Shown};

/// The camera's nearest allowed distance, kept clear of the camera's 0.1 m
/// near clipping plane.
const FRUIT_NEAREST: f32 = 0.12;

/// The height above the ground at which the cluster is attached.
pub(crate) const FRUIT_HEIGHT: f32 = 1.0;

/// A single fruit or flower cluster, from its [`FruitOptions`].
///
/// A changed cluster grows again at once; invalid settings keep the
/// previous cluster. [`ViewerEvent::FruitBuilt`] reports each cluster
/// grown. A cluster hangs in still air.
pub struct FruitSubject;

impl Subject for FruitSubject {
    type Settings = FruitOptions;
    const NAME: &'static str = "fruit";
    const STILL_AIR: bool = true;

    fn spawn(world: &mut World, settings: &FruitOptions) -> Entity {
        world
            .spawn((
                FruitParams(Arc::new(settings.clone())),
                fruit_transform(settings),
            ))
            .id()
    }

    fn update(world: &mut World, shown: &mut Shown, _: &FruitOptions, new: &FruitOptions) {
        world
            .entity_mut(shown.entity)
            .insert((FruitParams(Arc::new(new.clone())), fruit_transform(new)));
    }

    fn view(_: &World, options: &FruitOptions) -> OrbitCamera {
        let radius = Fruit::new(options.clone(), options.default_seed, 0)
            .map_or(2.0 * options.size, |fruit| fruit.radius())
            .max(0.02);
        OrbitCamera {
            focus: Vec3::new(0.0, FRUIT_HEIGHT, 0.0),
            yaw: 0.6,
            pitch: 0.2,
            distance: (4.0 * radius).max(FRUIT_NEAREST),
            limits: OrbitLimits {
                distance: (FRUIT_NEAREST, (200.0 * radius).max(2.0)),
                pitch: (-1.2, 1.5),
            },
        }
    }
}

/// Where a cluster is shown: attached `FRUIT_HEIGHT` above the ground,
/// turned as it would grow on a level branch. A cluster's own +Y is its
/// facing, so hanging clusters are turned upside down, and outward ones face
/// the camera's starting side with their down still down.
fn fruit_transform(options: &FruitOptions) -> Transform {
    let rotation = match options.facing {
        Facing::Up => Quat::IDENTITY,
        Facing::Hang => Quat::from_rotation_x(std::f32::consts::PI),
        Facing::Outward => {
            Quat::from_rotation_y(std::f32::consts::PI)
                * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
        }
    };
    Transform::from_xyz(0.0, FRUIT_HEIGHT, 0.0).with_rotation(rotation)
}

/// Adds the fruit subject.
pub(crate) fn plugin(app: &mut App) {
    app.add_subject::<FruitSubject>().add_observer(report_fruit);
}

/// Reports a grown fruit or flower cluster's mesh counts per detail level,
/// or why its settings were rejected.
fn report_fruit(
    event: On<FruitFinished>,
    state: Res<SceneState>,
    mut events: MessageWriter<ViewerEvent>,
) {
    let Some((shown, _)) = state.get::<FruitSubject>() else {
        return;
    };
    if event.entity != shown.entity {
        return;
    }
    let result = event.result.clone().map(|sizes| BuildStats {
        levels: sizes
            .into_iter()
            .map(|(vertices, triangles)| MeshSize {
                vertices,
                triangles,
            })
            .collect(),
        build_time: shown.requested.elapsed(),
    });
    events.write(ViewerEvent::FruitBuilt(result));
}
