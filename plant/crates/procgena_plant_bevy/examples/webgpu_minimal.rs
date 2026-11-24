//! The minimal example, ready for the browser: what a game adds to run grass
//! on WebGPU. It runs natively too.
//!
//! A game turns on Bevy's `webgpu` feature for wasm. Bevy's default features
//! render through WebGL2 there, which has no compute shaders, so grass would
//! draw nothing:
//!
//! ```toml
//! [target.'cfg(target_arch = "wasm32")'.dependencies]
//! bevy = { version = "0.20", features = ["webgpu"] }
//! ```
//!
//! To run it in the browser, build it for `wasm32-unknown-unknown`, run
//! `wasm-bindgen --target web` on it, and serve the page from localhost
//! (WebGPU needs HTTPS or localhost); `docs/web.md` has the steps.
//!
//! Natively: `cargo run --release --example webgpu_minimal`. See
//! `docs/web.md` for the browsers tested and how fast grass is there.

use bevy::prelude::*;
use procgena_plant_bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "procgena_plant on WebGPU".into(),
                    // In a browser the canvas fills the element it is in.
                    fit_canvas_to_parent: true,
                    ..default()
                }),
                ..default()
            }),
            PlantPlugins.set(PlantPlugin {
                settings: settings(),
                ..default()
            }),
        ))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            tell_the_player.run_if(resource_changed::<PlantSupport>),
        )
        .run();
}

/// Grass costs more in a browser than natively on the same GPU, and on an
/// integrated GPU only `LOW` holds 60 Hz in a full window (`docs/web.md`).
fn settings() -> PlantSettings {
    if cfg!(target_arch = "wasm32") {
        PlantSettings::LOW
    } else {
        PlantSettings::default()
    }
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 1.5, 6.0).looking_at(Vec3::new(0.0, 0.4, 0.0), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(64.0, 64.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.2, 0.15, 0.09))),
    ));
    commands.spawn(PlantScatter::from_preset(
        &PlantScatterPreset::MEADOW,
        Vec2::splat(64.0),
    ));
}

/// On a browser or GPU that cannot draw grass (WebGL2, or fewer than 10
/// storage buffers per shader stage), the rest of the scene still renders
/// and grass logs one warning. This says why on the screen too.
fn tell_the_player(support: Res<PlantSupport>, mut commands: Commands) {
    if matches!(*support, PlantSupport::Unknown | PlantSupport::Supported) {
        return;
    }
    commands.spawn((
        Text::new(format!("No grass on this device: {}", *support)),
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            left: px(12),
            ..default()
        },
    ));
}
