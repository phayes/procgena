//! Generate trees in background tasks and display their game-level mesh,
//! lower mesh and impostor as child entities.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::camera::primitives::{Aabb, MeshAabb};
use bevy::camera::visibility::VisibilityRange;
use bevy::mesh::MeshTag;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use procgena_core::chain::{DistanceChain, Lod, Step, ViewScale};
use procgena_core::{LodRange, Quality, SpecimenId};
use procgena_core_bevy::{LodQuality, visibility_range};
use procgena_impostor::BakeSettings;
use procgena_specimen::{GeneratedSpecimen, Specimen};
use procgena_tree::{
    ClusterPart, LodDetail, MeshBuffers, OptionsError, TextureCompression, TextureSize, Tree,
    TreeCacheKey, TreeMesh, TreeOptions, TreePreset, TrellisWire,
};

use crate::clusters::{
    BuiltClusters, ClusterImpostors, TreeClusters, add_meshes, build_clusters, single_tree_clusters,
};
use crate::impostor::{ImpostorShown, TreeImpostorAssets};
use crate::material::{
    BarkTextures, LeafTextures, TreeMaterial, bark_material, flower_material, fruit_material,
    leaf_material, trellis_material,
};
use crate::material_key::{BarkKey, LeafKey, TrellisKey};
use crate::mesh::{bark_mesh, cluster_mesh, leaf_mesh};
use crate::scatter::PreparedTrees;
use crate::wind::Wind;

/// A generated tree positioned by this entity's transform.
///
/// Add this component to an entity after installing [`TreePlugin`](crate::TreePlugin).
/// The tree appears when background generation finishes, as bark and leaf
/// children for each drawing step: its game-level mesh, its lower mesh and,
/// once baked, its impostor, each shown at the sizes on screen the shared
/// [`DistanceChain`] gives it. Visible trellis wires get their own children.
/// [`TreeOptions::game_level`] sets the game-level mesh's detail; the lower
/// mesh is derived from it.
/// Trees with identical options share meshes and materials. Once the parts
/// spawn, the entity also gets a [`GeneratedSpecimen`] describing the tree's
/// meshes, which other generators, such as clumps of snow or moss, grow on.
/// Changing this component rebuilds its parts; removing it despawns the
/// generated children and removes the [`GeneratedSpecimen`].
#[derive(Component, Debug, Clone, PartialEq)]
#[require(Transform, Visibility)]
pub enum TreeParams {
    /// Generate from options supplied in memory. Use [`TreeParams::new`] to wrap owned options.
    Options(Arc<TreeOptions>),
    /// Load a preset from a `.tree.yml` file and build its options.
    /// The tree waits for the asset to load and rebuilds when Bevy reports it modified.
    /// Live file edits require Bevy asset watching to be enabled.
    Asset(Handle<TreePresetAsset>),
}

impl TreeParams {
    /// Creates a tree component from options. Generation starts after it is spawned.
    pub fn new(options: TreeOptions) -> Self {
        Self::Options(Arc::new(options))
    }

    /// Creates a tree component using the settings of a preset.
    pub fn preset(preset: &TreePreset) -> Self {
        Self::new(preset.tree.clone())
    }
}

/// A tree preset loaded from a `.tree.yml` file for [`TreeParams::Asset`].
///
/// The file names a built-in preset as its `base` and gives only what
/// differs; see [`TreePreset::from_yaml`]. Its key is the file name without
/// `.tree.yml`. The loader checks the settings before making the asset
/// available.
#[derive(Asset, TypePath, Debug, Clone, PartialEq)]
pub struct TreePresetAsset(pub TreePreset);

/// Identifies a generated child entity that draws one part of a [`TreeParams`].
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreePart {
    /// Which drawing step this part belongs to. Trellis wires and clusters
    /// use [`Step::Game`].
    pub step: Step,
    /// The geometry drawn by this child entity.
    pub kind: TreePartKind,
}

/// Parts that can be drawn as children of a tree entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TreePartKind {
    /// The trunk and branches, drawn together.
    Bark,
    /// The tree's leaves, drawn together.
    Leaves,
    /// The tree's fruit and flower clusters. One child draws them all
    /// through the shared instanced renderer of `procgena_core_bevy` (an
    /// `InstancedScatter` with its `InstancedAttached`): each cluster is
    /// drawn at the step its own size on screen gives, until the tree's
    /// impostor takes over. Its step is [`Step::Game`].
    Clusters,
    /// A single support wire for a trellis.
    Trellis,
    /// The tree's impostor: one quad showing a baked view of the tree,
    /// drawn when the tree is small on screen; see
    /// [`TreePlugin::impostor`](crate::TreePlugin::impostor).
    Impostor,
}

/// Reads and checks tree preset files with the `.tree.yml` extension.
#[derive(Debug, Default, TypePath)]
pub(crate) struct TreePresetLoader;

impl AssetLoader for TreePresetLoader {
    type Asset = TreePresetAsset;
    type Settings = ();
    type Error = BevyError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        context: &mut LoadContext<'_>,
    ) -> Result<TreePresetAsset, BevyError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let file_name = context
            .path()
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let key = file_name.strip_suffix(".tree.yml").unwrap_or(file_name);
        let preset = TreePreset::from_yaml(key, std::str::from_utf8(&bytes)?)?;
        Ok(TreePresetAsset(preset))
    }

    fn extensions(&self) -> &[&str] {
        &["tree.yml"]
    }
}

/// The bark image size each mesh step is drawn with when the plugin has a
/// texture folder: 1k at the game level, 512 for the lower mesh.
pub(crate) fn bark_texture_size(step: Step) -> TextureSize {
    match step {
        Step::Game => TextureSize::Size1k,
        Step::Lower | Step::Impostor => TextureSize::Size512,
    }
}

/// Texture location and format, and the impostors, copied from the plugin
/// configuration.
#[derive(Resource, Debug, Clone)]
pub(crate) struct SpawnSettings {
    pub texture_root: Option<String>,
    pub texture_compression: TextureCompression,
    pub impostor: Option<BakeSettings>,
    /// How fruit and flower clusters' impostors are baked. They are drawn
    /// once the tree's own impostor is baked; unused without it.
    pub fruit_impostor: Option<BakeSettings>,
}

/// The fade bands of each step of the shared [`DistanceChain`], by
/// [`Step::index`], for a copy `size` metres across (its bounding-sphere
/// diameter) seen through a [`ViewScale::DEFAULT`] camera at `quality`.
pub(crate) fn chain_ranges(size: f32, quality: Quality) -> [LodRange; 3] {
    let factor = DistanceChain::factor(size, ViewScale::DEFAULT);
    DistanceChain::DEFAULT.ranges(factor, quality)
}

/// The fade bands of the game-level and lower meshes of a tree with no
/// impostor: like [`chain_ranges`], but the lower mesh is drawn on until the
/// cull, where the impostor would have ended.
pub(crate) fn mesh_ranges(size: f32, quality: Quality) -> [LodRange; 2] {
    let [game, lower, impostor] = chain_ranges(size, quality);
    let lower = LodRange {
        fade_in: lower.fade_in,
        fade_out: impostor.fade_out,
    };
    [game, lower]
}

