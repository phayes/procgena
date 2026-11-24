//! Bakes a simple object into an impostor and draws the object and its
//! impostor side by side, the object on the left.
//!
//! ```sh
//! cargo run -p procgena_impostor --example impostor
//! cargo run -p procgena_impostor --example impostor -- --headless --screenshot impostor.png
//! ```
//!
//! `--orbit <degrees>` turns the camera round the pair, and `--height <m>`
//! raises it (a negative height looks from below). `--coverage <degrees>`
//! bakes views only down to that viewer elevation; the default covers every
//! direction. `--hang` hangs the object upside down, moved a little
//! sideways, and draws its impostor that way through a material of the
//! example's own, which composes its transform and offset in its vertex
//! shader as the crate's "Drawing impostors with your own transform"
//! describes. `--headless` renders without a window and needs
//! `--screenshot`, which saves one picture once the impostor is ready and
//! exits.

use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::RenderTarget;
use bevy::camera::visibility::NoAutoAabb;
use bevy::material::OpaqueRendererMethod;
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType, TextureFormat};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::shader::{Shader, ShaderRef};
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use procgena_core::{GeneratorKind, OptionsHash, PoolId, SpecimenId};
use procgena_impostor::{
    BakeOutput, BakePart, BakeSettings, BakedImpostor, IMPOSTOR_PREPASS_SHADER, IMPOSTOR_SHADER,
    Impostor, ImpostorKey, ImpostorMaterial, ImpostorPlugin, ImpostorQuad, ImpostorReady,
    ImpostorRequest, ImpostorSettings, ImpostorUniform, ImpostorWrites, ViewCoverage,
    bake_standard,
};

/// The `--hang` material's vertex shader.
const HUNG_SHADER: Handle<Shader> = uuid_handle!("6f1d2c0e-8a4b-4e57-9c3d-2b7a1e5f9d40");
/// The `--hang` material's prepass vertex shader.
const HUNG_PREPASS_SHADER: Handle<Shader> = uuid_handle!("0c9e4a71-3d2f-4b8e-a615-7f2d9b3c8e12");

/// An impostor drawn with a transform of its own on top of its entity's,
/// plus a world offset: a stand-in for an attached object, such as fruit,
/// placed by its host.
type HungImpostorMaterial = ExtendedMaterial<StandardMaterial, HungImpostor>;

/// The impostor's own bindings, at the numbers the impostor shaders read,
/// then where it hangs.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
struct HungImpostor {
    #[uniform(100)]
    params: ImpostorUniform,
    #[texture(101)]
    #[sampler(102)]
    albedo: Handle<Image>,
    #[texture(103)]
    normal: Handle<Image>,
    #[uniform(104)]
    hang: Hang,
}

/// Where a hung impostor hangs within its entity, and how far it is moved.
#[derive(ShaderType, Reflect, Debug, Clone)]
struct Hang {
    entity_from_object: Mat4,
    object_from_entity: Mat4,
    world_offset: Vec4,
}

impl MaterialExtension for HungImpostor {
    fn vertex_shader() -> ShaderRef {
        HUNG_SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        IMPOSTOR_SHADER.into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        HUNG_PREPASS_SHADER.into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        IMPOSTOR_PREPASS_SHADER.into()
    }
}

/// Where `--hang` hangs the object within its entity: upside down from 4 m
/// up.
fn hung_transform() -> Transform {
    Transform::from_xyz(0.0, 4.0, 0.0).with_rotation(Quat::from_rotation_z(std::f32::consts::PI))
}

/// How far `--hang` moves the object sideways, in world units.
const HANG_OFFSET: Vec3 = Vec3::new(0.0, 0.0, 0.6);

/// Frames to wait after the impostor is ready before the picture.
const SETTLE_FRAMES: u32 = 20;

#[derive(Resource)]
struct Options {
    screenshot: Option<String>,
    headless: bool,
    orbit: f32,
    height: f32,
    cache: Option<std::path::PathBuf>,
    coverage: ViewCoverage,
    hang: bool,
}

