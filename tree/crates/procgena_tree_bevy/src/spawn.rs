//! Generate trees in background tasks and display their parts as child entities.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::camera::primitives::{Aabb, MeshAabb};
use bevy::camera::visibility::{NoAutoAabb, VisibilityRange};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use procgena_core::{Lod, LodLevel, LodRange, Quality, SpecimenId};
use procgena_core_bevy::{LodQuality, visibility_range};
use procgena_impostor::ImpostorLevel;
use procgena_specimen::{GeneratedSpecimen, Specimen};
use procgena_tree::{
    BranchShape, ClusterPart, LodDetail, MeshBuffers, OptionsError, Skeleton, TextureCompression,
    TextureSize, TreeCacheKey, TreeMesh, TreeOptions, TreePreset, TrellisWire, default_lod,
};

use crate::clusters::{
    BuiltClusters, TreeClusters, add_meshes, build_clusters, single_tree_clusters,
};
use crate::impostor::{ImpostorShown, TreeImpostorAssets};
use crate::material::{
    BarkTextures, LeafTextures, TreeMaterial, average_bark_material, bark_material,
    flower_material, fruit_material, leaf_material, trellis_material,
};
use crate::material_key::{BarkKey, LeafKey, TrellisKey};
use crate::mesh::{bark_mesh, billboard_aabb, cluster_mesh, leaf_mesh};
use crate::scatter::PreparedTrees;
use crate::wind::Wind;

/// A generated tree positioned by this entity's transform.
///
/// Add this component to an entity after installing [`TreePlugin`](crate::TreePlugin).
/// The tree appears when background generation finishes. Bark and leaf children
/// provide each configured detail level; visible trellis wires get their own children.
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
    /// Index in the detail levels sorted by distance, starting at 0. Trellis wires use 0.
    pub lod: usize,
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
    /// The tree's fruit and flower clusters. One child draws them all, at
    /// every detail level, through the shared instanced renderer of
    /// `procgena_core_bevy` (an `InstancedScatter` with its
    /// `InstancedAttached`): each cluster is drawn at the detail its own
    /// distance gives, until the tree's impostor takes over. Its `lod` is 0.
    Clusters,
    /// A single support wire for a trellis.
    Trellis,
    /// The tree's impostor: one quad showing a baked view of the tree,
    /// drawn beyond every mesh level; see
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

/// Mesh and material settings for one level of
/// [`TreePlugin::lod`](crate::TreePlugin::lod).
/// The plugin automatically renders [`BranchShape::Ribbons`] from both sides.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TreeLevel {
    /// Mesh detail for this level, passed to [`Skeleton::mesh`].
    pub detail: LodDetail,
    /// Bark texture size or flat-colour shading for this level.
    pub bark: BarkTexture,
    /// Leaf image to use for this level's flat leaf cards.
    pub leaves: LeafTexture,
}

/// Bark appearance for a detail level. These choices apply when the plugin has
/// a texture folder configured; without one, bark uses its tint alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BarkTexture {
    /// Use colour, normal and roughness images at the given size. Smaller
    /// images use less memory. Supply the files under the plugin's texture folder.
    Textured(TextureSize),
    /// Use one colour that approximates textured bark viewed from a distance.
    /// Requires no bark images; see [`average_bark_material`] for tint handling
    /// and the fallback for unknown bark kinds.
    AverageColor,
}

/// Image used on each flat leaf card. Requires a configured plugin texture folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LeafTexture {
    /// Use the leaf kind's regular colour image ([`Color`](procgena_tree::LeafMap::Color))
    /// and its normal map when available.
    #[default]
    Leaf,
    /// Use the leaf kind's [`Clump`](procgena_tree::LeafMap::Clump) image to show several
    /// leaves per card, without a normal map. For distant trees, increase
    /// [`leaf_stride`](procgena_tree::LodDetail::leaf_stride) to draw fewer cards and
    /// [`leaf_scale`](procgena_tree::LodDetail::leaf_scale) to make them larger.
    Clump,
}

