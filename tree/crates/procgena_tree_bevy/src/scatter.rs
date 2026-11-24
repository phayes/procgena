//! Tree Scatters: many generated trees spread over ground and drawn by one
//! entity.

mod prepare;
mod promote;
mod render;

pub use prepare::{
    PrepareTreeScatter, TreeScatterPreparing, TreeScatterReady, tree_scatter_variants,
};
pub(crate) use prepare::{PreparedTrees, start_preparing, stop_preparing, track_preparing};
pub use promote::{PromotedTree, TreeScatterHit, TreeScatterTrees};
pub(crate) use promote::{demote_trees, sync_promoted_trees};
pub(crate) use render::GroveRenderPlugin;

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::camera::primitives::Aabb;
use bevy::math::DVec2;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use procgena_specimen::{GeneratedSpecimen, Specimen, TagFilter};
use procgena_tree::{
    CopyAddress, FlatGround, Ground, HeightGrid, ScatterArea, ScatterInput, TreeCopy, TreeOptions,
    TreeScatter, TreeScatterError, TreeScatterPlacement, TreeScatterPreset,
};

use crate::impostor::TreeImpostorAssets;
use crate::material::TreeMaterial;
use crate::spawn::{
    CacheState, Generated, HeldTreeKeys, SpawnSettings, TreeCache, TreeKey, TreePartKind,
};

/// A tree Scatter, such as a grove or a woodland, spread over a rectangle of
/// ground centred on this entity.
///
/// Add it after installing [`TreePlugin`](crate::TreePlugin). The grove
/// draws all its trees itself, without an entity per tree, so one grove can
/// hold thousands of trees, up to
/// [`MAX_SCATTER_COPIES`](procgena_tree::MAX_SCATTER_COPIES). Trees of the
/// same species and shape variant share their meshes, and switch detail
/// levels and sway in the wind like single trees. The grove entity gets a
/// [`TreeScatterInstances`] listing every tree, and a [`TreeScatterAssets`]
/// with their meshes; to give one tree an entity of its own, see
/// [`PromotedTree`].
///
/// Changing the component updates the grove in place: trees that keep their
/// spot, species and shape stay as they are, and only trees of a changed
/// species are generated again. Lowering the density only removes trees.
/// Removing the component removes the trees.
///
/// The grove is drawn in every 3D camera while its entity is visible.
/// Trellis wires are not drawn in groves.
#[derive(Component, Debug, Clone, PartialEq)]
#[require(Transform, Visibility)]
pub struct TreeScatterParams {
    /// Which trees grow, and how they vary.
    pub tree_scatter: TreeScatterSource,
    /// Width (X) and depth (Z) of the ground, in metres. Each side must be
    /// above 0 and at most [`MAX_SCATTER_SIZE`](procgena_tree::MAX_SCATTER_SIZE).
    pub size: Vec2,
    /// The ground the trees stand on.
    pub ground: TreeScatterGround,
}

impl TreeScatterParams {
    /// A Scatter on flat ground at the entity's height, `size` metres across.
    pub fn new(tree_scatter: TreeScatter, size: Vec2) -> Self {
        Self {
            tree_scatter: TreeScatterSource::Options(Arc::new(tree_scatter)),
            size,
            ground: TreeScatterGround::Flat,
        }
    }

    /// A preset's Scatter on flat ground, `size` metres across.
    pub fn preset(preset: &TreeScatterPreset, size: Vec2) -> Self {
        Self::new(preset.tree_scatter.clone(), size)
    }

    /// The same Scatter, standing on its parent's surface instead; see
    /// [`TreeScatterGround::Host`].
    pub fn on_host(mut self, filter: TagFilter, cell: f32) -> Self {
        self.ground = TreeScatterGround::Host { filter, cell };
        self
    }
}

/// Where a [`TreeScatterParams`] gets its Scatter.
#[derive(Debug, Clone, PartialEq)]
pub enum TreeScatterSource {
    /// A Scatter held in memory.
    Options(Arc<TreeScatter>),
    /// A Scatter preset loaded from a `.tree_scatter.yml` file. The trees
    /// wait for the file to load, and update when the file changes if Bevy
    /// is watching asset files.
    Asset(Handle<TreeScatterAsset>),
}

/// The ground a [`TreeScatterParams`] stands its trees on.
#[derive(Debug, Clone, PartialEq)]
pub enum TreeScatterGround {
    /// Flat ground at the entity's height.
    Flat,
    /// The surface of the entity's parent, which must have a
    /// [`GeneratedSpecimen`], such as terrain or a rock from another procgena
    /// generator.
    ///
    /// Spawn the grove as a child of that host and leave its own transform
    /// at its default. The ground is the highest surface of the specimen's
    /// first level that `filter` matches, read in the host's own
    /// coordinates with its Y as up and sampled every `cell` metres. Trees
    /// are left out where it has no surface. Sizes and spacings stay in
    /// world metres however the host is scaled, but a host stretched
    /// differently along different axes is rejected with
    /// [`TreeScatterBuildError::NonUniformScale`].
    ///
    /// The grove waits until the host has its specimen, and updates when the
    /// specimen, the host's scale or these settings change. If the host loses
    /// its specimen, the trees are removed.
    Host {
        /// Which parts of the host's specimen count as ground.
        filter: TagFilter,
        /// Spacing of the ground samples in metres. Must be positive.
        cell: f32,
    },
}

/// A tree Scatter preset loaded from a `.tree_scatter.yml` file for
/// [`TreeScatterSource::Asset`].
///
/// The file may name a built-in preset as its `base` and give only what
/// differs; see [`TreeScatterPreset::from_yaml`]. Its key is the file name
/// without `.tree_scatter.yml`. Files with invalid settings fail to load.
#[derive(Asset, TypePath, Debug, Clone, PartialEq)]
pub struct TreeScatterAsset(pub TreeScatterPreset);

/// One tree of a grove, where it was placed.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedTree {
    /// The tree's species: its position in [`TreeScatter::trees`].
    pub tree: usize,
    /// Which of the species' shape variants it is.
    pub variant: u32,
    /// The spot it stands on; see [`TreeCopy::address`]. A spot keeps its
    /// address through edits that leave a tree on it.
    pub address: CopyAddress,
    /// Where the tree stands, and how it is sized, leaned and turned, relative
    /// to the grove entity.
    pub transform: Transform,
    /// The tree's species variant: its position in
    /// [`TreeScatterAssets::variants`].
    pub(crate) slot: u32,
}

/// Every tree of a grove. The plugin adds it to the grove entity when the
/// trees are placed and replaces it whenever they are placed again, so
/// `Changed<TreeScatterInstances>` finds groves whose trees moved.
///
/// Groves draw their trees themselves; the trees are not entities. To give
/// one tree an entity of its own, for a collider or to interact with it, see
/// [`PromotedTree`](crate::PromotedTree).
#[derive(Component, Debug, Clone)]
pub struct TreeScatterInstances {
    trees: Arc<[PlacedTree]>,
    by_address: Arc<HashMap<CopyAddress, u32>>,
}

impl TreeScatterInstances {
    fn new(trees: Vec<PlacedTree>) -> Self {
        let by_address = trees
            .iter()
            .enumerate()
            .map(|(index, tree)| (tree.address, index as u32))
            .collect();
        Self {
            trees: trees.into(),
            by_address: Arc::new(by_address),
        }
    }

    /// The trees, in no particular order.
    pub fn trees(&self) -> &[PlacedTree] {
        &self.trees
    }

