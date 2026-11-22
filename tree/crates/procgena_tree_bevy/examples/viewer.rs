//! Explore tree presets, wind animation and distance-based mesh detail.
//!
//! Run `cargo run -p procgena_tree_bevy --example viewer` from the repository.
//! The viewer uses the repository's `assets/textures` folder with BC7 textures.
//! Run `tools/textures/download.sh` first to fetch the bark and leaf textures.
//!
//! Controls:
//! - Left / Right: choose the previous / next preset.
//! - F: switch between one tree and a 7 × 7 forest to inspect detail transitions.
//! - Space: turn trunk and branch movement on or off; leaves keep fluttering.
//! - N: switch bark normal maps on or off by changing texture coordinates.
//! - J: turn shape variation on or off so trees of the same preset can look different.
//! - R: generate new tree shapes while variation is on.
//! - L: cycle the leaf texture through every kind in `procgena_tree::LEAF_KINDS`, starting
//!   from the preset's own leaves. Changing preset returns to the preset's leaves.
//! - [ / ]: make fruit and flowers less / more mature (unripe to ripe fruit,
//!   bud to faded flower). Changing preset returns to the preset's own maturity.
//! - Hold the left mouse button to look around; use WASD and Q / E to fly.
//!
//! Pass options after `--`, for example:
//! `cargo run -p procgena_tree_bevy --example viewer -- --preset oak_medium --forest`.
//! Use `--vary` to start with shape variation enabled,
//! `--maturity <0–1>` to set fruit and flower maturity, `--leaves <kind>` to use
//! another leaf texture from `procgena_tree::LEAF_KINDS`, `--bark <kind>` to choose a
//! bark texture from `procgena_tree::BARK_KINDS`, `--bark-tint <RRGGBB>` to tint the bark
//! with a six-digit hexadecimal RGB colour. The tint multiplies the texture's
//! colour, so it can darken or recolour it but cannot brighten it; `FFFFFF` keeps
//! the texture's own colour. Both bark options stay in effect when changing presets.
//! Use `--still` for leaf-only wind or `--flat` for mirrored bark coordinates
//! without normal maps (this does not enable flat shading). Set camera positions
//! with `--eye x y z` and `--target x y z`. Use `--resolution <width> <height>` to set
//! the window or screenshot size in pixels (default: 1280 × 720).
//! Use `--screenshot <file.png>` to save a screenshot and close the viewer once
//! the trees and textures (the images that give bark and leaves their appearance)
//! are ready. If loading or saving fails, the viewer exits with an error.
//! It also exits with an error if the scene is not ready within two minutes
//! after the trees finish generating; tree generation itself has no time limit.
//! Add `--headless` with `--screenshot` to capture without opening a window.
//! This keeps trees and leaves still so wind does not change their appearance
//! between screenshots. Use `--lod <0–3>` to lock the
//! viewer to one default detail level at all distances. For example, `--lod 3`
//! lets you inspect the distant tree’s camera-facing shapes and shadows up close.

use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::camera::RenderTarget;
use bevy::camera_controller::free_camera::{FreeCamera, FreeCameraPlugin};
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use procgena_tree::{BARK_KINDS, BarkUvMode, LEAF_KINDS, Rng, TreeOptions, TreePreset, WindParams};
use procgena_tree_bevy::{
    TreeGenerating, TreeMaterial, TreePart, Wind, ZeTree, ZeTreePlugin, default_tree_lod_levels,
};

/// Distance between neighbouring trees in the forest, in world units.
const FOREST_SPACING: f32 = 45.0;
/// Number of rows and columns in the square forest.
const FOREST_SIZE: i32 = 7;
/// Vary shape settings by up to 15% either way, then round branch and leaf counts.
const JIGGLE: f32 = 0.15;
/// Change in fruit and flower maturity for each press of [ or ].
const MATURITY_STEP: f32 = 0.1;
/// Extra frames to allow loaded textures to appear before taking a screenshot.
const SETTLE_FRAMES: u32 = 10;
/// Seconds to wait for the scene to be ready for a screenshot after tree generation
/// finishes. If the scene is still not ready, exit with an error.
const SCREENSHOT_TIMEOUT: f32 = 120.0;