/// Returns [`default_lod`]'s four levels, at the same distances, with their
/// materials. Bark uses 1k textures at the nearest level and 512 textures at
/// the next two. The fourth level uses a flat average bark colour and images
/// of several leaves per card. These material choices require a configured
/// plugin texture folder.
pub fn default_tree_lod() -> Lod<TreeLevel> {
    let lod = default_lod();
    let last = lod.levels.len() - 1;
    let mut index = 0;
    lod.map_detail(|detail| {
        let textured = |size| (BarkTexture::Textured(size), LeafTexture::Leaf);
        let (bark, leaves) = match index {
            0 => textured(TextureSize::Size1k),
            i if i < last => textured(TextureSize::Size512),
            _ => (BarkTexture::AverageColor, LeafTexture::Clump),
        };
        index += 1;
        TreeLevel {
            detail,
            bark,
            leaves,
        }
    })
}

/// Texture location, format and detail levels copied from the plugin configuration.
#[derive(Resource, Debug, Clone)]
pub(crate) struct SpawnSettings {
    pub texture_root: Option<String>,
    pub texture_compression: TextureCompression,
    /// The mesh and far-form levels, without the impostor.
    pub lod: Lod<TreeLevel>,
    pub impostor: Option<ImpostorLevel>,
}

impl SpawnSettings {
    /// Every level a tree is drawn with: the mesh levels, then the impostor,
    /// if any, as the last level (`None` as its detail).
    pub(crate) fn draw_lod(&self) -> Lod<Option<TreeLevel>> {
        let mut lod = self.lod.clone().map_detail(Some);
        if let Some(impostor) = self.impostor {
            lod.levels.push(impostor.level(None));
        }
        lod
    }

    /// Each mesh level's fade bands at `quality`, nearest first, as drawn
    /// before the tree's impostor is baked: the farthest level is drawn at
    /// every distance beyond.
    pub(crate) fn mesh_ranges(&self, quality: Quality) -> Vec<LodRange> {
        self.lod.ranges(self.factor(quality))
    }

    /// Each level's fade bands at `quality` once the impostor is baked: the
    /// mesh levels, then the impostor. `None` without an impostor.
    pub(crate) fn impostor_ranges(&self, quality: Quality) -> Option<Vec<LodRange>> {
        self.impostor?;
        Some(self.draw_lod().ranges(self.factor(quality)))
    }

    /// The factor every distance is multiplied by. Trees are not sized yet:
    /// each one counts as the levels' reference size.
    fn factor(&self, quality: Quality) -> f32 {
        self.lod.factor(self.lod.reference_size, quality)
    }
}

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

/// A part’s Bevy mesh and optional bounds covering its camera-facing rotations.
pub(crate) struct PartMesh<M> {
    pub(crate) mesh: M,
    /// Bounds covering every camera-facing rotation of a billboard part; `None`
    /// for other parts, whose mesh bounds Bevy computes.
    bounds: Option<Aabb>,
    /// Bounds of the part as drawn, in tree coordinates: `bounds` for a
    /// billboard part, the mesh's own bounds otherwise. Wind sway is not
    /// included.
    pub(crate) extent: Aabb,
}

/// Meshes produced by a background task for one detail level; each is absent
/// when the level has no triangles of that kind.
struct LevelMeshes {
    level: TreeLevel,
    bark: Option<PartMesh<Mesh>>,
    leaves: Option<PartMesh<Mesh>>,
}

/// What a background task generates for one tree.
struct TaskMeshes {
    levels: Vec<LevelMeshes>,
    /// The tree as a tagged specimen.
    specimen: Arc<Specimen>,
    /// The cluster specimens and tables, drawn instanced.
    clusters: Vec<BuiltClusters>,
    /// The impostor's baked level's fruit and flowers, each merged into one
    /// mesh, for the impostor bake only.
    baked_clusters: Vec<(ClusterPart, Mesh)>,
}

/// A generated tree, or the validation error that prevented generation.
type TaskOutput = Result<TaskMeshes, OptionsError>;