/// The size a tree copy's steps switch by: `diameter`, the tree's
/// bounding-sphere diameter as built, times the largest scale of
/// `transform`.
pub(crate) fn copy_size(diameter: f32, transform: &Transform) -> f32 {
    diameter * transform.scale.abs().max_element()
}

/// The wind phase, from 0 to 1, of a tree drawn on its own, so that it sways
/// as its copy in a grove would. Trees without it sway with phase 0.
#[derive(Component, Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct TreeWindPhase(pub(crate) f32);

/// The tree's [`TreeOptions::cache_key`], used to find shared generation
/// results. Entities with the same key reuse one [`Generated`] entry.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TreeKey(TreeCacheKey);

impl TreeKey {
    /// An interim specimen id for the tree, used until the caller passes its
    /// real [`SpecimenId`]: slot 0 of its pool, with its build seed (its
    /// options' `default_seed`) in the World seed's place.
    ///
    /// For a tree built on its own this is the id the architecture gives it.
    /// A grove variant's build seed is already its real specimen's seed, so
    /// each variant still gets its own id, but not its real World slot.
    pub(crate) fn interim_specimen_id(self) -> SpecimenId {
        SpecimenId::new(self.0.seed as u32, self.0.pool, 0)
    }

    /// The hash of the game levels of the tree and its attached fruit,
    /// which fix its meshes but not its specimen id.
    pub(crate) fn detail(self) -> u64 {
        self.0.detail
    }
}

/// Keeps the generated trees with these keys in the [`TreeCache`] while the
/// entity has it, for entities that draw trees without a [`TreeKey`] of their
/// own, such as groves.
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub(crate) struct HeldTreeKeys(pub(crate) Vec<TreeKey>);

/// Present on a [`TreeParams`] entity while its tree is being prepared.
///
/// The plugin adds this whenever a tree is spawned or its [`TreeParams`] changes,
/// and removes it once the tree's parts have been spawned or generation has
/// failed. A [`TreeFinished`] event is triggered at the same moment. While a
/// `.tree.yml` asset has not loaded yet, the marker stays on.
#[derive(Component, Debug, Default)]
pub struct TreeGenerating;

/// Triggered on a [`TreeParams`] entity when preparing its tree ends.
///
/// Observe it on the entity, or globally, to learn when a new or changed tree
/// has appeared. On success the tree's [`TreePart`] children are queued with
/// the same commands, so they exist once this frame's commands have run.
#[derive(EntityEvent, Debug, Clone, PartialEq)]
pub struct TreeFinished {
    /// The tree entity.
    pub entity: Entity,
    /// `Ok` when the parts were spawned, or why the options were rejected.
    pub result: Result<(), OptionsError>,
}

/// A part’s Bevy mesh and its bounds.
pub(crate) struct PartMesh<M> {
    pub(crate) mesh: M,
    /// Bounds of the part, in tree coordinates. Wind sway is not included.
    pub(crate) extent: Aabb,
}

/// Meshes produced by a background task for one mesh step; each is absent
/// when the step has no triangles of that kind.
struct LevelMeshes {
    bark: Option<PartMesh<Mesh>>,
    leaves: Option<PartMesh<Mesh>>,
}

/// What a background task generates for one tree.
struct TaskMeshes {
    /// The game-level mesh, then the lower mesh.
    levels: [LevelMeshes; 2],
    /// The details the two meshes were built at.
    lod: Lod<LodDetail>,
    /// The game-level mesh's [`TreeMesh::bounding_diameter`].
    diameter: f32,
    /// The tree as a tagged specimen.
    specimen: Arc<Specimen>,
    /// The cluster specimens and tables, drawn instanced.
    clusters: Vec<BuiltClusters>,
    /// The game-level mesh's fruit and flowers, each merged into one mesh,
    /// for the impostor bake only; empty without an impostor.
    baked_clusters: Vec<(ClusterPart, Mesh)>,
}

/// A generated tree, or the validation error that prevented generation.
type TaskOutput = Result<TaskMeshes, OptionsError>;

/// Mesh handles and materials for one mesh step.
struct LevelAssets {
    /// Bark mesh, absent when this step contains no bark triangles.
    bark: Option<PartMesh<Handle<Mesh>>>,
    /// Leaf mesh, absent when this step contains no leaf triangles.
    leaves: Option<PartMesh<Handle<Mesh>>>,
    /// Bark material for this step's image size, shared by trees with
    /// matching bark settings.
    bark_material: Handle<TreeMaterial>,
    /// Leaf material, shared by both steps and by trees with matching leaf
    /// settings.
    leaf_material: Handle<TreeMaterial>,
}

/// Meshes and materials reused by trees with identical options.
pub(crate) struct Generated {
    /// The game-level mesh, then the lower mesh.
    levels: [LevelAssets; 2],
    /// The details the two meshes were built at, which also choose the
    /// clusters each one keeps.
    pub(crate) lod: Lod<LodDetail>,
    /// The game-level mesh's bounding-sphere diameter, in metres.
    diameter: f32,
    /// The tree's fruit and flower cluster specimens and tables.
    pub(crate) clusters: Vec<TreeClusters>,
    /// The game-level mesh's fruit and flowers, merged, with their
    /// materials, for the impostor bake only.
    pub(crate) baked_clusters: Vec<(Handle<Mesh>, Handle<TreeMaterial>)>,
    /// One cylinder mesh and material shared by all wires, plus each wire's placement.
    pub(crate) trellis: Option<(Handle<Mesh>, Handle<StandardMaterial>, Vec<Transform>)>,
    /// The tree as a tagged specimen, shared by every tree with these options.
    pub(crate) specimen: Arc<Specimen>,
}

impl Generated {
    /// A sphere around every mesh part at both steps, in the tree's own
    /// coordinates, enlarged by `allowance` for wind sway; a point at the
    /// origin for a tree with no parts.
    pub(crate) fn bounds(&self, allowance: f32) -> (Vec3, f32) {
        let (min, max) = self.parts().map(|(_, _, part, _)| part.extent).fold(
            (Vec3::INFINITY, Vec3::NEG_INFINITY),
            |(min, max), aabb: Aabb| (min.min(aabb.min().into()), max.max(aabb.max().into())),
        );
        if min.x > max.x {
            return (Vec3::ZERO, 0.0);
        }
        let center = (min + max) / 2.0;
        (center, (max - center).length() * allowance)
    }

