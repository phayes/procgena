//! Variation, one kind at a time: a wild field with each kind of variation
//! switched on and off, to see what each adds.
//!
//! The field is loaded from its `.plant_scatter.yml` file in `assets/scatters`,
//! so editing the file and saving it changes the running example; the
//! switches apply on top.
//!
//! * 1–3 pick the field: summer wild, autumn wild, wildflower meadow.
//! * P: the other plants in the mix: seed heads, flowers, clover, broad
//!   leaves. Off leaves the grass, the field's last plant.
//! * T: tufts and bare patches (each plant's `tufts`, the field's `bare_share`).
//! * B: blade shape (`twist`, `droop`, `fold`; heads belong to the plants).
//! * C: colour variation (`patch_amount`, `dry_tips`, `dead_share`).
//! * S: grass shadows.
//!
//! WASD and the mouse fly the camera (hold the left button to look).
//! `--arpg` uses DEMO ARPG GAME's camera instead: orthographic, 30° down, 45° yaw.
//! `--walk` sends the person on a walk through the field, parting the grass,
//! with the camera following.
//! For screenshots, `--field N` (1–3) and `--no-plants`, `--no-tufts`,
//! `--no-shape` and `--no-colour` set the start.
//!
//! ```text
//! cargo run --example variation
//! cargo run --example variation -- --arpg
//! cargo run --example variation -- --fixed-time --no-tufts --screenshot shots/variation.png
//! cargo run --example variation -- --walk --fixed-time --record target/walk   # frames for a video
//! ```

mod common;

use bevy::camera::ScalingMode;
use bevy::camera_controller::free_camera::{FreeCamera, FreeCameraPlugin};
use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;
use procgena_plant_bevy::prelude::*;

const FIELDS: [&str; 3] = ["summer_wild", "autumn_wild", "wildflower_meadow"];

/// Which kinds of variation are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Switches {
    plants: bool,
    tufts: bool,
    shape: bool,
    colour: bool,
}

/// How the example starts, from the command line.
#[derive(Resource, Clone, Copy)]
struct Start {
    arpg: bool,
    walk: bool,
    file: usize,
    switches: Switches,
}

/// The loaded field preset and the variation switches applied to its settings.
#[derive(Resource)]
struct Shown {
    files: Vec<Handle<PlantScatterPresetAsset>>,
    file: usize,
    switches: Switches,
    dirty: bool,
}

fn main() {
    let args = common::ExampleArgs::parse();
    let start = Start {
        arpg: args.has("--arpg"),
        walk: args.has("--walk"),
        file: args
            .value("--field")
            .and_then(|n| n.parse::<usize>().ok())
            .map_or(0, |n| n.clamp(1, FIELDS.len()) - 1),
        switches: Switches {
            plants: !args.has("--no-plants"),
            tufts: !args.has("--no-tufts"),
            shape: !args.has("--no-shape"),
            colour: !args.has("--no-colour"),
        },
    };
    let mut app = App::new();
    args.install(&mut app, "procgena_plant: variation");
    app.add_plugins((PlantPlugins, FreeCameraPlugin))
        .insert_resource(start)
        .add_systems(Startup, setup)
        .add_systems(Update, ((switch, refresh, title).chain(), walk))
        .run();
}

fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    start: Res<Start>,
) {
    if start.walk {
        commands.spawn((Camera3d::default(), Following));
    } else if start.arpg {
        let rotation =
            Quat::from_euler(EulerRot::YXZ, 45f32.to_radians(), -30f32.to_radians(), 0.0);
        commands.spawn((
            Camera3d::default(),
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::FixedVertical {
                    viewport_height: 14.64,
                },
                ..OrthographicProjection::default_3d()
            }),
            Transform::from_translation(rotation * Vec3::new(0.0, 0.0, 40.0))
                .with_rotation(rotation),
        ));
    } else {
        commands.spawn((
            Camera3d::default(),
            Transform::from_xyz(0.0, 3.5, 14.0).looking_at(Vec3::new(0.0, 0.0, 0.0), Vec3::Y),
            FreeCamera::default(),
        ));
    }
    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, -0.5, -0.5, 0.0)),
        // The first cascade only 12 m deep, so near blades are several shadow
        // texels wide: with one cascade over 60 m a texel was wider than a
        // blade, and blade shadows blinked on and off as the grass swayed.
        CascadeShadowConfigBuilder {
            num_cascades: 3,
            first_cascade_far_bound: 12.0,
            maximum_distance: 60.0,
            ..default()
        }
        .build(),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(200.0, 200.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.20, 0.15, 0.09),
            perceptual_roughness: 0.95,
            ..default()
        })),
    ));
    let person = common::spawn_person(&mut commands, &mut meshes, &mut materials, Vec3::ZERO);
    if start.walk {
        commands.entity(person).insert((
            Walking,
            PlantInteractor {
                radius: 0.5,
                strength: 1.0,
                ..default()
            },
        ));
    }

    // Empty until the file has loaded; `refresh` fills it in.
    commands.spawn(PlantScatter::new(Vec2::splat(80.0), Vec::new()));
    commands.insert_resource(Shown {
        files: FIELDS
            .iter()
            .map(|name| assets.load(format!("scatters/{name}.plant_scatter.yml")))
            .collect(),
        file: start.file,
        switches: start.switches,
        dirty: true,
    });
}