/// Mesh handles and materials for one detail level.
struct LevelAssets {
    /// Bark mesh, absent when this level contains no bark triangles.
    bark: Option<PartMesh<Handle<Mesh>>>,
    /// Leaf mesh, absent when this level contains no leaf triangles.
    leaves: Option<PartMesh<Handle<Mesh>>>,
    /// Bark material, shared by levels with matching bark settings and sidedness.
    bark_material: Handle<TreeMaterial>,
    /// Leaf material, shared by levels using the same leaf image selection.
    leaf_material: Handle<TreeMaterial>,
}

/// Meshes and materials reused by trees with identical options.
pub(crate) struct Generated {
    /// Detail levels, nearest first.
    levels: Vec<LevelAssets>,
    /// Each level's fade bands until the impostor is baked, one per level.
    ranges: Vec<LodRange>,
    /// Each level's fade bands, then the impostor's, once the impostor is
    /// baked. `None` when trees have no impostor.
    impostor_ranges: Option<Vec<LodRange>>,
    /// The tree's fruit and flower cluster specimens and tables.
    pub(crate) clusters: Vec<TreeClusters>,
    /// The impostor's baked level's fruit and flowers, merged, with their
    /// materials, for the impostor bake only.
    pub(crate) baked_clusters: Vec<(Handle<Mesh>, Handle<TreeMaterial>)>,
    /// One cylinder mesh and material shared by all wires, plus each wire's placement.
    pub(crate) trellis: Option<(Handle<Mesh>, Handle<StandardMaterial>, Vec<Transform>)>,
    /// The tree as a tagged specimen, shared by every tree with these options.
    pub(crate) specimen: Arc<Specimen>,
}

impl Generated {
    /// A sphere around every mesh part at every level, in the tree's own
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

    /// Every mesh part of every detail level, nearest level first, with its
    /// material. Trellis wires and clusters are not included.
    pub(crate) fn parts(
        &self,
    ) -> impl Iterator<
        Item = (
            usize,
            TreePartKind,
            &PartMesh<Handle<Mesh>>,
            &Handle<TreeMaterial>,
        ),
    > {
        self.levels
            .iter()
            .enumerate()
            .flat_map(move |(lod, level)| {
                [
                    (TreePartKind::Bark, &level.bark, &level.bark_material),
                    (TreePartKind::Leaves, &level.leaves, &level.leaf_material),
                ]
                .into_iter()
                .filter_map(move |(kind, mesh, material)| {
                    Some((lod, kind, mesh.as_ref()?, material))
                })
            })
    }

    /// The number of detail levels.
    pub(crate) fn level_count(&self) -> usize {
        self.levels.len()
    }

