//! The one camera the demo draws through.

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use procgena_terrain_bevy::TerrainViewer;

use crate::Launch;
use crate::settings::DemoOptions;

/// Marks the camera the player sees through. The terrain streams around
/// it.
#[derive(Component, Debug, Default)]
pub struct MainCamera;

/// The image the camera draws into when headless.
#[derive(Resource)]
pub struct Offscreen(pub Handle<Image>);

/// Spawns the [`MainCamera`].
pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, spawn_camera);
    }
}

/// Spawns the camera, drawing into an image when headless.
fn spawn_camera(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    launch: Res<Launch>,
    options: Res<DemoOptions>,
) {
    let mut camera = commands.spawn((
        MainCamera,
        TerrainViewer,
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: options.camera.fov.to_radians(),
            ..default()
        }),
        Transform::from_xyz(0.0, 2.0, 8.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
    ));
    if launch.headless {
        let size = launch.resolution;
        let image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
        let handle = images.add(image);
        camera.insert(RenderTarget::Image(handle.clone().into()));
        commands.insert_resource(Offscreen(handle));
    }
}