/// The person out for a walk (`--walk`).
#[derive(Component)]
struct Walking;

/// The camera that follows them.
#[derive(Component)]
struct Following;

/// Where the walk is `t` seconds in: a stroll along +X, weaving a little.
fn walk_path(t: f32) -> Vec3 {
    let x = -12.0 + 1.3 * t;
    Vec3::new(x, 0.0, 1.5 * (x * 0.25).sin())
}

fn walk(
    time: Res<Time>,
    mut walkers: Query<&mut Transform, (With<Walking>, Without<Following>)>,
    mut cameras: Query<&mut Transform, With<Following>>,
) {
    let t = time.elapsed_secs();
    let at = walk_path(t);
    for mut transform in &mut walkers {
        // A little bob with every step.
        transform.translation = at + Vec3::Y * (0.9 + 0.04 * (t * 6.0).sin().abs());
    }
    for mut transform in &mut cameras {
        // Behind and to the side, 30° down like DEMO ARPG GAME's camera, so the
        // trail of bent grass shows.
        let eye = Vec3::new(at.x - 3.5, 4.5, 5.6);
        *transform = Transform::from_translation(eye)
            .looking_at(Vec3::new(at.x + 0.5, 0.5, at.z * 0.5), Vec3::Y);
    }
}

fn switch(
    keys: Res<ButtonInput<KeyCode>>,
    mut shown: ResMut<Shown>,
    mut settings: ResMut<PlantSettings>,
) {
    let before = (shown.file, shown.switches);
    for (key, file) in [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3]
        .into_iter()
        .zip(0..)
    {
        if keys.just_pressed(key) {
            shown.file = file;
        }
    }
    let switches = &mut shown.switches;
    for (key, on) in [
        (KeyCode::KeyP, &mut switches.plants),
        (KeyCode::KeyT, &mut switches.tufts),
        (KeyCode::KeyB, &mut switches.shape),
        (KeyCode::KeyC, &mut switches.colour),
    ] {
        if keys.just_pressed(key) {
            *on = !*on;
        }
    }
    if (shown.file, shown.switches) != before {
        shown.dirty = true;
    }
    if keys.just_pressed(KeyCode::KeyS) {
        settings.shadows = if settings.shadows == PlantShadows::Off {
            PlantShadows::Near
        } else {
            PlantShadows::Off
        };
    }
}

/// Reapplies the switches when the field file or a switch changes.
fn refresh(
    mut shown: ResMut<Shown>,
    mut events: MessageReader<AssetEvent<PlantScatterPresetAsset>>,
    presets: Res<Assets<PlantScatterPresetAsset>>,
    mut fields: Query<&mut PlantScatter>,
) {
    let file = shown.files[shown.file].id();
    if events
        .read()
        .any(|event| event.is_loaded_with_dependencies(file) || event.is_modified(file))
    {
        shown.dirty = true;
    }
    if !shown.dirty {
        return;
    }
    let Some(PlantScatterPresetAsset(preset)) = presets.get(file) else {
        return;
    };
    for mut field in &mut fields {
        field.options = apply(&preset.plant_scatter, shown.switches);
    }
    shown.dirty = false;
}

/// `base` with the kinds of variation that are off taken out.
fn apply(base: &PlantScatterOptions, on: Switches) -> PlantScatterOptions {
    let mut field = base.clone();
    if !on.plants
        && let Some(background) = field.plants.last().copied()
    {
        // Only the background plant, as dense as the whole mix was.
        let density = field.density();
        field.plants = vec![background].into();
        field = field.with_density(density);
    }
    if !on.tufts {
        field
            .plants
            .to_mut()
            .iter_mut()
            .for_each(|entry| entry.plant.tufts = 0.0);
        field.variation.bare_share = 0.0;
    }
    if !on.shape {
        for entry in field.plants.to_mut() {
            for segment in [&mut entry.plant.segment, &mut entry.plant.accent.segment] {
                segment.twist = 0.0;
                segment.droop = 0.0;
                segment.fold = 0.0;
            }
        }
    }
    if !on.colour {
        field.variation.patch_amount = 0.0;
        field.variation.dry_tips = 0.0;
        field.variation.dead_share = 0.0;
    }
    field
}

fn title(shown: Res<Shown>, mut windows: Query<&mut Window>) {
    if !shown.is_changed() {
        return;
    }
    let mark = |on: bool| if on { "on" } else { "off" };
    let s = shown.switches;
    let text = format!(
        "procgena_plant: variation — {} (1-3) | plants {} (P) | tufts {} (T) | shape {} (B) | colour {} (C)",
        FIELDS[shown.file],
        mark(s.plants),
        mark(s.tufts),
        mark(s.shape),
        mark(s.colour),
    );
    for mut window in &mut windows {
        window.title.clone_from(&text);
    }
}