    /// Every mesh part of both mesh steps, game level first, with its
    /// material. Trellis wires and clusters are not included.
    pub(crate) fn parts(
        &self,
    ) -> impl Iterator<
        Item = (
            Step,
            TreePartKind,
            &PartMesh<Handle<Mesh>>,
            &Handle<TreeMaterial>,
        ),
    > {
        [Step::Game, Step::Lower]
            .into_iter()
            .zip(&self.levels)
            .flat_map(move |(step, level)| {
                [
                    (TreePartKind::Bark, &level.bark, &level.bark_material),
                    (TreePartKind::Leaves, &level.leaves, &level.leaf_material),
                ]
                .into_iter()
                .filter_map(move |(kind, mesh, material)| {
                    Some((step, kind, mesh.as_ref()?, material))
                })
            })
    }

    /// The tree's size before it is scaled, in metres: the bounding-sphere
    /// diameter of its game-level mesh, which its steps switch by.
    pub(crate) fn diameter(&self) -> f32 {
        self.diameter
    }
}

enum Entry {
    Pending(Task<TaskOutput>, Arc<TreeOptions>),
    Ready(Arc<Generated>),
    Failed(OptionsError),
}

/// Where a tree's generation stands in the [`TreeCache`].
pub(crate) enum CacheState<'a> {
    /// Still generating, or never requested.
    Pending,
    Ready(&'a Arc<Generated>),
    /// Generation failed; the error has been logged.
    Failed,
}

/// Reuses generated trees when their options match. Trees with different shapes
/// can still share texture images and materials (surface colour, texture and shine)
/// when those settings match, avoiding duplicate assets.
#[derive(Resource, Default)]
pub(crate) struct TreeCache {
    trees: HashMap<TreeKey, Entry>,
    bark_textures: HashMap<(String, TextureSize), BarkTextures>,
    /// Leaf images by leaf kind.
    leaf_textures: HashMap<String, LeafTextures>,
    bark_materials: HashMap<BarkKey, Handle<TreeMaterial>>,
    leaf_materials: HashMap<LeafKey, Handle<TreeMaterial>>,
    trellis_materials: HashMap<TrellisKey, Handle<StandardMaterial>>,
    /// Shared fruit appearance, retained even when no trees use it.
    fruit_material: Option<Handle<TreeMaterial>>,
    /// Shared flower appearance, retained even when no trees use it.
    flower_material: Option<Handle<TreeMaterial>>,
    /// Each generated tree's impostor, once baked, from the shared
    /// [`Impostors`](procgena_impostor::Impostors) cache.
    impostors: HashMap<TreeKey, TreeImpostorAssets>,
    /// The real specimen id of each tree a Scatter asked for through
    /// [`TreeCache::request_specimen`].
    specimens: HashMap<TreeKey, SpecimenId>,
}

impl TreeCache {
    /// Starts generating a tree with `options` in the background unless one
    /// is cached or generating already, and returns its key.
    pub(crate) fn request(
        &mut self,
        options: &Arc<TreeOptions>,
        settings: &SpawnSettings,
    ) -> TreeKey {
        let key = TreeKey(options.cache_key());
        self.trees.entry(key).or_insert_with(|| {
            let bake = settings.impostor.is_some();
            let task_options = options.clone();
            let task =
                AsyncComputeTaskPool::get().spawn(async move { generate(&task_options, bake) });
            Entry::Pending(task, options.clone())
        });
        key
    }

    /// [`request`](Self::request) for a Scatter's tree whose real specimen
    /// id is `specimen`, which its impostor is then keyed by.
    pub(crate) fn request_specimen(
        &mut self,
        options: &Arc<TreeOptions>,
        specimen: SpecimenId,
        settings: &SpawnSettings,
    ) -> TreeKey {
        let key = self.request(options, settings);
        self.specimens.insert(key, specimen);
        key
    }

    /// The specimen id of the tree under `key`: the real one a Scatter gave
    /// through [`request_specimen`](Self::request_specimen), or for a tree
    /// built on its own, [`TreeKey::interim_specimen_id`].
    pub(crate) fn specimen_id(&self, key: TreeKey) -> SpecimenId {
        self.specimens
            .get(&key)
            .copied()
            .unwrap_or_else(|| key.interim_specimen_id())
    }

    /// Generated trees whose impostor isn't ready yet.
    pub(crate) fn ready_without_impostor(&self) -> Vec<(TreeKey, Arc<Generated>)> {
        self.trees
            .iter()
            .filter(|(key, _)| !self.impostors.contains_key(key))
            .filter_map(|(key, entry)| match entry {
                Entry::Ready(generated) => Some((*key, generated.clone())),
                _ => None,
            })
            .collect()
    }

    /// Keeps the baked impostor of `key`, if the tree is still cached.
    pub(crate) fn impostor_ready(&mut self, key: TreeKey, assets: TreeImpostorAssets) {
        if self.trees.contains_key(&key) {
            self.impostors.insert(key, assets);
        }
    }

    /// The impostor of `key`, once baked.
    pub(crate) fn impostor(&self, key: TreeKey) -> Option<&TreeImpostorAssets> {
        self.impostors.get(&key)
    }

    /// How the tree with `key` stands.
    pub(crate) fn state(&self, key: TreeKey) -> CacheState<'_> {
        match self.trees.get(&key) {
            Some(Entry::Ready(generated)) => CacheState::Ready(generated),
            Some(Entry::Failed(_)) => CacheState::Failed,
            Some(Entry::Pending(..)) | None => CacheState::Pending,
        }
    }
}

/// Removes generated children when a tree changes or loses its [`TreeParams`] component.
/// Changed trees are marked to generate replacement parts; unrelated children are kept.
pub(crate) fn reset_changed_trees(
    mut commands: Commands,
    changed: Query<(Entity, Option<&Children>), Changed<TreeParams>>,
    mut removed: RemovedComponents<TreeParams>,
    children: Query<&Children>,
    parts: Query<(), With<TreePart>>,
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
            .remove::<(TreeKey, GeneratedSpecimen, ImpostorShown)>()
            .insert(TreeGenerating);
    }
    for entity in removed.read() {
        despawn_parts(&mut commands, children.get(entity).ok());
        if let Ok(mut entity) = commands.get_entity(entity) {
            entity.remove::<(TreeKey, TreeGenerating, GeneratedSpecimen, ImpostorShown)>();
        }
    }
}

/// Marks trees for rebuilding when Bevy reports a change to their preset asset.
pub(crate) fn reload_modified_options(
    mut events: MessageReader<AssetEvent<TreePresetAsset>>,
    mut trees: Query<&mut TreeParams>,
) {
    for event in events.read() {
        if let AssetEvent::Modified { id } = event {
            for mut tree in &mut trees {
                if matches!(&*tree, TreeParams::Asset(handle) if handle.id() == *id) {
                    tree.set_changed();
                }
            }
        }
    }
}

/// Selects trees still waiting for an options key, including assets that have not loaded.
type AwaitingKey = (With<TreeGenerating>, Without<TreeKey>);