#[derive(Resource)]
struct Viewer {
    preset: usize,
    forest: bool,
    sway: bool,
    continuous: bool,
    vary: bool,
    /// Selects a new set of tree variations each time R is pressed.
    roll: i32,
    /// Fruit and flower maturity from 0 to 1; `None` keeps the preset's own values.
    maturity: Option<f32>,
    /// Index into [`LEAF_KINDS`]; `None` keeps the preset's own leaf texture.
    leaves: Option<usize>,
    /// Bark texture to use for all presets, chosen from [`BARK_KINDS`].
    /// `None` uses each preset's own bark texture.
    bark: Option<&'static str>,
    /// Bark tint to use for all presets, as a hexadecimal RGB colour (`0xRRGGBB`).
    /// Multiplies the texture's colour to darken or recolour it; `0xFFFFFF` keeps
    /// the texture's own colour. `None` uses each preset's own tint.
    bark_tint: Option<u32>,
    screenshot: Option<String>,
    /// Save a screenshot without opening a window, keeping trees and leaves still.
    headless: bool,
}

/// Holds the image drawn by the camera when no window is open, ready to capture.
#[derive(Resource)]
struct Offscreen(Handle<Image>);

#[derive(Component)]
struct Shown;

fn main() -> AppExit {
    let mut viewer = Viewer {
        preset: TreePreset::ALL
            .iter()
            .position(|&p| p == TreePreset::OakMedium)
            .unwrap(),
        forest: false,
        sway: true,
        continuous: true,
        vary: false,
        roll: 0,
        maturity: None,
        leaves: None,
        bark: None,
        bark_tint: None,
        screenshot: None,
        headless: false,
    };
    let mut size = UVec2::new(1280, 720);
    let mut eye = Vec3::new(0.0, 30.0, 110.0);
    let mut target = Vec3::new(0.0, 28.0, 0.0);
    let mut lod_levels = default_tree_lod_levels();

    // Apply command-line choices before creating the scene.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    let vec3 = |args: &[String], i: usize| {
        let n = |k: usize| args[i + k].parse::<f32>().expect("a number");
        Vec3::new(n(1), n(2), n(3))
    };
    while i < args.len() {
        match args[i].as_str() {
            "--preset" => {
                let name = args[i + 1].replace('_', " ");
                viewer.preset = TreePreset::ALL
                    .iter()
                    .position(|p| p.name().eq_ignore_ascii_case(&name))
                    .unwrap_or_else(|| panic!("unknown preset {name}"));
                i += 1;
            }
            "--forest" => viewer.forest = true,
            "--vary" => viewer.vary = true,
            "--maturity" => {
                let maturity = args[i + 1].parse::<f32>().expect("a number");
                viewer.maturity = Some(maturity.clamp(0.0, 1.0));
                i += 1;
            }
            "--leaves" => {
                let kind = &args[i + 1];
                viewer.leaves = Some(
                    LEAF_KINDS
                        .iter()
                        .position(|k| k == kind)
                        .unwrap_or_else(|| panic!("unknown leaf kind {kind}")),
                );
                i += 1;
            }
            "--bark" => {
                let kind = &args[i + 1];
                viewer.bark = Some(
                    BARK_KINDS
                        .into_iter()
                        .find(|k| k == kind)
                        .unwrap_or_else(|| panic!("unknown bark kind {kind}")),
                );
                i += 1;
            }
            "--bark-tint" => {
                let hex = args[i + 1].trim_start_matches('#');
                assert!(hex.len() == 6, "--bark-tint takes a RRGGBB hex colour");
                let tint = u32::from_str_radix(hex, 16).expect("a RRGGBB hex colour");
                viewer.bark_tint = Some(tint);
                i += 1;
            }
            "--still" => viewer.sway = false,
            "--flat" => viewer.continuous = false,
            "--eye" => {
                eye = vec3(&args, i);
                i += 3;
            }
            "--target" => {
                target = vec3(&args, i);
                i += 3;
            }
            "--screenshot" => {
                viewer.screenshot = Some(args[i + 1].clone());
                i += 1;
            }
            "--headless" => viewer.headless = true,
            "--resolution" => {
                let n = |k: usize| args[i + k].parse::<u32>().expect("a whole number");
                size = UVec2::new(n(1), n(2));
                i += 2;
            }
            "--lod" => {
                let index: usize = args[i + 1].parse().expect("a detail level number");
                let mut level = *lod_levels.get(index).expect("a default detail level");
                level.level.distance = 0.0;
                lod_levels = vec![level];
                i += 1;
            }
            other => panic!("unknown option {other}"),
        }
        i += 1;
    }

    assert!(
        !viewer.headless || viewer.screenshot.is_some(),
        "--headless requires --screenshot <file.png>"
    );

    // Keep updating the scene without a window so it can load and take a screenshot.
    let headless = viewer.headless;
    let defaults = DefaultPlugins
        .set(AssetPlugin {
            file_path: "../../assets".to_owned(),
            ..default()
        })
        .set(RenderPlugin {
            synchronous_pipeline_compilation: true,
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
        app.add_plugins((
            defaults.set(WindowPlugin {
                primary_window: Some(Window {
                    resolution: size.into(),
                    ..default()
                }),
                ..default()
            }),
            FreeCameraPlugin,
        ));
    }

    app.add_plugins(ZeTreePlugin {
        texture_root: Some("textures".to_owned()),
        lod_levels,
        ..default()
    })
    .insert_resource(viewer)
    .insert_resource(Camera3dSetup { eye, target, size })
    .add_systems(Startup, setup)
    .add_systems(Update, (controls, show_trees, screenshot))
    .run()
}

