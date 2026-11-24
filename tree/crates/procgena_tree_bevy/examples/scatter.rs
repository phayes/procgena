//! Grows a grove of trees from a preset, on flat or hilly ground.
//!
//! ```sh
//! cargo run -p procgena_tree_bevy --example scatter
//! cargo run -p procgena_tree_bevy --example scatter -- --preset pine_stand --hills
//! ```
//!
//! - Left/Right: switch tree Scatter preset.
//! - F: switch between flat ground and a hilly host mesh.
//! - Up/Down: make the grove denser or sparser.
//! - Hold the left mouse button to look around; WASD and Q/E to fly.
//!
//! `--preset <key>` starts from a tree Scatter preset key, such as `orchard`.
//! `--hills` starts on the hilly ground. `--textures` loads the repository's
//! bark and leaf textures (download them first with
//! `uv run --project tools textures download tree`); without it, trees use
//! plain colours. `--screenshot <file.png>` saves a picture once the grove is
//! ready and exits.

use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::camera_controller::free_camera::{FreeCamera, FreeCameraPlugin};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use procgena_specimen::{GeneratedSpecimen, Part, Specimen, SurfaceMesh, TagFilter, tags};
use procgena_tree::TreeScatterPreset;
use procgena_tree_bevy::{TreePlugin, TreeScatterFinished, TreeScatterParams};

/// Width and depth of the grove, in metres.
const GROVE_SIZE: f32 = 50.0;
/// Width and depth of the hilly ground, in metres; a little wider than the
/// grove so every tree has ground under it.
const HILLS_SIZE: f32 = 60.0;
/// Frames to wait after the grove finishes before saving a screenshot.
const SETTLE_FRAMES: u32 = 10;

#[derive(Resource)]
struct Viewer {
    preset: usize,
    hills: bool,
    density_scale: f32,
    screenshot: Option<String>,
}

/// The flat ground plane, and the hilly ground mesh with its specimen.
#[derive(Resource)]
struct Grounds {
    flat: Entity,
    hills: Entity,
}

/// Frames since the first grove was ready, or `None` before then.
#[derive(Resource, Default)]
struct Ready(Option<u32>);

/// The current grove entity.
#[derive(Resource)]
struct GroveEntity(Entity);

fn main() -> AppExit {
    // Read the command line.
    let mut viewer = Viewer {
        preset: 0,
        hills: false,
        density_scale: 1.0,
        screenshot: None,
    };
    let mut textures = false;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--preset" => {
                viewer.preset = TreeScatterPreset::all()
                    .position(|p| p.key == args[i + 1])
                    .expect("a tree Scatter preset key");
                i += 1;
            }
            "--hills" => viewer.hills = true,
            "--textures" => textures = true,
            "--screenshot" => {
                viewer.screenshot = Some(args[i + 1].clone());
                i += 1;
            }
            other => panic!("unknown argument {other}"),
        }
        i += 1;
    }

    App::new()
        .add_plugins((
            DefaultPlugins.set(AssetPlugin {
                file_path: "../../assets".to_owned(),
                ..default()
            }),
            FreeCameraPlugin,
            TreePlugin {
                texture_root: textures.then(|| "textures".to_owned()),
                ..default()
            },
        ))
        .insert_resource(viewer)
        .init_resource::<Ready>()
        .add_observer(grove_finished)
        .add_systems(Startup, setup)
        .add_systems(Update, (controls, screenshot))
        .run()
}

fn setup(
    mut commands: Commands,
    viewer: Res<Viewer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 22.0, 48.0).looking_at(Vec3::new(0.0, 2.0, 0.0), Vec3::Y),
        FreeCamera {
            walk_speed: 6.0,
            run_speed: 20.0,
            ..default()
        },
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(40.0, 80.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 400.0,
        ..default()
    });
    commands.insert_resource(ClearColor(Color::srgb(0.62, 0.76, 0.9)));

    // Flat ground, and hilly ground that groves can stand on as their host.
    let grass = materials.add(Color::srgb(0.32, 0.42, 0.22));
    let flat = commands
        .spawn((
            Mesh3d(meshes.add(Plane3d::default().mesh().size(HILLS_SIZE, HILLS_SIZE))),
            MeshMaterial3d(grass.clone()),
        ))
        .id();
    let surface = hill_surface();
    let hills = commands
        .spawn((
            Mesh3d(meshes.add(bevy_mesh(&surface))),
            MeshMaterial3d(grass),
            GeneratedSpecimen(Arc::new(Specimen::single(vec![Part::new(
                [tags::GROUND],
                surface,
            )]))),
        ))
        .id();
    let grounds = Grounds { flat, hills };
    let grove = spawn_grove(&mut commands, &viewer, &grounds);
    commands.insert_resource(grounds);
    commands.insert_resource(GroveEntity(grove));
}