/// Assigns an options key to each waiting tree and starts a background task only
/// when those options are not already cached.
pub(crate) fn start_generation(
    mut commands: Commands,
    waiting: Query<(Entity, &TreeParams), AwaitingKey>,
    options_assets: Res<Assets<TreePresetAsset>>,
    settings: Res<SpawnSettings>,
    mut cache: ResMut<TreeCache>,
) {
    for (entity, tree) in &waiting {
        let options = match tree {
            TreeParams::Options(options) => options.clone(),
            TreeParams::Asset(handle) => match options_assets.get(handle) {
                Some(asset) => Arc::new(asset.0.tree.clone()),
                None => continue,
            },
        };
        let key = cache.request(&options, &settings);
        commands.entity(entity).insert(key);
    }
}

/// Generates the tree's game-level and lower meshes from one skeleton, and
/// its fruit and flower clusters. With `bake`, the game-level mesh's
/// clusters are also merged for the impostor bake.
fn generate(options: &TreeOptions, bake: bool) -> TaskOutput {
    let tree = Tree::new(options.clone())?;
    let lod = tree.lod();
    let (options, skeleton, game) = tree.into_parts();
    let lower = skeleton.mesh(&options, &lod.lower);
    let meshes = Lod { game, lower };
    let specimen = Arc::new(skeleton.specimen(&lod, &meshes));

    let part = |buffers: &MeshBuffers, convert: &dyn Fn(&MeshBuffers) -> Mesh| {
        (!buffers.indices.is_empty()).then(|| {
            let mesh = convert(buffers);
            let extent = mesh.get_aabb().unwrap_or_default();
            PartMesh { mesh, extent }
        })
    };
    let levels = [&meshes.game, &meshes.lower].map(|mesh: &TreeMesh| LevelMeshes {
        bark: part(&mesh.branches, &|b| bark_mesh(b, &options.bark)),
        leaves: part(&mesh.leaves, &leaf_mesh),
    });

    // The game level's clusters, merged as that mesh composes them.
    let game = &meshes.game;
    let baked_clusters = if bake {
        [
            (ClusterPart::Fruit, &game.fruit),
            (ClusterPart::Flowers, &game.flowers),
        ]
        .into_iter()
        .filter(|(_, buffers)| !buffers.indices.is_empty())
        .map(|(part, buffers)| (part, cluster_mesh(buffers)))
        .collect()
    } else {
        Vec::new()
    };
    Ok(TaskMeshes {
        levels,
        lod,
        diameter: game.bounding_diameter(),
        specimen,
        clusters: build_clusters(&skeleton),
        baked_clusters,
    })
}

/// Collects completed tasks and registers shared meshes and materials.
/// Invalid options are logged and cached as a failed result.
pub(crate) fn finish_generation(
    mut cache: ResMut<TreeCache>,
    settings: Res<SpawnSettings>,
    wind: Res<Wind>,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut tree_materials: ResMut<Assets<TreeMaterial>>,
    mut standard_materials: ResMut<Assets<StandardMaterial>>,
) {
    let cache = &mut *cache;
    for entry in cache.trees.values_mut() {
        let Entry::Pending(task, options) = entry else {
            continue;
        };
        let Some(output) = block_on(poll_once(task)) else {
            continue;
        };
        let TaskMeshes {
            levels,
            lod,
            diameter,
            specimen,
            clusters,
            baked_clusters,
        } = match output {
            Ok(generated) => generated,
            Err(error) => {
                error!("procgena_tree_bevy: cannot generate tree: {error}");
                *entry = Entry::Failed(error);
                continue;
            }
        };

        // Share matching materials across trees and their two mesh steps:
        // bark at the step's image size, and one leaf material for both.
        // Reuse texture images; without a texture folder, use plain colours.
        let compression = settings.texture_compression;
        let root = settings.texture_root.as_deref();
        let key = LeafKey::new(&options.leaves, root.is_some());
        let leaves = cache
            .leaf_materials
            .entry(key)
            .or_insert_with(|| {
                let textures = root.map(|root| {
                    let kind = &options.leaves.kind;
                    cache
                        .leaf_textures
                        .entry(kind.clone())
                        .or_insert_with(|| {
                            LeafTextures::load(&asset_server, root, kind, compression)
                        })
                        .clone()
                });
                tree_materials.add(leaf_material(&options.leaves, textures.as_ref(), wind.0))
            })
            .clone();
        let [game, lower] = levels;
        let level_assets = [(Step::Game, game), (Step::Lower, lower)].map(|(step, l)| {
            let size = root.map(|_| bark_texture_size(step));
            let key = BarkKey::new(&options.bark, size);
            let bark = cache.bark_materials.entry(key).or_insert_with(|| {
                let textures = size.zip(root).map(|(size, root)| {
                    cache
                        .bark_textures
                        .entry((options.bark.kind.clone(), size))
                        .or_insert_with(|| {
                            BarkTextures::load(
                                &asset_server,
                                root,
                                &options.bark.kind,
                                size,
                                compression,
                            )
                        })
                        .clone()
                });
                tree_materials.add(bark_material(&options.bark, textures.as_ref(), wind.0))
            });
            let mut add = |part: Option<PartMesh<Mesh>>| {
                part.map(|p| PartMesh {
                    mesh: meshes.add(p.mesh),
                    extent: p.extent,
                })
            };
            LevelAssets {
                bark: add(l.bark),
                leaves: add(l.leaves),
                bark_material: bark.clone(),
                leaf_material: leaves.clone(),
            }
        });

        // All trees share one fruit material and one flower material because
        // these surface settings do not vary between trees.
        let fruit = cache
            .fruit_material
            .get_or_insert_with(|| tree_materials.add(fruit_material(wind.0)))
            .clone();
        let flowers = cache
            .flower_material
            .get_or_insert_with(|| tree_materials.add(flower_material(wind.0)))
            .clone();
        let trellis = (options.trellis.enabled && options.trellis.visible).then(|| {
            let wires = options.trellis.wires();
            let material = cache
                .trellis_materials
                .entry(TrellisKey::new(&options.trellis))
                .or_insert_with(|| standard_materials.add(trellis_material(&options.trellis)))
                .clone();
            (
                meshes.add(Cylinder::new(options.trellis.cylinder_radius, 1.0)),
                material,
                wires.iter().map(wire_transform).collect(),
            )
        });
        let material = |part| match part {
            ClusterPart::Fruit => fruit.clone(),
            ClusterPart::Flowers => flowers.clone(),
        };
        let baked_clusters = baked_clusters
            .into_iter()
            .map(|(part, mesh)| (meshes.add(mesh), material(part)))
            .collect();
        let clusters = add_meshes(clusters, &mut meshes, material);
        let generated = Generated {
            levels: level_assets,
            lod,
            diameter,
            clusters,
            baked_clusters,
            trellis,
            specimen,
        };
        *entry = Entry::Ready(Arc::new(generated));
    }
}

/// Bevy's visibility range for a part drawn over `range`, or `None` for a
/// part drawn at every distance.
pub(crate) fn part_range(range: &LodRange) -> Option<VisibilityRange> {
    (!range.is_always()).then(|| visibility_range(range))
}

