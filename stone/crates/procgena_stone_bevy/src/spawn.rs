//! Generate rocks in background tasks and display their detail levels as child entities.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use procgena_mesh::{GeneratedObject, Object};
use procgena_stone::{
    DetailLevel, OptionsError, Stone, StoneError, StoneOptions, TextureCompression, TextureSize,
};

use crate::material::{StoneMaterial, StoneTextures, stone_material};
use crate::mesh::stone_mesh;

/// Requests a generated rock positioned by the entity's transform.
///
/// Add this component to an entity after installing [`StonePlugin`](crate::StonePlugin).
/// The rock appears when background generation finishes, as one child entity
/// per detail level. Rocks with identical options share meshes and materials.
///
/// The rock is lifted to approximately local `y = 0`. Enabling
/// [`PlacementOptions::settle`](procgena_stone::PlacementOptions::settle) also turns
/// it onto an estimated resting side. The generated entity receives a
/// [`StoneEmbedDepth`] suggesting how far to bury it; apply that offset yourself.
/// It also receives a [`GeneratedObject`] describing the rock's meshes, which
/// other generators, such as clumps of snow or moss, grow on.
/// Changing this component rebuilds the rock; removing it despawns the generated
/// children and removes both components.
///
/// [`MeshOptions::detail`](procgena_stone::MeshOptions::detail) sets each detail
/// level's mesh resolution, switching distance and texture size.
#[derive(Component, Debug, Clone, PartialEq)]
#[require(Transform, Visibility)]
pub enum StoneParams {
    /// Generate from options supplied in memory. Use [`StoneParams::new`] to wrap owned options.
    Options(Arc<StoneOptions>),
    /// Load options from a JSON file with a `.stone.json` extension.
    /// The rock waits for the asset to load and rebuilds when Bevy reports it modified.
    /// Live file edits require Bevy asset watching to be enabled.
    Asset(Handle<StoneOptionsAsset>),
}

impl StoneParams {
    /// Creates a rock component from options. Generation starts after it is spawned.
    pub fn new(options: StoneOptions) -> Self {
        Self::Options(Arc::new(options))
    }
}

/// Rock settings loaded from a `.stone.json` file for [`StoneParams::Asset`].
///
/// Omitted JSON fields use their defaults; see [`StoneOptions::from_json`].
/// The loader validates the settings before making the asset available.
#[derive(Asset, TypePath, Debug, Clone, PartialEq)]
pub struct StoneOptionsAsset(pub StoneOptions);

/// Suggested burial depth for a generated [`StoneParams`], including its option scale.
///
/// Added when the rock's parts spawn and removed when the rock changes.
/// No offset is applied automatically. For an unrotated entity with unit
/// transform scale, subtract this value from `Transform::translation.y`.
/// Otherwise, scale and rotate the local offset `(0, -depth, 0)` before adding
/// it to the entity's translation. The value excludes transform scaling.
/// Burial targets the widest horizontal slice within the configured height cap.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct StoneEmbedDepth(pub f32);

/// Identifies a generated child entity that draws one detail level of a [`StoneParams`].
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StonePart {
    /// Index into the rock's [`MeshOptions::detail`](procgena_stone::MeshOptions::detail)
    /// levels, starting at 0 for the nearest.
    pub lod: usize,
}

/// Reads and validates rock JSON files with the `.stone.json` extension.
#[derive(Debug, Default, TypePath)]
pub(crate) struct StoneOptionsLoader;

impl AssetLoader for StoneOptionsLoader {
    type Asset = StoneOptionsAsset;
    type Settings = ();
    type Error = BevyError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _context: &mut LoadContext<'_>,
    ) -> Result<StoneOptionsAsset, BevyError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let options =
            StoneOptions::from_json(std::str::from_utf8(&bytes)?).map_err(StoneError::Json)?;
        options.validate().map_err(StoneError::Options)?;
        Ok(StoneOptionsAsset(options))
    }

    fn extensions(&self) -> &[&str] {
        &["stone.json"]
    }
}

