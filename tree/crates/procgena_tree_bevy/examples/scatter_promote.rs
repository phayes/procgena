//! Promotes single grove trees to entities of their own, the way a game
//! gives nearby or selected trees colliders or behaviour.
//!
//! ```sh
//! cargo run -p procgena_tree_bevy --example scatter_promote -- --mode distance
//! cargo run -p procgena_tree_bevy --example scatter_promote -- --mode click
//! ```
//!
//! - `--mode distance` (default): trees within 15 m of the camera become
//!   entities, each with a stand-in collider drawn as a yellow cylinder, and
//!   go back to the grove beyond 20 m. Fly with WASD and Q/E, and hold the
//!   left mouse button to look around.
//! - `--mode click`: clicking a tree promotes it and outlines it; clicking it
//!   again gives it back.
//!
//! `--headless --screenshot <file.png>` renders without a window and saves
//! one picture once the grove is ready; with `--mode click`, `--click <x> <y>`
//! clicks that pixel of the 1280 × 720 picture first.

use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::camera::RenderTarget;
use bevy::camera_controller::free_camera::{FreeCamera, FreeCameraPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::window::{ExitCondition, PrimaryWindow};
use bevy::winit::WinitPlugin;
use procgena_impostor::ImpostorBakeCamera;
use procgena_tree::TreeScatterPreset;
use procgena_tree_bevy::{
    PromotedTree, TreePlugin, TreeScatterAssets, TreeScatterFinished, TreeScatterInstances,
    TreeScatterParams, TreeScatterTrees,
};

/// Width and depth of the grove, in metres.
const GROVE_SIZE: f32 = 80.0;
/// Trees closer than this to the camera are promoted, in metres.
const PROMOTE_WITHIN: f32 = 15.0;
/// Promoted trees farther than this from the camera are given back, in
/// metres. Larger than [`PROMOTE_WITHIN`], so trees near the edge don't
/// switch back and forth.
const DEMOTE_BEYOND: f32 = 20.0;
/// Frames to wait after the grove is ready before the headless picture.
const SETTLE_FRAMES: u32 = 30;
/// Size of the headless picture, in pixels.
const PICTURE: UVec2 = UVec2::new(1280, 720);

/// What the example does, from the command line.
#[derive(Resource)]
struct Options {
    click_mode: bool,
    screenshot: Option<String>,
    /// A click to make once the grove is ready, in picture pixels.
    click: Option<Vec2>,
}

/// The image the camera draws into when headless.
#[derive(Resource)]
struct Offscreen(Handle<Image>);

/// A game's own component on a promoted tree: here, a stand-in collider.
#[derive(Component)]
struct Collider;

/// Frames since the grove was ready, or `None` before then.
#[derive(Resource, Default)]
struct Ready(Option<u32>);

fn main() -> AppExit {
    // Read the command line.
    let mut options = Options {
        click_mode: false,
        screenshot: None,
        click: None,
    };
    let mut headless = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().expect("a value after the option");
        match arg.as_str() {
            "--mode" => options.click_mode = value() == "click",
            "--headless" => headless = true,
            "--screenshot" => options.screenshot = Some(value()),
            "--click" => {
                let x = value().parse().expect("--click needs x and y");
                let y = value().parse().expect("--click needs x and y");
                options.click = Some(Vec2::new(x, y));
            }
            other => panic!("unknown argument {other}"),
        }
    }
    assert!(
        !headless || options.screenshot.is_some(),
        "--headless needs --screenshot <file.png>"
    );

    // With `--headless`, run without a window.
    let defaults = DefaultPlugins.set(AssetPlugin {
        file_path: "../../assets".to_owned(),
        ..default()
    });
    let mut app = App::new();
    if headless {
        app.add_plugins((
            defaults
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .disable::<WinitPlugin>(),
            ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(1.0 / 60.0)),
        ));
    } else {
        app.add_plugins((defaults, FreeCameraPlugin));
    }
    let click_mode = options.click_mode;
    app.add_plugins(TreePlugin {
        texture_root: Some("textures".to_owned()),
        ..default()
    })
    .insert_resource(options)
    .insert_resource(HeadlessFlag(headless))
    .init_resource::<Ready>()
    .add_observer(grove_finished)
    .add_systems(Startup, setup)
    .add_systems(Update, (draw_promoted, screenshot));
    if click_mode {
        app.add_systems(Update, promote_on_click);
    } else {
        app.add_systems(Update, promote_by_distance);
    }
    app.run()
}

/// Whether the example runs without a window.
#[derive(Resource)]
struct HeadlessFlag(bool);

fn setup(
    mut commands: Commands,
    headless: Res<HeadlessFlag>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // A camera standing among the trees, looking across the grove.
    let mut camera = commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(-6.0, 5.0, 18.0).looking_at(Vec3::new(4.0, 2.0, 0.0), Vec3::Y),
        FreeCamera {
            walk_speed: 6.0,
            run_speed: 20.0,
            ..default()
        },
    ));
    if headless.0 {
        let image =
            Image::new_target_texture(PICTURE.x, PICTURE.y, TextureFormat::Rgba8UnormSrgb, None);
        let handle = images.add(image);
        camera.insert(RenderTarget::Image(handle.clone().into()));
        commands.insert_resource(Offscreen(handle));
    }
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

    // Ground, and the grove on it.
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(GROVE_SIZE, GROVE_SIZE))),
        MeshMaterial3d(materials.add(Color::srgb(0.32, 0.42, 0.22))),
    ));
    commands.spawn(TreeScatterParams::preset(
        TreeScatterPreset::oak_woodland(),
        Vec2::splat(GROVE_SIZE),
    ));
}