/// Positions, rotates and stretches a unit-height cylinder to connect a wire's endpoints.
fn wire_transform(wire: &TrellisWire) -> Transform {
    let span = wire.end - wire.start;
    Transform {
        translation: (wire.start + wire.end) / 2.0,
        rotation: Quat::from_rotation_arc(Vec3::Y, span.normalize_or(Vec3::Y)),
        scale: Vec3::new(1.0, span.length(), 1.0),
    }
}

/// Adds child entities once a waiting tree's assets are ready, with fade
/// bands for the tree's size and the [`LodQuality`] in use then.
/// Stops waiting after a failed generation attempt. Either way, triggers [`TreeFinished`].
#[expect(clippy::type_complexity, reason = "Bevy system parameters")]
pub(crate) fn spawn_parts(
    mut commands: Commands,
    waiting: Query<
        (Entity, &TreeKey, Option<&Transform>, Option<&TreeWindPhase>),
        With<TreeGenerating>,
    >,
    cache: Res<TreeCache>,
    settings: Res<SpawnSettings>,
    quality: Res<LodQuality>,
) {
    for (entity, key, transform, phase) in &waiting {
        let result = match cache.trees.get(key) {
            Some(Entry::Ready(generated)) => {
                // Drawn without the impostor until `show_impostors` adds it.
                let specimen = cache.specimen_id(*key);
                let copy = SingleTree::new(generated, transform, phase);
                let ranges = mesh_ranges(copy.size, quality.0);
                spawn_tree_parts(&mut commands, entity, generated, &ranges, copy);
                let clusters = SingleTreeClusters {
                    specimen,
                    impostor: None,
                    copy,
                };
                spawn_clusters(&mut commands, entity, generated, &settings, clusters);
                commands
                    .entity(entity)
                    .insert(GeneratedSpecimen(generated.specimen.clone()));
                Ok(())
            }
            Some(Entry::Failed(error)) => Err(error.clone()),
            Some(Entry::Pending(..)) | None => continue,
        };
        commands.entity(entity).remove::<TreeGenerating>();
        commands.trigger(TreeFinished { entity, result });
    }
}

/// How much a single tree's bounding sphere is enlarged to cover wind sway.
const SWAY_ALLOWANCE: f32 = 1.15;

/// How a tree drawn on its own is drawn as a copy: the size its steps
/// switch by, and its wind phase.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SingleTree {
    /// The tree's bounding-sphere diameter as built times its `Transform`'s
    /// largest scale.
    pub(crate) size: f32,
    /// From 0 to 1; see [`TreeWindPhase`].
    pub(crate) wind_phase: f32,
}

impl SingleTree {
    /// The tree `generated`, placed with `transform` (unscaled without one)
    /// and swaying with `phase` (0 without one).
    pub(crate) fn new(
        generated: &Generated,
        transform: Option<&Transform>,
        phase: Option<&TreeWindPhase>,
    ) -> Self {
        Self {
            size: copy_size(
                generated.diameter(),
                transform.unwrap_or(&Transform::IDENTITY),
            ),
            wind_phase: phase.map_or(0.0, |p| p.0),
        }
    }
}

/// What a single tree's fruit and flower clusters are drawn with.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SingleTreeClusters<'a> {
    /// The tree's specimen id.
    pub(crate) specimen: SpecimenId,
    /// The tree's baked impostor, once it is shown: the tree's steps then
    /// end in it, so clusters stop where it fades in, and clusters with a
    /// baked impostor of their own draw it when they are small on screen.
    pub(crate) impostor: Option<&'a TreeImpostorAssets>,
    /// The tree as a copy.
    pub(crate) copy: SingleTree,
}

/// Spawns the child that draws `tree`'s fruit and flower clusters, if it
/// grows any.
pub(crate) fn spawn_clusters(
    commands: &mut Commands,
    tree: Entity,
    generated: &Generated,
    settings: &SpawnSettings,
    clusters: SingleTreeClusters<'_>,
) {
    let impostors = clusters.impostor.and_then(|assets| {
        settings.fruit_impostor?;
        Some(ClusterImpostors {
            quad: &assets.quad,
            baked: &assets.clusters,
        })
    });
    let Some((scatter, attached)) = single_tree_clusters(
        &generated.clusters,
        &generated.lod,
        clusters.specimen,
        clusters.impostor.is_some(),
        generated.bounds(SWAY_ALLOWANCE),
        clusters.copy,
        impostors,
    ) else {
        return;
    };
    commands.spawn((
        TreePart {
            step: Step::Game,
            kind: TreePartKind::Clusters,
        },
        scatter,
        attached,
        ChildOf(tree),
    ));
}

/// Spawns `tree`'s bark, leaf and trellis parts, each mesh step drawn over
/// its fade bands in `ranges`, swaying as `copy`.
fn spawn_tree_parts(
    commands: &mut Commands,
    tree: Entity,
    generated: &Generated,
    ranges: &[LodRange; 2],
    copy: SingleTree,
) {
    for (step, kind, mesh, material) in generated.parts() {
        // The tree shaders read the wind phase from the mesh tag, as the
        // shared renderer writes it for a grove's copies.
        let mut part = commands.spawn((
            TreePart { step, kind },
            Mesh3d(mesh.mesh.clone()),
            MeshMaterial3d(material.clone()),
            MeshTag::with_type::<TreeWindPhase>(copy.wind_phase.to_bits()),
            ChildOf(tree),
        ));
        if let Some(range) = part_range(&ranges[step.index()]) {
            part.insert(range);
        }
    }
    if let Some((mesh, material, transforms)) = &generated.trellis {
        for transform in transforms {
            commands.spawn((
                TreePart {
                    step: Step::Game,
                    kind: TreePartKind::Trellis,
                },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                *transform,
                ChildOf(tree),
            ));
        }
    }
}