/// Texture location and format copied from the plugin configuration.
#[derive(Resource, Debug, Clone)]
pub(crate) struct SpawnSettings {
    pub texture_root: Option<String>,
    pub texture_compression: TextureCompression,
}

/// The rock's options as JSON, used to find shared generation results.
/// Entities with the same key reuse one [`Generated`] entry.
#[derive(Component, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct StoneKey(Arc<str>);

/// Present on a [`StoneParams`] entity while its rock is being prepared.
///
/// The plugin adds this whenever a rock is spawned or its [`StoneParams`] changes,
/// and removes it once the rock's parts have been spawned or generation has
/// failed. A [`StoneFinished`] event is triggered at the same moment. While a
/// `.stone.json` asset has not loaded yet, the marker stays on.
#[derive(Component, Debug, Default)]
pub struct StoneGenerating;

/// Triggered on a [`StoneParams`] entity when preparing its rock ends.
///
/// Observe it on the entity, or globally, to learn when a new or changed rock
/// has appeared. On success the rock's [`StonePart`] children are queued with
/// the same commands, so they exist once this frame's commands have run.
#[derive(EntityEvent, Debug, Clone, PartialEq)]
pub struct StoneFinished {
    /// The rock entity.
    pub entity: Entity,
    /// `Ok` when the parts were spawned, or why the options were rejected.
    pub result: Result<(), OptionsError>,
}

/// A mesh produced by a background task for one detail level; absent when the
/// level has no triangles.
struct LevelMesh {
    level: DetailLevel,
    mesh: Option<Mesh>,
}

/// Background results: grounded meshes in detail-level order, plus
/// a burial suggestion scaled by the rock options but not an entity transform.
struct GeneratedMeshes {
    levels: Vec<LevelMesh>,
    embed_depth: f32,
    /// The rock as a tagged object, for generators that grow on it.
    object: Arc<Object>,
}

/// Grounded meshes and burial depth, or the settings error that stopped generation.
type TaskOutput = Result<GeneratedMeshes, OptionsError>;

/// Mesh, material and camera-distance range for one detail level.
struct LevelAssets {
    /// Camera-distance limits; `None` means the sole detail level has no distance limit.
    range: Option<VisibilityRange>,
    /// Absent when this level has no triangles.
    mesh: Option<Handle<Mesh>>,
    /// Material for this level's texture size, shared by levels with the same size.
    material: Handle<StoneMaterial>,
}

/// Meshes and materials reused by rocks with identical options, nearest level first.
struct Generated {
    levels: Vec<LevelAssets>,
    /// Suggested burial depth including option scale, before entity transform scaling.
    embed_depth: f32,
    /// The rock as a tagged object, shared by every rock with these options.
    object: Arc<Object>,
}

enum Entry {
    Pending(Task<TaskOutput>, Arc<StoneOptions>),
    Ready(Arc<Generated>),
    Failed(OptionsError),
}

/// Reuses generated rocks when their options match. Rocks with different shapes
/// can still share texture images and materials (surface colour, texture and shine)
/// when those settings match, avoiding duplicate assets.
#[derive(Resource, Default)]
pub(crate) struct StoneCache {
    stones: HashMap<StoneKey, Entry>,
    textures: HashMap<(String, TextureSize), StoneTextures>,
    materials: HashMap<MaterialKey, Handle<StoneMaterial>>,
}

/// Identifies matching rock surface settings so rocks with different shapes
/// can share a material.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MaterialKey {
    /// Selects the texture images and their resolution; `None` means no textures.
    textures: Option<(String, TextureSize)>,
    /// Surface colour, stored as number bits for exact comparison.
    color: [u32; 3],
    /// How dull or shiny the surface is, stored as number bits for exact comparison.
    roughness: u32,
    /// How metallic the surface is, stored as number bits for exact comparison.
    metallic: u32,
    /// Texture pattern scale, stored as number bits for exact comparison.
    texture_size: u32,
}

