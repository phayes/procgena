//! The land: a World's terrain streamed around the camera, a square of
//! forest placed in Ecotope tiles, and colliders near the lumberjack.

use std::collections::HashMap;
use std::sync::Arc;

use avian3d::prelude::*;
use bevy::math::DVec2;
use bevy::prelude::*;
use procgena_biome::{BiomePreset, BiomeTerrain, Ecotope, EcotopePreset, ScatterCopies};
use procgena_biome_bevy::{EcotopeCopies, EcotopeFinished, EcotopeParams, EcotopePlugin};
use procgena_core::Ground;
use procgena_core::scatter::{Mask, ScatterArea};
use procgena_core_bevy::LodQuality;
use procgena_deadwood_bevy::{DeadwoodGenerating, DeadwoodPlugin};
use procgena_mushroom_bevy::{MushroomGenerating, MushroomPlugin};
use procgena_plant_bevy::PlantPlugins;
use procgena_stone_bevy::{StoneGenerating, StonePlugin};
use procgena_terrain::{ChunkDetail, Flags, Landform, TerrainOptions, TerrainPreset, WorldTerrain};
use procgena_terrain_bevy::{
    RegionTracker, TerrainChunk, TerrainPlugin, TerrainProgress, TerrainStream, TerrainWater,
};
use procgena_tree_bevy::{TreeGenerating, TreePlugin};
use procgena_world::{World, WorldPreset};