/// Starts the screenshot countdown when the grove is ready.
fn grove_finished(event: On<TreeScatterFinished>, mut ready: ResMut<Ready>) {
    if let Err(error) = &event.result {
        error!("grove failed: {error}");
    }
    ready.0.get_or_insert(0);
}

/// Promotes the trees near the camera and gives back those that are now far
/// away.
fn promote_by_distance(
    mut commands: Commands,
    trees: TreeScatterTrees,
    camera: Single<&GlobalTransform, (With<Camera3d>, Without<ImpostorBakeCamera>)>,
    promoted: Query<(Entity, &PromotedTree, &GlobalTransform)>,
) {
    let eye = camera.translation();

    // Give back trees beyond the outer distance.
    let mut kept = Vec::new();
    for (entity, promotion, transform) in &promoted {
        if transform.translation().distance(eye) > DEMOTE_BEYOND {
            commands.entity(entity).despawn();
        } else {
            kept.push(*promotion);
        }
    }

    // Promote trees within the inner distance that aren't promoted yet.
    for (grove, tree) in trees.within(eye, PROMOTE_WITHIN) {
        let promotion = PromotedTree::new(grove, tree.address);
        if !kept.contains(&promotion) {
            commands.spawn((promotion, Collider));
        }
    }
}

/// Promotes the tree under a click, or gives a promoted one back.
#[expect(clippy::too_many_arguments, reason = "one Bevy system")]
fn promote_on_click(
    mut commands: Commands,
    mut options: ResMut<Options>,
    ready: Res<Ready>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    camera: Single<(&Camera, &GlobalTransform)>,
    trees: TreeScatterTrees,
    promoted: Query<(Entity, &PromotedTree)>,
) {
    // A click from the mouse, or the scripted one once the grove is ready.
    let clicked = mouse.is_some_and(|m| m.just_pressed(MouseButton::Left));
    let cursor = if clicked {
        window.and_then(|w| w.cursor_position())
    } else if ready.0.is_some() {
        options.click.take()
    } else {
        None
    };
    let Some(cursor) = cursor else {
        return;
    };

    let (camera, transform) = *camera;
    let Ok(ray) = camera.viewport_to_world(transform, cursor) else {
        return;
    };
    let Some(hit) = trees.ray_cast(ray) else {
        return;
    };
    info!("clicked tree {:?} at {:.1} m", hit.address, hit.distance);
    let promotion = PromotedTree::new(hit.grove, hit.address);
    match promoted.iter().find(|(_, p)| **p == promotion) {
        Some((entity, _)) => commands.entity(entity).despawn(),
        None => {
            commands.spawn((promotion, Collider));
        }
    }
}

/// Draws each promoted tree's stand-in collider, or its outline when
/// clicked.
fn draw_promoted(
    mut gizmos: Gizmos,
    options: Res<Options>,
    promoted: Query<(&PromotedTree, &GlobalTransform), With<Collider>>,
    groves: Query<(&TreeScatterInstances, &TreeScatterAssets)>,
) {
    for (promotion, transform) in &promoted {
        if !options.click_mode {
            let base = transform.translation();
            let collider = Cylinder::new(0.4, 6.0);
            let place = Isometry3d::from_translation(base + Vec3::Y * 3.0);
            gizmos.primitive_3d(&collider, place, Color::srgb(1.0, 0.9, 0.1));
            continue;
        }

        // Outline the tree's nearest detail level.
        let Ok((instances, assets)) = groves.get(promotion.grove) else {
            continue;
        };
        let Some(tree) = instances.get(&promotion.address) else {
            continue;
        };
        let parts = assets.of(tree).parts().filter(|p| p.lod == 0);
        let Some((min, max)) = parts
            .map(|p| (Vec3::from(p.bounds.min()), Vec3::from(p.bounds.max())))
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
        else {
            continue;
        };
        let local = Transform::from_translation((min + max) / 2.0).with_scale(max - min);
        gizmos.cube(transform.mul_transform(local), Color::srgb(1.0, 0.9, 0.1));
    }
}

/// With `--screenshot`, saves one picture once the grove has been ready for
/// a moment, then exits.
fn screenshot(
    mut commands: Commands,
    options: Res<Options>,
    offscreen: Option<Res<Offscreen>>,
    mut ready: ResMut<Ready>,
) {
    let (Some(path), Some(frames)) = (&options.screenshot, &mut ready.0) else {
        return;
    };
    *frames += 1;
    if *frames != SETTLE_FRAMES {
        return;
    }
    let target = match offscreen {
        Some(offscreen) => Screenshot::image(offscreen.0.clone()),
        None => Screenshot::primary_window(),
    };
    commands
        .spawn(target)
        .observe(save_to_disk(path.clone()))
        .observe(
            |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                exit.write(AppExit::Success);
            },
        );
}
