//! The land: a World's terrain streamed around the camera, a square of
//! forest placed in Ecotope tiles, and colliders near the lumberjack.

use std::collections::HashMap;
use std::sync::Arc;

use avian3d::prelude::*;
use bevy::math::DVec2;
use bevy::prelude::*;
use procgena_biome::{Ecotope, EcotopePreset, ScatterCopies};
use procgena_biome_bevy::{
    EcotopeCopies, EcotopeFinished, EcotopeParams, EcotopePlacing, EcotopePlugin,
};
use procgena_core::Ground;
use procgena_core::scatter::ScatterArea;
use procgena_core_bevy::LodQuality;
use procgena_deadwood_bevy::{DeadwoodGenerating, DeadwoodPlugin};
use procgena_mushroom_bevy::{MushroomGenerating, MushroomPlugin};
use procgena_plant_bevy::PlantPlugins;
use procgena_stone_bevy::{StoneGenerating, StonePlugin};
use procgena_terrain::{ChunkDetail, Flags, Landform, WorldTerrain};
use procgena_terrain_bevy::{
    RegionTracker, TerrainChunk, TerrainPlugin, TerrainProgress, TerrainStream, TerrainWater,
};
use procgena_tree_bevy::{TreeGenerating, TreePlugin};
use procgena_world::{World, WorldPreset};

use crate::camera::MainCamera;
use crate::player::{Player, Spawn};
use crate::settings::DemoOptions;
use crate::{GameState, Launch};

/// How far the terrain is drawn, in metres.
const VIEW_DISTANCE: f32 = 4000.0;

/// Fixed-point units per metre in a copy's footprint disc.
const FIXED_PER_METRE: f32 = 256.0;

/// How tall a trunk's collider is, in metres: well above the lumberjack's
/// head, since he can't climb.
const TRUNK_HEIGHT: f32 = 6.0;

/// Trunks thinner than this, in metres of radius, are walked through
/// (saplings and bushes).
const MIN_TRUNK_RADIUS: f32 = 0.08;

/// Stones smaller than this, in metres of footprint radius, are stepped
/// over.
const MIN_STONE_RADIUS: f32 = 0.35;

/// How far the lumberjack moves, in metres, before colliders near him are
/// gathered again.
const RESCAN_STEP: f32 = 8.0;

/// The demo's World and forest presets, built into the binary.
const WORLD_FILE: &str = include_str!("../presets/lumberjack.world.yml");
const FOREST_FILE: &str = include_str!("../presets/lumberjack_forest.ecotope.yml");

/// Builds the land and places the forest, then starts play.
pub struct LandPlugin;

impl Plugin for LandPlugin {
    fn build(&self, app: &mut App) {
        let launch = app.world().resource::<Launch>().clone();
        let land = Land::new(launch.seed);
        let plugin = TerrainPlugin::detailed(ChunkDetail::DEFAULT, VIEW_DISTANCE)
            .expect("the view distance is positive");
        app.add_plugins((
            plugin,
            TreePlugin::default(),
            StonePlugin::default(),
            DeadwoodPlugin::default(),
            MushroomPlugin::default(),
            PlantPlugins,
            EcotopePlugin,
        ))
        .insert_resource(LodQuality(launch.quality))
        .insert_resource(land)
        .init_resource::<Solids>()
        .add_systems(Startup, (spawn_terrain, spawn_tiles))
        .add_systems(
            Update,
            (
                paint_water,
                ground_colliders,
                gather_solids,
                solid_colliders.after(gather_solids),
                finish_loading.run_if(in_state(GameState::Loading)),
            ),
        )
        .add_observer(count_tile);
    }
}

/// The land everything stands on, shared by the terrain stream and the
/// forest.
#[derive(Resource, Clone)]
pub struct Land {
    /// The World's terrain.
    pub terrain: Arc<WorldTerrain>,
    /// Counts the terrain's region builds, for the loading progress.
    regions: RegionTracker,
    /// The forest planted over the play area.
    pub forest: Arc<Ecotope>,
}

