//! The sun, the sky colour and distance fog. The full atmospherics come
//! later; this is the plain light the first milestones are judged in.

use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;
use procgena_plant_bevy::PlantSun;

use crate::Launch;
use crate::camera::MainCamera;

/// The colour of the sky, and of the fog that fades the far land into it.
const SKY: Color = Color::srgb(0.62, 0.74, 0.86);

/// How dense the distance fog is, per metre.
const FOG_DENSITY: f32 = 0.0009;

/// How far the sun's shadows reach, in metres.
const SHADOW_DISTANCE: f32 = 220.0;

/// Spawns the sun and gives the camera its fog.
pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(SKY))
            .insert_resource(GlobalAmbientLight {
                color: Color::srgb(0.85, 0.9, 1.0),
                brightness: 500.0,
                ..default()
            })
            .add_systems(Startup, (spawn_sun, fog_the_camera));
    }
}

/// Spawns a morning sun, low in the south-east, casting shadows sharpest
/// near the camera unless the launch turned shadows off.
fn spawn_sun(mut commands: Commands, launch: Res<Launch>) {
    commands.spawn((
        PlantSun,
        DirectionalLight {
            illuminance: 15_000.0,
            color: Color::srgb(1.0, 0.95, 0.86),
            shadow_maps_enabled: launch.shadows,
            ..default()
        },
        CascadeShadowConfigBuilder {
            num_cascades: 4,
            first_cascade_far_bound: 12.0,
            maximum_distance: SHADOW_DISTANCE,
            ..default()
        }
        .build(),
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, 2.4, -0.62, 0.0)),
    ));
}

/// Fades the far land into the sky.
fn fog_the_camera(mut commands: Commands, camera: Single<Entity, With<MainCamera>>) {
    commands.entity(*camera).insert(DistanceFog {
        color: SKY,
        directional_light_color: Color::srgba(1.0, 0.92, 0.75, 0.4),
        directional_light_exponent: 24.0,
        falloff: FogFalloff::Exponential {
            density: FOG_DENSITY,
        },
    });
}