use crate::camera::MainCamera;
use crate::player::{Player, Spawn};
use crate::settings::{DemoOptions, LandFeatures};
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
        let features = app.world().resource::<DemoOptions>().land.clone();
        let land = Land::new(launch.seed, &features);
        let plugin = TerrainPlugin::detailed(ChunkDetail::DEFAULT, VIEW_DISTANCE)
            .expect("the view distance is positive");
        app.add_plugins((
            plugin,
            TreePlugin {
                texture_root: Some("tree/assets/textures".to_owned()),
                ..default()
            },
            StonePlugin {
                texture_root: Some("stone/assets/textures".to_owned()),
                ..default()
            },
            DeadwoodPlugin {
                texture_root: Some("deadwood/assets/textures".to_owned()),
                ..default()
            },
            MushroomPlugin {
                texture_root: Some("mushroom/assets/textures".to_owned()),
                ..default()
            },
            PlantPlugins,
            EcotopePlugin,
        ))
        .insert_resource(LodQuality(launch.quality))
        .insert_resource(land)
        .init_resource::<Solids>()
        .add_systems(Startup, (spawn_terrain, queue_tiles))
        .add_systems(
            Update,
            (
                feed_tiles,
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
    /// The demo's World, with `seed` instead of its own if given and only
    /// the land `features` asked for, and its forest.
    pub fn new(seed: Option<u32>, features: &LandFeatures) -> Self {
        let mut options = WorldPreset::from_yaml("lumberjack", WORLD_FILE)
            .expect("the demo's World preset is valid")
            .world;
        if let Some(seed) = seed {
            options.seed = seed;
        }
        let regions = RegionTracker::default();
        let world = World::with_biomes(options, biomes(features))
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

/// The built-in Biomes, each with its terrain's roads and rivers set as
/// `features` asks.
fn biomes(features: &LandFeatures) -> Vec<BiomePreset> {
    BiomePreset::all()
        .map(|preset| {
            let mut options = match &preset.biome.terrain {
                BiomeTerrain::Preset(key) => TerrainPreset::named(key)
                    .unwrap_or_else(|| panic!("Biome {} names no terrain preset {key}", preset.key))
                    .terrain
                    .clone(),
                BiomeTerrain::Inline(options) => options.clone(),
            };
            switch_features(&mut options, features);
            let mut preset = preset.clone();
            preset.biome.terrain = BiomeTerrain::Inline(options);
            preset
        })
        .collect()
}

/// Sets `options`' roads and river density to what `features` asks.
fn switch_features(options: &mut TerrainOptions, features: &LandFeatures) {
    options.features.roads.enabled = features.roads;
    options.features.rivers.density = features.river_density;
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

/// The forest's tiles: those waiting to be placed, nearest the start first,
/// and how many are placing and placed.
#[derive(Resource, Clone, Default)]
struct Tiles {
    /// The tiles not yet started, the next one last.
    waiting: Vec<ForestTile>,
    /// What each tile is placed with but its area.
    template: Option<EcotopeParams>,
    /// How many tiles may place at the same time.
    at_once: usize,
    /// How many are placing now.
    placing: usize,
    /// How many have been placed, of how many.
    placed: usize,
    total: usize,
    /// How many of the tiles play waits for have been placed, of how many.
    near_placed: usize,
    near_total: usize,
}

/// One tile of the forest.
#[derive(Component, Debug, Clone, Copy)]
struct ForestTile {
    /// The tile's centre, in World metres (x and z).
    centre: DVec2,
    /// Whether play waits for it.
    near: bool,
}

/// Spawns the terrain stream over the World, with the camera above the
/// start.
fn spawn_terrain(
    mut commands: Commands,
    land: Res<Land>,
    options: Res<DemoOptions>,
    mut camera: Single<&mut Transform, With<MainCamera>>,
) {
    // The ground is drawn in plain grass, without paint, for now.
    commands.spawn(TerrainStream {
        regions: Some(land.regions.clone()),
        paint: false,
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

/// Queues one Ecotope tile per square of the play area, nearest the start
/// first, all of the forest and, if the play area asks, kept off water,
/// roads and cliffs.
fn queue_tiles(mut commands: Commands, land: Res<Land>, options: Res<DemoOptions>) {
    let area = &options.play_area;

    // Every tile, with how many rings out from the start it lies.
    let [cx, cz] = area.centre;
    let middle = (f64::from(area.tiles) - 1.0) * 0.5;
    let size = f64::from(area.tile_size);
    let mut tiles: Vec<(f64, ForestTile)> = Vec::new();
    for j in 0..area.tiles {
        for i in 0..area.tiles {
            let (di, dj) = (f64::from(i) - middle, f64::from(j) - middle);
            let ring = di.abs().max(dj.abs());
            let centre = DVec2::new(cx + di * size, cz + dj * size);
            let near = ring <= 1.0;
            tiles.push((ring, ForestTile { centre, near }));
        }
    }

    // The nearest last, as they are taken from the end; ties by distance.
    tiles.sort_by(|(ra, a), (rb, b)| {
        let start = DVec2::new(cx, cz);
        (rb, b.centre.distance(start))
            .partial_cmp(&(ra, a.centre.distance(start)))
            .expect("tile positions are finite")
    });
    let near_total = tiles.iter().filter(|(_, t)| t.near).count();
    let waiting: Vec<ForestTile> = tiles.into_iter().map(|(_, t)| t).collect();
    commands.insert_resource(Tiles {
        total: waiting.len(),
        waiting,
        template: Some(EcotopeParams {
            ecotope: land.forest.clone(),
            seed: land.forest.default_seed,
            ground: land.terrain.clone(),
            area: ScatterArea::new(DVec2::ZERO, Vec2::splat(area.tile_size)),
            coverage: coverage(&land, area.keep_off),
            patch: None,
        }),
        at_once: area.placing_at_once as usize,
        near_total,
        ..default()
    });
}

/// The forest's coverage: none, or with `keep_off`, kept off water, roads
/// and cliffs. With `keep_off` the land is read at every spot a copy could
/// stand.
pub fn coverage(land: &Land, keep_off: bool) -> Option<Arc<dyn Mask>> {
    let terrain = land.terrain.clone();
    keep_off.then(|| {
        Arc::new(move |p: DVec2| if plantable(&terrain, p) { 1.0_f32 } else { 0.0 }) as Arc<_>
    })
}

/// Starts placing the next waiting tiles, while fewer than the play area
/// allows are placing.
fn feed_tiles(mut commands: Commands, mut tiles: ResMut<Tiles>) {
    while tiles.placing < tiles.at_once {
        let Some(tile) = tiles.waiting.pop() else {
            return;
        };
        let template = tiles.template.clone().expect("the tiles were queued");
        let area = ScatterArea::new(tile.centre, template.area.size);
        commands.spawn((EcotopeParams { area, ..template }, tile));
        tiles.placing += 1;
    }
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
fn count_tile(
    finished: On<EcotopeFinished>,
    forest: Query<&ForestTile>,
    mut tiles: ResMut<Tiles>,
    mut solids: ResMut<Solids>,
) {
    let Ok(tile) = forest.get(finished.entity) else {
        return;
    };
    // A tile that fails is counted too, so play doesn't wait for it.
    tiles.placing = tiles.placing.saturating_sub(1);
    tiles.placed += 1;
    if tile.near {
        tiles.near_placed += 1;
    }
    match &finished.result {
        Ok(summary) => {
            info!(
                "tile {}/{} placed in {:.1?}: {} copies",
                tiles.placed,
                tiles.total,
                summary.place_time,
                summary.copies()
            );
            for layer in &summary.layers {
                debug!("  {}: {} copies", layer.key, layer.copies);
            }
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

/// Entities whose asset is still being built.
type Generating = Or<(
    With<TreeGenerating>,
    With<StoneGenerating>,
    With<DeadwoodGenerating>,
    With<MushroomGenerating>,
)>;

/// Starts play once the tiles nearest the start are placed, no asset is
/// being built, the terrain has settled and the ground under the start is
/// solid. Logs progress every few seconds.
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
    let ready = tiles.near_placed >= tiles.near_total
        && generating.is_empty()
        && terrain.settled
        && solid_ground;
    let elapsed = time.elapsed_secs();
    if ready || elapsed - *last_log > 5.0 {
        *last_log = elapsed;
        info!(
            "loading at {elapsed:.0} s: near tiles {}/{} (all {}/{}), terrain {:.0} %, {} assets \
             generating, ground {}",
            tiles.near_placed,
            tiles.near_total,
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
