//! Generate rocks in background tasks and display their detail levels as child entities.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use procgena_core::{LodRange, PoolId, Quality, SpecimenId, content_hash};
use procgena_core_bevy::{LodQuality, visibility_range};
use procgena_impostor::ImpostorLevel;
use procgena_specimen::{GeneratedSpecimen, Specimen};
use procgena_stone::{
    DetailLevel, OptionsError, Stone, StoneDetail, StoneOptions, StonePreset, TextureCompression,
    TextureSize,
};

use crate::impostor::{StoneImpostorAssets, StoneImpostorShown, impostor_ranges};
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
/// It also receives a [`GeneratedSpecimen`] describing the rock's meshes, which
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
    /// Load a preset from a `.stone.yml` file and build its options.
    /// The rock waits for the asset to load and rebuilds when Bevy reports it modified.
    /// Live file edits require Bevy asset watching to be enabled.
    Asset(Handle<StonePresetAsset>),
}

impl StoneParams {
    /// Creates a rock component from options. Generation starts after it is spawned.
    pub fn new(options: StoneOptions) -> Self {
        Self::Options(Arc::new(options))
    }
}

/// A rock preset loaded from a `.stone.yml` file for [`StoneParams::Asset`].
///
/// The file names a built-in preset as its `base` and gives only what
/// differs; see [`StonePreset::from_yaml`](procgena_stone::StonePreset::from_yaml).
/// Its key is the file name without `.stone.yml`. The loader checks the
/// settings before making the asset available.
#[derive(Asset, TypePath, Debug, Clone, PartialEq)]
pub struct StonePresetAsset(pub StonePreset);

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
    /// levels, starting at 0 for the nearest. A rock's impostor, once
    /// shown (see [`StonePlugin::impostor`](crate::StonePlugin::impostor)),
    /// is one more part, numbered one past the last level.
    pub lod: usize,
}

/// Reads and checks rock preset files with the `.stone.yml` extension.
#[derive(Debug, Default, TypePath)]
pub(crate) struct StonePresetLoader;

impl AssetLoader for StonePresetLoader {
    type Asset = StonePresetAsset;
    type Settings = ();
    type Error = BevyError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        context: &mut LoadContext<'_>,
    ) -> Result<StonePresetAsset, BevyError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let file_name = context
            .path()
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let key = file_name.strip_suffix(".stone.yml").unwrap_or(file_name);
        let preset = StonePreset::from_yaml(key, std::str::from_utf8(&bytes)?)?;
        Ok(StonePresetAsset(preset))
    }

    fn extensions(&self) -> &[&str] {
        &["stone.yml"]
    }
}

/// Texture location and format, and the impostor, copied from the plugin
/// configuration.
#[derive(Resource, Debug, Clone)]
pub(crate) struct SpawnSettings {
    pub texture_root: Option<String>,
    pub texture_compression: TextureCompression,
    pub impostor: Option<ImpostorLevel>,
}

/// Names a rock's generated meshes: its pool and seed (which fix its shape
/// and look) plus the hash of its detail levels (which fix its meshes).
/// Entities with the same key reuse one [`Generated`] entry.
#[derive(Component, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct StoneKey {
    pool: PoolId,
    seed: u32,
    detail: u64,
}

impl StoneKey {
    /// An interim specimen id for the rock, used until the caller passes
    /// its real [`SpecimenId`]: slot 0 of its pool, with its build seed (its
    /// options' `default_seed`) in the World seed's place.
    ///
    /// For a rock built on its own this is the id the architecture gives it.
    /// A pool variant's build seed is already its real specimen's seed, so
    /// each variant still gets its own id, but not its real World slot.
    pub(crate) fn interim_specimen_id(&self) -> SpecimenId {
        SpecimenId::new(self.seed, self.pool, 0)
    }

    /// The hash of the rock's detail levels.
    pub(crate) fn detail(&self) -> u64 {
        self.detail
    }

    /// The key of a rock built from `options`.
    fn of(options: &StoneOptions) -> Self {
        Self {
            pool: options.pool_id(),
            seed: options.default_seed,
            detail: content_hash(&options.mesh.detail),
        }
    }
}

/// Present on a [`StoneParams`] entity while its rock is being prepared.
///
/// The plugin adds this whenever a rock is spawned or its [`StoneParams`] changes,
/// and removes it once the rock's parts have been spawned or generation has
/// failed. A [`StoneFinished`] event is triggered at the same moment. While a
/// `.stone.yml` asset has not loaded yet, the marker stays on.
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
    /// The rock as a tagged specimen, for generators that grow on it.
    specimen: Arc<Specimen>,
}

/// Grounded meshes and burial depth, or the settings error that stopped generation.
type TaskOutput = Result<GeneratedMeshes, OptionsError>;

/// Mesh, material and camera-distance range for one detail level.
pub(crate) struct LevelAssets {
    /// Camera-distance limits; `None` means the sole detail level has no distance limit.
    range: Option<VisibilityRange>,
    /// Absent when this level has no triangles.
    pub(crate) mesh: Option<Handle<Mesh>>,
    /// Material for this level's texture size, shared by levels with the same size.
    pub(crate) material: Handle<StoneMaterial>,
}