/// The image the camera draws into when headless.
#[derive(Resource)]
struct Offscreen(Handle<Image>);

/// Frames since the impostor was shown.
#[derive(Resource, Default)]
struct Shown(Option<u32>);

fn main() -> AppExit {
    let mut options = Options {
        screenshot: None,
        headless: false,
        orbit: 30.0,
        height: 3.0,
        cache: None,
        coverage: ViewCoverage::SPHERE,
        hang: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().expect("a value after the option");
        match arg.as_str() {
            "--headless" => options.headless = true,
            "--screenshot" => options.screenshot = Some(value()),
            "--orbit" => options.orbit = value().parse().expect("--orbit needs degrees"),
            "--height" => options.height = value().parse().expect("--height needs metres"),
            "--cache" => options.cache = Some(value().into()),
            "--coverage" => {
                let degrees = value().parse().expect("--coverage needs degrees");
                options.coverage = ViewCoverage::from_lowest_elevation(degrees);
            }
            "--hang" => options.hang = true,
            other => panic!("unknown argument {other}"),
        }
    }
    assert!(
        !options.headless || options.screenshot.is_some(),
        "--headless needs --screenshot <file.png>"
    );

    let mut app = App::new();
    if options.headless {
        app.add_plugins((
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .disable::<WinitPlugin>(),
            ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(1.0 / 60.0)),
        ));
    } else {
        app.add_plugins(DefaultPlugins);
    }
    app.add_plugins(ImpostorPlugin {
        settings: ImpostorSettings {
            cache_dir: options.cache.clone(),
            ..default()
        },
    });
    load_internal_asset!(app, HUNG_SHADER, "hung_impostor.wesl", Shader::from_wesl);
    load_internal_asset!(
        app,
        HUNG_PREPASS_SHADER,
        "hung_impostor_prepass.wesl",
        Shader::from_wesl
    );
    app.add_plugins(MaterialPlugin::<HungImpostorMaterial>::default())
        .insert_resource(options)
        .init_resource::<Shown>()
        .add_systems(Startup, setup)
        .add_observer(show_impostor)
        .add_systems(Update, screenshot)
        .run()
}