    /// Each level's fade bands, then the impostor's, as drawn once the
    /// impostor is baked; `None` when trees have no impostor.
    pub(crate) fn impostor_ranges(&self) -> Option<&[LodRange]> {
        self.impostor_ranges.as_deref()
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
    leaf_textures: HashMap<(String, LeafTexture), LeafTextures>,
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
            let levels = settings.lod.levels.clone();
            let baked = settings.impostor.map(|impostor| impostor.baked_level);
            let task_options = options.clone();
            let task = AsyncComputeTaskPool::get()
                .spawn(async move { generate(&task_options, &levels, baked) });
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

/// Generates one skeleton and meshes for each detail level, sorted by
/// distance, and the fruit and flower clusters. `baked` is the level an
/// impostor bakes, whose clusters are also merged for the bake.
/// Skips [`TreeMesh::billboard_cull`](procgena_tree::TreeMesh::billboard_cull) because
/// the tree shaders rotate the main shapes toward each view.
fn generate(
    options: &TreeOptions,
    levels: &[LodLevel<TreeLevel>],
    baked: Option<usize>,
) -> TaskOutput {
    let skeleton = Skeleton::generate(options)?;
    let part = |buffers: &MeshBuffers, convert: &dyn Fn(&MeshBuffers) -> Mesh| {
        (!buffers.indices.is_empty()).then(|| {
            let mesh = convert(buffers);
            let bounds = billboard_aabb(buffers);
            let extent = bounds.or_else(|| mesh.get_aabb()).unwrap_or_default();
            PartMesh {
                mesh,
                bounds,
                extent,
            }
        })
    };
    let lods: Vec<(LodLevel<LodDetail>, TreeMesh)> = levels
        .iter()
        .map(|level| {
            let mesh = skeleton.mesh(options, &level.detail.detail);
            let level = LodLevel {
                distance: level.distance,
                hysteresis: level.hysteresis,
                form: level.form,
                casts_shadows: level.casts_shadows,
                detail: level.detail.detail,
            };
            (level, mesh)
        })
        .collect();
    let specimen = Arc::new(skeleton.specimen(&lods));
    let meshes: Vec<LevelMeshes> = levels
        .iter()
        .zip(&lods)
        .map(|(level, (_, mesh))| LevelMeshes {
            level: level.detail,
            bark: part(&mesh.branches, &|b| bark_mesh(b, &options.bark)),
            leaves: part(&mesh.leaves, &leaf_mesh),
        })
        .collect();

    // The baked level's clusters, merged as that level composes them.
    let baked_clusters = baked
        .and_then(|level| lods.get(level))
        .map(|(_, mesh)| {
            [
                (ClusterPart::Fruit, &mesh.fruit),
                (ClusterPart::Flowers, &mesh.flowers),
            ]
            .into_iter()
            .filter(|(_, buffers)| !buffers.indices.is_empty())
            .map(|(part, buffers)| (part, cluster_mesh(buffers)))
            .collect()
        })
        .unwrap_or_default();
    Ok(TaskMeshes {
        levels: meshes,
        specimen,
        clusters: build_clusters(&skeleton),
        baked_clusters,
    })
}

/// Collects completed tasks and registers shared meshes and materials.
/// Invalid options are logged and cached as a failed result. Each tree's
/// fade bands use the [`LodQuality`] in use when it finishes.
#[expect(clippy::too_many_arguments, reason = "one Bevy system")]
pub(crate) fn finish_generation(
    mut cache: ResMut<TreeCache>,
    settings: Res<SpawnSettings>,
    quality: Res<LodQuality>,
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

        // Share matching materials across trees and their distance-based detail
        // levels. Bark visible from both sides needs a separate material.
        // Reuse texture images; without a texture folder, use plain colours.
        let compression = settings.texture_compression;
        let root = settings.texture_root.as_deref();
        let mut level_assets = Vec::with_capacity(levels.len());
        for l in levels {
            // Render both faces of each ribbon strip.
            let two_sided = l.level.detail.branch_shape == BranchShape::Ribbons;
            let look = root.map(|_| l.level.bark);
            let key = BarkKey::new(&options.bark, look, two_sided);
            let bark = cache.bark_materials.entry(key).or_insert_with(|| {
                let mut material = match (look, root) {
                    (Some(BarkTexture::Textured(size)), Some(root)) => {
                        let textures = cache
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
                            });
                        bark_material(&options.bark, Some(&*textures), wind.0)
                    }
                    (Some(BarkTexture::AverageColor), _) => {
                        average_bark_material(&options.bark, wind.0)
                    }
                    _ => bark_material(&options.bark, None, wind.0),
                };
                // Ribbon billboards have no tangents to orient a normal map.
                if two_sided {
                    material.base.cull_mode = None;
                    material.base.double_sided = true;
                    material.base.normal_map_texture = None;
                }
                tree_materials.add(material)
            });

            // Omit normal maps for camera-facing leaves, which have no tangents.
            let image = root.map(|_| l.level.leaves);
            let billboard = l.level.detail.leaf_billboard.is_some();
            let key = LeafKey::new(&options.leaves, image, billboard);
            let leaves = cache.leaf_materials.entry(key).or_insert_with(|| {
                let textures = image.zip(root).map(|(image, root)| {
                    let kind = &options.leaves.kind;
                    let textures = cache
                        .leaf_textures
                        .entry((kind.clone(), image))
                        .or_insert_with(|| match image {
                            LeafTexture::Leaf => {
                                LeafTextures::load(&asset_server, root, kind, compression)
                            }
                            LeafTexture::Clump => {
                                LeafTextures::load_clump(&asset_server, root, kind, compression)
                            }
                        })
                        .clone();
                    LeafTextures {
                        normal: textures.normal.filter(|_| !billboard),
                        ..textures
                    }
                });
                tree_materials.add(leaf_material(&options.leaves, textures.as_ref(), wind.0))
            });

            let mut add = |part: Option<PartMesh<Mesh>>| {
                part.map(|p| PartMesh {
                    mesh: meshes.add(p.mesh),
                    bounds: p.bounds,
                    extent: p.extent,
                })
            };
            level_assets.push(LevelAssets {
                bark: add(l.bark),
                leaves: add(l.leaves),
                bark_material: bark.clone(),
                leaf_material: leaves.clone(),
            });
        }

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
            ranges: settings.mesh_ranges(quality.0),
            impostor_ranges: settings.impostor_ranges(quality.0),
            clusters,
            baked_clusters,
            trellis,
            specimen,
        };
        *entry = Entry::Ready(Arc::new(generated));
    }
}