#[derive(Resource)]
struct Camera3dSetup {
    eye: Vec3,
    target: Vec3,
    /// Width and height in pixels for the image saved with `--headless`.
    size: UVec2,
}

fn setup(
    mut commands: Commands,
    camera: Res<Camera3dSetup>,
    viewer: Res<Viewer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let mut camera_entity = commands.spawn((
        Camera3d::default(),
        Transform::from_translation(camera.eye).looking_at(camera.target, Vec3::Y),
        FreeCamera {
            walk_speed: 20.0,
            run_speed: 80.0,
            ..default()
        },
    ));

    // Give the camera an image to draw into when there is no window.
    if viewer.headless {
        let image = Image::new_target_texture(
            camera.size.x,
            camera.size.y,
            TextureFormat::Rgba8UnormSrgb,
            None,
        );
        let handle = images.add(image);
        camera_entity.insert(RenderTarget::Image(handle.clone().into()));
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
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(800.0, 800.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.32, 0.42, 0.22))),
    ));
}

fn controls(keys: Res<ButtonInput<KeyCode>>, mut viewer: ResMut<Viewer>) {
    let count = TreePreset::ALL.len();
    if keys.just_pressed(KeyCode::ArrowRight) {
        viewer.preset = (viewer.preset + 1) % count;
        viewer.maturity = None;
        viewer.leaves = None;
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        viewer.preset = (viewer.preset + count - 1) % count;
        viewer.maturity = None;
        viewer.leaves = None;
    }

    // Step to the next leaf kind, starting after the preset's own leaves.
    if keys.just_pressed(KeyCode::KeyL) {
        let current = viewer.leaves.unwrap_or_else(|| {
            let kind = TreePreset::ALL[viewer.preset].options().leaves.kind;
            LEAF_KINDS.iter().position(|&k| k == kind).unwrap_or(0)
        });
        viewer.leaves = Some((current + 1) % LEAF_KINDS.len());
    }

    // Step maturity from the preset's fruit maturity the first time it changes.
    let step = match (
        keys.just_pressed(KeyCode::BracketLeft),
        keys.just_pressed(KeyCode::BracketRight),
    ) {
        (true, false) => -MATURITY_STEP,
        (false, true) => MATURITY_STEP,
        _ => 0.0,
    };
    if step != 0.0 {
        let current = viewer
            .maturity
            .unwrap_or_else(|| TreePreset::ALL[viewer.preset].options().fruit.maturity);
        viewer.maturity = Some((current + step).clamp(0.0, 1.0));
    }
    if keys.just_pressed(KeyCode::KeyF) {
        viewer.forest = !viewer.forest;
    }
    if keys.just_pressed(KeyCode::Space) {
        viewer.sway = !viewer.sway;
    }
    if keys.just_pressed(KeyCode::KeyN) {
        viewer.continuous = !viewer.continuous;
    }
    if keys.just_pressed(KeyCode::KeyJ) {
        viewer.vary = !viewer.vary;
    }
    if keys.just_pressed(KeyCode::KeyR) && viewer.vary {
        viewer.roll += 1;
    }
}