impl Land {
    /// The demo's World, with `seed` instead of its own if given, and its
    /// forest.
    fn new(seed: Option<u32>) -> Self {
        let mut options = WorldPreset::from_yaml("lumberjack", WORLD_FILE)
            .expect("the demo's World preset is valid")
            .world;
        if let Some(seed) = seed {
            options.seed = seed;
        }
        let regions = RegionTracker::default();
        let world = World::new(options)
            .expect("the demo's World builds")
            .with_progress(regions.hook());
        let forest = EcotopePreset::from_yaml("lumberjack_forest", FOREST_FILE)
            .expect("the demo's forest preset is valid")
            .ecotope;
        Self {
            terrain: world.terrain().clone(),
            regions,
            forest: Arc::new(forest),
        }
    }

    /// The ground's height at World position `p` (x and z, metres).
    pub fn height(&self, p: Vec2) -> f32 {
        Ground::height(&*self.terrain, p.as_dvec2())
    }
}

/// The ground, trunks and stones the lumberjack can't walk through.
#[derive(Resource, Default)]
struct Solids {
    /// Every trunk and big stone of every tile: where it stands (World
    /// metres) and its radius.
    all: Vec<Solid>,
    /// Whether `all` must be gathered again.
    stale: bool,
    /// The colliders spawned now, by their index in `all`.
    spawned: HashMap<usize, Entity>,
    /// Where they were gathered around last.
    centre: Option<Vec3>,
}

/// A trunk or stone to collide with.
#[derive(Debug, Clone, Copy)]
struct Solid {
    /// The foot of the trunk or the stone's origin, in World metres.
    at: Vec3,
    /// Its radius, in metres.
    radius: f32,
    /// A trunk, rather than a stone.
    trunk: bool,
}

/// Marks a collider made for a trunk or stone.
#[derive(Component)]
struct SolidCollider;

/// How many tiles have been placed, of how many.
#[derive(Resource, Debug, Clone, Copy, Default)]
struct Tiles {
    placed: usize,
    total: usize,
}

/// Spawns the terrain stream over the World, with the camera above the
/// start.
fn spawn_terrain(
    mut commands: Commands,
    land: Res<Land>,
    options: Res<DemoOptions>,
    mut camera: Single<&mut Transform, With<MainCamera>>,
) {
    commands.spawn(TerrainStream {
        regions: Some(land.regions.clone()),
        ..TerrainStream::new(land.terrain.clone())
    });
    let centre = start(&options);
    let ground = land.height(centre);
    camera.translation = Vec3::new(centre.x, ground + 2.0, centre.y + 6.0);
    camera.look_at(Vec3::new(centre.x, ground + 1.0, centre.y), Vec3::Y);
    commands.insert_resource(Spawn(Vec3::new(centre.x, ground, centre.y)));
}

/// The start: the middle of the play area.
fn start(options: &DemoOptions) -> Vec2 {
    let [x, z] = options.play_area.centre;
    Vec2::new(x as f32, z as f32)
}

/// Spawns one Ecotope tile per square of the play area, all of the forest,
/// kept off water, roads and cliffs.
fn spawn_tiles(mut commands: Commands, land: Res<Land>, options: Res<DemoOptions>) {
    let area = &options.play_area;
    let ground: Arc<dyn Ground> = land.terrain.clone();
    let terrain = land.terrain.clone();
    let coverage: Arc<dyn procgena_core::scatter::Mask> =
        Arc::new(move |p: DVec2| if plantable(&terrain, p) { 1.0 } else { 0.0 });
    let [cx, cz] = area.centre;
    let half = f64::from(area.size()) * 0.5;
    let size = f64::from(area.tile_size);
    for j in 0..area.tiles {
        for i in 0..area.tiles {
            let centre = DVec2::new(
                cx - half + (f64::from(i) + 0.5) * size,
                cz - half + (f64::from(j) + 0.5) * size,
            );
            commands.spawn(EcotopeParams {
                ecotope: land.forest.clone(),
                seed: land.forest.default_seed,
                ground: ground.clone(),
                area: ScatterArea::new(centre, Vec2::splat(area.tile_size)),
                coverage: Some(coverage.clone()),
                patch: None,
            });
        }
    }
    let total = (area.tiles * area.tiles) as usize;
    commands.insert_resource(Tiles { placed: 0, total });
}