impl MaterialKey {
    /// Identifies the requested surface settings and optional texture images.
    fn new(
        material: &procgena_stone::MaterialOptions,
        textures: Option<(String, TextureSize)>,
    ) -> Self {
        Self {
            textures,
            color: material.color.map(f32::to_bits),
            roughness: material.roughness.to_bits(),
            metallic: material.metallic.to_bits(),
            texture_size: material.texture_size.to_bits(),
        }
    }
}

/// Removes generated children when a rock changes or loses its [`StoneParams`] component.
/// Changed rocks are marked to generate replacement parts; unrelated children are kept.
pub(crate) fn reset_changed_stones(
    mut commands: Commands,
    changed: Query<(Entity, Option<&Children>), Changed<StoneParams>>,
    mut removed: RemovedComponents<StoneParams>,
    children: Query<&Children>,
    parts: Query<(), With<StonePart>>,
) {
    let despawn_parts = |commands: &mut Commands, kids: Option<&Children>| {
        for &child in kids.into_iter().flatten() {
            if parts.contains(child) {
                commands.entity(child).despawn();
            }
        }
    };
    for (entity, kids) in &changed {
        despawn_parts(&mut commands, kids);
        commands
            .entity(entity)
            .remove::<(StoneKey, StoneEmbedDepth, GeneratedObject)>()
            .insert(StoneGenerating);
    }
    for entity in removed.read() {
        despawn_parts(&mut commands, children.get(entity).ok());
        if let Ok(mut entity) = commands.get_entity(entity) {
            entity.remove::<(StoneKey, StoneGenerating, StoneEmbedDepth, GeneratedObject)>();
        }
    }
}

/// Marks rocks for rebuilding when Bevy reports a change to their options asset.
pub(crate) fn reload_modified_options(
    mut events: MessageReader<AssetEvent<StoneOptionsAsset>>,
    mut stones: Query<&mut StoneParams>,
) {
    for event in events.read() {
        if let AssetEvent::Modified { id } = event {
            for mut stone in &mut stones {
                if matches!(&*stone, StoneParams::Asset(handle) if handle.id() == *id) {
                    stone.set_changed();
                }
            }
        }
    }
}

/// Selects rocks still waiting for an options key, including assets that have not loaded.
type AwaitingKey = (With<StoneGenerating>, Without<StoneKey>);

/// Assigns an options key to each waiting rock and starts a background task only
/// when those options are not already cached.
pub(crate) fn start_generation(
    mut commands: Commands,
    waiting: Query<(Entity, &StoneParams), AwaitingKey>,
    options_assets: Res<Assets<StoneOptionsAsset>>,
    mut cache: ResMut<StoneCache>,
) {
    for (entity, stone) in &waiting {
        let options = match stone {
            StoneParams::Options(options) => options.clone(),
            StoneParams::Asset(handle) => match options_assets.get(handle) {
                Some(asset) => Arc::new(asset.0.clone()),
                None => continue,
            },
        };
        let key = StoneKey(options.to_json().into());
        cache.stones.entry(key.clone()).or_insert_with(|| {
            let task_options = options.clone();
            let task = AsyncComputeTaskPool::get().spawn(async move { generate(&task_options) });
            Entry::Pending(task, options)
        });
        commands.entity(entity).insert(key);
    }
}

/// Generates grounded meshes for each of `options.mesh.detail`'s levels, and
/// a shared burial suggestion. Applies `options.scale` but does not bury meshes.
/// Returns an error for invalid rock settings.
fn generate(options: &StoneOptions) -> TaskOutput {
    let stone = Stone::new(options.clone())?;
    let levels = stone
        .levels()
        .map(|(&level, mesh)| LevelMesh {
            level,
            mesh: (mesh.triangle_count() > 0).then(|| stone_mesh(mesh)),
        })
        .collect();
    Ok(GeneratedMeshes {
        levels,
        embed_depth: stone.embed_depth(),
        object: Arc::new(stone.object()),
    })
}