    /// The tree standing on the spot at `address`, if any.
    pub fn get(&self, address: &CopyAddress) -> Option<&PlacedTree> {
        self.index_of(address)
            .map(|index| &self.trees[index as usize])
    }

    /// The position of the tree on the spot at `address` in
    /// [`trees`](Self::trees).
    pub(crate) fn index_of(&self, address: &CopyAddress) -> Option<u32> {
        self.by_address.get(address).copied()
    }
}

/// The meshes and materials of a grove's trees: one entry for each species
/// variant that has trees. The plugin adds it with
/// [`TreeScatterInstances`] and updates it as the variants finish generating.
#[derive(Component, Clone)]
pub struct TreeScatterAssets {
    variants: Arc<[TreeScatterVariant]>,
}

impl TreeScatterAssets {
    /// The variants, in the order [`PlacedTree`]s refer to them.
    pub fn variants(&self) -> &[TreeScatterVariant] {
        &self.variants
    }

    /// The variant of `tree`.
    pub fn of(&self, tree: &PlacedTree) -> &TreeScatterVariant {
        &self.variants[tree.slot as usize]
    }

    /// Whether every variant has finished generating, successfully or not.
    fn settled(&self) -> bool {
        self.variants.iter().all(TreeScatterVariant::settled)
    }
}

impl fmt::Debug for TreeScatterAssets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.variants.iter()).finish()
    }
}

/// One species variant of a grove, and what it is drawn with once generated.
#[derive(Clone)]
pub struct TreeScatterVariant {
    tree: usize,
    variant: u32,
    options: Arc<TreeOptions>,
    key: TreeKey,
    generated: Option<Arc<Generated>>,
    failed: bool,
    /// The impostor drawn beyond the farthest level, once baked.
    impostor: Option<TreeImpostorAssets>,
}

impl TreeScatterVariant {
    /// The species: its position in [`TreeScatter::trees`].
    pub fn tree(&self) -> usize {
        self.tree
    }

    /// Which of the species' shape variants this is.
    pub fn variant(&self) -> u32 {
        self.variant
    }

    /// The variant's tree settings.
    pub fn options(&self) -> &Arc<TreeOptions> {
        &self.options
    }

    /// Whether the variant's meshes are ready. A variant that is still
    /// generating, or whose settings were rejected, draws no trees.
    pub fn is_ready(&self) -> bool {
        self.generated.is_some()
    }

    /// The variant's tree as a tagged specimen, in the tree's own coordinates,
    /// once generated. Use it to build colliders.
    pub fn specimen(&self) -> Option<&Arc<Specimen>> {
        self.generated.as_ref().map(|g| &g.specimen)
    }

    /// The parts each detail level draws, nearest level first; empty until
    /// the variant is ready.
    pub fn parts(&self) -> impl Iterator<Item = TreeScatterPart<'_>> {
        self.generated.iter().flat_map(|generated| {
            generated
                .parts()
                .map(|(lod, kind, mesh, material)| TreeScatterPart {
                    lod,
                    kind,
                    mesh: &mesh.mesh,
                    material,
                    bounds: mesh.extent,
                })
        })
    }

    /// Whether the variant is drawn as an impostor far away. Until its
    /// impostor is baked, its farthest level is drawn at every distance
    /// beyond.
    pub fn has_impostor(&self) -> bool {
        self.impostor.is_some()
    }

    /// The variant's impostor, once baked.
    pub(crate) fn impostor(&self) -> Option<&TreeImpostorAssets> {
        self.impostor.as_ref()
    }

    /// Whether generation has ended, successfully or not.
    fn settled(&self) -> bool {
        self.generated.is_some() || self.failed
    }

    /// Whether `cache` has news for the variant: the end of its generation,
    /// or its impostor.
    fn has_news(&self, cache: &TreeCache) -> bool {
        if !self.settled() {
            return !matches!(cache.state(self.key), CacheState::Pending);
        }
        self.generated.is_some() && self.impostor.is_none() && cache.impostor(self.key).is_some()
    }

    /// Fills in the variant's generation result and impostor from `cache`,
    /// where they are ready.
    fn refresh(&mut self, cache: &TreeCache) {
        if !self.settled() {
            match cache.state(self.key) {
                CacheState::Ready(generated) => self.generated = Some(generated.clone()),
                CacheState::Failed => self.failed = true,
                CacheState::Pending => {}
            }
        }
        if self.generated.is_some() && self.impostor.is_none() {
            self.impostor = cache.impostor(self.key).cloned();
        }
    }
}

impl fmt::Debug for TreeScatterVariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TreeScatterVariant")
            .field("tree", &self.tree)
            .field("variant", &self.variant)
            .field("ready", &self.is_ready())
            .field("failed", &self.failed)
            .finish_non_exhaustive()
    }
}

/// One mesh a grove variant draws at one detail level.
#[derive(Debug, Clone, Copy)]
pub struct TreeScatterPart<'a> {
    /// The detail level, from 0 for the nearest.
    pub lod: usize,
    /// What the mesh is.
    pub kind: TreePartKind,
    /// The mesh, in the tree's own coordinates.
    pub mesh: &'a Handle<Mesh>,
    /// Its material.
    pub material: &'a Handle<TreeMaterial>,
    /// Bounds of the mesh as drawn, in the tree's own coordinates, including
    /// every way a camera-facing shape can turn. Wind sway is not included.
    pub bounds: Aabb,
}

/// How much a grove draws: its tree count, and the vertices and triangles
/// of all its trees at each detail level. The plugin adds it when the grove
/// finishes building, before [`TreeScatterFinished`].
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct TreeScatterStats {
    /// Trees in the grove, including any whose variant failed to generate.
    pub trees: usize,
    /// Sizes of every drawable tree at each detail level, nearest first.
    pub levels: Vec<TreeScatterLevelSize>,
}

/// The size of a grove's trees at one detail level.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TreeScatterLevelSize {
    /// Vertices of every tree at this level.
    pub vertices: usize,
    /// Triangles of every tree at this level.
    pub triangles: usize,
}

/// Present on a grove while its trees are being placed or generated,
/// including while it waits for its file or its host.
#[derive(Component, Debug, Default)]
pub struct TreeScatterGenerating;

/// Triggered on a grove entity when a build ends, successfully or not.
///
/// On success every tree of the grove has been placed and generated, and the
/// grove has its [`TreeScatterInstances`], [`TreeScatterAssets`] and
/// [`TreeScatterStats`].
#[derive(EntityEvent, Debug, Clone, PartialEq)]
pub struct TreeScatterFinished {
    /// The grove entity.
    pub entity: Entity,
    /// `Ok` when the trees are in place, or why the grove could not be built.
    pub result: Result<(), TreeScatterBuildError>,
}

/// Why a grove could not be built.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TreeScatterBuildError {
    /// A setting is out of range, or the Scatter asks for too many trees.
    Scatter(TreeScatterError),
    /// No part of the host's specimen matched the ground filter.
    NoGround,
    /// [`TreeScatterGround::Host`]'s `cell` is not positive.
    BadCell,
    /// The grove's host is scaled differently along different axes.
    NonUniformScale,
}

impl fmt::Display for TreeScatterBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scatter(error) => write!(f, "{error}"),
            Self::NoGround => write!(f, "no part of the grove's host matches its ground filter"),
            Self::BadCell => write!(f, "the grove's ground cell size must be positive"),
            Self::NonUniformScale => write!(f, "the grove's host has a non-uniform scale"),
        }
    }
}

impl std::error::Error for TreeScatterBuildError {}

