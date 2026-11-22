//! Ground, sun, sky and wind shared by every scene mode.

use super::viewport::ViewportActivity;
use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;
use procgena_plant::prelude::{PlantSun, PlantWind};
use procgena_tree::WindParams;
use procgena_tree_bevy::Wind;

/// Sky colour, also used for the distance fog.
const SKY: Color = Color::srgb(0.58, 0.725, 0.973);
/// Ground colour: earth with a little green.
const SOIL: Color = Color::srgb(0.36, 0.32, 0.2);
/// Width and depth of the ground plane, in world units.
const GROUND_SIZE: f32 = 600.0;
/// Shadows are drawn out to this distance from the camera, in world units.
const SHADOW_DISTANCE: f32 = 300.0;

/// Lighting, ground, wind and camera motion for the scene.
///
/// Replace it with [`ViewerCommand::SetEnvironment`](super::ViewerCommand::SetEnvironment).
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct EnvironmentSettings {
    /// Compass direction of the sun in degrees. 0 puts the sun on the +Z side,
    /// 90 on the +X side.
    pub sun_azimuth: f32,
    /// Height of the sun above the horizon in degrees, 0 to 90.
    pub sun_elevation: f32,
    /// Whether the ground plane is drawn.
    pub show_ground: bool,
    /// Whether the camera turns slowly around the subject on its own.
    pub auto_rotate: bool,
    /// Auto-rotation speed in degrees per second. Negative values turn the other way.
    pub rotate_speed: f32,
    /// Whether trees and plants move in the wind.
    pub wind: bool,
    /// Side length of the square plant field, in metres, for
    /// [`SceneMode::Plants`](super::SceneMode::Plants). For example, 40 makes a
    /// 40-by-40-metre field. Must be greater than zero.
    pub field_size: f32,
}

impl Default for EnvironmentSettings {
    fn default() -> Self {
        Self {
            sun_azimuth: 90.0,
            sun_elevation: 30.0,
            show_ground: true,
            auto_rotate: false,
            rotate_speed: 10.0,
            wind: true,
            field_size: 40.0,
        }
    }
}

/// The light marked as the sun.
#[derive(Component)]
pub(crate) struct Sun;

/// The ground plane.
#[derive(Component)]
pub(crate) struct Ground;

/// Spawns the sun and the ground, and sets the sky and ambient light.
pub(crate) fn spawn_environment(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(ClearColor(SKY));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: 400.0,
        ..default()
    });

    commands.spawn((
        Sun,
        PlantSun,
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        CascadeShadowConfigBuilder {
            maximum_distance: SHADOW_DISTANCE,
            ..default()
        }
        .build(),
    ));
    commands.spawn((
        Ground,
        Mesh3d(meshes.add(Plane3d::default().mesh().size(GROUND_SIZE, GROUND_SIZE))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: SOIL,
            perceptual_roughness: 1.0,
            ..default()
        })),
    ));
}

/// The fog added to the scene camera, fading distant ground into the sky.
pub(crate) fn sky_fog() -> DistanceFog {
    DistanceFog {
        color: SKY,
        falloff: FogFalloff::Exponential { density: 0.0006 },
        ..default()
    }
}

/// Applies changed settings to the sun, ground, wind and render activity.
pub(crate) fn apply_environment(
    settings: Res<EnvironmentSettings>,
    mut sun: Query<&mut Transform, With<Sun>>,
    mut ground: Query<&mut Visibility, With<Ground>>,
    mut tree_wind: ResMut<Wind>,
    mut grass_wind: ResMut<PlantWind>,
    mut activity: ResMut<ViewportActivity>,
) {
    if !settings.is_changed() {
        return;
    }

    // Sun direction: the light shines from its position toward the origin.
    let elevation = settings.sun_elevation.clamp(0.0, 90.0).to_radians();
    let azimuth = settings.sun_azimuth.to_radians();
    let toward_sun = Vec3::new(
        elevation.cos() * azimuth.sin(),
        elevation.sin(),
        elevation.cos() * azimuth.cos(),
    );
    for mut transform in &mut sun {
        *transform = Transform::from_translation(toward_sun).looking_at(Vec3::ZERO, Vec3::Y);
    }

    // Ground.
    for mut visibility in &mut ground {
        *visibility = if settings.show_ground {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }

    // Wind for trees and grass.
    *tree_wind = Wind(if settings.wind {
        WindParams::default()
    } else {
        WindParams {
            strength: Vec3::ZERO,
            ..default()
        }
    });
    *grass_wind = if settings.wind {
        PlantWind::BREEZE
    } else {
        PlantWind {
            strength: 0.0,
            sway: 0.0,
            flutter: 0.0,
            ..PlantWind::BREEZE
        }
    };

    // Animation needs a frame every refresh; otherwise render on change only.
    activity.continuous = settings.wind || settings.auto_rotate;
    activity.request_frames(ViewportActivity::SETTLE_FRAMES);
}