/// The visibility range a part with fade bands `range` is drawn with, or
/// `None` when it is drawn at every distance and needs no range.
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

/// Adds child entities once a waiting tree's assets are ready.
/// Stops waiting after a failed generation attempt. Either way, triggers [`TreeFinished`].
pub(crate) fn spawn_parts(
    mut commands: Commands,
    waiting: Query<(Entity, &TreeKey), With<TreeGenerating>>,
    cache: Res<TreeCache>,
    settings: Res<SpawnSettings>,
) {
    for (entity, key) in &waiting {
        let result = match cache.trees.get(key) {
            Some(Entry::Ready(generated)) => {
                // Drawn without the impostor until `show_impostors` adds it.
                let specimen = cache.specimen_id(*key);
                spawn_tree_parts(&mut commands, entity, generated);
                spawn_clusters(&mut commands, entity, generated, specimen, &settings, false);
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

/// Spawns the child that draws `tree`'s fruit and flower clusters, if it
/// grows any. `specimen` is the tree's specimen id. With `impostor`, the
/// tree's levels end in its baked impostor, so clusters stop where it
/// fades in.
pub(crate) fn spawn_clusters(
    commands: &mut Commands,
    tree: Entity,
    generated: &Generated,
    specimen: SpecimenId,
    settings: &SpawnSettings,
    impostor: bool,
) {
    let Some((scatter, attached)) = single_tree_clusters(
        &generated.clusters,
        specimen,
        &settings.lod.levels,
        tree_lod(settings, impostor),
        generated.bounds(SWAY_ALLOWANCE),
    ) else {
        return;
    };
    commands.spawn((
        TreePart {
            lod: 0,
            kind: TreePartKind::Clusters,
        },
        scatter,
        attached,
        ChildOf(tree),
    ));
}

/// A single tree's levels as its clusters see them: the mesh levels, then
/// the impostor once it is shown.
pub(crate) fn tree_lod(settings: &SpawnSettings, impostor: bool) -> Lod {
    if impostor {
        settings.draw_lod().map_detail(|_| ())
    } else {
        settings.lod.clone().map_detail(|_| ())
    }
}

fn spawn_tree_parts(commands: &mut Commands, tree: Entity, generated: &Generated) {
    for (lod, kind, mesh, material) in generated.parts() {
        let mut part = commands.spawn((
            TreePart { lod, kind },
            Mesh3d(mesh.mesh.clone()),
            MeshMaterial3d(material.clone()),
            ChildOf(tree),
        ));
        if let Some(range) = part_range(&generated.ranges[lod]) {
            part.insert(range);
        }
        // Use rotation-aware bounds so Bevy does not hide a billboard whose
        // stored shape is outside the view but whose rotated shape is visible.
        if let Some(bounds) = mesh.bounds {
            part.insert((bounds, NoAutoAabb));
        }
    }
    if let Some((mesh, material, transforms)) = &generated.trellis {
        for transform in transforms {
            commands.spawn((
                TreePart {
                    lod: 0,
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
    use procgena_tree::LOD_REFERENCE_SIZE;

    /// Results of every [`TreeFinished`] event, in order.
    #[derive(Resource, Default)]
    struct Finished(Vec<Result<(), OptionsError>>);

    /// Creates a test app with one full-detail level and the spawn systems,
    /// without rendering or loading textures.
    fn spawn_app() -> App {
        spawn_app_with(Lod::new(
            LOD_REFERENCE_SIZE,
            vec![LodLevel::new(
                0.0,
                0.0,
                TreeLevel {
                    detail: LodDetail::default(),
                    bark: BarkTexture::Textured(TextureSize::Size1k),
                    leaves: LeafTexture::Leaf,
                },
            )],
        ))
    }

    /// Creates a test app with the supplied levels and the spawn systems,
    /// without rendering or loading textures.
    fn spawn_app_with(lod: Lod<TreeLevel>) -> App {
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
                lod,
                impostor: None,
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

        app.update();
        assert!(app.world().entity(tree).contains::<TreeGenerating>());
        assert_eq!(part_count(&mut app, tree), 0);

        update_until_finished(&mut app, tree);
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

        // New options generate again.
        let mut options = TreePreset::oak_small().tree.clone();
        options.default_seed += 1;
        *app.world_mut().get_mut::<TreeParams>(tree).unwrap() = TreeParams::new(options);
        app.update();
        assert!(app.world().entity(tree).contains::<TreeGenerating>());
        update_until_finished(&mut app, tree);
        assert_eq!(app.world().resource::<Finished>().0.len(), 3);
    }

    #[test]
    fn finished_trees_carry_their_specimen_until_they_change() {
        let mut app = spawn_app();
        let tree = app
            .world_mut()
            .spawn(TreeParams::preset(TreePreset::oak_small()))
            .id();
        app.update();
        assert!(!app.world().entity(tree).contains::<GeneratedSpecimen>());

        // The specimen arrives with the parts and matches the configured level.
        update_until_finished(&mut app, tree);
        let expected = {
            let tree = procgena_tree::Tree::from_preset(TreePreset::oak_small()).unwrap();
            tree.specimen(&tree.lod_meshes(&[LodLevel::new(0.0, 0.0, LodDetail::default())]))
        };
        let specimen = app.world().get::<GeneratedSpecimen>(tree).unwrap();
        assert_eq!(*specimen.0, expected);

        // Changing the options removes the stale specimen until the rebuild ends.
        let mut options = TreePreset::oak_small().tree.clone();
        options.default_seed += 1;
        *app.world_mut().get_mut::<TreeParams>(tree).unwrap() = TreeParams::new(options);
        app.update();
        assert!(!app.world().entity(tree).contains::<GeneratedSpecimen>());
        update_until_finished(&mut app, tree);
        assert!(app.world().entity(tree).contains::<GeneratedSpecimen>());

        // Removing the request removes the specimen.
        app.world_mut().entity_mut(tree).remove::<TreeParams>();
        app.update();
        assert!(!app.world().entity(tree).contains::<GeneratedSpecimen>());
    }

    /// Checks that all four default levels have bark and leaves, and that bark
    /// is two-sided only at the fourth level, which uses ribbons.
    #[test]
    fn ribbon_bark_is_drawn_two_sided() {
        let mut app = spawn_app_with(default_tree_lod());
        let tree = app
            .world_mut()
            .spawn(TreeParams::preset(TreePreset::oak_small()))
            .id();
        update_until_finished(&mut app, tree);

        let mut parts = app
            .world_mut()
            .query::<(&TreePart, &MeshMaterial3d<TreeMaterial>)>();
        let materials = app.world().resource::<Assets<TreeMaterial>>();
        let mut seen = Vec::new();
        for (part, material) in parts.iter(app.world()) {
            seen.push((part.lod, part.kind));
            if part.kind == TreePartKind::Bark {
                let base = &materials.get(&material.0).unwrap().base;
                let ribbons = part.lod == 3;
                assert_eq!(base.cull_mode.is_none(), ribbons, "lod {}", part.lod);
                assert_eq!(base.double_sided, ribbons, "lod {}", part.lod);
            }
        }
        for lod in 0..4 {
            assert!(seen.contains(&(lod, TreePartKind::Bark)), "lod {lod}");
            assert!(seen.contains(&(lod, TreePartKind::Leaves)), "lod {lod}");
        }
    }

    /// Checks that LOD3 spawns only the main parts, with rotation data and bounds
    /// for the camera-facing bark and leaves. Fruit is not a per-level part
    /// (see `clusters_are_drawn_by_one_instanced_child`). Crossed fallback
    /// shapes must not be spawned.
    #[test]
    fn farthest_level_parts_turn_toward_the_camera() {
        let mut app = spawn_app_with(default_tree_lod());
        let tree = app
            .world_mut()
            .spawn(TreeParams::preset(TreePreset::apple()))
            .id();
        update_until_finished(&mut app, tree);

        let mut parts = app
            .world_mut()
            .query::<(&TreePart, &Mesh3d, Option<&Aabb>, Has<NoAutoAabb>)>();
        let meshes = app.world().resource::<Assets<Mesh>>();
        let mut far_kinds = Vec::new();
        for (part, mesh, bounds, fixed) in parts.iter(app.world()) {
            let mesh = meshes.get(&mesh.0).unwrap();
            let billboard = mesh.contains_attribute(crate::ATTRIBUTE_BILLBOARD_PIVOT)
                && mesh.contains_attribute(crate::ATTRIBUTE_BILLBOARD_AXIS);
            let far =
                part.lod == 3 && matches!(part.kind, TreePartKind::Bark | TreePartKind::Leaves);
            assert_eq!(billboard, far, "{part:?}");
            assert_eq!(fixed, far, "{part:?}");
            assert_eq!(bounds.is_some(), far, "{part:?}");
            if part.lod == 3 {
                far_kinds.push(part.kind);
            }
        }
        far_kinds.sort_by_key(|k| *k as u8);
        assert_eq!(far_kinds, [TreePartKind::Bark, TreePartKind::Leaves]);
    }

    /// Checks that a fruiting tree draws its clusters through one instanced
    /// child: the tree's levels as slot 0, one slot per fruit specimen with
    /// a part per fruit level, and one row per cluster for the levels that
    /// keep it, plus an enlarged row for the farthest level, which keeps
    /// every second cluster. A tree without fruit gets no such child.
    #[test]
    fn clusters_are_drawn_by_one_instanced_child() {
        let mut app = spawn_app_with(default_tree_lod());
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

        // The tree's levels, then the fruit specimens.
        let skeleton = Skeleton::generate(&TreePreset::apple().tree).unwrap();
        let (placed, fruit) = skeleton.clusters(ClusterPart::Fruit);
        assert_eq!(scatter.slots.len(), 1 + fruit.len());
        assert!(scatter.slots[0].parts.is_empty());
        assert_eq!(scatter.slots[0].lod.levels.len(), 4);
        for slot in &scatter.slots[1..] {
            assert_eq!(slot.lod.levels.len(), 3, "fruit levels");
            assert!(!slot.parts.is_empty());
        }
        assert_eq!(scatter.copies.len(), 1);

        // Every cluster at levels 0 to 2, every second one enlarged at 3.
        let rows = &attached.tables[0];
        let near = rows.iter().filter(|row| row.host_levels == 0b0111).count();
        let far: Vec<_> = rows
            .iter()
            .filter(|row| row.host_levels == 0b1000)
            .collect();
        assert_eq!(near, placed.len());
        assert_eq!(far.len(), placed.len().div_ceil(2));
        assert_eq!(rows.len(), near + far.len(), "no other masks");
        assert!(
            rows.iter()
                .all(|row| (row.slot as usize) < scatter.slots.len())
        );
        assert!(rows.iter().all(|row| row.slot >= 1));
    }

    /// Checks that camera-facing leaves omit normal maps even when their leaf
    /// texture set provides one.
    #[test]
    fn billboard_leaves_leave_out_the_normal_map() {
        let levels = default_lod().levels;
        let leaf_image = |level: &LodLevel<LodDetail>| LodLevel {
            distance: level.distance,
            hysteresis: level.hysteresis,
            form: level.form,
            casts_shadows: level.casts_shadows,
            detail: TreeLevel {
                detail: level.detail,
                bark: BarkTexture::AverageColor,
                leaves: LeafTexture::Leaf,
            },
        };
        let near_and_farthest = vec![leaf_image(&levels[0]), leaf_image(&levels[3])];
        let mut app = spawn_app_with(Lod::new(LOD_REFERENCE_SIZE, near_and_farthest));
        app.world_mut().resource_mut::<SpawnSettings>().texture_root = Some("textures".into());
        let mut options = TreePreset::oak_small().tree.clone();
        options.leaves.kind = "white_oak".to_owned();
        assert!(procgena_tree::leaf_has_normal_map(&options.leaves.kind));
        let tree = app.world_mut().spawn(TreeParams::new(options)).id();
        update_until_finished(&mut app, tree);

        let mut parts = app
            .world_mut()
            .query::<(&TreePart, &MeshMaterial3d<TreeMaterial>)>();
        let materials = app.world().resource::<Assets<TreeMaterial>>();
        let mut seen = 0;
        for (part, material) in parts.iter(app.world()) {
            if part.kind == TreePartKind::Leaves {
                let base = &materials.get(&material.0).unwrap().base;
                assert_eq!(base.normal_map_texture.is_some(), part.lod == 0, "{part:?}");
                seen += 1;
            }
        }
        assert_eq!(seen, 2);
    }

    /// Returns the distinct material identifiers for one kind of part on a tree.
    fn part_materials(
        app: &mut App,
        tree: Entity,
        kind: TreePartKind,
    ) -> HashSet<AssetId<TreeMaterial>> {
        let mut parts = app
            .world_mut()
            .query::<(&TreePart, &MeshMaterial3d<TreeMaterial>, &ChildOf)>();
        parts
            .iter(app.world())
            .filter(|(part, _, parent)| part.kind == kind && parent.parent() == tree)
            .map(|(_, material, _)| material.0.id())
            .collect()
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
            let shared = part_materials(&mut app, first, kind);
            assert_eq!(shared.len(), 1, "{kind:?}");
            assert_eq!(part_materials(&mut app, second, kind), shared, "{kind:?}");
        }

        // Changing bark colour creates a separate bark material; leaves still
        // share their original material.
        let mut tinted = oak.clone();
        tinted.bark.tint ^= 0x10;
        let third = spawn(&mut app, tinted);
        assert!(
            part_materials(&mut app, third, TreePartKind::Bark).is_disjoint(&part_materials(
                &mut app,
                first,
                TreePartKind::Bark
            ))
        );
        assert_eq!(
            part_materials(&mut app, third, TreePartKind::Leaves),
            part_materials(&mut app, first, TreePartKind::Leaves)
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

    #[test]
    fn detail_levels_hand_over_at_their_distances() {
        let settings = |lod| SpawnSettings {
            texture_root: None,
            texture_compression: TextureCompression::Bc7,
            lod,
            impostor: None,
        };
        let default = settings(default_tree_lod());
        let ranges: Vec<VisibilityRange> = default
            .mesh_ranges(Quality::High)
            .iter()
            .map(|r| part_range(r).unwrap())
            .collect();
        assert_eq!(ranges[0].start_margin, 0.0..0.0);
        assert_eq!(ranges[0].end_margin, 9.5..10.5);
        assert_eq!(ranges[1].start_margin, 9.5..10.5);
        assert_eq!(ranges[1].end_margin, 23.75..26.25);
        assert_eq!(ranges[2].start_margin, 23.75..26.25);
        assert_eq!(ranges[2].end_margin, 47.5..52.5);
        assert_eq!(ranges[3].start_margin, 47.5..52.5);
        assert_eq!(ranges[3].end_margin.start, f32::INFINITY);

        // A lower quality setting switches every level sooner.
        let low = default.mesh_ranges(Quality::Low);
        let fade_in = &low[1].fade_in;
        assert!((fade_in.start - 3.8).abs() < 1e-5 && (fade_in.end - 4.2).abs() < 1e-5);

        // A lone detail level must remain visible at every camera distance.
        let mut one = default_tree_lod();
        one.levels.truncate(1);
        let single = settings(one).mesh_ranges(Quality::High);
        assert!(part_range(&single[0]).is_none());
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
