//! A meadow driven by a wind model of your own instead of the built-in one.
//!
//! `PlantPlugin { wind: false, ..default() }` leaves the built-in wind out.
//! From then on the grass reads `PlantWindFrame`, and this example writes it
//! from a tiny `Weather` resource in `PlantWindSystems::Sample`. The blades
//! and the `FarPlantMaterial` ground both follow the same frame.
//!
//! ```text
//! cargo run --release --example custom_wind
//! cargo run --release --example custom_wind -- --fixed-time --screenshot shots/custom_wind.png
//! ```

mod common;

use bevy::camera_controller::free_camera::{FreeCamera, FreeCameraPlugin};
use bevy::prelude::*;
use procgena_plant_bevy::prelude::*;

fn main() {
    let mut app = App::new();
    common::ExampleArgs::parse().install(&mut app, "procgena_plant: custom wind");
    app.add_plugins((
        PlantPlugins.set(PlantPlugin {
            // The built-in gusts, sway and flutter are off. We write the frame.
            wind: false,
            ..default()
        }),
        FreeCameraPlugin,
    ))
    .insert_resource(Weather::default())
    .add_systems(Startup, setup)
    .add_systems(Update, drive_wind.in_set(PlantWindSystems::Sample))
    .run();
}

/// What a weather system would provide: a direction, a strength and a wind
/// speed. This one is constant; [`drive_wind`] adds the movement over time.
#[derive(Resource, Debug, Clone, Copy)]
struct Weather {
    /// Direction the weather system says the wind blows toward, in world XZ.
    direction: Vec2,
    /// Strength the weather system reports, 0 to 1.
    strength: f32,
    /// How fast gusts cross the field, in metres per second.
    speed: f32,
}

impl Default for Weather {
    fn default() -> Self {
        Self {
            direction: Vec2::new(1.0, 0.2),
            strength: 0.55,
            speed: 5.0,
        }
    }
}

/// The whole custom wind model: turn the weather's direction slowly and
/// breathe its strength, then hand the frame to the grass.
fn drive_wind(time: Res<Time>, weather: Res<Weather>, mut wind: ResMut<PlantWindFrame>) {
    let seconds = time.elapsed_secs();
    let turn = seconds * 0.08;
    let (sin, cos) = turn.sin_cos();
    let direction = Vec2::new(
        weather.direction.x * cos - weather.direction.y * sin,
        weather.direction.x * sin + weather.direction.y * cos,
    );
    let breath = 0.75 + 0.25 * (seconds * 0.9).sin();
    wind.direction = direction;
    wind.strength = weather.strength * breath;
    // Roll the gusts across the field along the turning wind. The frame
    // starts as `PlantWind::BREEZE`'s, which sets the gust size and the sway.
    wind.advance(time.delta_secs(), weather.speed);
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut far_materials: ResMut<Assets<FarPlantMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        FreeCamera::default(),
        Transform::from_xyz(-8.0, 4.0, 14.0).looking_at(Vec3::new(0.0, 0.5, 0.0), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    let field = PlantScatter::from_preset(&PlantScatterPreset::MEADOW, Vec2::splat(100.0));
    let soil = StandardMaterial {
        base_color: Color::srgb(0.2, 0.15, 0.09),
        perceptual_roughness: 0.95,
        ..default()
    };
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(300.0, 300.0))),
        MeshMaterial3d(far_materials.add(FarPlantMaterial {
            base: soil,
            extension: FarPlants::new(&field.options),
        })),
    ));
    commands.spawn(field);
}