/// Whether the forest may grow at `p`: dry, off roads, and not a cliff.
fn plantable(terrain: &WorldTerrain, p: DVec2) -> bool {
    let Some(labels) = terrain.annotate(p) else {
        return false;
    };
    let kept_off = Flags::UNDERWATER | Flags::RIVER | Flags::POND | Flags::ROAD | Flags::ROAD_BANK;
    !labels.flags.intersects(kept_off)
        && !matches!(
            labels.landform,
            Landform::Cliff | Landform::Riverbed | Landform::Pondbed | Landform::Road
        )
}

/// Counts placed tiles, and gathers the solids again after each.
fn count_tile(finished: On<EcotopeFinished>, mut tiles: ResMut<Tiles>, mut solids: ResMut<Solids>) {
    match &finished.result {
        Ok(summary) => {
            tiles.placed += 1;
            info!(
                "tile {}/{} placed in {:.1?}: {} copies",
                tiles.placed,
                tiles.total,
                summary.place_time,
                summary.copies()
            );
        }
        Err(error) => error!("a forest tile could not be placed: {error}"),
    }
    solids.stale = true;
}

/// Gives every chunk's water its material.
fn paint_water(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut material: Local<Option<Handle<StandardMaterial>>>,
    added: Query<Entity, Added<TerrainWater>>,
) {
    for entity in &added {
        let handle = material.get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::srgba(0.1, 0.24, 0.3, 0.85),
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.08,
                ..default()
            })
        });
        commands
            .entity(entity)
            .insert(MeshMaterial3d(handle.clone()));
    }
}

/// Where colliders are wanted around: the lumberjack, or the camera before
/// he is spawned.
fn collider_centre(
    player: &Query<&Transform, With<Player>>,
    camera: &Query<&Transform, (With<MainCamera>, Without<Player>)>,
) -> Option<Vec3> {
    player
        .iter()
        .next()
        .or_else(|| camera.iter().next())
        .map(|t| t.translation)
}

/// Makes the finest terrain chunks near the lumberjack solid, from the
/// meshes they are drawn with, and drops the colliders of chunks that have
/// moved out of reach. Each chunk drawn again is a new entity, so its
/// collider is made again with it.
fn ground_colliders(
    mut commands: Commands,
    options: Res<DemoOptions>,
    meshes: Res<Assets<Mesh>>,
    chunks: Query<(Entity, &TerrainChunk, &Mesh3d, Has<Collider>)>,
    player: Query<&Transform, With<Player>>,
    camera: Query<&Transform, (With<MainCamera>, Without<Player>)>,
) {
    let Some(centre) = collider_centre(&player, &camera) else {
        return;
    };
    let reach = options.play_area.collider_radius;
    for (entity, chunk, mesh, solid) in &chunks {
        if chunk.key.level != 0 {
            continue;
        }
        let near = {
            let min = chunk.key.min_corner().as_vec2();
            let size = chunk.key.size() as f32;
            let nearest = Vec2::new(centre.x, centre.z).clamp(min, min + size);
            nearest.distance(Vec2::new(centre.x, centre.z)) < reach
        };
        match (near, solid) {
            (true, false) => {
                let Some(mesh) = meshes.get(&mesh.0) else {
                    continue;
                };
                let Some(collider) = Collider::trimesh_from_mesh(mesh) else {
                    warn!("chunk {:?} has no triangles to collide with", chunk.key);
                    continue;
                };
                commands
                    .entity(entity)
                    .insert((RigidBody::Static, collider));
            }
            (false, true) => {
                commands.entity(entity).remove::<(RigidBody, Collider)>();
            }
            _ => {}
        }
    }
}

/// Gathers every trunk and big stone of the placed tiles, in World metres,
/// after each tile is placed.
fn gather_solids(mut solids: ResMut<Solids>, tiles: Query<&EcotopeCopies>) {
    if !solids.stale {
        return;
    }
    let mut all = Vec::new();
    for copies in &tiles {
        let centre = copies.area().centre.as_vec2();
        let offset = Vec3::new(centre.x, 0.0, centre.y);
        for layer in copies.placement().layers() {
            match layer.copies() {
                ScatterCopies::Tree(trees) => {
                    for tree in trees.instances() {
                        let radius = tree.footprint.radius as f32 / FIXED_PER_METRE;
                        if radius >= MIN_TRUNK_RADIUS {
                            all.push(Solid {
                                at: tree.position + offset,
                                radius,
                                trunk: true,
                            });
                        }
                    }
                }
                ScatterCopies::Stone(stones) => {
                    for stone in stones.instances() {
                        if stone.radius >= MIN_STONE_RADIUS {
                            all.push(Solid {
                                at: stone.position + offset,
                                radius: stone.radius,
                                trunk: false,
                            });
                        }
                    }
                }
                ScatterCopies::Deadwood(_)
                | ScatterCopies::Mushroom(_)
                | ScatterCopies::Plant(_) => {}
            }
        }
    }
    *solids = Solids {
        all,
        stale: false,
        spawned: HashMap::new(),
        centre: None,
    };
}

