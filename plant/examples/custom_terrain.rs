//! Far grass in a terrain material of the game's own: a ground with its own
//! shader (`custom_terrain.wesl`) that calls `plant_far_color`, as a game
//! with its own terrain shader would, instead of using `FarPlantMaterial`.
//!
//! The material holds a `FarPlantUniform` and the gust texture
//! (`FarPlants::GUSTS`); a system rewrites the uniform every frame from the
//! wind and the grass radius (`FarPlants::uniform`), so the gusts roll across
//! the far ground in step with the blades.
//!
//! ```text
//! cargo run --release --example custom_terrain
//! cargo run --release --example custom_terrain -- --fixed-time --screenshot shots/custom_terrain.png
//! ```

mod common;

use bevy::asset::uuid_handle;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use procgena_plant::FarPlantUniform;
use procgena_plant::prelude::*;

/// The terrain shader, added from the file next to this example.
const TERRAIN_SHADER: Handle<Shader> = uuid_handle!("4f1e2a9c-7b3d-4c6e-8a5f-2d9b1c0e7a64");

fn main() {
    let mut app = App::new();
    common::ExampleArgs::parse().install(&mut app, "procgena_plant: custom terrain material");
    app.add_plugins((PlantPlugins, MaterialPlugin::<Terrain>::default()))
        .insert_resource(TerrainPlants(FarPlants::new(&FieldPreset::MEADOW.field)))
        .add_systems(Startup, setup)
        .add_systems(Update, follow_the_plants.after(PlantWindSystems::Sample))
        .run();
}

/// The game's terrain material.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
struct Terrain {
    #[uniform(0)]
    far: FarPlantUniform,
    #[texture(1)]
    #[sampler(2)]
    gusts: Handle<Image>,
    #[uniform(3)]
    look: TerrainLook,
}

/// The terrain's own settings. Mirrors `Terrain` in `custom_terrain.wesl`.
#[derive(ShaderType, Debug, Clone, Copy)]
struct TerrainLook {
    soil: Vec4,
    sun: Vec4,
}

impl Material for Terrain {
    fn fragment_shader() -> ShaderRef {
        TERRAIN_SHADER.into()
    }
}

/// What the far grass on the terrain looks like.
#[derive(Resource)]
struct TerrainPlants(FarPlants);

/// Keeps the terrain's far grass on this frame's wind and radius.
fn follow_the_plants(
    terrain_plants: Res<TerrainPlants>,
    wind: Res<PlantWindFrame>,
    settings: Res<PlantSettings>,
    mut terrains: ResMut<Assets<Terrain>>,
) {
    let far = terrain_plants.0.uniform(&wind, &settings);
    for (_, terrain) in terrains.iter_mut() {
        terrain.far = far;
    }
}

fn setup(
    mut commands: Commands,
    mut shaders: ResMut<Assets<Shader>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut terrains: ResMut<Assets<Terrain>>,
    terrain_plants: Res<TerrainPlants>,
) {
    shaders
        .insert(
            &TERRAIN_SHADER,
            Shader::from_wesl(include_str!("custom_terrain.wesl"), "custom_terrain.wesl"),
        )
        .expect("a fixed handle is always valid");
    let sun = Vec3::new(4.0, 8.0, 3.0);
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 3.0, 12.0).looking_at(Vec3::new(0.0, 0.0, -40.0), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_translation(sun).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(1000.0, 1000.0))),
        MeshMaterial3d(
            terrains.add(Terrain {
                // `follow_the_plants` brings it up to date every frame.
                far: terrain_plants
                    .0
                    .uniform(&PlantWindFrame::default(), &PlantSettings::default()),
                gusts: FarPlants::GUSTS,
                look: TerrainLook {
                    soil: LinearRgba::from(Color::srgb(0.2, 0.15, 0.09)).to_vec4(),
                    sun: sun.normalize().extend(0.0),
                },
            }),
        ),
    ));
    commands.spawn(PlantField::from_preset(
        &FieldPreset::MEADOW,
        Vec2::splat(400.0),
    ));
}