/// Collects completed tasks and registers shared meshes and materials.
/// Invalid options are logged and cached as a failed result.
pub(crate) fn finish_generation(
    mut cache: ResMut<StoneCache>,
    settings: Res<SpawnSettings>,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StoneMaterial>>,
) {
    let cache = &mut *cache;
    for entry in cache.stones.values_mut() {
        let Entry::Pending(task, options) = entry else {
            continue;
        };
        let Some(output) = block_on(poll_once(task)) else {
            continue;
        };
        let GeneratedMeshes {
            levels,
            embed_depth,
            object,
        } = match output {
            Ok(generated) => generated,
            Err(error) => {
                error!("procgena_stone_bevy: cannot generate rock: {error}");
                *entry = Entry::Failed(error);
                continue;
            }
        };

        // Share matching materials across rocks and their distance-based detail
        // levels. Match texture images, image resolution, colour, roughness and
        // pattern scale. Reuse texture images; without a texture folder, use
        // plain colours.
        let compression = settings.texture_compression;
        let ranges = visibility_ranges(levels.iter().map(|l| l.level), options.scale);
        let mut level_assets = Vec::with_capacity(levels.len());
        for (l, range) in levels.into_iter().zip(ranges) {
            let size = l.level.texture_size;
            let set = &options.material.texture;
            let textured = settings.texture_root.is_some().then(|| (set.clone(), size));
            let key = MaterialKey::new(&options.material, textured);
            let material = cache.materials.entry(key).or_insert_with(|| {
                let textures = settings.texture_root.as_ref().map(|root| {
                    cache
                        .textures
                        .entry((set.clone(), size))
                        .or_insert_with(|| {
                            StoneTextures::load(&asset_server, root, set, size, compression)
                        })
                        .clone()
                });
                materials.add(stone_material(&options.material, textures.as_ref()))
            });
            level_assets.push(LevelAssets {
                range,
                mesh: l.mesh.map(|m| meshes.add(m)),
                material: material.clone(),
            });
        }
        *entry = Entry::Ready(Arc::new(Generated {
            levels: level_assets,
            embed_depth,
            object,
        }));
    }
}

/// Builds camera-distance limits for detail levels supplied in nearest-first
/// order, for a rock of world size `scale`. A single level has no distance limit.
///
/// Distances are multiplied by `scale / 2.5`, the ratio to the default rock
/// size. The first level starts at distance zero; the last extends
/// indefinitely. At each intervening switch, adjacent levels fade over a band
/// extending `distance × hysteresis` to either side of the scaled switching distance.
pub(crate) fn visibility_ranges(
    levels: impl Iterator<Item = DetailLevel>,
    scale: f32,
) -> Vec<Option<VisibilityRange>> {
    let levels: Vec<DetailLevel> = levels.collect();
    if levels.len() < 2 {
        return vec![None; levels.len()];
    }
    let factor = scale / StoneOptions::default().scale;
    let margin = |level: &DetailLevel| {
        let distance = level.distance * factor;
        let half = distance * level.hysteresis;
        (distance - half).max(0.0)..distance + half
    };
    (0..levels.len())
        .map(|i| {
            let start_margin = if i == 0 { 0.0..0.0 } else { margin(&levels[i]) };
            let end_margin = match levels.get(i + 1) {
                Some(next) => margin(next),
                None => f32::INFINITY..f32::INFINITY,
            };
            Some(VisibilityRange {
                start_margin,
                end_margin,
                use_aabb: false,
            })
        })
        .collect()
}

/// Adds child entities once a waiting rock's assets are ready.
/// Stops waiting after a failed generation attempt. Either way, triggers [`StoneFinished`].
pub(crate) fn spawn_parts(
    mut commands: Commands,
    waiting: Query<(Entity, &StoneKey), With<StoneGenerating>>,
    cache: Res<StoneCache>,
) {
    for (entity, key) in &waiting {
        let result = match cache.stones.get(key) {
            Some(Entry::Ready(generated)) => {
                spawn_stone_parts(&mut commands, entity, generated);
                commands.entity(entity).insert((
                    StoneEmbedDepth(generated.embed_depth),
                    GeneratedObject(generated.object.clone()),
                ));
                Ok(())
            }
            Some(Entry::Failed(error)) => Err(error.clone()),
            Some(Entry::Pending(..)) | None => continue,
        };
        commands.entity(entity).remove::<StoneGenerating>();
        commands.trigger(StoneFinished { entity, result });
    }
}