/// Rebuilds the displayed trees and applies wind settings when viewer controls change.
fn show_trees(
    mut commands: Commands,
    viewer: Res<Viewer>,
    shown: Query<Entity, With<Shown>>,
    mut wind: ResMut<Wind>,
) {
    if !viewer.is_changed() {
        return;
    }
    wind.0 = if viewer.headless {
        WindParams {
            strength: Vec3::ZERO,
            ..default()
        }
    } else if viewer.sway {
        WindParams::default()
    } else {
        WindParams::LEAVES_ONLY
    };
    for entity in &shown {
        commands.entity(entity).despawn();
    }

    let preset = TreePreset::ALL[viewer.preset];
    let mut options: TreeOptions = preset.options();
    options.bark.uv_mode = if viewer.continuous {
        BarkUvMode::Continuous
    } else {
        BarkUvMode::Mirrored
    };
    if let Some(maturity) = viewer.maturity {
        options.fruit.maturity = maturity;
        options.flowers.maturity = maturity;
    }
    if let Some(leaves) = viewer.leaves {
        options.leaves.kind = LEAF_KINDS[leaves].to_owned();
    }
    if let Some(bark) = viewer.bark {
        options.bark.kind = bark.to_owned();
    }
    if let Some(tint) = viewer.bark_tint {
        options.bark.tint = tint;
    }
    info!(
        "{} (leaves {}, maturity {:.1}, bark normal maps {}, wind {}, variation {})",
        preset.name(),
        options.leaves.kind,
        options.fruit.maturity,
        if viewer.continuous { "on" } else { "off" },
        if viewer.headless {
            "off"
        } else if viewer.sway {
            "full"
        } else {
            "leaves only"
        },
        if viewer.vary { "on" } else { "off" },
    );
    let tree = ZeTree::new(options.clone());
    let half = if viewer.forest { FOREST_SIZE / 2 } else { 0 };
    for x in -half..=half {
        for z in -half..=half {
            let position = Vec3::new(x as f32, 0.0, -z as f32) * FOREST_SPACING;
            // Vary each copy's rotation to make the repeated tree shape less obvious.
            let turn = Quat::from_rotation_y((x * 7 + z * 13) as f32);
            // Give each forest position a repeatable variation; pressing R selects a new set.
            let tree = if viewer.vary {
                let cell = (x + half) * FOREST_SIZE + (z + half);
                let mut rng = Rng::new(viewer.roll.wrapping_mul(1000).wrapping_add(cell));
                let mut varied = options.clone();
                varied.jiggle(JIGGLE, &mut rng);
                ZeTree::new(varied)
            } else {
                tree.clone()
            };
            commands.spawn((
                tree,
                Transform::from_translation(position).with_rotation(turn),
                Shown,
            ));
        }
    }
}