/// Reads and checks tree Scatter preset files with the `.tree_scatter.yml`
/// extension.
#[derive(Debug, Default, TypePath)]
pub(crate) struct TreeScatterLoader;

impl AssetLoader for TreeScatterLoader {
    type Asset = TreeScatterAsset;
    type Settings = ();
    type Error = BevyError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        context: &mut LoadContext<'_>,
    ) -> Result<TreeScatterAsset, BevyError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let file_name = context
            .path()
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let key = file_name
            .strip_suffix(".tree_scatter.yml")
            .unwrap_or(file_name);
        let preset = TreeScatterPreset::from_yaml(key, std::str::from_utf8(&bytes)?)?;
        Ok(TreeScatterAsset(preset))
    }

    fn extensions(&self) -> &[&str] {
        &["tree_scatter.yml"]
    }
}

// --------------------------------------------------------------------------
// Build inputs

/// Everything a grove's placement depends on. When a grove's input differs
/// from its last one, it is placed again.
#[derive(Debug, Clone)]
pub(crate) struct Input {
    grove: Arc<TreeScatter>,
    size: Vec2,
    ground: InputGround,
}

#[derive(Debug, Clone)]
enum InputGround {
    Flat,
    /// A host's specimen, with the host's (uniform) world scale.
    Host {
        filter: TagFilter,
        cell: f32,
        host: Arc<Specimen>,
        scale: f32,
    },
}

impl InputGround {
    /// True when both are the same ground. Host specimens can be large, so
    /// they are compared by identity (the same shared copy), not content.
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Flat, Self::Flat) => true,
            (
                Self::Host {
                    filter: f,
                    cell: c,
                    host: h,
                    scale: s,
                },
                Self::Host {
                    filter: g,
                    cell: d,
                    host: i,
                    scale: t,
                },
            ) => f == g && c == d && Arc::ptr_eq(h, i) && s == t,
            _ => false,
        }
    }

    /// The host's world scale, or 1 for flat ground.
    fn scale(&self) -> f32 {
        match self {
            Self::Flat => 1.0,
            Self::Host { scale, .. } => *scale,
        }
    }
}

impl Input {
    /// True when this input places `grove` at `size` on `ground`.
    fn is(&self, grove: &TreeScatter, size: Vec2, ground: &InputGround) -> bool {
        self.size == size && self.ground.same(ground) && *self.grove == *grove
    }
}

/// On a grove while its placement runs on a background thread.
#[derive(Component)]
pub(crate) struct Placing {
    input: Input,
    task: Task<Result<Vec<TreeCopy>, TreeScatterBuildError>>,
}

/// The input of the grove's last finished placement, successful or not, so
/// it isn't placed again until the input changes.
#[derive(Component)]
pub(crate) struct Placed {
    input: Input,
    /// Whether the placement failed. A failed grove has no trees and has
    /// already reported its error.
    failed: bool,
}

/// What a grove needs this frame.
enum Wanted<'a> {
    /// Nothing yet: its file or its host's specimen isn't available.
    Wait,
    /// Place `grove` at `size` on `ground`, or fail with `error` if given.
    Place {
        grove: GroveRef<'a>,
        size: Vec2,
        ground: InputGround,
        error: Option<TreeScatterBuildError>,
    },
}

/// A grove borrowed from its component or its loaded file.
enum GroveRef<'a> {
    Shared(&'a Arc<TreeScatter>),
    Loaded(&'a TreeScatter),
}

impl GroveRef<'_> {
    fn get(&self) -> &TreeScatter {
        match self {
            Self::Shared(grove) => grove,
            Self::Loaded(grove) => grove,
        }
    }

    fn to_arc(&self) -> Arc<TreeScatter> {
        match self {
            Self::Shared(grove) => Arc::clone(grove),
            Self::Loaded(grove) => Arc::new((*grove).clone()),
        }
    }
}

// --------------------------------------------------------------------------
// Systems

/// Starts a background placement for every grove whose input differs from
/// its running or last finished placement, and marks groves that are
/// waiting.
#[expect(clippy::type_complexity, reason = "Bevy system parameters")]
pub(crate) fn start_groves(
    mut commands: Commands,
    groves: Query<(
        Entity,
        &TreeScatterParams,
        Option<&ChildOf>,
        Option<&Placing>,
        Option<&Placed>,
        Has<TreeScatterGenerating>,
    )>,
    hosts: Query<&GeneratedSpecimen>,
    transforms: TransformHelper,
    assets: Res<Assets<TreeScatterAsset>>,
) {
    for (entity, params, parent, placing, placed, generating) in &groves {
        let want = wanted(params, parent, &hosts, &transforms, &assets);
        let Wanted::Place {
            grove,
            size,
            ground,
            error,
        } = want
        else {
            // While waiting, remove trees laid on a host that lost its
            // specimen.
            if placing.is_some() || placed.is_some() {
                clear_trees(&mut commands, entity);
                commands.entity(entity).remove::<(Placing, Placed)>();
            }
            if !generating {
                commands.entity(entity).insert(TreeScatterGenerating);
            }
            continue;
        };

        // Let a running placement land before starting the next, so a grove
        // edited every frame still updates. Skip groves already placed for
        // this input.
        if placing.is_some() || placed.is_some_and(|p| p.input.is(grove.get(), size, &ground)) {
            continue;
        }
        let input = Input {
            grove: grove.to_arc(),
            size,
            ground,
        };
        if let Some(error) = error {
            fail(&mut commands, entity, input, error);
            continue;
        }
        let task_input = input.clone();
        let task = AsyncComputeTaskPool::get().spawn(async move { place(&task_input) });
        commands
            .entity(entity)
            .insert((TreeScatterGenerating, Placing { input, task }));
    }
}

/// Works out what grove, size and ground `params` asks for this frame.
fn wanted<'a>(
    params: &'a TreeScatterParams,
    parent: Option<&ChildOf>,
    hosts: &Query<&GeneratedSpecimen>,
    transforms: &TransformHelper,
    assets: &'a Assets<TreeScatterAsset>,
) -> Wanted<'a> {
    let grove = match &params.tree_scatter {
        TreeScatterSource::Options(grove) => GroveRef::Shared(grove),
        TreeScatterSource::Asset(handle) => match assets.get(handle) {
            Some(asset) => GroveRef::Loaded(&asset.0.tree_scatter),
            None => return Wanted::Wait,
        },
    };
    let (filter, cell) = match &params.ground {
        TreeScatterGround::Flat => {
            return Wanted::Place {
                grove,
                size: params.size,
                ground: InputGround::Flat,
                error: None,
            };
        }
        TreeScatterGround::Host { filter, cell } => (filter, *cell),
    };

    // Wait for the host's specimen. Work out the host's world scale from its
    // parents, because Bevy may not have updated it yet this frame.
    let host = parent.map(ChildOf::parent);
    let found = host.and_then(|h| {
        Some((
            hosts.get(h).ok()?,
            transforms.compute_global_transform(h).ok()?,
        ))
    });
    let Some((specimen, transform)) = found else {
        return Wanted::Wait;
    };
    let (scale, _, _) = transform.to_scale_rotation_translation();
    let tolerance = scale.abs().max_element() * 1e-4;
    let uniform = scale.x > 0.0
        && (scale.x - scale.y).abs() <= tolerance
        && (scale.x - scale.z).abs() <= tolerance;
    let error = if !uniform {
        Some(TreeScatterBuildError::NonUniformScale)
    } else if !(cell > 0.0 && cell.is_finite()) {
        Some(TreeScatterBuildError::BadCell)
    } else {
        None
    };
    Wanted::Place {
        grove,
        size: params.size,
        ground: InputGround::Host {
            filter: filter.clone(),
            cell,
            host: specimen.0.clone(),
            scale: scale.x,
        },
        error,
    }
}