/// Removes unused tree generation entries and their unneeded bark, leaf and
/// trellis materials. Their shapes and materials can be freed when nothing else
/// references them. Texture images and shared fruit and flower materials stay
/// cached for reuse.
#[expect(clippy::too_many_arguments, reason = "one Bevy system")]
pub(crate) fn prune_cache(
    mut removed: RemovedComponents<TreeKey>,
    mut removed_held: RemovedComponents<HeldTreeKeys>,
    mut removed_prepared: RemovedComponents<PreparedTrees>,
    changed_held: Query<(), Changed<HeldTreeKeys>>,
    changed_prepared: Query<(), Changed<PreparedTrees>>,
    keys: Query<&TreeKey>,
    held: Query<&HeldTreeKeys>,
    prepared: Query<&PreparedTrees>,
    mut cache: ResMut<TreeCache>,
) {
    let removed =
        removed.read().count() + removed_held.read().count() + removed_prepared.read().count();
    if removed == 0 && changed_held.is_empty() && changed_prepared.is_empty() {
        return;
    }
    let used: HashSet<&TreeKey> = keys
        .iter()
        .chain(held.iter().flat_map(|held| &held.0))
        .chain(prepared.iter().flat_map(|prepared| &prepared.keys))
        .collect();
    cache.trees.retain(|key, _| used.contains(key));

    // Drop the impostors and specimen ids of trees no longer cached. Bakes
    // already started finish, and stay in the shared cache.
    let cache = &mut *cache;
    cache
        .impostors
        .retain(|key, _| cache.trees.contains_key(key));
    cache
        .specimens
        .retain(|key, _| cache.trees.contains_key(key));

    // Preserve materials referenced by the generated trees still in the cache.
    let mut drawn = HashSet::new();
    let mut wires = HashSet::new();
    for entry in cache.trees.values() {
        let Entry::Ready(generated) = entry else {
            continue;
        };
        for level in &generated.levels {
            drawn.insert(level.bark_material.id());
            drawn.insert(level.leaf_material.id());
        }
        if let Some((_, material, _)) = &generated.trellis {
            wires.insert(material.id());
        }
    }
    cache.bark_materials.retain(|_, m| drawn.contains(&m.id()));
    cache.leaf_materials.retain(|_, m| drawn.contains(&m.id()));
    cache
        .trellis_materials
        .retain(|_, m| wires.contains(&m.id()));
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    /// Results of every [`TreeFinished`] event, in order.
    #[derive(Resource, Default)]
    struct Finished(Vec<Result<(), OptionsError>>);

    /// Creates a test app with the spawn systems and no impostors, without
    /// rendering or loading textures.
    fn spawn_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<StandardMaterial>()
            .init_asset::<TreeMaterial>()
            .init_asset::<TreePresetAsset>()
            .init_resource::<Wind>()
            .init_resource::<TreeCache>()
            .init_resource::<Finished>()
            .init_resource::<LodQuality>()
            .insert_resource(SpawnSettings {
                texture_root: None,
                texture_compression: TextureCompression::Bc7,
                impostor: None,
                fruit_impostor: None,
            })
            .add_systems(
                Update,
                (
                    reset_changed_trees,
                    start_generation,
                    finish_generation,
                    spawn_parts,
                )
                    .chain(),
            )
            .add_observer(|event: On<TreeFinished>, mut finished: ResMut<Finished>| {
                finished.0.push(event.result.clone());
            });
        app
    }

    /// Updates the app until `tree` loses [`TreeGenerating`].
    fn update_until_finished(app: &mut App, tree: Entity) {
        let deadline = Instant::now() + Duration::from_secs(30);
        app.update();
        while app.world().entity(tree).contains::<TreeGenerating>() {
            assert!(Instant::now() < deadline, "tree generation did not finish");
            std::thread::sleep(Duration::from_millis(1));
            app.update();
        }
    }

    fn part_count(app: &mut App, tree: Entity) -> usize {
        let mut parts = app.world_mut().query::<(&TreePart, &ChildOf)>();
        parts
            .iter(app.world())
            .filter(|(_, parent)| parent.parent() == tree)
            .count()
    }

    #[test]
    fn generating_marker_lasts_until_parts_spawn() {
        let mut app = spawn_app();
        let tree = app
            .world_mut()
            .spawn(TreeParams::preset(TreePreset::oak_small()))
            .id();

        // The marker is on the tree exactly while it has no parts. A small
        // tree can finish within the frame that starts it, so this is
        // checked after every frame until it finishes.
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            app.update();
            let generating = app.world().entity(tree).contains::<TreeGenerating>();
            assert_eq!(generating, part_count(&mut app, tree) == 0);
            if !generating {
                break;
            }
            assert!(Instant::now() < deadline, "tree generation did not finish");
            std::thread::sleep(Duration::from_millis(1));
        }
        let parts = part_count(&mut app, tree);
        assert!(parts > 0);
        assert_eq!(app.world().resource::<Finished>().0, vec![Ok(())]);

        // Touching the tree replaces its parts. The options are cached, so
        // this finishes within one update.
        app.world_mut()
            .get_mut::<TreeParams>(tree)
            .unwrap()
            .set_changed();
        app.update();
        assert!(!app.world().entity(tree).contains::<TreeGenerating>());
        assert_eq!(part_count(&mut app, tree), parts);
        assert_eq!(app.world().resource::<Finished>().0, vec![Ok(()), Ok(())]);

        // New options generate again, under a new key. A small tree can
        // finish generating within the frame that starts it, so the
        // generation is seen by its key and its finish, not by the marker.
        let old_key = *app.world().get::<TreeKey>(tree).unwrap();
        let mut options = TreePreset::oak_small().tree.clone();
        options.default_seed += 1;
        *app.world_mut().get_mut::<TreeParams>(tree).unwrap() = TreeParams::new(options);
        update_until_finished(&mut app, tree);
        assert_ne!(*app.world().get::<TreeKey>(tree).unwrap(), old_key);
        assert!(app.world().resource::<TreeCache>().trees.len() >= 2);
        assert_eq!(app.world().resource::<Finished>().0.len(), 3);
    }

    #[test]
    fn finished_trees_carry_their_specimen_until_they_change() {
        let mut app = spawn_app();
        // A tree carries a specimen exactly when it isn't generating. A small
        // tree can finish within the frame that starts it, so this is checked
        // after every frame rather than at a set one.
        let specimen_matches_state = |app: &App, tree: Entity| {
            let entity = app.world().entity(tree);
            assert_eq!(
                entity.contains::<GeneratedSpecimen>(),
                !entity.contains::<TreeGenerating>()
            );
        };
        let tree = app
            .world_mut()
            .spawn(TreeParams::preset(TreePreset::oak_small()))
            .id();
        app.update();
        specimen_matches_state(&app, tree);

        // The specimen arrives with the parts and is the tree's own.
        update_until_finished(&mut app, tree);
        let expected = Tree::from_preset(TreePreset::oak_small())
            .unwrap()
            .specimen();
        let specimen = app.world().get::<GeneratedSpecimen>(tree).unwrap();
        assert_eq!(*specimen.0, expected);

        // Changing the options replaces the stale specimen with the new
        // tree's once the rebuild ends.
        let mut options = TreePreset::oak_small().tree.clone();
        options.default_seed += 1;
        let reseeded = Tree::new(options.clone()).unwrap().specimen();
        assert_ne!(reseeded, expected);
        *app.world_mut().get_mut::<TreeParams>(tree).unwrap() = TreeParams::new(options);
        app.update();
        specimen_matches_state(&app, tree);
        update_until_finished(&mut app, tree);
        let specimen = app.world().get::<GeneratedSpecimen>(tree).unwrap();
        assert_eq!(*specimen.0, reseeded);

        // Removing the request removes the specimen.
        app.world_mut().entity_mut(tree).remove::<TreeParams>();
        app.update();
        assert!(!app.world().entity(tree).contains::<GeneratedSpecimen>());
    }

    /// Checks that a tree without an impostor draws bark and leaves at both
    /// mesh steps, single-sided bark included, each over its step's bands
    /// for the tree's size: the lower mesh until the cull.
    #[test]
    fn both_mesh_steps_are_drawn_over_their_bands() {
        let mut app = spawn_app();
        let scale = 1.5;
        let tree = app
            .world_mut()
            .spawn((
                TreeParams::preset(TreePreset::oak_small()),
                Transform::from_scale(Vec3::splat(scale)),
            ))
            .id();
        update_until_finished(&mut app, tree);

        let diameter = Tree::from_preset(TreePreset::oak_small())
            .unwrap()
            .mesh()
            .bounding_diameter();
        let ranges = mesh_ranges(diameter * scale, Quality::High);
        let mut parts = app.world_mut().query::<(
            &TreePart,
            &MeshMaterial3d<TreeMaterial>,
            Option<&VisibilityRange>,
        )>();
        let materials = app.world().resource::<Assets<TreeMaterial>>();
        let mut seen = Vec::new();
        for (part, material, range) in parts.iter(app.world()) {
            seen.push((part.step, part.kind));
            let expected = part_range(&ranges[part.step.index()]).unwrap();
            assert!(range == Some(&expected), "{part:?} has the wrong range");
            if part.kind == TreePartKind::Bark {
                let base = &materials.get(&material.0).unwrap().base;
                assert!(base.cull_mode.is_some() && !base.double_sided, "{part:?}");
            }
        }
        seen.sort_by_key(|(step, kind)| (step.index(), *kind as u8));
        assert_eq!(
            seen,
            [
                (Step::Game, TreePartKind::Bark),
                (Step::Game, TreePartKind::Leaves),
                (Step::Lower, TreePartKind::Bark),
                (Step::Lower, TreePartKind::Leaves),
            ]
        );
    }

    /// Checks that a fruiting tree draws its clusters through one instanced
    /// child: the tree's steps as slot 0, one slot per fruit specimen with
    /// its game-level and lower meshes, and one row per cluster for the
    /// tree's game-level mesh, plus an enlarged row for every cluster its
    /// lower mesh keeps. A tree without fruit gets no such child.
    #[test]
    fn clusters_are_drawn_by_one_instanced_child() {
        let mut app = spawn_app();
        let apple = app
            .world_mut()
            .spawn(TreeParams::preset(TreePreset::apple()))
            .id();
        let oak = app
            .world_mut()
            .spawn(TreeParams::preset(TreePreset::oak_small()))
            .id();
        update_until_finished(&mut app, apple);
        update_until_finished(&mut app, oak);

        let mut children = app.world_mut().query::<(
            &TreePart,
            &ChildOf,
            &procgena_core_bevy::instanced::InstancedScatter,
            &procgena_core_bevy::instanced::InstancedAttached,
        )>();
        let clusters: Vec<_> = children.iter(app.world()).collect();
        assert_eq!(clusters.len(), 1, "one clusters child, on the apple tree");
        let (part, parent, scatter, attached) = clusters[0];
        assert_eq!(part.kind, TreePartKind::Clusters);
        assert_eq!(parent.parent(), apple);

        // The tree's steps, then the fruit specimens, neither with an
        // impostor yet.
        let tree = Tree::from_preset(TreePreset::apple()).unwrap();
        let (_, fruit) = tree.skeleton().clusters(ClusterPart::Fruit);
        assert_eq!(scatter.slots.len(), 1 + fruit.len());
        assert!(scatter.slots[0].parts.is_empty());
        assert!(!scatter.slots[0].impostor);
        assert!(scatter.slots[0].cull);
        for slot in &scatter.slots[1..] {
            assert!(!slot.impostor, "fruit steps");
            assert!(!slot.cull, "the tree decides");
            assert!(!slot.parts.is_empty());
        }
        assert_eq!(scatter.copies.len(), 1);

        // Every cluster on the game-level mesh, the ones the lower mesh
        // keeps enlarged on it.
        let lod = tree.lod();
        assert_eq!(lod.game.cluster_stride, 1);
        assert_ne!(lod.game.cluster_scale, lod.lower.cluster_scale);
        let table = tree.skeleton().cluster_table(ClusterPart::Fruit);
        let kept = table
            .iter()
            .filter(|row| row.index % lod.lower.cluster_stride == 0)
            .count();
        assert!(kept < table.len());
        let rows = &attached.tables[0];
        let on = |step| procgena_core_bevy::instanced::AttachedRow::steps([step]);
        let game = rows
            .iter()
            .filter(|row| row.host_steps == on(Step::Game))
            .count();
        let lower = rows
            .iter()
            .filter(|row| row.host_steps == on(Step::Lower))
            .count();
        assert_eq!(game, table.len());
        assert_eq!(lower, kept);
        assert_eq!(rows.len(), game + lower, "no other masks");
        assert!(
            rows.iter()
                .all(|row| (row.slot as usize) < scatter.slots.len())
        );
        assert!(rows.iter().all(|row| row.slot >= 1));
    }

    /// Returns the distinct material identifiers for one kind of part on a
    /// tree at `step`, or at both mesh steps without one.
    fn part_materials(
        app: &mut App,
        tree: Entity,
        kind: TreePartKind,
        step: Option<Step>,
    ) -> HashSet<AssetId<TreeMaterial>> {
        let mut parts = app
            .world_mut()
            .query::<(&TreePart, &MeshMaterial3d<TreeMaterial>, &ChildOf)>();
        parts
            .iter(app.world())
            .filter(|(part, _, parent)| {
                part.kind == kind && parent.parent() == tree && step.is_none_or(|s| s == part.step)
            })
            .map(|(_, material, _)| material.0.id())
            .collect()
    }

    /// With textures, the lower mesh draws smaller bark images than the
    /// game-level mesh, so the steps' bark materials differ, while both
    /// steps share one leaf material.
    #[test]
    fn textured_bark_is_sized_by_step_and_leaves_are_shared() {
        let mut app = spawn_app();
        app.world_mut().resource_mut::<SpawnSettings>().texture_root = Some("textures".into());
        let options = TreePreset::oak_small().tree.clone();
        assert!(options.bark.textured);
        let tree = app.world_mut().spawn(TreeParams::new(options)).id();
        update_until_finished(&mut app, tree);

        let bark = |app: &mut App, step| part_materials(app, tree, TreePartKind::Bark, Some(step));
        let game = bark(&mut app, Step::Game);
        let lower = bark(&mut app, Step::Lower);
        assert_eq!(game.len(), 1);
        assert_eq!(lower.len(), 1);
        assert!(game.is_disjoint(&lower));
        assert_eq!(
            part_materials(&mut app, tree, TreePartKind::Leaves, None).len(),
            1
        );
        assert_eq!(bark_texture_size(Step::Game), TextureSize::Size1k);
        assert_eq!(bark_texture_size(Step::Lower), TextureSize::Size512);
    }

    #[test]
    fn trees_that_look_the_same_share_materials() {
        let mut app = spawn_app();
        app.add_systems(Update, prune_cache.after(spawn_parts));
        let spawn = |app: &mut App, options: TreeOptions| {
            let tree = app
                .world_mut()
                .spawn(TreeParams::Options(Arc::new(options)))
                .id();
            update_until_finished(app, tree);
            tree
        };
        let oak = TreePreset::oak_small().tree.clone();

        // Changing the random seed varies tree shape while preserving the
        // shared bark and leaf materials.
        let first = spawn(&mut app, oak.clone());
        let reseeded = TreeOptions {
            default_seed: oak.default_seed + 1,
            ..oak.clone()
        };
        let second = spawn(&mut app, reseeded);
        for kind in [TreePartKind::Bark, TreePartKind::Leaves] {
            let shared = part_materials(&mut app, first, kind, None);
            assert_eq!(shared.len(), 1, "{kind:?}");
            assert_eq!(
                part_materials(&mut app, second, kind, None),
                shared,
                "{kind:?}"
            );
        }

        // Changing bark colour creates a separate bark material; leaves still
        // share their original material.
        let mut tinted = oak.clone();
        tinted.bark.tint ^= 0x10;
        let third = spawn(&mut app, tinted);
        assert!(
            part_materials(&mut app, third, TreePartKind::Bark, None).is_disjoint(&part_materials(
                &mut app,
                first,
                TreePartKind::Bark,
                None
            ))
        );
        assert_eq!(
            part_materials(&mut app, third, TreePartKind::Leaves, None),
            part_materials(&mut app, first, TreePartKind::Leaves, None)
        );
        assert_eq!(app.world().resource::<TreeCache>().bark_materials.len(), 2);

        // Once the differently coloured tree is removed, its unique bark
        // material is no longer needed in the cache.
        app.world_mut().entity_mut(third).despawn();
        app.update();
        let cache = app.world().resource::<TreeCache>();
        assert_eq!(cache.bark_materials.len(), 1);
        assert_eq!(cache.leaf_materials.len(), 1);
    }

    #[test]
    fn preset_files_load_partial_options() {
        // Write a partial settings file into a fresh asset folder.
        let dir =
            std::env::temp_dir().join(format!("procgena_tree_bevy_assets_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("tall.tree.yml"),
            "base: oak_small\ntree:\n  default_seed: 9\n  branch:\n    levels: 2\n",
        )
        .unwrap();

        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin {
                file_path: dir.to_string_lossy().into_owned(),
                ..default()
            },
        ))
        .init_asset::<TreePresetAsset>()
        .register_asset_loader(TreePresetLoader);
        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<TreePresetAsset>("tall.tree.yml");
        let deadline = Instant::now() + Duration::from_secs(30);
        while !app
            .world()
            .resource::<Assets<TreePresetAsset>>()
            .contains(&handle)
        {
            assert!(Instant::now() < deadline, "the preset file did not load");
            std::thread::sleep(Duration::from_millis(1));
            app.update();
        }
        std::fs::remove_dir_all(&dir).unwrap();

        // Omitted settings keep the base's.
        let mut expected = TreePreset::oak_small().tree.clone();
        expected.default_seed = 9;
        expected.branch.levels = 2;
        let assets = app.world().resource::<Assets<TreePresetAsset>>();
        let preset = &assets.get(&handle).unwrap().0;
        assert_eq!(preset.tree, expected);
        assert_eq!(preset.key, "tall");
    }

    #[test]
    fn rejected_options_finish_with_their_error() {
        let mut app = spawn_app();
        let mut options = TreePreset::oak_small().tree.clone();
        options.branch.sections[0] = 0;
        let tree = app.world_mut().spawn(TreeParams::new(options)).id();

        update_until_finished(&mut app, tree);
        assert_eq!(part_count(&mut app, tree), 0);
        assert_eq!(
            app.world().resource::<Finished>().0,
            vec![Err(OptionsError::ZeroSections { level: 0 })]
        );
    }

    /// The steps switch where the shared chain does for the tree's size on
    /// screen. Without an impostor the lower mesh is drawn until the cull;
    /// with one, the lower mesh hands over to it.
    #[test]
    fn mesh_steps_hand_over_where_the_chain_switches() {
        let chain = DistanceChain::DEFAULT;
        let centre = |band: &std::ops::Range<f32>| (band.start + band.end) / 2.0;
        let close = |a: f32, b: f32| (a - b).abs() <= b * 1e-5;

        let size = 10.0;
        let factor = DistanceChain::factor(size, ViewScale::DEFAULT);
        let [game, lower] = mesh_ranges(size, Quality::High);
        assert_eq!(game.fade_in, 0.0..0.0);
        assert!(close(centre(&game.fade_out), factor / chain.lower_below));
        assert_eq!(lower.fade_in, game.fade_out);
        assert!(close(centre(&lower.fade_out), factor / chain.cull_below));

        // With the impostor, the lower mesh ends where it starts, and it
        // ends at the cull.
        let [chain_game, chain_lower, impostor] = chain_ranges(size, Quality::High);
        assert_eq!(chain_game, game);
        assert_eq!(chain_lower.fade_in, lower.fade_in);
        assert!(close(
            centre(&chain_lower.fade_out),
            factor / chain.impostor_below
        ));
        assert_eq!(impostor.fade_in, chain_lower.fade_out);
        assert_eq!(impostor.fade_out, lower.fade_out);

        // A lower quality setting, and a smaller tree, switch sooner.
        let low = mesh_ranges(size, Quality::Low);
        assert!(centre(&low[1].fade_in) < centre(&lower.fade_in));
        let half = mesh_ranges(size / 2.0, Quality::High);
        assert!(close(
            centre(&half[1].fade_in),
            centre(&lower.fade_in) / 2.0
        ));
    }

    /// A copy switches by its built diameter times its largest scale, in
    /// any axis or direction.
    #[test]
    fn copies_switch_by_their_scaled_diameter() {
        let transform = Transform::from_scale(Vec3::new(1.0, 2.0, -3.0));
        assert_eq!(copy_size(4.0, &transform), 12.0);
        assert_eq!(copy_size(4.0, &Transform::IDENTITY), 4.0);
    }

    #[test]
    fn wire_cylinders_span_their_wires() {
        let wire = TrellisWire {
            start: Vec3::new(-2.0, 1.0, 3.0),
            end: Vec3::new(4.0, 1.0, 3.0),
        };
        let transform = wire_transform(&wire);
        // Transform both ends of the unit cylinder to check that they reach the wire endpoints.
        let bottom = transform.transform_point(Vec3::new(0.0, -0.5, 0.0));
        let top = transform.transform_point(Vec3::new(0.0, 0.5, 0.0));
        assert!(bottom.abs_diff_eq(wire.start, 1e-5) || bottom.abs_diff_eq(wire.end, 1e-5));
        assert!(top.abs_diff_eq(wire.start, 1e-5) || top.abs_diff_eq(wire.end, 1e-5));
        assert!(!bottom.abs_diff_eq(top, 1e-3));
    }
}