/// Spawns the grove the viewer's settings ask for, on the matching ground,
/// and shows only that ground.
fn spawn_grove(commands: &mut Commands, viewer: &Viewer, grounds: &Grounds) -> Entity {
    let grove = commands.spawn(grove_params(viewer)).id();
    let (shown, hidden) = if viewer.hills {
        commands.entity(grove).insert(ChildOf(grounds.hills));
        (grounds.hills, grounds.flat)
    } else {
        (grounds.flat, grounds.hills)
    };
    commands.entity(shown).insert(Visibility::Visible);
    commands.entity(hidden).insert(Visibility::Hidden);
    grove
}

/// The grove the viewer's settings ask for.
fn grove_params(viewer: &Viewer) -> TreeScatterParams {
    let preset = TreeScatterPreset::all()
        .nth(viewer.preset)
        .expect("a preset index");
    info!("grove: {}", preset.name);
    let mut grove = preset.tree_scatter.clone();
    grove.density_scale *= viewer.density_scale;
    let params = TreeScatterParams::new(grove, Vec2::splat(GROVE_SIZE));
    if viewer.hills {
        params.on_host(TagFilter::any_of([tags::GROUND]), 0.5)
    } else {
        params
    }
}

/// Rolling hills a few metres high, slightly lifted so they hide the flat
/// ground.
fn hill_surface() -> SurfaceMesh {
    let mut surface = SurfaceMesh::ground(HILLS_SIZE, 120);
    for p in &mut surface.positions {
        let (x, z) = (p[0], p[2]);
        p[1] = 0.05 + 1.6 * (x * 0.11).sin() * (z * 0.09).cos() + 0.8 * (x * 0.05 + z * 0.07).sin();
    }
    surface
}

/// Converts a surface to a Bevy mesh with smooth normals.
fn bevy_mesh(surface: &SurfaceMesh) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, surface.positions.clone())
    .with_inserted_indices(Indices::U32(surface.indices.clone()));
    mesh.compute_smooth_normals();
    mesh
}

fn controls(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut viewer: ResMut<Viewer>,
    grounds: Res<Grounds>,
    mut grove: ResMut<GroveEntity>,
) {
    let count = TreeScatterPreset::all().count();
    let before = (viewer.preset, viewer.hills, viewer.density_scale);
    if keys.just_pressed(KeyCode::ArrowRight) {
        viewer.preset = (viewer.preset + 1) % count;
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        viewer.preset = (viewer.preset + count - 1) % count;
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        viewer.density_scale = (viewer.density_scale * 1.25).min(4.0);
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        viewer.density_scale = (viewer.density_scale / 1.25).max(0.05);
    }
    if keys.just_pressed(KeyCode::KeyF) {
        viewer.hills = !viewer.hills;
    }
    if before == (viewer.preset, viewer.hills, viewer.density_scale) {
        return;
    }

    // Changing the ground moves the grove onto or off the hills; otherwise
    // the grove updates in place.
    if before.1 != viewer.hills {
        commands.entity(grove.0).despawn();
        grove.0 = spawn_grove(&mut commands, &viewer, &grounds);
    } else {
        commands.entity(grove.0).insert(grove_params(&viewer));
    }
}

/// Logs a grove that failed, and starts the screenshot countdown when the
/// first grove is ready.
fn grove_finished(event: On<TreeScatterFinished>, mut ready: ResMut<Ready>) {
    if let Err(error) = &event.result {
        error!("grove failed: {error}");
    }
    ready.0.get_or_insert(0);
}

/// Saves one screenshot a few frames after the first grove is ready, then
/// exits.
fn screenshot(mut commands: Commands, viewer: Res<Viewer>, mut ready: ResMut<Ready>) {
    let (Some(path), Some(frames)) = (&viewer.screenshot, &mut ready.0) else {
        return;
    };
    *frames += 1;
    if *frames != SETTLE_FRAMES {
        return;
    }
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path.clone()))
        .observe(
            |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                exit.write(AppExit::Success);
            },
        );
}