/// Meshes and materials reused by rocks with identical options, nearest level first.
pub(crate) struct Generated {
    pub(crate) levels: Vec<LevelAssets>,
    /// Each level's fade bands, then the impostor's, as drawn once the
    /// rock's impostor is baked; `None` when the rock has no impostor.
    pub(crate) impostor_ranges: Option<Vec<LodRange>>,
    /// Suggested burial depth including option scale, before entity transform scaling.
    embed_depth: f32,
    /// The rock as a tagged specimen, shared by every rock with these options.
    specimen: Arc<Specimen>,
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
    /// Each generated rock's impostor, once baked, from the shared
    /// [`Impostors`](procgena_impostor::Impostors) cache.
    impostors: HashMap<StoneKey, StoneImpostorAssets>,
}

impl StoneCache {
    /// Generated rocks that draw an impostor that isn't ready yet.
    pub(crate) fn ready_without_impostor(&self) -> Vec<(StoneKey, Arc<Generated>)> {
        self.stones
            .iter()
            .filter(|(key, _)| !self.impostors.contains_key(key))
            .filter_map(|(key, entry)| match entry {
                Entry::Ready(generated) if generated.impostor_ranges.is_some() => {
                    Some((key.clone(), generated.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// Keeps the baked impostor of `key`.
    pub(crate) fn impostor_ready(&mut self, key: StoneKey, assets: StoneImpostorAssets) {
        self.impostors.insert(key, assets);
    }

    /// The rock generated for `key` and its impostor, once both are ready.
    pub(crate) fn with_impostor(
        &self,
        key: &StoneKey,
    ) -> Option<(&Generated, &StoneImpostorAssets)> {
        match (self.stones.get(key), self.impostors.get(key)) {
            (Some(Entry::Ready(generated)), Some(impostor)) => Some((generated, impostor)),
            _ => None,
        }
    }
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
            .remove::<(
                StoneKey,
                StoneEmbedDepth,
                GeneratedSpecimen,
                StoneImpostorShown,
            )>()
            .insert(StoneGenerating);
    }
    for entity in removed.read() {
        despawn_parts(&mut commands, children.get(entity).ok());
        if let Ok(mut entity) = commands.get_entity(entity) {
            entity.remove::<(
                StoneKey,
                StoneGenerating,
                StoneEmbedDepth,
                GeneratedSpecimen,
                StoneImpostorShown,
            )>();
        }
    }
}

/// Marks rocks for rebuilding when Bevy reports a change to their options asset.
pub(crate) fn reload_modified_options(
    mut events: MessageReader<AssetEvent<StonePresetAsset>>,
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
    options_assets: Res<Assets<StonePresetAsset>>,
    mut cache: ResMut<StoneCache>,
) {
    for (entity, stone) in &waiting {
        let options = match stone {
            StoneParams::Options(options) => options.clone(),
            StoneParams::Asset(handle) => match options_assets.get(handle) {
                Some(asset) => Arc::new(asset.0.stone.clone()),
                None => continue,
            },
        };
        let key = StoneKey::of(&options);
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
        specimen: Arc::new(stone.specimen()),
    })
}

/// Collects completed tasks and registers shared meshes and materials.
/// Invalid options are logged and cached as a failed result.
pub(crate) fn finish_generation(
    mut cache: ResMut<StoneCache>,
    settings: Res<SpawnSettings>,
    quality: Res<LodQuality>,
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
            specimen,
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
        let ranges = level_ranges(&options.mesh.detail, options.scale, quality.0);
        let mut level_assets = Vec::with_capacity(levels.len());
        for (l, range) in levels.into_iter().zip(ranges) {
            let range = bevy_range(&range);
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
        let impostor_ranges = settings.impostor.and_then(|impostor| {
            impostor_ranges(&options.mesh.detail, options.scale, quality.0, impostor)
        });
        *entry = Entry::Ready(Arc::new(Generated {
            levels: level_assets,
            impostor_ranges,
            embed_depth,
            specimen,
        }));
    }
}

/// Each of `detail`'s levels' fade bands, nearest first, for a rock `scale`
/// metres across drawn at `quality`.
pub(crate) fn level_ranges(detail: &StoneDetail, scale: f32, quality: Quality) -> Vec<LodRange> {
    let lod = detail.lod();
    lod.ranges(lod.factor(scale, quality))
}

/// Bevy's visibility range for a level drawn over `range`, or `None` for a
/// level drawn at every distance.
pub(crate) fn bevy_range(range: &LodRange) -> Option<VisibilityRange> {
    (!range.is_always()).then(|| visibility_range(range))
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
                    GeneratedSpecimen(generated.specimen.clone()),
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
    cache.impostors.retain(|key, _| used.contains(key));

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
            .init_asset::<StonePresetAsset>()
            .register_asset_loader(StonePresetLoader)
            .init_resource::<StoneCache>()
            .init_resource::<LodQuality>()
            .init_resource::<Finished>()
            .insert_resource(SpawnSettings {
                texture_root: None,
                texture_compression: TextureCompression::Bc7,
                impostor: None,
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
        let options = StoneOptions {
            default_seed: 7,
            ..quick()
        };
        *app.world_mut().get_mut::<StoneParams>(stone).unwrap() = StoneParams::new(options);
        app.update();
        assert!(app.world().entity(stone).contains::<StoneGenerating>());
        update_until_finished(&mut app, stone);
        assert_eq!(app.world().resource::<Finished>().0.len(), 3);

        // A different detail set is a different rock: it generates its own levels.
        let mut options = StoneOptions {
            default_seed: 7,
            ..quick()
        };
        options.mesh.detail = StoneDetail::single(16, TextureSize::Size512);
        *app.world_mut().get_mut::<StoneParams>(stone).unwrap() = StoneParams::new(options);
        app.update();
        assert!(app.world().entity(stone).contains::<StoneGenerating>());
        update_until_finished(&mut app, stone);
        assert_eq!(part_lods(&mut app, stone), [0]);
        assert_eq!(app.world().resource::<StoneCache>().stones.len(), 3);
    }

    #[test]
    fn finished_rocks_carry_their_specimen_until_they_change() {
        let mut app = spawn_app();
        let stone = app.world_mut().spawn(StoneParams::new(quick())).id();
        app.update();
        assert!(!app.world().entity(stone).contains::<GeneratedSpecimen>());

        // The specimen arrives with the parts and matches the rock's levels.
        update_until_finished(&mut app, stone);
        let specimen = app.world().get::<GeneratedSpecimen>(stone).unwrap();
        assert_eq!(*specimen.0, Stone::new(quick()).unwrap().specimen());

        // Changing the options removes the stale specimen until the rebuild ends.
        let options = StoneOptions {
            default_seed: 7,
            ..quick()
        };
        *app.world_mut().get_mut::<StoneParams>(stone).unwrap() = StoneParams::new(options);
        app.update();
        assert!(!app.world().entity(stone).contains::<GeneratedSpecimen>());
        update_until_finished(&mut app, stone);
        assert!(app.world().entity(stone).contains::<GeneratedSpecimen>());

        // Removing the request removes the specimen.
        app.world_mut().entity_mut(stone).remove::<StoneParams>();
        app.update();
        assert!(!app.world().entity(stone).contains::<GeneratedSpecimen>());
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
        let second = spawn(
            &mut app,
            StoneOptions {
                default_seed: 7,
                ..quick()
            },
        );
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
    fn preset_files_load_partial_options() {
        // Write a partial settings file into a fresh asset folder.
        let dir =
            std::env::temp_dir().join(format!("procgena_stone_bevy_assets_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("rock.stone.yml"),
            "stone:\n  default_seed: 9\n  material:\n    texture: lichen_rock_02\n",
        )
        .unwrap();

        let mut app = spawn_app_with(AssetPlugin {
            file_path: dir.to_string_lossy().into_owned(),
            ..default()
        });
        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<StonePresetAsset>("rock.stone.yml");
        let stone = app
            .world_mut()
            .spawn(StoneParams::Asset(handle.clone()))
            .id();
        update_until_finished(&mut app, stone);
        std::fs::remove_dir_all(&dir).unwrap();

        // Omitted settings keep their defaults.
        let mut expected = StoneOptions {
            default_seed: 9,
            ..StoneOptions::default()
        };
        expected.material.texture = "lichen_rock_02".to_owned();
        let assets = app.world().resource::<Assets<StonePresetAsset>>();
        let preset = &assets.get(&handle).unwrap().0;
        assert_eq!(preset.stone, expected);
        assert_eq!(preset.key, "rock");
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
        let high = Quality::High;
        let ranges: Vec<VisibilityRange> = level_ranges(&StoneDetail::MEDIUM, 2.5, high)
            .iter()
            .map(|r| bevy_range(r).unwrap())
            .collect();
        assert_eq!(ranges[0].start_margin, 0.0..0.0);
        assert_eq!(ranges[0].end_margin, 14.25..15.75);
        assert_eq!(ranges[1].start_margin, 14.25..15.75);
        assert_eq!(ranges[1].end_margin, 38.0..42.0);
        assert_eq!(ranges[2].start_margin, 38.0..42.0);
        assert_eq!(ranges[2].end_margin.start, f32::INFINITY);

        // A rock twice the default size switches twice as far away.
        let large = level_ranges(&StoneDetail::MEDIUM, 5.0, high);
        assert_eq!(large[0].fade_out, 28.5..31.5);

        // A lower quality switches nearer.
        let low = level_ranges(&StoneDetail::MEDIUM, 2.5, Quality::Low);
        let out = &low[0].fade_out;
        assert!(
            (out.start - 5.7).abs() < 1e-5 && (out.end - 6.3).abs() < 1e-5,
            "{out:?}"
        );

        // A lone detail level must remain visible at every camera distance.
        let single = StoneDetail::new(vec![StoneDetail::MEDIUM.levels()[0]]);
        let single = level_ranges(&single, 2.5, high);
        assert!(matches!(single[..], [ref r] if bevy_range(r).is_none()));
    }
}