/// Removes a grove's trees, records `input` as attempted, and reports
/// `error`.
fn fail(commands: &mut Commands, entity: Entity, input: Input, error: TreeScatterBuildError) {
    error!("procgena_tree_bevy: cannot build grove: {error}");
    clear_trees(commands, entity);
    commands
        .entity(entity)
        .remove::<(Placing, TreeScatterGenerating)>()
        .insert(Placed {
            input,
            failed: true,
        });
    commands.trigger(TreeScatterFinished {
        entity,
        result: Err(error),
    });
}

/// Ground read from a host's specimen in the host's units, given in world
/// metres by multiplying by the host's scale.
struct ScaledGround {
    grid: HeightGrid,
    scale: f32,
}

impl Ground for ScaledGround {
    fn height(&self, p: DVec2) -> f32 {
        self.grid.height(p / f64::from(self.scale)) * self.scale
    }

    // Scaling both the positions and the heights leaves the slope as it is.
    fn gradient(&self, p: DVec2) -> Vec2 {
        self.grid.gradient(p / f64::from(self.scale))
    }
}

/// Places the grove described by `input`, in metres from the grove entity,
/// with the grove's own seed. Runs on a background thread.
fn place(input: &Input) -> Result<Vec<TreeCopy>, TreeScatterBuildError> {
    // The grove entity's transform puts the area's centre in the world.
    let area = ScatterArea::new(DVec2::ZERO, input.size);
    let at = |ground: &dyn Ground| {
        TreeScatterPlacement::new(
            &input.grove,
            &ScatterInput::new(ground, area, input.grove.default_seed),
        )
    };
    let placement = match &input.ground {
        InputGround::Flat => at(&FlatGround),
        InputGround::Host {
            filter,
            cell,
            host,
            scale,
        } => {
            let grid = host
                .height_grid(0, filter, cell / scale)
                .ok_or(TreeScatterBuildError::NoGround)?;
            let ground = ScaledGround {
                grid,
                scale: *scale,
            };
            at(&ground)
        }
    };
    placement
        .map(|p| p.instances().to_vec())
        .map_err(TreeScatterBuildError::Scatter)
}

/// Gives each grove whose placement finished its new trees, and starts
/// generating any species variant that isn't cached yet.
pub(crate) fn finish_placing(
    mut commands: Commands,
    mut placing: Query<(Entity, &mut Placing, Option<&TreeScatterAssets>)>,
    mut cache: ResMut<TreeCache>,
    settings: Res<SpawnSettings>,
) {
    for (entity, mut p, old_assets) in &mut placing {
        let Some(result) = block_on(poll_once(&mut p.task)) else {
            continue;
        };
        let input = p.input.clone();
        let instances = match result {
            Ok(instances) => instances,
            Err(error) => {
                fail(&mut commands, entity, input, error);
                continue;
            }
        };
        commands.entity(entity).remove::<Placing>().insert(Placed {
            input: input.clone(),
            failed: false,
        });

        // One variant entry for each species variant that has trees. A
        // variant the grove had before keeps its generated meshes.
        let mut slots: HashMap<(usize, u32), u32> = HashMap::new();
        let mut variants: Vec<TreeScatterVariant> = Vec::new();
        let shrink = 1.0 / input.ground.scale();
        let trees = instances
            .into_iter()
            .map(|instance| {
                let pair = (instance.tree, instance.variant);
                let slot = *slots.entry(pair).or_insert_with(|| {
                    let options =
                        Arc::new(input.grove.trees[instance.tree].variant(instance.variant));
                    let key = cache.request(&options, &settings);
                    let kept = old_assets
                        .and_then(|old| old.variants.iter().find(|v| v.key == key && v.settled()));
                    variants.push(TreeScatterVariant {
                        tree: instance.tree,
                        variant: instance.variant,
                        options,
                        key,
                        generated: kept.and_then(|v| v.generated.clone()),
                        failed: kept.is_some_and(|v| v.failed),
                        impostor: kept.and_then(|v| v.impostor.clone()),
                    });
                    variants.len() as u32 - 1
                });
                PlacedTree {
                    tree: instance.tree,
                    variant: instance.variant,
                    address: instance.address,
                    transform: Transform::from_matrix(
                        Mat4::from_scale(Vec3::splat(shrink)) * Mat4::from(instance.transform),
                    ),
                    slot,
                }
            })
            .collect();

        let keys = variants.iter().map(|v| v.key).collect();
        commands.entity(entity).insert((
            TreeScatterInstances::new(trees),
            TreeScatterAssets {
                variants: variants.into(),
            },
            HeldTreeKeys(keys),
        ));
    }
}

/// Fills in each grove's variants as they finish generating and as their
/// impostors are baked.
pub(crate) fn refresh_grove_assets(
    mut groves: Query<&mut TreeScatterAssets>,
    cache: Res<TreeCache>,
) {
    for mut assets in &mut groves {
        if !assets.variants.iter().any(|v| v.has_news(&cache)) {
            continue;
        }
        let mut variants = assets.variants.to_vec();
        for variant in &mut variants {
            variant.refresh(&cache);
        }
        assets.variants = variants.into();
    }
}

/// Ends the build of each placed grove once every variant has finished
/// generating: adds its [`TreeScatterStats`] and reports it with
/// [`TreeScatterFinished`].
#[expect(clippy::type_complexity, reason = "Bevy system parameters")]
pub(crate) fn finish_groves(
    mut commands: Commands,
    groves: Query<
        (Entity, &Placed, &TreeScatterInstances, &TreeScatterAssets),
        (With<TreeScatterGenerating>, Without<Placing>),
    >,
    meshes: Res<Assets<Mesh>>,
) {
    for (entity, placed, instances, assets) in &groves {
        if placed.failed || !assets.settled() {
            continue;
        }
        commands
            .entity(entity)
            .remove::<TreeScatterGenerating>()
            .insert(grove_stats(instances, assets, &meshes));
        commands.trigger(TreeScatterFinished {
            entity,
            result: Ok(()),
        });
    }
}

/// Counts the trees of a grove and the vertices and triangles they draw at
/// each detail level.
fn grove_stats(
    instances: &TreeScatterInstances,
    assets: &TreeScatterAssets,
    meshes: &Assets<Mesh>,
) -> TreeScatterStats {
    let mut counts = vec![0usize; assets.variants.len()];
    for tree in instances.trees() {
        counts[tree.slot as usize] += 1;
    }
    let mut levels: Vec<TreeScatterLevelSize> = Vec::new();
    for (variant, count) in assets.variants.iter().zip(counts) {
        for part in variant.parts() {
            let Some(mesh) = meshes.get(part.mesh) else {
                continue;
            };
            if levels.len() <= part.lod {
                levels.resize(part.lod + 1, TreeScatterLevelSize::default());
            }
            let level = &mut levels[part.lod];
            level.vertices += count * mesh.count_vertices();
            level.triangles += count * mesh.indices().map_or(0, |i| i.len() / 3);
        }
    }
    TreeScatterStats {
        trees: instances.trees().len(),
        levels,
    }
}

/// Removes the trees and build state of groves that lost their
/// [`TreeScatterParams`].
pub(crate) fn remove_groves(
    mut commands: Commands,
    mut removed: RemovedComponents<TreeScatterParams>,
) {
    for entity in removed.read() {
        if commands.get_entity(entity).is_ok() {
            clear_trees(&mut commands, entity);
            commands
                .entity(entity)
                .remove::<(Placing, Placed, TreeScatterGenerating)>();
        }
    }
}