/// Saves one screenshot once the trees and textures are ready, then closes the viewer.
/// Exits with an error if loading or saving fails, or if the scene is still not
/// ready after [`SCREENSHOT_TIMEOUT`] seconds with no tree generation in progress.
#[allow(clippy::too_many_arguments)]
fn screenshot(
    mut commands: Commands,
    viewer: Res<Viewer>,
    offscreen: Option<Res<Offscreen>>,
    time: Res<Time>,
    asset_server: Res<AssetServer>,
    materials: Res<Assets<TreeMaterial>>,
    parts: Query<(), With<TreePart>>,
    generating: Query<(), With<TreeGenerating>>,
    mut exit: MessageWriter<AppExit>,
    mut idle_since: Local<Option<f32>>,
    mut settled: Local<u32>,
    mut taken: Local<bool>,
) {
    let Some(path) = &viewer.screenshot else {
        return;
    };
    if *taken {
        return;
    }
    let now = time.elapsed_secs();

    // Begin timing when tree generation finishes; restart if any tree generates again.
    let idle_since = if generating.is_empty() {
        *idle_since.get_or_insert(now)
    } else {
        *idle_since = None;
        now
    };

    // Avoid capturing missing trees or textures; stop if loading fails or takes too long.
    let textures = match tree_textures_state(&materials, &asset_server) {
        Ok(loaded) => loaded,
        Err(message) => {
            error!("screenshot failed: {message}");
            exit.write(AppExit::error());
            *taken = true;
            return;
        }
    };
    let ready = textures && !parts.is_empty() && generating.is_empty();
    if !ready {
        *settled = 0;
        if now - idle_since > SCREENSHOT_TIMEOUT {
            error!(
                "screenshot failed: scene not ready {SCREENSHOT_TIMEOUT} seconds after \
                 tree generation finished"
            );
            exit.write(AppExit::error());
            *taken = true;
        }
        return;
    }

    // Allow a few more frames for the loaded textures to appear in the scene.
    *settled += 1;
    if *settled < SETTLE_FRAMES {
        return;
    }
    *taken = true;
    let target = match offscreen {
        Some(offscreen) => Screenshot::image(offscreen.0.clone()),
        None => Screenshot::primary_window(),
    };
    // Save the screenshot and close the viewer, reporting an error if saving fails.
    let path = path.clone();
    commands.spawn(target).observe(
        move |captured: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
            match save_png(&captured.image, &path) {
                Ok(()) => {
                    info!("screenshot saved to {path}");
                    exit.write(AppExit::Success);
                }
                Err(message) => {
                    error!("screenshot failed: could not save {path}: {message}");
                    exit.write(AppExit::error());
                }
            }
        },
    );
}

/// Saves the screenshot as a PNG file at `path`, without transparency.
/// Returns an error message if the captured image cannot be converted or saved.
fn save_png(frame: &Image, path: &str) -> Result<(), String> {
    let rgb = frame
        .clone()
        .try_into_dynamic()
        .map_err(|error| error.to_string())?
        .to_rgb8();
    rgb.save_with_format(path, image::ImageFormat::Png)
        .map_err(|error| error.to_string())
}

/// Checks whether the textures that give trees their colour and surface detail
/// are loaded. Returns `Ok(true)` when all are ready, `Ok(false)` while waiting,
/// or an error identifying a texture that failed to load.
fn tree_textures_state(
    materials: &Assets<TreeMaterial>,
    asset_server: &AssetServer,
) -> Result<bool, String> {
    let mut loaded = true;
    for (_, material) in materials.iter() {
        let base = &material.base;
        let textures = [
            &base.base_color_texture,
            &base.normal_map_texture,
            &base.metallic_roughness_texture,
            &base.occlusion_texture,
            &base.emissive_texture,
        ];
        for texture in textures.into_iter().flatten() {
            if asset_server.load_state(texture).is_failed() {
                let path = texture.path().map(ToString::to_string).unwrap_or_default();
                return Err(format!("texture {path} failed to load"));
            }
            loaded &= asset_server.is_loaded_with_dependencies(texture);
        }
    }
    Ok(loaded)
}
