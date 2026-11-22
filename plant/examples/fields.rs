//! Browse all field presets side by side and edit their appearance live.
//! Each plot follows its `.field.ron` file in `assets/fields`; save a
//! file to update the plot while the example runs with file watching enabled.
//!
//! The rows are the preset groups of `FieldPreset::GROUPS`, near to far, each
//! left to right, so a preset added to a group shows up here without editing
//! the example. Each patch sits on its own ground (`common::setting`), and a
//! 1.8 m figure stands in front for scale.
//!
//! WASD and the mouse fly the camera (hold the left button to look). 1–4 pick
//! the wind (calm, breeze, gusty, storm); S turns grass shadows off and on.
//!
//! ```text
//! cargo run --example fields
//! cargo run --example fields -- --fixed-time --screenshot shots/fields.png
//! ```

mod common;

use bevy::camera_controller::free_camera::{FreeCamera, FreeCameraPlugin};
use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;
use procgena_plant::prelude::*;

/// Side of each preset's patch, and the gap between patches, in metres.
const PATCH: f32 = 7.0;
const GAP: f32 = 1.5;

const WINDS: [(&str, PlantWind); 6] = [
    ("calm", PlantWind::CALM),
    ("breeze", PlantWind::BREEZE),
    ("gusty", PlantWind::GUSTY),
    ("storm", PlantWind::STORM),
    // The breeze with its movement reshaped (see `PlantWind::sway` and the
    // other multipliers): steady, with no gusts and no sway ...
    (
        "steady",
        PlantWind {
            sway: 0.0,
            gust_contrast: 0.0,
            ..PlantWind::BREEZE
        },
    ),
    // ... and a lively one, with deep waves and blades that sway fast.
    (
        "lively",
        PlantWind {
            sway: 2.5,
            sway_speed: 2.0,
            gust_contrast: 2.5,
            ..PlantWind::BREEZE
        },
    ),
];

fn main() {
    let mut app = App::new();
    common::ExampleArgs::parse().install(&mut app, "procgena_plant: fields (wind: breeze)");
    app.add_plugins((PlantPlugins, FreeCameraPlugin))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (toggle_shadows, pick_wind, common::follow_field_files),
        )
        .run();
}

fn toggle_shadows(keys: Res<ButtonInput<KeyCode>>, mut settings: ResMut<PlantSettings>) {
    if keys.just_pressed(KeyCode::KeyS) {
        settings.shadows = if settings.shadows == PlantShadows::Off {
            PlantShadows::Near
        } else {
            PlantShadows::Off
        };
        info!("grass shadows: {:?}", settings.shadows);
    }
}

fn pick_wind(
    keys: Res<ButtonInput<KeyCode>>,
    mut wind: ResMut<PlantWind>,
    mut windows: Query<&mut Window>,
) {
    let picked = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
    ]
    .iter()
    .position(|key| keys.just_pressed(*key));
    if let Some(index) = picked {
        let (name, preset) = &WINDS[index];
        *wind = *preset;
        for mut window in &mut windows {
            window.title = format!("procgena_plant: fields (wind: {name})");
        }
    }
}

fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 36.0, 44.0).looking_at(Vec3::new(0.0, 0.0, -26.0), Vec3::Y),
        FreeCamera::default(),
    ));
    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, -0.5, -0.55, 0.0)),
        // One cascade over the whole scene: grass's default shadow mode casts
        // into the first cascade only, so every patch shows its shadows.
        CascadeShadowConfigBuilder {
            num_cascades: 1,
            maximum_distance: 100.0,
            ..default()
        }
        .build(),
    ));
    // Between the patches, a little below their own ground.
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(240.0, 240.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.16, 0.15, 0.14),
            perceptual_roughness: 0.95,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.02, 0.0),
    ));
    let _ = common::spawn_person(
        &mut commands,
        &mut meshes,
        &mut materials,
        Vec3::new(0.0, 0.0, PATCH * 0.5 + 2.5),
    );

    for (row, group) in FieldPreset::GROUPS.iter().enumerate() {
        let z = -(row as f32) * (PATCH + GAP);
        let width = group.len() as f32 * (PATCH + GAP) - GAP;
        for (column, preset) in group.iter().enumerate() {
            let x = -width * 0.5 + PATCH * 0.5 + column as f32 * (PATCH + GAP);
            let density_map = common::setting::spawn_setting(
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut images,
                &common::setting::setting(&preset.key),
                Vec3::new(x, 0.0, z),
                Vec2::splat(PATCH),
            );
            // The preset's own field until its file has loaded.
            commands.spawn((
                PlantField {
                    density_map,
                    ..PlantField::from_preset(preset, Vec2::splat(PATCH))
                },
                common::FieldFile(assets.load(format!(
                    "fields/{}.field.ron",
                    preset.key.to_ascii_lowercase()
                ))),
                Transform::from_xyz(x, 0.0, z),
            ));
        }
    }
}