/// Removes a grove's trees.
fn clear_trees(commands: &mut Commands, entity: Entity) {
    commands.entity(entity).remove::<(
        TreeScatterInstances,
        TreeScatterAssets,
        TreeScatterStats,
        HeldTreeKeys,
    )>();
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::time::{Duration, Instant};

    use procgena_core::CellId;
    use procgena_core::{Lod, LodLevel};
    use procgena_core_bevy::LodQuality;
    use procgena_specimen::{Part, SurfaceMesh, tags};
    use procgena_tree::{
        AddressPrefix, LOD_REFERENCE_SIZE, LodDetail, ScatterTree, TextureCompression, TextureSize,
        TreePreset,
    };

    use super::*;
    use crate::spawn::{
        BarkTexture, LeafTexture, TreeGenerating, TreeLevel, TreeParams, TreePart, TreePresetAsset,
        finish_generation, prune_cache, reset_changed_trees, spawn_parts, start_generation,
    };
    use crate::wind::Wind;

    /// Results of every [`TreeScatterFinished`] event, in order.
    #[derive(Resource, Default)]
    struct Finished(Vec<Result<(), TreeScatterBuildError>>);

    /// An app with one full-detail level and the grove and tree systems in
    /// the plugin's order, without rendering or loading textures.
    fn grove_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<StandardMaterial>()
            .init_asset::<TreeMaterial>()
            .init_asset::<TreePresetAsset>()
            .init_asset::<TreeScatterAsset>()
            .init_resource::<Wind>()
            .init_resource::<TreeCache>()
            .init_resource::<Finished>()
            .init_resource::<LodQuality>()
            .insert_resource(SpawnSettings {
                texture_root: None,
                texture_compression: TextureCompression::Bc7,
                lod: Lod::new(
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
                ),
                impostor: None,
            })
            .add_systems(
                Update,
                (
                    remove_groves,
                    finish_placing,
                    start_groves,
                    stop_preparing,
                    start_preparing,
                    sync_promoted_trees,
                    demote_trees,
                    reset_changed_trees,
                    start_generation,
                    finish_generation,
                    spawn_parts,
                    refresh_grove_assets,
                    prune_cache,
                    finish_groves,
                    track_preparing,
                )
                    .chain(),
            )
            .add_observer(
                |event: On<TreeScatterFinished>, mut finished: ResMut<Finished>| {
                    finished.0.push(event.result.clone());
                },
            );
        app
    }

    /// A small grove of two quick-to-build species, two variants each.
    fn small_grove() -> TreeScatter {
        TreeScatter::new(vec![
            ScatterTree::new(TreePreset::bush_2())
                .with_variants(2)
                .scaled(0.4),
            ScatterTree::new(TreePreset::pine_small())
                .with_variants(2)
                .scaled(0.2),
        ])
    }

    const SIZE: Vec2 = Vec2::new(12.0, 12.0);

    /// Updates the app until `grove` loses [`TreeScatterGenerating`].
    fn update_until_finished(app: &mut App, grove: Entity) {
        let deadline = Instant::now() + Duration::from_secs(60);
        app.update();
        while app
            .world()
            .entity(grove)
            .contains::<TreeScatterGenerating>()
        {
            assert!(Instant::now() < deadline, "grove did not finish");
            std::thread::sleep(Duration::from_millis(1));
            app.update();
        }
    }

    /// A placed tree and the generated variant it is drawn from, compared by
    /// identity. It holds the variant, so its address can't be reused by a
    /// later one while the test still compares against it.
    #[derive(Clone)]
    struct Drawn {
        tree: PlacedTree,
        generated: Option<*const Generated>,
        _held: Option<Arc<Generated>>,
    }

    impl PartialEq for Drawn {
        fn eq(&self, other: &Self) -> bool {
            self.tree == other.tree && self.generated == other.generated
        }
    }

    impl fmt::Debug for Drawn {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("Drawn")
                .field("tree", &self.tree)
                .field("generated", &self.generated)
                .finish()
        }
    }

    /// The grove's trees by the spot they stand on.
    fn trees(app: &App, grove: Entity) -> HashMap<CopyAddress, Drawn> {
        let entity = app.world().entity(grove);
        let (Some(instances), Some(assets)) = (
            entity.get::<TreeScatterInstances>(),
            entity.get::<TreeScatterAssets>(),
        ) else {
            return HashMap::new();
        };
        instances
            .trees()
            .iter()
            .map(|tree| {
                let held = assets.of(tree).generated.clone();
                let drawn = Drawn {
                    tree: tree.clone(),
                    generated: held.as_ref().map(Arc::as_ptr),
                    _held: held,
                };
                (tree.address, drawn)
            })
            .collect()
    }

    fn set_grove(app: &mut App, entity: Entity, grove: TreeScatter) {
        app.world_mut()
            .get_mut::<TreeScatterParams>(entity)
            .unwrap()
            .tree_scatter = TreeScatterSource::Options(Arc::new(grove));
    }

    #[test]
    fn a_grove_places_its_trees_and_shares_each_variants_meshes() {
        let mut app = grove_app();
        let grove = small_grove();
        let entity = app
            .world_mut()
            .spawn(TreeScatterParams::new(grove.clone(), SIZE))
            .id();
        update_until_finished(&mut app, entity);
        assert_eq!(app.world().resource::<Finished>().0, vec![Ok(())]);

        // Every placed tree, and no tree entities.
        let area = ScatterArea::new(DVec2::ZERO, SIZE);
        let input = ScatterInput::new(&FlatGround, area, grove.default_seed);
        let placed = TreeScatterPlacement::new(&grove, &input).unwrap();
        let drawn = trees(&app, entity);
        assert!(drawn.len() > 4);
        assert_eq!(drawn.len(), placed.instances().len());
        assert!(drawn.values().all(|d| d.generated.is_some()));
        assert!(app.world().entity(entity).get::<Children>().is_none());

        // At most two variants of each of two species: four shared variants.
        let assets = app.world().get::<TreeScatterAssets>(entity).unwrap();
        assert!(assets.variants().len() <= 4);
        let distinct: HashSet<_> = drawn.values().map(|d| d.generated).collect();
        assert_eq!(distinct.len(), assets.variants().len());
        for variant in assets.variants() {
            let parts: Vec<_> = variant.parts().collect();
            assert!(!parts.is_empty() && variant.specimen().is_some());
            assert!(
                parts
                    .iter()
                    .all(|p| p.bounds.half_extents.max_element() > 0.0)
            );
        }

        // The stats count every tree's vertices at the single detail level.
        let stats = app.world().get::<TreeScatterStats>(entity).unwrap();
        assert_eq!(stats.trees, drawn.len());
        assert_eq!(stats.levels.len(), 1);
        let meshes = app.world().resource::<Assets<Mesh>>();
        let instances = app.world().get::<TreeScatterInstances>(entity).unwrap();
        let vertices: usize = instances
            .trees()
            .iter()
            .flat_map(|tree| assets.of(tree).parts())
            .map(|part| meshes.get(part.mesh).unwrap().count_vertices())
            .sum();
        assert_eq!(stats.levels[0].vertices, vertices);
        assert!(stats.levels[0].triangles > 0);
    }

    #[test]
    fn a_grove_keeps_its_variants_cached_until_it_is_removed() {
        let mut app = grove_app();
        let entity = app
            .world_mut()
            .spawn(TreeScatterParams::new(small_grove(), SIZE))
            .id();
        update_until_finished(&mut app, entity);
        let keys = app.world().get::<HeldTreeKeys>(entity).unwrap().clone();
        let cached = |app: &App| {
            let cache = app.world().resource::<TreeCache>();
            keys.0
                .iter()
                .filter(|key| matches!(cache.state(**key), CacheState::Ready(_)))
                .count()
        };
        assert_eq!(cached(&app), keys.0.len());

        // A single tree coming and going prunes the cache; the grove's
        // variants stay.
        let tree = app
            .world_mut()
            .spawn(TreeParams::preset(TreePreset::bush_1()))
            .id();
        app.update();
        app.world_mut().entity_mut(tree).despawn();
        app.update();
        assert_eq!(cached(&app), keys.0.len());

        // Removing the grove lets them go.
        app.world_mut()
            .entity_mut(entity)
            .remove::<TreeScatterParams>();
        app.update();
        app.update();
        assert_eq!(cached(&app), 0);
    }

    #[test]
    fn lowering_density_keeps_the_remaining_trees_untouched() {
        let mut app = grove_app();
        let entity = app
            .world_mut()
            .spawn(TreeScatterParams::new(small_grove(), SIZE))
            .id();
        update_until_finished(&mut app, entity);
        let before = trees(&app, entity);

        let sparse = small_grove().with_density(small_grove().density() * 0.5);
        set_grove(&mut app, entity, sparse);
        update_until_finished(&mut app, entity);
        let after = trees(&app, entity);

        assert!(!after.is_empty() && after.len() < before.len());
        for (id, tree) in &after {
            assert_eq!(before.get(id), Some(tree), "a kept tree changed");
        }
        assert_eq!(app.world().resource::<Finished>().0.len(), 2);
    }

    #[test]
    fn changing_one_species_rebuilds_only_its_trees() {
        let mut app = grove_app();
        let entity = app
            .world_mut()
            .spawn(TreeScatterParams::new(small_grove(), SIZE))
            .id();
        update_until_finished(&mut app, entity);
        let before = trees(&app, entity);

        let mut changed = small_grove();
        changed.trees[1].tree.default_seed += 1;
        set_grove(&mut app, entity, changed);
        update_until_finished(&mut app, entity);
        let after = trees(&app, entity);

        assert_eq!(after.len(), before.len());
        for (id, drawn) in &after {
            let old = &before[id];
            assert_eq!(
                drawn.tree.transform, old.tree.transform,
                "trees keep their spot"
            );
            if drawn.tree.tree == 0 {
                assert_eq!(
                    drawn.generated, old.generated,
                    "the unchanged species was rebuilt"
                );
            } else {
                assert_ne!(
                    drawn.generated, old.generated,
                    "the changed species kept its meshes"
                );
            }
        }
    }

    #[test]
    fn a_grove_edited_every_frame_keeps_updating() {
        // A wide, sparse grove, so each placement visits many sites and
        // takes longer than a frame.
        let wide = Vec2::splat(300.0);
        let sparse = |per_hectare: f32| small_grove().with_density(per_hectare);
        let mut app = grove_app();
        let entity = app
            .world_mut()
            .spawn(TreeScatterParams::new(sparse(40.0), wide))
            .id();
        update_until_finished(&mut app, entity);
        let first: HashSet<CopyAddress> = trees(&app, entity).into_keys().collect();

        // Drag the density down a little every frame, as a slider would.
        let mut changed = false;
        for step in 1..=40 {
            set_grove(&mut app, entity, sparse(40.0 - step as f32 * 0.5));
            app.update();
            let now: HashSet<CopyAddress> = trees(&app, entity).into_keys().collect();
            changed |= now != first;
        }
        assert!(
            changed,
            "the trees never changed while the grove was being edited"
        );
    }

    #[test]
    fn a_new_host_specimen_moves_the_trees_and_a_missing_surface_is_reported() {
        let mut app = grove_app();
        let level = |height: f32| {
            let mut ground = SurfaceMesh::ground(16.0, 4);
            ground.positions.iter_mut().for_each(|p| p[1] = height);
            GeneratedSpecimen(Arc::new(Specimen::single(vec![Part::new(
                [tags::GROUND],
                ground,
            )])))
        };
        let host = app
            .world_mut()
            .spawn((level(0.0), Transform::default()))
            .id();
        let entity = app
            .world_mut()
            .spawn((
                TreeScatterParams::new(small_grove(), SIZE)
                    .on_host(TagFilter::any_of([tags::GROUND]), 0.5),
                ChildOf(host),
            ))
            .id();
        update_until_finished(&mut app, entity);
        let height = |app: &mut App| {
            let drawn = trees(app, entity);
            drawn.values().next().unwrap().tree.transform.translation.y
        };
        assert!(height(&mut app) < 0.0);

        // A new specimen on the host places the trees again, on its surface.
        app.world_mut().entity_mut(host).insert(level(3.0));
        update_until_finished(&mut app, entity);
        assert!((height(&mut app) - (3.0 - small_grove().variation.sink)).abs() < 1e-3);

        // A filter that matches nothing has no ground.
        app.world_mut()
            .get_mut::<TreeScatterParams>(entity)
            .unwrap()
            .ground = TreeScatterGround::Host {
            filter: TagFilter::any_of([tags::BARK]),
            cell: 0.5,
        };
        update_until_finished(&mut app, entity);
        assert_eq!(
            app.world().resource::<Finished>().0.last(),
            Some(&Err(TreeScatterBuildError::NoGround))
        );
        assert!(trees(&app, entity).is_empty());
    }

    #[test]
    fn setting_the_same_grove_again_does_nothing() {
        let mut app = grove_app();
        let entity = app
            .world_mut()
            .spawn(TreeScatterParams::new(small_grove(), SIZE))
            .id();
        update_until_finished(&mut app, entity);
        let before = trees(&app, entity);

        // An equal grove in a new copy, and a plain change notification.
        set_grove(&mut app, entity, small_grove());
        app.update();
        app.world_mut()
            .get_mut::<TreeScatterParams>(entity)
            .unwrap()
            .set_changed();
        app.update();
        assert!(
            !app.world()
                .entity(entity)
                .contains::<TreeScatterGenerating>()
        );
        assert_eq!(trees(&app, entity), before);
        assert_eq!(app.world().resource::<Finished>().0.len(), 1);
    }

    #[test]
    fn host_ground_waits_for_its_specimen_and_follows_its_scale() {
        let mut app = grove_app();
        let host = app
            .world_mut()
            .spawn((
                Transform::from_scale(Vec3::splat(2.0)),
                Visibility::default(),
            ))
            .id();
        let entity = app
            .world_mut()
            .spawn((
                TreeScatterParams::new(small_grove(), SIZE)
                    .on_host(TagFilter::any_of([tags::GROUND]), 0.25),
                ChildOf(host),
            ))
            .id();
        app.update();
        assert!(
            app.world()
                .entity(entity)
                .contains::<TreeScatterGenerating>()
        );
        assert!(trees(&app, entity).is_empty());

        // A level square 1 unit up and 8 units across: 16 m wide and 2 m up
        // in the world, enough for the whole 12 m grove.
        let mut ground = SurfaceMesh::ground(8.0, 8);
        for p in &mut ground.positions {
            p[1] += 1.0;
        }
        let specimen = Specimen::single(vec![Part::new([tags::GROUND], ground)]);
        app.world_mut()
            .entity_mut(host)
            .insert(GeneratedSpecimen(Arc::new(specimen)));
        update_until_finished(&mut app, entity);
        let placed = trees(&app, entity);
        assert!(!placed.is_empty());

        // Trees stand on the surface, sunk 0.1 m (0.05 host units) into it,
        // at their own world size: in the half-scale host, at most half the
        // largest size variation allows.
        let variation = small_grove().variation;
        let largest = 1.0 + 3.0 * variation.scale_jitter;
        for drawn in placed.values() {
            let transform = &drawn.tree.transform;
            let sunk = 1.0 - variation.sink / 2.0;
            assert!((transform.translation.y - sunk).abs() < 1e-3);
            assert!(transform.scale.x <= 0.5 * largest + 1e-4);
        }

        // Losing the specimen removes the trees.
        app.world_mut()
            .entity_mut(host)
            .remove::<GeneratedSpecimen>();
        app.update();
        app.update();
        assert!(trees(&app, entity).is_empty());
        assert!(
            app.world()
                .entity(entity)
                .contains::<TreeScatterGenerating>()
        );
    }

    #[test]
    fn a_bad_ground_cell_is_reported() {
        let mut app = grove_app();
        let ground = SurfaceMesh::ground(8.0, 2);
        let specimen = Specimen::single(vec![Part::new([tags::GROUND], ground)]);
        let host = app
            .world_mut()
            .spawn((GeneratedSpecimen(Arc::new(specimen)), Transform::default()))
            .id();
        let entity = app
            .world_mut()
            .spawn((
                TreeScatterParams::new(small_grove(), SIZE)
                    .on_host(TagFilter::any_of([tags::GROUND]), 0.0),
                ChildOf(host),
            ))
            .id();
        app.update();
        assert!(
            !app.world()
                .entity(entity)
                .contains::<TreeScatterGenerating>()
        );
        assert_eq!(
            app.world().resource::<Finished>().0,
            vec![Err(TreeScatterBuildError::BadCell)]
        );
    }

    #[test]
    fn removing_the_component_removes_the_trees() {
        let mut app = grove_app();
        let entity = app
            .world_mut()
            .spawn(TreeScatterParams::new(small_grove(), SIZE))
            .id();
        update_until_finished(&mut app, entity);
        assert!(!trees(&app, entity).is_empty());

        app.world_mut()
            .entity_mut(entity)
            .remove::<TreeScatterParams>();
        app.update();
        assert!(trees(&app, entity).is_empty());
    }

    #[test]
    fn a_grove_asset_places_trees_and_follows_its_changes() {
        let mut app = grove_app();
        let handle = app
            .world_mut()
            .resource_mut::<Assets<TreeScatterAsset>>()
            .add(TreeScatterAsset(TreeScatterPreset {
                key: "small".into(),
                name: "Small".into(),
                description: "".into(),
                group: "File".into(),
                tree_scatter: small_grove(),
            }));
        let entity = app
            .world_mut()
            .spawn(TreeScatterParams {
                tree_scatter: TreeScatterSource::Asset(handle.clone()),
                size: SIZE,
                ground: TreeScatterGround::Flat,
            })
            .id();
        update_until_finished(&mut app, entity);
        let full = trees(&app, entity).len();
        assert!(full > 0);

        // Editing the loaded grove updates the trees.
        let sparse = small_grove().with_density(small_grove().density() * 0.5);
        app.world_mut()
            .resource_mut::<Assets<TreeScatterAsset>>()
            .get_mut(&handle)
            .unwrap()
            .0
            .tree_scatter = sparse;
        update_until_finished(&mut app, entity);
        assert!(trees(&app, entity).len() < full);
    }

    #[test]
    fn scatter_files_load_partial_settings() {
        // Write a partial Scatter file into a fresh asset folder.
        let dir = std::env::temp_dir().join(format!(
            "procgena_tree_bevy_scatter_assets_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("woods.tree_scatter.yml"),
            "base: pine_stand\ntree_scatter:\n  default_seed: 9\n  patchiness: 0.5\n",
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
        .init_asset::<TreeScatterAsset>()
        .register_asset_loader(TreeScatterLoader);
        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<TreeScatterAsset>("woods.tree_scatter.yml");
        let deadline = Instant::now() + Duration::from_secs(30);
        while !app
            .world()
            .resource::<Assets<TreeScatterAsset>>()
            .contains(&handle)
        {
            assert!(Instant::now() < deadline, "the Scatter file did not load");
            std::thread::sleep(Duration::from_millis(1));
            app.update();
        }
        std::fs::remove_dir_all(&dir).unwrap();

        // Omitted settings keep the base's.
        let mut expected = TreeScatterPreset::pine_stand().tree_scatter.clone();
        expected.default_seed = 9;
        expected.patchiness = 0.5;
        let assets = app.world().resource::<Assets<TreeScatterAsset>>();
        let preset = &assets.get(&handle).unwrap().0;
        assert_eq!(preset.tree_scatter, expected);
        assert_eq!(preset.key, "woods");
    }

    /// Marks a host's own component on a promoted tree.
    #[derive(Component)]
    struct Collider;

    /// Whether `entity` has tree parts as children.
    fn has_parts(app: &App, entity: Entity) -> bool {
        let world = app.world();
        world
            .get::<Children>(entity)
            .is_some_and(|kids| kids.iter().any(|c| world.entity(c).contains::<TreePart>()))
    }

    /// A finished small grove and its trees.
    fn finished_grove(app: &mut App) -> (Entity, HashMap<CopyAddress, Drawn>) {
        let grove = app
            .world_mut()
            .spawn(TreeScatterParams::new(small_grove(), SIZE))
            .id();
        update_until_finished(app, grove);
        let drawn = trees(app, grove);
        (grove, drawn)
    }

    #[test]
    fn a_promoted_tree_is_an_ordinary_tree_until_it_is_demoted() {
        let mut app = grove_app();
        let (grove, drawn) = finished_grove(&mut app);
        let tree = drawn.values().next().unwrap().tree.clone();
        let promoted = app
            .world_mut()
            .spawn((PromotedTree::new(grove, tree.address), Collider))
            .id();
        app.update();

        // The entity is the grove's tree, with its parts, in the grove.
        let entity = app.world().entity(promoted);
        let assets = app.world().get::<TreeScatterAssets>(grove).unwrap();
        let options = assets.of(&tree).options();
        assert!(matches!(entity.get::<TreeParams>(), Some(TreeParams::Options(o)) if o == options));
        assert_eq!(entity.get::<Transform>(), Some(&tree.transform));
        assert_eq!(entity.get::<ChildOf>().map(ChildOf::parent), Some(grove));
        assert!(!entity.contains::<TreeGenerating>());
        assert!(has_parts(&app, promoted));

        // Demoting removes the tree but keeps the host's components.
        app.world_mut()
            .entity_mut(promoted)
            .remove::<PromotedTree>();
        app.update();
        let entity = app.world().entity(promoted);
        assert!(entity.contains::<Collider>());
        assert!(!entity.contains::<TreeParams>());
        assert!(!has_parts(&app, promoted));
    }

    #[test]
    fn promoting_a_spot_without_a_tree_is_reported_and_keeps_the_entity() {
        let mut app = grove_app();
        let (grove, drawn) = finished_grove(&mut app);
        let missing = (0..)
            .map(|x| AddressPrefix::ALONE.copy(CellId::new(x, 0), 0))
            .find(|address| !drawn.contains_key(address))
            .unwrap();
        let promoted = app
            .world_mut()
            .spawn((PromotedTree::new(grove, missing), Collider))
            .id();
        app.update();
        let entity = app.world().entity(promoted);
        assert!(entity.contains::<Collider>());
        assert!(!entity.contains::<PromotedTree>() && !entity.contains::<TreeParams>());
    }

    #[test]
    fn promoted_trees_follow_their_grove() {
        let mut app = grove_app();
        let (grove, before) = finished_grove(&mut app);

        // Promote every tree.
        let promoted: Vec<(CopyAddress, Entity)> = before
            .values()
            .map(|d| {
                let id = d.tree.address;
                (id, app.world_mut().spawn(PromotedTree::new(grove, id)).id())
            })
            .collect();
        app.update();

        // Thinning the grove despawns the promoted trees whose spot emptied
        // and keeps the rest.
        let sparse = small_grove().with_density(small_grove().density() * 0.5);
        set_grove(&mut app, grove, sparse.clone());
        update_until_finished(&mut app, grove);
        let after = trees(&app, grove);
        let (kept, gone): (Vec<_>, Vec<_>) =
            promoted.iter().partition(|(id, _)| after.contains_key(id));
        assert!(!kept.is_empty() && !gone.is_empty());
        assert!(
            gone.iter()
                .all(|(_, e)| app.world().get_entity(*e).is_err())
        );
        assert!(kept.iter().all(|(_, e)| app.world().get_entity(*e).is_ok()));

        // A new shape for a species with promoted trees left reaches them.
        let species = after[&kept[0].0].tree.tree;
        let mut reshaped = sparse;
        reshaped.trees[species].tree.default_seed += 1;
        set_grove(&mut app, grove, reshaped);
        update_until_finished(&mut app, grove);
        app.update();
        let assets = app.world().get::<TreeScatterAssets>(grove).unwrap().clone();
        let instances = app
            .world()
            .get::<TreeScatterInstances>(grove)
            .unwrap()
            .clone();
        for (id, entity) in kept {
            let options = assets.of(instances.get(&id).unwrap()).options();
            let params = app.world().get::<TreeParams>(entity);
            assert!(matches!(params, Some(TreeParams::Options(o)) if o == options));
            assert!(has_parts(&app, entity));
        }
    }

    #[test]
    fn grove_trees_are_found_by_ray_and_by_distance() {
        use bevy::ecs::system::RunSystemOnce;

        let mut app = grove_app();
        let (grove, drawn) = finished_grove(&mut app);
        let target = drawn.values().next().unwrap().tree.clone();
        let base = target.transform.translation;

        let (down, up, near) = app
            .world_mut()
            .run_system_once(move |trees: TreeScatterTrees| {
                let above = base + Vec3::Y * 100.0;
                let down = trees.ray_cast(Ray3d::new(above, Dir3::NEG_Y));
                let up = trees.ray_cast(Ray3d::new(above, Dir3::Y));
                let near: Vec<CopyAddress> =
                    trees.within(base, 3.0).map(|(_, t)| t.address).collect();
                (down, up, near)
            })
            .unwrap();

        // Straight down onto a tree hits a tree over that spot; straight up
        // from above every crown hits nothing.
        let hit = down.expect("a ray down onto a tree hits");
        assert_eq!(hit.grove, grove);
        assert!(
            hit.point.y > base.y && (hit.distance - (100.0 - hit.point.y + base.y)).abs() < 1e-3
        );
        assert!(up.is_none());

        // The trees within 3 m of the tree's base, the tree included.
        let expected: HashSet<CopyAddress> = drawn
            .values()
            .filter(|d| d.tree.transform.translation.distance(base) <= 3.0)
            .map(|d| d.tree.address)
            .collect();
        assert!(expected.contains(&target.address));
        assert_eq!(near.into_iter().collect::<HashSet<_>>(), expected);
    }

    /// Every [`TreeScatterReady`] event's entity, in order.
    #[derive(Resource, Default)]
    struct Ready(Vec<Entity>);

    /// Spawns a [`PrepareTreeScatter`] for `grove` and updates until it reports
    /// ready, returning its entity and the keys of the trees it holds.
    fn prepare(app: &mut App, grove: TreeScatter) -> (Entity, Vec<TreeKey>) {
        app.init_resource::<Ready>().add_observer(
            |event: On<TreeScatterReady>, mut ready: ResMut<Ready>| {
                ready.0.push(event.entity);
            },
        );
        let entity = app.world_mut().spawn(PrepareTreeScatter::new(grove)).id();
        let deadline = Instant::now() + Duration::from_secs(60);
        app.update();
        while !app.world().resource::<Ready>().0.contains(&entity) {
            assert!(Instant::now() < deadline, "grove was not prepared");
            std::thread::sleep(Duration::from_millis(1));
            app.update();
        }
        let keys = app
            .world()
            .get::<PreparedTrees>(entity)
            .unwrap()
            .keys
            .clone();
        (entity, keys)
    }

    /// How many of `keys` are generated and cached.
    fn cached(app: &App, keys: &[TreeKey]) -> usize {
        let cache = app.world().resource::<TreeCache>();
        keys.iter()
            .filter(|key| matches!(cache.state(**key), CacheState::Ready(_)))
            .count()
    }

    #[test]
    fn grove_variants_lists_every_variant_of_every_species() {
        let grove = small_grove();
        let variants = tree_scatter_variants(&grove);
        assert_eq!(variants.len(), 4);
        let ids: HashSet<_> = variants.iter().map(|v| v.cache_key()).collect();
        assert_eq!(ids.len(), 4, "variants differ");
        assert_eq!(*variants[1], grove.trees[0].variant(1));
        assert_eq!(*variants[2], grove.trees[1].variant(0));
    }

    #[test]
    fn a_prepared_grove_is_placed_from_the_prepared_trees() {
        let mut app = grove_app();
        let (preparer, prepared) = prepare(&mut app, small_grove());

        // Every variant is generated; with impostors off, that's all.
        let progress = *app.world().get::<TreeScatterPreparing>(preparer).unwrap();
        assert_eq!(progress, TreeScatterPreparing { done: 4, total: 4 });
        assert_eq!(cached(&app, &prepared), 4);

        // The grove's trees are all among them, so nothing new is generated.
        let grove = app
            .world_mut()
            .spawn(TreeScatterParams::new(small_grove(), SIZE))
            .id();
        update_until_finished(&mut app, grove);
        let held = app.world().get::<HeldTreeKeys>(grove).unwrap().0.clone();
        assert!(!held.is_empty());
        assert!(held.iter().all(|key| prepared.contains(key)));

        // Despawning the preparer keeps the trees the grove uses.
        app.world_mut().entity_mut(preparer).despawn();
        app.update();
        assert_eq!(cached(&app, &held), held.len());
    }

    #[test]
    fn prepared_trees_stay_cached_until_the_component_is_removed() {
        let mut app = grove_app();
        let (preparer, prepared) = prepare(&mut app, small_grove());

        // A single tree coming and going prunes the cache; the prepared
        // trees stay.
        let tree = app
            .world_mut()
            .spawn(TreeParams::preset(TreePreset::bush_1()))
            .id();
        app.update();
        app.world_mut().entity_mut(tree).despawn();
        app.update();
        assert_eq!(cached(&app, &prepared), prepared.len());

        // Removing the component lets them go, and its progress with them.
        app.world_mut()
            .entity_mut(preparer)
            .remove::<PrepareTreeScatter>();
        app.update();
        app.update();
        assert_eq!(cached(&app, &prepared), 0);
        assert!(
            !app.world()
                .entity(preparer)
                .contains::<TreeScatterPreparing>()
        );
    }
}