/// Keeps a collider on every trunk and big stone near the lumberjack:
/// an upright capsule per trunk, a sphere per stone.
fn solid_colliders(
    mut commands: Commands,
    options: Res<DemoOptions>,
    mut solids: ResMut<Solids>,
    old: Query<Entity, With<SolidCollider>>,
    player: Query<&Transform, With<Player>>,
    camera: Query<&Transform, (With<MainCamera>, Without<Player>)>,
) {
    let Some(centre) = collider_centre(&player, &camera) else {
        return;
    };
    // A fresh gathering replaces every collider.
    if solids.centre.is_none() {
        for entity in &old {
            commands.entity(entity).despawn();
        }
    }
    if solids
        .centre
        .is_some_and(|last| last.distance(centre) < RESCAN_STEP)
    {
        return;
    }
    solids.centre = Some(centre);

    // Keep the colliders in reach, drop the rest, and add the new ones.
    let reach = options.play_area.collider_radius;
    let Solids { all, spawned, .. } = &mut *solids;
    let near =
        |solid: &Solid| Vec2::new(solid.at.x - centre.x, solid.at.z - centre.z).length() < reach;
    spawned.retain(|index, entity| {
        let keep = near(&all[*index]);
        if !keep {
            commands.entity(*entity).despawn();
        }
        keep
    });
    for (index, solid) in all.iter().enumerate() {
        if spawned.contains_key(&index) || !near(solid) {
            continue;
        }
        let (collider, lift) = if solid.trunk {
            let length = TRUNK_HEIGHT - 2.0 * solid.radius;
            (
                Collider::capsule(solid.radius, length.max(0.0)),
                TRUNK_HEIGHT * 0.5,
            )
        } else {
            (Collider::sphere(solid.radius), 0.0)
        };
        let entity = commands
            .spawn((
                SolidCollider,
                RigidBody::Static,
                collider,
                Transform::from_translation(solid.at + Vec3::Y * lift),
            ))
            .id();
        spawned.insert(index, entity);
    }
}

/// Entities whose asset is still being built, or tiles being placed.
type Generating = Or<(
    With<EcotopePlacing>,
    With<TreeGenerating>,
    With<StoneGenerating>,
    With<DeadwoodGenerating>,
    With<MushroomGenerating>,
)>;

/// Starts play once every tile is placed and drawn, the terrain has
/// settled and the ground under the start is solid. Logs progress every
/// few seconds.
#[expect(clippy::too_many_arguments, reason = "one Bevy system")]
fn finish_loading(
    time: Res<Time<Real>>,
    tiles: Res<Tiles>,
    spawn: Res<Spawn>,
    progress: Query<&TerrainProgress>,
    generating: Query<(), Generating>,
    spatial: SpatialQuery,
    mut last_log: Local<f32>,
    mut next: ResMut<NextState<GameState>>,
) {
    let terrain = progress.iter().next().copied().unwrap_or_default();
    let solid_ground = spatial
        .cast_ray(
            spawn.0 + Vec3::Y * 50.0,
            Dir3::NEG_Y,
            100.0,
            true,
            &SpatialQueryFilter::default(),
        )
        .is_some();
    let ready =
        tiles.placed >= tiles.total && generating.is_empty() && terrain.settled && solid_ground;
    let elapsed = time.elapsed_secs();
    if ready || elapsed - *last_log > 5.0 {
        *last_log = elapsed;
        info!(
            "loading at {elapsed:.0} s: tiles {}/{}, terrain {:.0} %, {} assets generating, ground \
             {}",
            tiles.placed,
            tiles.total,
            terrain.fraction * 100.0,
            generating.iter().count(),
            if solid_ground {
                "solid"
            } else {
                "not yet solid"
            },
        );
    }
    if ready {
        next.set(GameState::Playing);
    }
}