fn setup(
    mut commands: Commands,
    options: Res<Options>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bake_materials: ResMut<Assets<procgena_impostor::BakeStandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    // A simple object: a trunk and two crowns, standing on the origin.
    let bark = StandardMaterial::from(Color::srgb(0.45, 0.3, 0.18));
    let leaves = StandardMaterial::from(Color::srgb(0.2, 0.55, 0.2));
    let parts = [
        (
            meshes.add(
                Cylinder::new(0.15, 2.0)
                    .mesh()
                    .build()
                    .translated_by(Vec3::Y),
            ),
            bark,
        ),
        (
            meshes.add(Sphere::new(0.9).mesh().build().translated_by(Vec3::Y * 2.4)),
            leaves.clone(),
        ),
        (
            meshes.add(
                Sphere::new(0.6)
                    .mesh()
                    .build()
                    .translated_by(Vec3::new(0.5, 3.1, 0.2)),
            ),
            leaves,
        ),
    ];

    // The object itself, on the left; with `--hang`, hung as its impostor
    // will be.
    let mut placement = Transform::from_xyz(-1.5, 0.0, 0.0);
    if options.hang {
        placement = placement * hung_transform();
        placement.translation += HANG_OFFSET;
    }
    let object = commands.spawn((placement, Visibility::default())).id();
    for (mesh, material) in &parts {
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(materials.add(material.clone())),
            ChildOf(object),
        ));
    }

    // Ask for its impostor.
    let bake_parts = parts
        .iter()
        .map(|(mesh, material)| {
            BakePart::new(
                mesh.clone(),
                bake_materials.add(bake_standard(material, BakeOutput::Albedo)),
                bake_materials.add(bake_standard(material, BakeOutput::Normal)),
            )
        })
        .collect();
    // The primitives aren't generated, so they stand in as slot 0 of a pool
    // of their own.
    let pool = PoolId::new(
        GeneratorKind::from_key("impostor.example.kind"),
        OptionsHash(0),
    );
    let specimen = SpecimenId::new(0, pool, 0);
    commands.spawn(ImpostorRequest {
        key: ImpostorKey::specimen(
            specimen,
            BakeSettings::default().with_coverage(options.coverage),
            &[],
        ),
        parts: bake_parts,
        wait_for: Vec::new(),
    });

    // Ground, light and camera.
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(12.0, 12.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.5, 0.5, 0.45))),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 8000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    let orbit = options.orbit.to_radians();
    let eye = Vec3::new(8.0 * orbit.sin(), options.height, 8.0 * orbit.cos());
    let mut camera = commands.spawn((
        Camera3d::default(),
        Transform::from_translation(eye).looking_at(Vec3::new(0.0, 1.6, 0.0), Vec3::Y),
    ));
    if options.headless {
        let image = images.add(Image::new_target_texture(
            1280,
            720,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
        camera.insert(RenderTarget::Image(image.clone().into()));
        commands.insert_resource(Offscreen(image));
    }
}

/// Draws the impostor on the right once it is baked: with the crate's own
/// material, or hung with the example's.
#[expect(clippy::too_many_arguments, reason = "one Bevy observer")]
fn show_impostor(
    event: On<ImpostorReady>,
    mut commands: Commands,
    options: Res<Options>,
    baked: Query<&BakedImpostor>,
    quad: Res<ImpostorQuad>,
    mut materials: ResMut<Assets<ImpostorMaterial>>,
    mut hung_materials: ResMut<Assets<HungImpostorMaterial>>,
    mut shown: ResMut<Shown>,
) {
    let Ok(BakedImpostor(impostor)) = baked.get(event.entity) else {
        return;
    };
    info!("impostor {} ready", event.key);
    let placement = Transform::from_xyz(1.5, 0.0, 0.0);
    if options.hang {
        let material = hung_materials.add(hung_material(impostor));
        // The quad is placed by the shader, so bounds that reach the hung
        // object keep it from being culled.
        let bounds =
            bevy::camera::primitives::Aabb::from_min_max(Vec3::splat(-5.0), Vec3::splat(5.0));
        commands.spawn((
            Mesh3d(quad.0.clone()),
            MeshMaterial3d(material),
            bounds,
            NoAutoAabb,
            placement,
        ));
    } else {
        let material = materials.add(impostor.material());
        commands.spawn((impostor.bundle(&quad, material), placement));
    }
    shown.0 = Some(0);
}

/// A material drawing `impostor` hung as [`hung_transform`] and
/// [`HANG_OFFSET`] place it.
fn hung_material(impostor: &Impostor) -> HungImpostorMaterial {
    let hung = hung_transform().to_matrix();
    ExtendedMaterial {
        base: StandardMaterial {
            alpha_mode: AlphaMode::Mask(0.5),
            perceptual_roughness: 0.9,
            double_sided: true,
            cull_mode: None,
            opaque_render_method: OpaqueRendererMethod::Forward,
            ..default()
        },
        extension: HungImpostor {
            params: ImpostorUniform {
                center: impostor.center,
                radius: impostor.radius,
                frames: impostor.frames,
                lowest_elevation: impostor.coverage.lowest_elevation_deg(),
            },
            albedo: impostor.albedo.clone(),
            normal: impostor.normal.clone(),
            hang: Hang {
                entity_from_object: hung,
                object_from_entity: hung.inverse(),
                world_offset: HANG_OFFSET.extend(0.0),
            },
        },
    }
}

/// With `--screenshot`, saves one picture a little after the impostor
/// appears, then exits.
fn screenshot(
    mut commands: Commands,
    options: Res<Options>,
    offscreen: Option<Res<Offscreen>>,
    writes: Res<ImpostorWrites>,
    mut shown: ResMut<Shown>,
) {
    let (Some(path), Some(frames)) = (&options.screenshot, &mut shown.0) else {
        return;
    };
    // Let the impostor be saved before exiting.
    if *frames >= SETTLE_FRAMES && writes.pending() > 0 {
        return;
    }
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