fn spawn_stone_parts(commands: &mut Commands, stone: Entity, generated: &Generated) {
    for (lod, level) in generated.levels.iter().enumerate() {
        let Some(mesh) = &level.mesh else { continue };
        let mut part = commands.spawn((
            StonePart { lod },
            Mesh3d(mesh.clone()),
            MeshMaterial3d(level.material.clone()),
            ChildOf(stone),
        ));
        if let Some(range) = &level.range {
            part.insert(range.clone());
        }
    }
}

/// Removes unused rock generation entries and their unneeded materials.
/// Their shapes and materials can be freed when nothing else references them.
/// Texture images stay cached for reuse.
pub(crate) fn prune_cache(
    mut removed: RemovedComponents<StoneKey>,
    keys: Query<&StoneKey>,
    mut cache: ResMut<StoneCache>,
) {
    if removed.read().count() == 0 {
        return;
    }
    let used: HashSet<&StoneKey> = keys.iter().collect();
    cache.stones.retain(|key, _| used.contains(key));

    // Preserve materials referenced by the generated rocks still in the cache.
    let drawn: HashSet<AssetId<StoneMaterial>> = cache
        .stones
        .values()
        .filter_map(|entry| match entry {
            Entry::Ready(generated) => Some(generated.levels.iter()),
            _ => None,
        })
        .flatten()
        .map(|level| level.material.id())
        .collect();
    cache
        .materials
        .retain(|_, material| drawn.contains(&material.id()));
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use procgena_stone::StoneDetail;

    use super::*;

    /// Results of every [`StoneFinished`] event, in order.
    #[derive(Resource, Default)]
    struct Finished(Vec<Result<(), OptionsError>>);

    /// Default options with two small detail levels, so generation in tests is quick.
    fn quick() -> StoneOptions {
        let mut options = StoneOptions::default();
        options.mesh.detail = StoneDetail::new(vec![
            DetailLevel::new(24, 0.0, TextureSize::Size512),
            DetailLevel::new(12, 15.0, TextureSize::Size512),
        ]);
        options
    }

    /// An app running only the spawn systems, without rendering or textures.
    fn spawn_app() -> App {
        spawn_app_with(AssetPlugin::default())
    }

    /// Like [`spawn_app`], reading asset files as configured by `assets`.
    fn spawn_app_with(assets: AssetPlugin) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, assets))
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<StoneMaterial>()
            .init_asset::<StoneOptionsAsset>()
            .register_asset_loader(StoneOptionsLoader)
            .init_resource::<StoneCache>()
            .init_resource::<Finished>()
            .insert_resource(SpawnSettings {
                texture_root: None,
                texture_compression: TextureCompression::Bc7,
            })
            .add_systems(
                Update,
                (
                    reset_changed_stones,
                    start_generation,
                    finish_generation,
                    spawn_parts,
                )
                    .chain(),
            )
            .add_observer(|event: On<StoneFinished>, mut finished: ResMut<Finished>| {
                finished.0.push(event.result.clone());
            });
        app
    }

    /// Updates the app until `stone` loses [`StoneGenerating`].
    fn update_until_finished(app: &mut App, stone: Entity) {
        let deadline = Instant::now() + Duration::from_secs(30);
        app.update();
        while app.world().entity(stone).contains::<StoneGenerating>() {
            assert!(Instant::now() < deadline, "rock generation did not finish");
            std::thread::sleep(Duration::from_millis(1));
            app.update();
        }
    }

    /// Detail levels of the generated children of `stone`, sorted.
    fn part_lods(app: &mut App, stone: Entity) -> Vec<usize> {
        let mut parts = app.world_mut().query::<(&StonePart, &ChildOf)>();
        let mut lods: Vec<usize> = parts
            .iter(app.world())
            .filter(|(_, parent)| parent.parent() == stone)
            .map(|(part, _)| part.lod)
            .collect();
        lods.sort_unstable();
        lods
    }

    #[test]
    fn generating_marker_lasts_until_parts_spawn() {
        let mut app = spawn_app();
        let stone = app.world_mut().spawn(StoneParams::new(quick())).id();

        app.update();
        assert!(app.world().entity(stone).contains::<StoneGenerating>());
        assert!(part_lods(&mut app, stone).is_empty());

        update_until_finished(&mut app, stone);
        assert_eq!(part_lods(&mut app, stone), [0, 1]);
        assert_eq!(app.world().resource::<Finished>().0, vec![Ok(())]);

        // Touching the rock replaces its parts. The options are cached, so
        // this finishes within one update.
        app.world_mut()
            .get_mut::<StoneParams>(stone)
            .unwrap()
            .set_changed();
        app.update();
        assert!(!app.world().entity(stone).contains::<StoneGenerating>());
        assert_eq!(part_lods(&mut app, stone), [0, 1]);
        assert_eq!(app.world().resource::<Finished>().0, vec![Ok(()), Ok(())]);

        // New options generate again.
        let options = StoneOptions { seed: 7, ..quick() };
        *app.world_mut().get_mut::<StoneParams>(stone).unwrap() = StoneParams::new(options);
        app.update();
        assert!(app.world().entity(stone).contains::<StoneGenerating>());
        update_until_finished(&mut app, stone);
        assert_eq!(app.world().resource::<Finished>().0.len(), 3);

        // A different detail set is a different rock: it generates its own levels.
        let mut options = StoneOptions { seed: 7, ..quick() };
        options.mesh.detail = StoneDetail::single(16, TextureSize::Size512);
        *app.world_mut().get_mut::<StoneParams>(stone).unwrap() = StoneParams::new(options);
        app.update();
        assert!(app.world().entity(stone).contains::<StoneGenerating>());
        update_until_finished(&mut app, stone);
        assert_eq!(part_lods(&mut app, stone), [0]);
        assert_eq!(app.world().resource::<StoneCache>().stones.len(), 3);
    }

    #[test]
    fn finished_rocks_carry_their_object_until_they_change() {
        let mut app = spawn_app();
        let stone = app.world_mut().spawn(StoneParams::new(quick())).id();
        app.update();
        assert!(!app.world().entity(stone).contains::<GeneratedObject>());

        // The object arrives with the parts and matches the rock's levels.
        update_until_finished(&mut app, stone);
        let object = app.world().get::<GeneratedObject>(stone).unwrap();
        assert_eq!(*object.0, Stone::new(quick()).unwrap().object());

        // Changing the options removes the stale object until the rebuild ends.
        let options = StoneOptions { seed: 7, ..quick() };
        *app.world_mut().get_mut::<StoneParams>(stone).unwrap() = StoneParams::new(options);
        app.update();
        assert!(!app.world().entity(stone).contains::<GeneratedObject>());
        update_until_finished(&mut app, stone);
        assert!(app.world().entity(stone).contains::<GeneratedObject>());

        // Removing the request removes the object.
        app.world_mut().entity_mut(stone).remove::<StoneParams>();
        app.update();
        assert!(!app.world().entity(stone).contains::<GeneratedObject>());
    }

    /// Returns the distinct material identifiers used by a rock's child parts.
    fn part_materials(app: &mut App, stone: Entity) -> HashSet<AssetId<StoneMaterial>> {
        let mut parts = app
            .world_mut()
            .query::<(&MeshMaterial3d<StoneMaterial>, &ChildOf)>();
        parts
            .iter(app.world())
            .filter(|(_, parent)| parent.parent() == stone)
            .map(|(material, _)| material.0.id())
            .collect()
    }

    #[test]
    fn rocks_that_look_the_same_share_one_material() {
        let mut app = spawn_app();
        app.add_systems(Update, prune_cache.after(spawn_parts));
        let spawn = |app: &mut App, options: StoneOptions| {
            let stone = app.world_mut().spawn(StoneParams::new(options)).id();
            update_until_finished(app, stone);
            stone
        };

        // Changing the random seed varies rock shape while preserving the
        // shared material at every detail level.
        let first = spawn(&mut app, quick());
        let second = spawn(&mut app, StoneOptions { seed: 7, ..quick() });
        let shared = part_materials(&mut app, first);
        assert_eq!(shared.len(), 1);
        assert_eq!(part_materials(&mut app, second), shared);

        // Changing surface colour requires a separate material.
        let mut tinted = quick();
        tinted.material.color = [0.5, 0.4, 0.3];
        let third = spawn(&mut app, tinted);
        assert!(part_materials(&mut app, third).is_disjoint(&shared));
        assert_eq!(app.world().resource::<StoneCache>().materials.len(), 2);

        // Once the differently coloured rock is removed, its unique material
        // is no longer needed in the cache.
        app.world_mut().entity_mut(third).despawn();
        app.update();
        assert_eq!(app.world().resource::<StoneCache>().materials.len(), 1);
    }

    #[test]
    fn json_assets_load_partial_options() {
        // Write a partial settings file into a fresh asset folder.
        let dir =
            std::env::temp_dir().join(format!("procgena_stone_bevy_assets_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("rock.stone.json"),
            r#"{"seed": 9, "material": {"texture": "lichen_rock_02"}}"#,
        )
        .unwrap();

        let mut app = spawn_app_with(AssetPlugin {
            file_path: dir.to_string_lossy().into_owned(),
            ..default()
        });
        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<StoneOptionsAsset>("rock.stone.json");
        let stone = app
            .world_mut()
            .spawn(StoneParams::Asset(handle.clone()))
            .id();
        update_until_finished(&mut app, stone);
        std::fs::remove_dir_all(&dir).unwrap();

        // Omitted settings keep their defaults.
        let mut expected = StoneOptions {
            seed: 9,
            ..StoneOptions::default()
        };
        expected.material.texture = "lichen_rock_02".to_owned();
        let assets = app.world().resource::<Assets<StoneOptionsAsset>>();
        assert_eq!(assets.get(&handle).unwrap().0, expected);
        assert_eq!(part_lods(&mut app, stone), [0, 1, 2]);
        assert_eq!(app.world().resource::<Finished>().0, vec![Ok(())]);
    }

    #[test]
    fn rejected_options_finish_with_their_error() {
        let mut app = spawn_app();
        let options = StoneOptions {
            scale: 0.0,
            ..quick()
        };
        let stone = app.world_mut().spawn(StoneParams::new(options)).id();

        update_until_finished(&mut app, stone);
        assert!(part_lods(&mut app, stone).is_empty());
        let finished = &app.world().resource::<Finished>().0;
        assert!(
            matches!(
                finished[..],
                [Err(OptionsError::FloatOutOfRange { field: "scale", .. })]
            ),
            "{finished:?}"
        );
    }

    #[test]
    fn detail_levels_hand_over_at_scaled_distances() {
        let levels = StoneDetail::MEDIUM.levels().to_vec();
        let ranges: Vec<VisibilityRange> = visibility_ranges(levels.iter().copied(), 2.5)
            .into_iter()
            .map(Option::unwrap)
            .collect();
        assert_eq!(ranges[0].start_margin, 0.0..0.0);
        assert_eq!(ranges[0].end_margin, 14.25..15.75);
        assert_eq!(ranges[1].start_margin, 14.25..15.75);
        assert_eq!(ranges[1].end_margin, 38.0..42.0);
        assert_eq!(ranges[2].start_margin, 38.0..42.0);
        assert_eq!(ranges[2].end_margin.start, f32::INFINITY);

        // A rock twice the default size switches twice as far away.
        let large = visibility_ranges(levels.iter().copied(), 5.0);
        assert_eq!(large[0].as_ref().unwrap().end_margin, 28.5..31.5);

        // A lone detail level must remain visible at every camera distance.
        let single = visibility_ranges([levels[0]].into_iter(), 2.5);
        assert!(matches!(single[..], [None]));
    }
}
