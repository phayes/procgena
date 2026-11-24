//! Components that request dead wood, and the systems that build it on
//! background threads and draw it: a single piece as child entities, one or
//! two per detail level, and a scatter through the shared instanced
//! renderer.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use bevy::asset::UntypedHandle;
use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::{NoAutoAabb, VisibilityRange};
use bevy::light::NotShadowCaster;
use bevy::math::DVec2;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use bevy::transform::helper::TransformHelper;
use procgena_core::scatter::{ScatterArea, ScatterInput};
use procgena_core::{ContentHash, ContentHasher, FarForm, LevelForm, PoolId, Quality, SpecimenId};
use procgena_core_bevy::instanced::{
    InstancedCopy, InstancedPart, InstancedScatter, InstancedSlot, InstancedStandardMaterial,
    PartMesh, SpecimenCache,
};
use procgena_core_bevy::{LodQuality, visibility_range};
use procgena_deadwood::{
    DeadwoodLod, DeadwoodMesh, DeadwoodOptions, DeadwoodPreset, FlatGround, Ground, HeightGrid,
    LodLevel, OptionsError, Piece, RestingPose, ScatterError, ScatterOptions, StickScatter,
    TextureCompression, TextureSize, lod_size, specimen_from_meshes,
};
use procgena_specimen::{GeneratedSpecimen, Specimen, TagFilter};

use crate::material::{
    BarkTextures, DeadwoodMaterial, EndTextures, LeafTextures, deadwood_material, leaf_material,
};
use crate::mesh::{deadwood_mesh, ribbon_aabb, ribbon_mesh};

/// Asks for one piece of dead wood (a log, branch or stump) resting on the
/// ground at the entity's position.
///
/// Spawn it after adding [`DeadwoodPlugin`](crate::DeadwoodPlugin). The
/// piece is built in the background and appears as [`DeadwoodPart`]
/// children, one or two per detail level. Pieces with the same options
/// share meshes and materials, so many identical ones are cheap.
///
/// The piece lies on flat ground at the entity's `y`. Once the parts
/// appear, the entity also gets a [`DeadwoodPose`], whose
/// [`burial`](RestingPose::burial) says how far to lower the entity to sink
/// a decayed piece into the ground, and a [`GeneratedSpecimen`] describing
/// the piece, so other generators can grow on it (for example a
/// `procgena_clump_bevy` moss child). Changing the options rebuilds the
/// piece; removing this component removes the parts, the pose and the
/// specimen.
#[derive(Component, Debug, Clone, PartialEq)]
#[require(Transform, Visibility)]
pub enum DeadwoodParams {
    /// Options held in memory. [`DeadwoodParams::new`] wraps them for you.
    Options(Arc<DeadwoodOptions>),
    /// A preset loaded from a `.deadwood.yml` file. The piece waits for the
    /// file to load, and rebuilds when the file changes if Bevy is watching
    /// asset files.
    Asset(Handle<DeadwoodPresetAsset>),
}

impl DeadwoodParams {
    /// Asks for a piece made from `options`.
    pub fn new(options: DeadwoodOptions) -> Self {
        Self::Options(Arc::new(options))
    }

    /// Asks for a piece made from a preset's settings.
    pub fn preset(preset: &DeadwoodPreset) -> Self {
        Self::new(preset.deadwood.clone())
    }
}

/// Spreads sticks and twigs over a rectangle of ground centred on the
/// entity.
///
/// The sticks are a few variants, each drawn many times. Once built, the
/// entity gets an [`InstancedScatter`] that draws every stick through the
/// shared instanced renderer, with no entity per stick. The scatter is
/// seeded with [`ScatterOptions::default_seed`].
#[derive(Component, Debug, Clone, PartialEq)]
#[require(Transform, Visibility)]
pub struct DeadwoodScatterParams {
    /// How many sticks, of what sizes and decay.
    pub scatter: ScatterOptions,
    /// Width (X) and depth (Z) of the rectangle to cover, in metres. Both
    /// must be positive.
    pub size: Vec2,
    /// The ground the sticks lie on.
    pub ground: ScatterGround,
}

/// The ground a [`DeadwoodScatterParams`] lays its sticks on.
#[derive(Debug, Clone, PartialEq)]
pub enum ScatterGround {
    /// Flat ground at the entity's `y`.
    Flat,
    /// The surface of the entity's parent, which must have a
    /// [`GeneratedSpecimen`], such as terrain from another procgena
    /// generator.
    ///
    /// Spawn the scatter as a child of that host and leave its own
    /// transform at its default. The ground is the highest surface of the
    /// specimen's first level that `filter` matches, read in the host's own
    /// coordinates with its Y as up, sampled every `cell` metres; sticks
    /// are left out where it has no surface. Lengths stay in world metres
    /// however the host is scaled, but a host stretched differently along
    /// different axes is rejected with
    /// [`DeadwoodBuildError::NonUniformScale`].
    ///
    /// The scatter waits until the host has its specimen, and rebuilds when
    /// the specimen, the host's scale or these settings change. If the host
    /// loses its specimen, the sticks are removed.
    Host {
        /// Which parts of the host's specimen count as ground.
        filter: TagFilter,
        /// Spacing of the ground samples in metres. Must be positive.
        cell: f32,
    },
}

/// A dead wood preset loaded from a `.deadwood.yml` file for
/// [`DeadwoodParams::Asset`].
///
/// The file names a built-in preset as its `base` and gives only what
/// differs; see [`DeadwoodPreset::from_yaml`]. Its key is the file name
/// without `.deadwood.yml`. Files with unknown or out-of-range settings
/// fail to load.
#[derive(Asset, TypePath, Debug, Clone, PartialEq)]
pub struct DeadwoodPresetAsset(pub DeadwoodPreset);

/// Marks a child entity that draws one detail level of a single piece.
/// The plugin spawns and removes these; don't add them yourself.
/// Scatters draw their sticks through an [`InstancedScatter`] instead.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeadwoodPart {
    /// Which piece: 0 for a single piece, or the stick's number in a
    /// scatter.
    pub instance: usize,
    /// Detail level, from 0 for the nearest.
    pub lod: usize,
    /// What the part draws.
    pub kind: DeadwoodPartKind,
}

/// What one [`DeadwoodPart`] draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeadwoodPartKind {
    /// Round stems, with a [`DeadwoodMaterial`].
    Wood,
    /// Thin stems drawn as flat strips that turn toward the camera, with a
    /// [`DeadwoodMaterial`].
    Ribbon,
    /// Leaf cards, with a [`StandardMaterial`] from
    /// [`leaf_material`](crate::leaf_material). Only drawn when the plugin
    /// loads textures.
    Leaves,
}

/// Where a single piece came to rest, added to a [`DeadwoodParams`] entity
/// with its parts. See [`RestingPose`].
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct DeadwoodPose(pub RestingPose);

/// Present while a piece or scatter is being prepared, including while it
/// waits for its options file or its host.
#[derive(Component, Debug, Default)]
pub struct DeadwoodGenerating;

/// Event triggered on a piece or scatter entity when a build ends,
/// successfully or not.
///
/// Observe it on the entity, or globally, to react when dead wood appears.
/// On success, the new [`DeadwoodPart`] children have been queued with the
/// same commands, so they exist once this frame's commands have run.
#[derive(EntityEvent, Debug, Clone, PartialEq)]
pub struct DeadwoodFinished {
    /// The entity that was built.
    pub entity: Entity,
    /// `Ok` when the parts were spawned, or why the build failed.
    pub result: Result<(), DeadwoodBuildError>,
}

/// Why a piece or scatter could not be built.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum DeadwoodBuildError {
    /// A setting is out of range.
    Options(OptionsError),
    /// The scatter's settings are out of range or ask for too many sticks.
    Scatter(ScatterError),
    /// No part of the scatter host's specimen matched the ground filter.
    NoGround,
    /// [`ScatterGround::Host`]'s `cell` is not positive.
    BadCell,
    /// The scatter's host is scaled differently along different axes.
    NonUniformScale,
}

impl fmt::Display for DeadwoodBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeadwoodBuildError::Options(e) => write!(f, "{e}"),
            DeadwoodBuildError::Scatter(e) => write!(f, "{e}"),
            DeadwoodBuildError::NoGround => {
                write!(f, "no part of the scatter's host matches its ground filter")
            }
            DeadwoodBuildError::BadCell => {
                write!(f, "the scatter's ground cell size must be positive")
            }
            DeadwoodBuildError::NonUniformScale => {
                write!(f, "the scatter's host has a non-uniform scale")
            }
        }
    }
}

impl std::error::Error for DeadwoodBuildError {}

impl From<OptionsError> for DeadwoodBuildError {
    fn from(e: OptionsError) -> Self {
        DeadwoodBuildError::Options(e)
    }
}

/// Reads and checks dead wood preset files with the `.deadwood.yml`
/// extension.
#[derive(Debug, Default, TypePath)]
pub(crate) struct DeadwoodPresetLoader;

impl AssetLoader for DeadwoodPresetLoader {
    type Asset = DeadwoodPresetAsset;
    type Settings = ();
    type Error = BevyError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        context: &mut LoadContext<'_>,
    ) -> Result<DeadwoodPresetAsset, BevyError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let file_name = context
            .path()
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let key = file_name.strip_suffix(".deadwood.yml").unwrap_or(file_name);
        let preset = DeadwoodPreset::from_yaml(key, std::str::from_utf8(&bytes)?)?;
        Ok(DeadwoodPresetAsset(preset))
    }

    fn extensions(&self) -> &[&str] {
        &["deadwood.yml"]
    }
}

/// The plugin's settings, for the build systems.
#[derive(Resource, Debug, Clone)]
pub(crate) struct SpawnSettings {
    /// Detail levels for a piece [`reference_size`](DeadwoodLod) across;
    /// each piece scales them to its own size.
    pub lod: DeadwoodLod,
    /// Where to load bark and end-grain textures from, or `None` for vertex
    /// colours and drawn rings only.
    pub textures: Option<TextureSettings>,
}

/// Where and in what form to load bark and end-grain textures.
#[derive(Debug, Clone)]
pub(crate) struct TextureSettings {
    /// Folder relative to Bevy's asset folder that holds `bark/` and
    /// `end/`.
    pub root: String,
    /// Image size to load.
    pub size: TextureSize,
    /// File format to load.
    pub compression: TextureCompression,
}

// --------------------------------------------------------------------------
// Build inputs and results

/// Everything a build depends on. When an entity's input differs from its
/// last one, it is rebuilt.
#[derive(Debug, Clone)]
enum Input {
    /// One piece.
    Single(Arc<DeadwoodOptions>),
    /// A stick scatter on flat ground, and the size of its area.
    FlatScatter(ScatterOptions, Vec2),
    /// A stick scatter over a host's specimen, with the host's (uniform)
    /// world scale.
    HostScatter {
        scatter: ScatterOptions,
        size: Vec2,
        filter: TagFilter,
        cell: f32,
        host: Arc<Specimen>,
        scale: f32,
    },
}

// Host specimens can be large, so they are compared by identity (the same
// shared copy) rather than by content.
impl PartialEq for Input {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Input::Single(a), Input::Single(b)) => a == b,
            (Input::FlatScatter(a, x), Input::FlatScatter(b, y)) => a == b && x == y,
            (
                Input::HostScatter {
                    scatter: a,
                    size: x,
                    filter: f,
                    cell: c,
                    host: h,
                    scale: s,
                },
                Input::HostScatter {
                    scatter: b,
                    size: y,
                    filter: g,
                    cell: d,
                    host: i,
                    scale: t,
                },
            ) => a == b && x == y && f == g && c == d && Arc::ptr_eq(h, i) && s == t,
            _ => false,
        }
    }
}

/// On an entity while its build runs on a background thread.
#[derive(Component)]
pub(crate) struct Building {
    /// What is being built.
    input: Input,
    /// The background work.
    task: Task<Result<Built, DeadwoodBuildError>>,
}

/// The input of the entity's last finished build, successful or not, so it
/// isn't built again until the input changes.
#[derive(Component)]
pub(crate) struct Attempted(Input);

/// What a background build produces, before anything is handed to Bevy's
/// asset storage.
struct Built {
    /// The different pieces to draw.
    variants: Vec<BuiltVariant>,
    /// Where to draw copies of them, relative to the requesting entity.
    instances: Vec<(u32, Transform)>,
    /// A single piece's specimen and resting pose; `None` for scatters.
    single: Option<(Specimen, RestingPose)>,
    /// A scatter's specimen id for each variant; empty for a single piece.
    specimens: Vec<SpecimenId>,
}

/// Names one built piece at one set of detail levels. Variants with the
/// same key share uploaded meshes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct VariantKey {
    /// The pool of the piece's options.
    pool: PoolId,
    /// The seed the piece was shaped from.
    seed: u32,
    /// The hash of the piece's scaled detail levels (see [`lod_hash`]).
    lod: u64,
}

/// One variant's meshes, one per detail level.
struct BuiltVariant {
    /// Which piece and detail levels these meshes are.
    key: VariantKey,
    /// The variant's options.
    options: DeadwoodOptions,
    /// Detail levels for a piece of the reference size.
    lod: DeadwoodLod,
    /// One entry per detail level.
    levels: Vec<BuiltLevel>,
    /// A sphere around every level's meshes, as a centre and radius.
    bounds: (Vec3, f32),
}

/// One detail level's meshes.
struct BuiltLevel {
    /// The round stems, or `None` if the level has none.
    surfaces: Option<Mesh>,
    /// Thin stems as flat strips, and their bounds.
    ribbon: Option<(Mesh, Aabb)>,
    /// Leaf cards, or `None` if the level has none.
    leaves: Option<Mesh>,
}

/// One variant's meshes and materials, after adding them to Bevy's assets.
struct VariantAssets {
    /// Detail levels for a piece of the reference size.
    lod: DeadwoodLod,
    /// The variant's size, from [`lod_size`], which scales `lod`'s
    /// distances.
    size: f32,
    /// One entry per detail level.
    levels: Vec<LevelAssets>,
}

/// One detail level's mesh and material handles.
struct LevelAssets {
    /// The round stems and their material.
    surfaces: Option<(Handle<Mesh>, Handle<DeadwoodMaterial>)>,
    /// Flat strips, their material and their bounds.
    ribbon: Option<(Handle<Mesh>, Handle<DeadwoodMaterial>, Aabb)>,
    /// Leaf cards and their material, when the level has cards and
    /// textures are loaded.
    leaves: Option<(Handle<Mesh>, Handle<InstancedStandardMaterial>)>,
}

/// Uploaded variants and materials, shared by every entity that needs the
/// same ones.
#[derive(Resource, Default)]
pub(crate) struct DeadwoodCache {
    /// Variants by [`BuiltVariant::key`].
    variants: HashMap<VariantKey, Arc<VariantAssets>>,
    /// Materials by the hash of the options that affect them (see
    /// [`material_hash`]) and whether they are for strips.
    materials: HashMap<(u64, bool), Handle<DeadwoodMaterial>>,
    /// Bark texture images by set name, kept for reuse.
    bark_textures: HashMap<String, BarkTextures>,
    /// End-grain texture images by set name, kept for reuse.
    end_textures: HashMap<String, EndTextures>,
    /// Leaf materials by texture set name.
    leaf_materials: HashMap<String, Handle<InstancedStandardMaterial>>,
}

// --------------------------------------------------------------------------
// Systems

/// Removes the parts, markers, pose and specimen of entities whose piece or
/// scatter component was removed.
pub(crate) fn remove_deadwood(
    mut commands: Commands,
    mut singles: RemovedComponents<DeadwoodParams>,
    mut scatters: RemovedComponents<DeadwoodScatterParams>,
    children: Query<&Children>,
    parts: Query<(), With<DeadwoodPart>>,
) {
    let removed: Vec<Entity> = singles.read().chain(scatters.read()).collect();
    for entity in removed {
        despawn_parts(&mut commands, children.get(entity).ok(), &parts);
        if let Ok(mut entity) = commands.get_entity(entity) {
            entity.remove::<(
                DeadwoodGenerating,
                Building,
                Attempted,
                GeneratedSpecimen,
                DeadwoodPose,
                InstancedScatter,
            )>();
        }
    }
}

/// Despawns the [`DeadwoodPart`]s among `kids`, leaving other children
/// alone.
fn despawn_parts(
    commands: &mut Commands,
    kids: Option<&Children>,
    parts: &Query<(), With<DeadwoodPart>>,
) {
    for &child in kids.into_iter().flatten() {
        if parts.contains(child) {
            commands.entity(child).despawn();
        }
    }
}

/// What an entity with a piece or scatter component needs this frame.
enum Wanted {
    /// Nothing yet: its options file or its host's specimen isn't available.
    Wait,
    /// The input can't be built, for this reason.
    Fail(Input, DeadwoodBuildError),
    /// The input to build.
    Build(Input),
}

/// Starts a background build for every entity whose input differs from its
/// running or last finished build, and marks entities that are waiting.
#[expect(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "Bevy system parameters"
)]
pub(crate) fn start_generation(
    mut commands: Commands,
    singles: Query<(Entity, &DeadwoodParams)>,
    scatters: Query<(Entity, &DeadwoodScatterParams, Option<&ChildOf>)>,
    state: Query<(
        Option<&Building>,
        Option<&Attempted>,
        Option<&Children>,
        Has<DeadwoodGenerating>,
    )>,
    hosts: Query<&GeneratedSpecimen>,
    transforms: TransformHelper,
    assets: Res<Assets<DeadwoodPresetAsset>>,
    parts: Query<(), With<DeadwoodPart>>,
    settings: Res<SpawnSettings>,
) {
    // Work out what each entity needs.
    let mut wanted: Vec<(Entity, Wanted)> = Vec::new();
    for (entity, params) in &singles {
        let options = match params {
            DeadwoodParams::Options(o) => Some(o.clone()),
            DeadwoodParams::Asset(handle) => {
                assets.get(handle).map(|a| Arc::new(a.0.deadwood.clone()))
            }
        };
        wanted.push((
            entity,
            options.map_or(Wanted::Wait, |o| Wanted::Build(Input::Single(o))),
        ));
    }
    for (entity, params, parent) in &scatters {
        let (filter, cell) = match &params.ground {
            ScatterGround::Flat => {
                let input = Input::FlatScatter(params.scatter.clone(), params.size);
                wanted.push((entity, Wanted::Build(input)));
                continue;
            }
            ScatterGround::Host { filter, cell } => (filter, *cell),
        };

        // Wait for the host's specimen. Work out the host's world scale from
        // its parents, because Bevy may not have updated it yet this frame.
        let host = parent.map(ChildOf::parent);
        let found = host.and_then(|h| {
            Some((
                hosts.get(h).ok()?,
                transforms.compute_global_transform(h).ok()?,
            ))
        });
        let Some((specimen, transform)) = found else {
            wanted.push((entity, Wanted::Wait));
            continue;
        };
        let (scale, _, _) = transform.to_scale_rotation_translation();
        let input = Input::HostScatter {
            scatter: params.scatter.clone(),
            size: params.size,
            filter: filter.clone(),
            cell,
            host: specimen.0.clone(),
            scale: scale.x,
        };
        let tolerance = scale.abs().max_element() * 1e-4;
        let uniform = scale.x > 0.0
            && (scale.x - scale.y).abs() <= tolerance
            && (scale.x - scale.z).abs() <= tolerance;
        wanted.push((
            entity,
            if !uniform {
                Wanted::Fail(input, DeadwoodBuildError::NonUniformScale)
            } else if !(cell > 0.0 && cell.is_finite()) {
                Wanted::Fail(input, DeadwoodBuildError::BadCell)
            } else {
                Wanted::Build(input)
            },
        ));
    }

    // Act on each entity whose input changed.
    for (entity, want) in wanted {
        let Ok((building, attempted, kids, generating)) = state.get(entity) else {
            continue;
        };
        let input = match want {
            Wanted::Wait => {
                // If the host lost its specimen, remove the sticks laid on it.
                if building.is_some() || attempted.is_some() {
                    despawn_parts(&mut commands, kids, &parts);
                    commands.entity(entity).remove::<(
                        Building,
                        Attempted,
                        GeneratedSpecimen,
                        DeadwoodPose,
                        InstancedScatter,
                    )>();
                }
                if !generating {
                    commands.entity(entity).insert(DeadwoodGenerating);
                }
                continue;
            }
            Wanted::Fail(input, _) | Wanted::Build(input)
                if is_current(&input, building, attempted) =>
            {
                continue;
            }
            Wanted::Fail(input, error) => {
                despawn_parts(&mut commands, kids, &parts);
                commands
                    .entity(entity)
                    .remove::<(
                        Building,
                        DeadwoodGenerating,
                        GeneratedSpecimen,
                        DeadwoodPose,
                        InstancedScatter,
                    )>()
                    .insert(Attempted(input));
                commands.trigger(DeadwoodFinished {
                    entity,
                    result: Err(error),
                });
                continue;
            }
            Wanted::Build(input) => input,
        };
        let lod = settings.lod.clone();
        let task_input = input.clone();
        let task = AsyncComputeTaskPool::get().spawn(async move { build(&task_input, &lod) });
        commands
            .entity(entity)
            .remove::<(GeneratedSpecimen, DeadwoodPose)>()
            .insert((DeadwoodGenerating, Building { input, task }));
    }
}

/// Returns true if `input` is already being built, or was the last thing
/// built.
fn is_current(input: &Input, building: Option<&Building>, attempted: Option<&Attempted>) -> bool {
    building.is_some_and(|b| b.input == *input) || attempted.is_some_and(|a| a.0 == *input)
}

/// Ground read from a host's specimen in the host's units, given in world
/// metres by multiplying by the host's scale.
struct ScaledGround {
    /// The ground in the host's units.
    grid: HeightGrid,
    /// The host's world scale.
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

/// Builds the meshes and copy transforms for `input`. Runs on a background
/// thread. `lod` is for a piece of
/// [`DEFAULT_LOD_SIZE`](procgena_deadwood::DEFAULT_LOD_SIZE).
fn build(input: &Input, lod: &DeadwoodLod) -> Result<Built, DeadwoodBuildError> {
    match input {
        Input::Single(options) => {
            let piece = Piece::new((**options).clone())?;
            let (variant, meshes) = build_variant(&piece, lod)?;
            let specimen = specimen_from_meshes(&meshes);
            Ok(Built {
                variants: vec![variant],
                instances: vec![(0, Transform::IDENTITY)],
                single: Some((specimen, *piece.pose())),
                specimens: Vec::new(),
            })
        }
        Input::FlatScatter(scatter, size) => scatter_sticks(scatter, *size, &FlatGround, 1.0, lod),
        Input::HostScatter {
            scatter,
            size,
            filter,
            cell,
            host,
            scale,
        } => {
            // Sample the host's surface in its own units, then work in
            // world metres; the parts are scaled back down to the host's
            // units at the end.
            let grid = host
                .height_grid(0, filter, cell / scale)
                .ok_or(DeadwoodBuildError::NoGround)?;
            let ground = ScaledGround {
                grid,
                scale: *scale,
            };
            scatter_sticks(scatter, *size, &ground, *scale, lod)
        }
    }
}

/// Scatters sticks over a `size` rectangle of `ground` (in world metres)
/// centred on the entity, seeded by the scatter's default seed, and builds
/// every variant. Each stick's transform is divided by `scale`, the
/// parent's world scale, so it comes out in world metres.
fn scatter_sticks(
    scatter: &ScatterOptions,
    size: Vec2,
    ground: &dyn Ground,
    scale: f32,
    lod: &DeadwoodLod,
) -> Result<Built, DeadwoodBuildError> {
    let area = ScatterArea::new(DVec2::ZERO, size);
    let input = ScatterInput::new(ground, area, scatter.default_seed);
    let built = StickScatter::new(scatter, &input).map_err(DeadwoodBuildError::Scatter)?;
    let variants = built
        .variants()
        .iter()
        .map(|piece| build_variant(piece, lod).map(|(variant, _)| variant))
        .collect::<Result<Vec<_>, _>>()?;
    let shrink = Mat4::from_scale(Vec3::splat(1.0 / scale));
    let instances = built
        .instances()
        .iter()
        .map(|i| {
            let local = shrink * Mat4::from(i.transform);
            (i.variant, Transform::from_matrix(local))
        })
        .collect();
    // Variant `v` is the specimen in slot `v` of the scatter's variant pool.
    let pool = scatter.variant_pool();
    let specimens = (0..variants.len() as u32)
        .map(|v| SpecimenId::new(scatter.default_seed, pool, v))
        .collect();
    Ok(Built {
        variants,
        instances,
        single: None,
        specimens,
    })
}

/// Meshes `piece` at every level of `lod` (see [`Piece::lod_meshes`]) and
/// converts the meshes for Bevy. Also returns the core meshes, for building
/// the piece's specimen.
fn build_variant(
    piece: &Piece,
    lod: &DeadwoodLod,
) -> Result<(BuiltVariant, Vec<(LodLevel, DeadwoodMesh)>), OptionsError> {
    let options = piece.options().clone();
    let meshes = piece.lod_meshes(lod)?;
    let levels: Vec<BuiltLevel> = meshes
        .iter()
        .map(|(_, mesh)| BuiltLevel {
            surfaces: (mesh.surfaces.triangle_count() > 0).then(|| deadwood_mesh(&mesh.surfaces)),
            ribbon: mesh
                .ribbons
                .as_ref()
                .filter(|r| r.strip.triangle_count() > 0)
                .map(|r| (ribbon_mesh(r), ribbon_aabb(r))),
            leaves: mesh.leaves.as_ref().map(deadwood_mesh),
        })
        .collect();
    let bounds = sphere_around(levels.iter().flat_map(|level| {
        let boxes = [&level.surfaces, &level.leaves].map(|m| m.as_ref().and_then(mesh_box));
        boxes
            .into_iter()
            .flatten()
            .chain(level.ribbon.as_ref().map(|(_, aabb)| *aabb))
    }));
    let key = VariantKey {
        pool: options.pool_id(),
        seed: options.default_seed,
        lod: lod_hash(lod),
    };
    Ok((
        BuiltVariant {
            key,
            options,
            lod: lod.clone(),
            levels,
            bounds,
        },
        meshes,
    ))
}

/// The box around `mesh`'s positions, or `None` if it has none.
fn mesh_box(mesh: &Mesh) -> Option<Aabb> {
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return None;
    };
    let (min, max) = positions
        .iter()
        .map(|&p| Vec3::from(p))
        .fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(min, max), p| {
            (min.min(p), max.max(p))
        });
    (min.x <= max.x).then(|| Aabb::from_min_max(min, max))
}

/// A sphere around every box in `boxes`, as a centre and radius; a point at
/// the origin if there are none.
fn sphere_around(boxes: impl Iterator<Item = Aabb>) -> (Vec3, f32) {
    let (min, max) = boxes.fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(min, max), aabb| {
        (min.min(aabb.min().into()), max.max(aabb.max().into()))
    });
    if min.x > max.x {
        return (Vec3::ZERO, 0.0);
    }
    let center = (min + max) / 2.0;
    (center, (max - center).length())
}

/// Collects finished background builds: replaces each entity's parts with
/// the new ones, adds a single piece's [`GeneratedSpecimen`] and
/// [`DeadwoodPose`], and triggers [`DeadwoodFinished`]. A failed build
/// is logged and leaves no parts.
#[expect(clippy::too_many_arguments, reason = "Bevy system parameters")]
pub(crate) fn finish_generation(
    mut commands: Commands,
    mut building: Query<(Entity, &mut Building, Option<&Children>)>,
    parts: Query<(), With<DeadwoodPart>>,
    mut cache: ResMut<DeadwoodCache>,
    mut specimens: ResMut<SpecimenCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<DeadwoodMaterial>>,
    mut standard: ResMut<Assets<InstancedStandardMaterial>>,
    asset_server: Res<AssetServer>,
    settings: Res<SpawnSettings>,
    quality: Res<LodQuality>,
) {
    for (entity, mut b, kids) in &mut building {
        let Some(result) = block_on(poll_once(&mut b.task)) else {
            continue;
        };
        let input = b.input.clone();
        despawn_parts(&mut commands, kids, &parts);
        commands
            .entity(entity)
            .remove::<(Building, DeadwoodGenerating)>()
            .insert(Attempted(input));

        let built = match result {
            Ok(built) => built,
            Err(error) => {
                error!("procgena_deadwood_bevy: cannot build dead wood: {error}");
                commands.trigger(DeadwoodFinished {
                    entity,
                    result: Err(error),
                });
                continue;
            }
        };

        // A scatter draws through the shared instanced renderer.
        let textures = (&*asset_server, settings.textures.as_ref());
        if built.single.is_none() {
            let materials = (&mut *materials, &mut *standard);
            let mut uploads = (&mut *cache, &mut *meshes, materials, textures);
            let scatter = scatter_draw(built, &mut specimens, &mut uploads);
            commands.entity(entity).insert(scatter);
            commands.trigger(DeadwoodFinished {
                entity,
                result: Ok(()),
            });
            continue;
        }
        commands.entity(entity).remove::<InstancedScatter>();

        // Add each variant to Bevy's assets, reusing identical variants that
        // other entities already added.
        let variants: Vec<Arc<VariantAssets>> = built
            .variants
            .into_iter()
            .map(|v| {
                let materials = (&mut *materials, &mut *standard);
                upload(v, &mut cache, &mut meshes, materials, textures)
            })
            .collect();

        // Spawn one child per level and kind of part (round stems, strips,
        // leaf cards), each shown only within its own distance band, scaled
        // by the piece's size and the quality setting.
        let ranges: Vec<Vec<VisibilityRange>> = variants
            .iter()
            .map(|v| variant_ranges(&v.lod, v.size, quality.0))
            .collect();
        for (i, (v, transform)) in built.instances.iter().enumerate() {
            let variant = &variants[*v as usize];
            let ranges = &ranges[*v as usize];
            for (lod, (level, range)) in variant.levels.iter().zip(ranges).enumerate() {
                let shadows = variant.lod.levels[lod].casts_shadows;
                if let Some((mesh, material)) = &level.surfaces {
                    let mut part = commands.spawn((
                        DeadwoodPart {
                            instance: i,
                            lod,
                            kind: DeadwoodPartKind::Wood,
                        },
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        *transform,
                        range.clone(),
                        ChildOf(entity),
                    ));
                    if !shadows {
                        part.insert(NotShadowCaster);
                    }
                }
                if let Some((mesh, material, aabb)) = &level.ribbon {
                    let mut part = commands.spawn((
                        DeadwoodPart {
                            instance: i,
                            lod,
                            kind: DeadwoodPartKind::Ribbon,
                        },
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        *transform,
                        range.clone(),
                        *aabb,
                        NoAutoAabb,
                        ChildOf(entity),
                    ));
                    if !shadows {
                        part.insert(NotShadowCaster);
                    }
                }
                if let Some((mesh, material)) = &level.leaves {
                    commands.spawn((
                        DeadwoodPart {
                            instance: i,
                            lod,
                            kind: DeadwoodPartKind::Leaves,
                        },
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        *transform,
                        range.clone(),
                        ChildOf(entity),
                    ));
                }
            }
        }
        if let Some((specimen, pose)) = built.single {
            commands
                .entity(entity)
                .insert((GeneratedSpecimen(Arc::new(specimen)), DeadwoodPose(pose)));
        }
        commands.trigger(DeadwoodFinished {
            entity,
            result: Ok(()),
        });
    }
}

/// The material assets [`upload`] adds to: dead wood, and leaf cards.
type MaterialAssets<'a> = (
    &'a mut Assets<DeadwoodMaterial>,
    &'a mut Assets<InstancedStandardMaterial>,
);

/// The asset server, and where to load textures from when they are wanted.
type TextureSource<'a> = (&'a AssetServer, Option<&'a TextureSettings>);

/// Builds the shared renderer's view of a built scatter: a slot per
/// variant, with its parts from `specimens` (uploading variants that aren't
/// cached yet with `uploads`), and a copy per stick.
fn scatter_draw(
    built: Built,
    specimens: &mut SpecimenCache,
    (cache, meshes, materials, textures): &mut (
        &mut DeadwoodCache,
        &mut Assets<Mesh>,
        MaterialAssets,
        TextureSource,
    ),
) -> InstancedScatter {
    let mut sizes = Vec::with_capacity(built.variants.len());
    let slots: Vec<InstancedSlot> = built
        .variants
        .into_iter()
        .zip(&built.specimens)
        .map(|(variant, &specimen)| {
            let lod = variant.lod.clone().map_detail(|_| ());
            let levels = lod.levels.len() as u32;
            let bounds = variant.bounds;
            sizes.push(lod_size(&variant.options));
            let parts = specimens
                .get_levels(specimen, 0, levels)
                .unwrap_or_else(|| {
                    let materials = (&mut *materials.0, &mut *materials.1);
                    let assets = upload(variant, cache, meshes, materials, *textures);
                    let mut parts = Vec::new();
                    for (level, assets) in assets.levels.iter().enumerate() {
                        let level_parts = level_parts(level as u32, assets);
                        specimens.insert(specimen, 0, level as u32, level_parts.clone());
                        parts.extend(level_parts);
                    }
                    parts
                });
            InstancedSlot {
                specimen,
                piece: 0,
                lod,
                parts,
                bounds,
            }
        })
        .collect();
    let copies: Vec<InstancedCopy> = built
        .instances
        .iter()
        .map(|(v, transform)| InstancedCopy {
            slot: *v,
            transform: transform.compute_affine(),
            size: sizes[*v as usize],
            wind_phase: 0.0,
        })
        .collect();
    InstancedScatter {
        slots: slots.into(),
        copies: copies.into(),
    }
}

/// One level's parts for the shared renderer: round stems, strips and leaf
/// cards, whichever the level has.
fn level_parts(level: u32, assets: &LevelAssets) -> Vec<InstancedPart> {
    let part = |mesh: &Handle<Mesh>, material: UntypedHandle| InstancedPart {
        level,
        mesh: PartMesh::Mesh(mesh.clone()),
        material,
    };
    let mut parts = Vec::new();
    if let Some((mesh, material)) = &assets.surfaces {
        parts.push(part(mesh, material.clone().untyped()));
    }
    if let Some((mesh, material, _)) = &assets.ribbon {
        parts.push(part(mesh, material.clone().untyped()));
    }
    if let Some((mesh, material)) = &assets.leaves {
        parts.push(part(mesh, material.clone().untyped()));
    }
    parts
}

/// Adds a variant's meshes and materials to Bevy's assets and returns their
/// handles, or returns the cached handles for an identical variant.
/// Materials are shared by every variant whose options give the same
/// material.
///
/// `textures` is the asset server and, when textures are wanted, where to
/// load them from; the variant's bark and end-grain sets, if it names them,
/// are loaded once and then reused.
fn upload(
    variant: BuiltVariant,
    cache: &mut DeadwoodCache,
    meshes: &mut Assets<Mesh>,
    (materials, standard): MaterialAssets,
    (asset_server, textures): TextureSource,
) -> Arc<VariantAssets> {
    if let Some(found) = cache.variants.get(&variant.key) {
        return found.clone();
    }

    // The bark texture set, if this piece names one and textures are on.
    let bark = match (&variant.options.material.bark_texture, textures) {
        (Some(set), Some(t)) => Some(
            cache
                .bark_textures
                .entry(set.clone())
                .or_insert_with(|| {
                    BarkTextures::load(asset_server, &t.root, set, t.size, t.compression)
                })
                .clone(),
        ),
        _ => None,
    };
    let end = match (&variant.options.material.end_texture, textures) {
        (Some(set), Some(t)) => Some(
            cache
                .end_textures
                .entry(set.clone())
                .or_insert_with(|| {
                    EndTextures::load(asset_server, &t.root, set, t.size, t.compression)
                })
                .clone(),
        ),
        _ => None,
    };

    // Everything `deadwood_material` reads from the options.
    let o = &variant.options;
    let material_key = material_hash(o);
    let mut material = |ribbon: bool| {
        cache
            .materials
            .entry((material_key, ribbon))
            .or_insert_with(|| {
                materials.add(deadwood_material(o, bark.as_ref(), end.as_ref(), ribbon))
            })
            .clone()
    };
    // Leaf cards need their texture to cut out the leaves, so without
    // textures they are left out.
    let leaf_set = &o.leaves.texture;
    let mut leaf = || {
        let t = textures?;
        let handle = cache
            .leaf_materials
            .entry(leaf_set.clone())
            .or_insert_with(|| {
                let images = LeafTextures::load(asset_server, &t.root, leaf_set, t.compression);
                standard.add(leaf_material(&images))
            });
        Some(handle.clone())
    };
    let levels: Vec<LevelAssets> = variant
        .levels
        .into_iter()
        .map(|level| LevelAssets {
            surfaces: level.surfaces.map(|m| (meshes.add(m), material(false))),
            ribbon: level
                .ribbon
                .map(|(m, aabb)| (meshes.add(m), material(true), aabb)),
            leaves: level
                .leaves
                .and_then(|m| leaf().map(|handle| (meshes.add(m), handle))),
        })
        .collect();
    let assets = Arc::new(VariantAssets {
        size: lod_size(&variant.options),
        lod: variant.lod,
        levels,
    });
    cache.variants.insert(variant.key, assets.clone());
    assets
}

/// Returns a hash of every setting of `lod`: its reference size, each
/// level's distance, hysteresis, form, shadows and detail, then the cull
/// distance.
fn lod_hash(lod: &DeadwoodLod) -> u64 {
    let DeadwoodLod {
        reference_size,
        levels,
        cull_distance,
    } = lod;
    let mut h = ContentHasher::new();
    h.write_f32(*reference_size);
    h.write_usize(levels.len());
    for LodLevel {
        distance,
        hysteresis,
        form,
        casts_shadows,
        detail,
    } in levels
    {
        h.write_f32(*distance);
        h.write_f32(*hysteresis);
        h.write_u32(form_number(*form));
        h.write_bool(*casts_shadows);
        detail.content_hash(&mut h);
    }
    cull_distance.content_hash(&mut h);
    h.finish()
}

/// Numbers each level form for [`lod_hash`].
///
/// # Panics
///
/// Panics on a far form this crate doesn't know yet, so that a new form
/// can't silently share a number with another.
fn form_number(form: LevelForm) -> u32 {
    match form {
        LevelForm::Mesh => 0,
        LevelForm::Far(FarForm::Strips) => 1,
        LevelForm::Far(FarForm::BillboardLeaves) => 2,
        LevelForm::Far(FarForm::Card) => 3,
        LevelForm::Far(other) => panic!("no hash number for far form {other:?}"),
        LevelForm::Impostor => 4,
    }
}

/// Returns a hash of everything [`deadwood_material`] reads from `o`: the
/// material settings, decay, diameter and seed.
fn material_hash(o: &DeadwoodOptions) -> u64 {
    let mut h = ContentHasher::new();
    o.material.content_hash(&mut h);
    h.write_f32(o.decay);
    h.write_f32(o.diameter);
    h.write_u32(o.default_seed);
    h.finish()
}

/// Returns the distance band to show each level of `lod` in, for a piece
/// `size` metres across (see [`lod_size`]) drawn at `quality`.
fn variant_ranges(lod: &DeadwoodLod, size: f32, quality: Quality) -> Vec<VisibilityRange> {
    lod.ranges(lod.factor(size, quality))
        .iter()
        .map(visibility_range)
        .collect()
}

/// After parts or scatters are removed or replaced, forgets cached variants
/// and materials that no remaining part or scatter draws, so Bevy can free
/// them.
pub(crate) fn prune_cache(
    mut removed: RemovedComponents<DeadwoodPart>,
    mut removed_scatters: RemovedComponents<InstancedScatter>,
    replaced: Query<(), (Changed<InstancedScatter>, With<DeadwoodScatterParams>)>,
    parts: Query<&Mesh3d, With<DeadwoodPart>>,
    scatters: Query<&InstancedScatter, With<DeadwoodScatterParams>>,
    mut cache: ResMut<DeadwoodCache>,
) {
    let removed = removed.read().count() + removed_scatters.read().count();
    if removed == 0 && replaced.is_empty() {
        return;
    }
    // Keep variants that some part or scatter still draws, then the
    // materials those variants use.
    let scattered = scatters
        .iter()
        .flat_map(|scatter| scatter.slots.iter())
        .flat_map(|slot| slot.parts.iter())
        .filter_map(|part| match &part.mesh {
            PartMesh::Mesh(mesh) => Some(mesh.id()),
            PartMesh::External(_) => None,
        });
    let drawn: HashSet<AssetId<Mesh>> = parts.iter().map(|m| m.0.id()).chain(scattered).collect();
    cache.variants.retain(|_, v| {
        v.levels.iter().any(|l| {
            l.surfaces
                .iter()
                .map(|(m, _)| m.id())
                .chain(l.ribbon.iter().map(|(m, _, _)| m.id()))
                .chain(l.leaves.iter().map(|(m, _)| m.id()))
                .any(|id| drawn.contains(&id))
        })
    });
    let used_leaves: HashSet<AssetId<InstancedStandardMaterial>> = cache
        .variants
        .values()
        .flat_map(|v| v.levels.iter())
        .flat_map(|l| l.leaves.iter().map(|(_, m)| m.id()))
        .collect();
    cache
        .leaf_materials
        .retain(|_, m| used_leaves.contains(&m.id()));
    let used: HashSet<AssetId<DeadwoodMaterial>> = cache
        .variants
        .values()
        .flat_map(|v| v.levels.iter())
        .flat_map(|l| {
            l.surfaces
                .iter()
                .map(|(_, m)| m.id())
                .chain(l.ribbon.iter().map(|(_, m, _)| m.id()))
        })
        .collect();
    cache.materials.retain(|_, m| used.contains(&m.id()));
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use procgena_deadwood::default_lod;
    use procgena_specimen::{Part, SurfaceMesh, tags};

    use super::*;

    /// Results of every [`DeadwoodFinished`] event, in order.
    #[derive(Resource, Default)]
    struct Finished(Vec<Result<(), DeadwoodBuildError>>);

    /// An app running only the build systems, without rendering.
    fn app() -> App {
        app_with(AssetPlugin::default())
    }

    /// An app running only the build systems, without rendering, that
    /// loads files with `assets`.
    fn app_with(assets: AssetPlugin) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, assets))
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<DeadwoodMaterial>()
            .init_asset::<InstancedStandardMaterial>()
            .init_resource::<Finished>()
            .add_observer(
                |event: On<DeadwoodFinished>, mut finished: ResMut<Finished>| {
                    finished.0.push(event.result.clone());
                },
            );
        crate::add_systems(&mut app, default_lod());
        app
    }

    /// Updates until `entity` has finished a build, or panics after 60 s.
    fn run_until_finished(app: &mut App, entity: Entity) {
        let deadline = Instant::now() + Duration::from_secs(60);
        app.update();
        while app.world().entity(entity).contains::<DeadwoodGenerating>()
            || app.world().entity(entity).contains::<Building>()
        {
            assert!(Instant::now() < deadline, "build did not finish");
            std::thread::sleep(Duration::from_millis(1));
            app.update();
        }
        app.update();
    }

    /// The parts under `entity`.
    fn parts(app: &mut App, entity: Entity) -> Vec<DeadwoodPart> {
        let mut query = app.world_mut().query::<(&DeadwoodPart, &ChildOf)>();
        query
            .iter(app.world())
            .filter(|(_, parent)| parent.parent() == entity)
            .map(|(part, _)| *part)
            .collect()
    }

    #[test]
    fn a_log_gets_every_level_its_specimen_and_pose() {
        let mut app = app();
        let entity = app
            .world_mut()
            .spawn(DeadwoodParams::preset(DeadwoodPreset::rotten_log()))
            .id();
        run_until_finished(&mut app, entity);

        let parts = parts(&mut app, entity);
        let mut levels: Vec<usize> = parts
            .iter()
            .filter(|p| p.kind == DeadwoodPartKind::Wood)
            .map(|p| p.lod)
            .collect();
        levels.sort();
        assert_eq!(levels, [0, 1, 2]);

        // The last level is strips only, and casts no shadows.
        let mut query = app
            .world_mut()
            .query::<(&DeadwoodPart, &ChildOf, Has<NotShadowCaster>)>();
        for (part, parent, no_shadow) in query.iter(app.world()) {
            if parent.parent() == entity {
                assert_eq!(no_shadow, part.lod == 3, "{part:?}");
            }
        }
        assert!(
            parts
                .iter()
                .any(|p| p.kind == DeadwoodPartKind::Ribbon && p.lod == 3),
            "{parts:?}"
        );
        assert!(app.world().entity(entity).contains::<GeneratedSpecimen>());
        let pose = app.world().get::<DeadwoodPose>(entity).unwrap();
        assert!(pose.0.burial > 0.0, "a rotten log suggests burial");
        assert_eq!(app.world().resource::<Finished>().0, vec![Ok(())]);

        // Removing the request removes the parts, specimen and pose.
        app.world_mut()
            .entity_mut(entity)
            .remove::<DeadwoodParams>();
        app.update();
        assert!(self::parts(&mut app, entity).is_empty());
        assert!(!app.world().entity(entity).contains::<GeneratedSpecimen>());
        assert!(!app.world().entity(entity).contains::<DeadwoodPose>());
    }

    #[test]
    fn textured_pieces_get_bark_and_end_grain_but_strips_do_not() {
        let mut app = app();
        app.world_mut().resource_mut::<SpawnSettings>().textures = Some(TextureSettings {
            root: "textures".to_owned(),
            size: TextureSize::Size1k,
            compression: TextureCompression::Bc7,
        });
        // A branch, which has strips at a distance. Its default options
        // name a bark and an end-grain set.
        let entity = app
            .world_mut()
            .spawn(DeadwoodParams::preset(DeadwoodPreset::dead_branch()))
            .id();
        run_until_finished(&mut app, entity);

        let mut query = app
            .world_mut()
            .query::<(&DeadwoodPart, &MeshMaterial3d<DeadwoodMaterial>)>();
        let found: Vec<(DeadwoodPart, Handle<DeadwoodMaterial>)> = query
            .iter(app.world())
            .map(|(part, material)| (*part, material.0.clone()))
            .collect();
        let materials = app.world().resource::<Assets<DeadwoodMaterial>>();
        let ribbon = |p: &DeadwoodPart| p.kind == DeadwoodPartKind::Ribbon;
        assert!(found.iter().any(|(p, _)| ribbon(p)) && found.iter().any(|(p, _)| !ribbon(p)));
        for (part, handle) in found {
            let extension = &materials.get(&handle).unwrap().extension;
            let expected = !ribbon(&part);
            assert_eq!(extension.bark_color.is_some(), expected, "{part:?}");
            assert_eq!(extension.end_color.is_some(), expected, "{part:?}");
            assert_eq!(extension.surface.end_textured == 1, expected, "{part:?}");
        }
    }

    #[test]
    fn a_branch_gets_flat_strips_at_a_distance() {
        let mut app = app();
        let entity = app
            .world_mut()
            .spawn(DeadwoodParams::preset(DeadwoodPreset::dead_branch()))
            .id();
        run_until_finished(&mut app, entity);
        let parts = parts(&mut app, entity);
        assert!(
            parts.iter().any(|p| p.kind == DeadwoodPartKind::Ribbon),
            "{parts:?}"
        );
        assert!(
            parts
                .iter()
                .any(|p| p.kind == DeadwoodPartKind::Wood && p.lod == 0)
        );
    }

    #[test]
    fn leaf_cards_need_textures_and_thin_out_with_distance() {
        // Without textures, leaves can't be cut out, so none are drawn.
        let mut app = app();
        let entity = app
            .world_mut()
            .spawn(DeadwoodParams::preset(DeadwoodPreset::seedling_log()))
            .id();
        run_until_finished(&mut app, entity);
        let parts = parts(&mut app, entity);
        assert!(!parts.iter().any(|p| p.kind == DeadwoodPartKind::Leaves));

        // With textures, every level but the farthest gets leaf cards, all
        // sharing one leaf material.
        let mut app = self::app();
        app.world_mut().resource_mut::<SpawnSettings>().textures = Some(TextureSettings {
            root: "textures".to_owned(),
            size: TextureSize::Size1k,
            compression: TextureCompression::Bc7,
        });
        let entity = app
            .world_mut()
            .spawn(DeadwoodParams::preset(DeadwoodPreset::seedling_log()))
            .id();
        run_until_finished(&mut app, entity);
        let mut query = app
            .world_mut()
            .query::<(
                &DeadwoodPart,
                &MeshMaterial3d<InstancedStandardMaterial>,
                &ChildOf,
            )>();
        let leaves: Vec<(usize, Handle<InstancedStandardMaterial>)> = query
            .iter(app.world())
            .filter(|(p, _, parent)| {
                parent.parent() == entity && p.kind == DeadwoodPartKind::Leaves
            })
            .map(|(p, m, _)| (p.lod, m.0.clone()))
            .collect();
        let mut levels: Vec<usize> = leaves.iter().map(|(lod, _)| *lod).collect();
        levels.sort();
        assert_eq!(levels, [0, 1, 2]);
        assert!(leaves.iter().all(|(_, m)| *m == leaves[0].1));
        let materials = app.world().resource::<Assets<InstancedStandardMaterial>>();
        let material = &materials.get(&leaves[0].1).unwrap().base;
        assert!(material.base_color_texture.is_some());
        assert!(matches!(material.alpha_mode, AlphaMode::Mask(_)));
        assert!(material.double_sided);
    }

    #[test]
    fn identical_pieces_share_meshes_and_rebuild_on_change() {
        let mut app = app();
        let a = app
            .world_mut()
            .spawn(DeadwoodParams::preset(DeadwoodPreset::stick()))
            .id();
        let b = app
            .world_mut()
            .spawn(DeadwoodParams::preset(DeadwoodPreset::stick()))
            .id();
        run_until_finished(&mut app, a);
        run_until_finished(&mut app, b);
        let meshes = |app: &mut App, e: Entity| {
            let mut q = app.world_mut().query::<(&Mesh3d, &ChildOf)>();
            let mut ids: Vec<_> = q
                .iter(app.world())
                .filter(|(_, p)| p.parent() == e)
                .map(|(m, _)| m.0.id())
                .collect();
            ids.sort();
            ids
        };
        assert_eq!(meshes(&mut app, a), meshes(&mut app, b));

        // New options rebuild.
        let mut options = DeadwoodPreset::stick().deadwood.clone();
        options.default_seed = 9;
        *app.world_mut().get_mut::<DeadwoodParams>(a).unwrap() = DeadwoodParams::new(options);
        run_until_finished(&mut app, a);
        assert_ne!(meshes(&mut app, a), meshes(&mut app, b));
        assert_eq!(app.world().resource::<Finished>().0.len(), 3);
    }

    #[test]
    fn preset_files_load_over_their_base() {
        // Write a partial preset file into a fresh asset folder.
        let dir = std::env::temp_dir().join(format!(
            "procgena_deadwood_bevy_assets_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("short.deadwood.yml"),
            "base: stick\ndeadwood:\n  default_seed: 9\n  bark:\n    cracks: 0.8\n",
        )
        .unwrap();

        let mut app = app_with(AssetPlugin {
            file_path: dir.to_string_lossy().into_owned(),
            ..default()
        });
        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<DeadwoodPresetAsset>("short.deadwood.yml");
        let entity = app
            .world_mut()
            .spawn(DeadwoodParams::Asset(handle.clone()))
            .id();
        run_until_finished(&mut app, entity);
        std::fs::remove_dir_all(&dir).unwrap();

        // Settings the file leaves out keep the base's values.
        let mut expected = DeadwoodPreset::stick().deadwood.clone();
        expected.default_seed = 9;
        expected.bark.cracks = 0.8;
        let assets = app.world().resource::<Assets<DeadwoodPresetAsset>>();
        let preset = &assets.get(&handle).unwrap().0;
        assert_eq!(preset.deadwood, expected);
        assert_eq!(preset.key, "short");
        assert!(!parts(&mut app, entity).is_empty());
        assert_eq!(app.world().resource::<Finished>().0, vec![Ok(())]);
    }

    #[test]
    fn invalid_options_finish_with_an_error() {
        let mut app = app();
        let options = DeadwoodOptions {
            length: 0.0,
            ..DeadwoodOptions::default()
        };
        let entity = app.world_mut().spawn(DeadwoodParams::new(options)).id();
        run_until_finished(&mut app, entity);
        assert!(parts(&mut app, entity).is_empty());
        assert!(matches!(
            app.world().resource::<Finished>().0[..],
            [Err(DeadwoodBuildError::Options(_))]
        ));
    }

    /// The copies `entity` draws through the shared renderer, if any.
    fn copies(app: &App, entity: Entity) -> Vec<InstancedCopy> {
        app.world()
            .entity(entity)
            .get::<InstancedScatter>()
            .map(|scatter| scatter.copies.to_vec())
            .unwrap_or_default()
    }

    #[test]
    fn a_flat_scatter_draws_every_stick() {
        let mut app = app();
        let scatter = ScatterOptions::kindling();
        let entity = app
            .world_mut()
            .spawn(DeadwoodScatterParams {
                scatter: scatter.clone(),
                size: Vec2::splat(6.0),
                ground: ScatterGround::Flat,
            })
            .id();
        run_until_finished(&mut app, entity);
        let area = ScatterArea::new(DVec2::ZERO, Vec2::splat(6.0));
        let expected = StickScatter::new(
            &scatter,
            &ScatterInput::new(&FlatGround, area, scatter.default_seed),
        )
        .unwrap()
        .instances()
        .len();
        assert_eq!(copies(&app, entity).len(), expected);
        assert!(parts(&mut app, entity).is_empty(), "no entity per stick");
        assert!(!app.world().entity(entity).contains::<GeneratedSpecimen>());

        // Every variant is a slot with parts at every level that has
        // meshes, cached by specimen and level.
        let drawn = app
            .world()
            .entity(entity)
            .get::<InstancedScatter>()
            .unwrap();
        assert_eq!(drawn.slots.len(), scatter.variants as usize);
        let cache = app.world().resource::<SpecimenCache>();
        for slot in drawn.slots.iter() {
            assert!(slot.parts.iter().any(|p| p.level == 0));
            assert!(slot.bounds.1 > 0.0);
            let levels = slot.lod.levels.len() as u32;
            assert_eq!(
                cache.get_levels(slot.specimen, slot.piece, levels),
                Some(slot.parts.clone())
            );
        }
    }

    /// A host entity with a ground specimen, at `scale`.
    fn host(app: &mut App, scale: Vec3) -> Entity {
        let part = Part::new([tags::GROUND], SurfaceMesh::ground(4.0, 8));
        app.world_mut()
            .spawn((
                GeneratedSpecimen(Arc::new(Specimen::single(vec![part]))),
                Transform::from_scale(scale),
            ))
            .id()
    }

    /// Kindling over a 2 m square, laid on its parent's ground.
    fn host_scatter(filter: TagFilter) -> DeadwoodScatterParams {
        DeadwoodScatterParams {
            scatter: ScatterOptions {
                density: 6.0,
                ..ScatterOptions::kindling()
            },
            size: Vec2::splat(2.0),
            ground: ScatterGround::Host { filter, cell: 0.1 },
        }
    }

    #[test]
    fn a_host_scatter_waits_for_its_host_and_lies_on_it() {
        let mut app = app();
        let ground = TagFilter::any_of([tags::GROUND]);
        let orphan = app.world_mut().spawn(host_scatter(ground.clone())).id();
        app.update();
        assert!(app.world().entity(orphan).contains::<DeadwoodGenerating>());

        let host = host(&mut app, Vec3::splat(2.0));
        let entity = app
            .world_mut()
            .spawn((host_scatter(ground), ChildOf(host)))
            .id();
        run_until_finished(&mut app, entity);
        assert!(!copies(&app, entity).is_empty());
        assert_eq!(app.world().resource::<Finished>().0, vec![Ok(())]);

        // The host is scaled by 2, so each stick is scaled by a half to stay
        // in world metres.
        for copy in copies(&app, entity) {
            let (scale, _, _) = copy.transform.to_scale_rotation_translation();
            assert!((scale - Vec3::splat(0.5)).abs().max_element() < 1e-4);
        }

        // Losing the host's specimen removes the sticks.
        app.world_mut()
            .entity_mut(host)
            .remove::<GeneratedSpecimen>();
        app.update();
        assert!(copies(&app, entity).is_empty());
        assert!(app.world().entity(entity).contains::<DeadwoodGenerating>());
    }

    #[test]
    fn host_scatter_errors() {
        // A stretched host.
        let mut app = app();
        let host = host(&mut app, Vec3::new(1.0, 2.0, 1.0));
        let ground = TagFilter::any_of([tags::GROUND]);
        app.world_mut().spawn((host_scatter(ground), ChildOf(host)));
        let rock = TagFilter::any_of([tags::ROCK]);
        let entity = app
            .world_mut()
            .spawn((host_scatter(rock), ChildOf(host)))
            .id();
        run_until_finished(&mut app, entity);
        let mut results = app.world().resource::<Finished>().0.clone();
        results.sort_by_key(|r| format!("{r:?}"));
        assert_eq!(
            results,
            vec![
                Err(DeadwoodBuildError::NonUniformScale),
                Err(DeadwoodBuildError::NonUniformScale),
            ]
        );

        // A host whose specimen has no matching ground.
        let mut app = self::app();
        let host = self::host(&mut app, Vec3::ONE);
        let rock = TagFilter::any_of([tags::ROCK]);
        let entity = app
            .world_mut()
            .spawn((host_scatter(rock), ChildOf(host)))
            .id();
        run_until_finished(&mut app, entity);
        assert_eq!(
            app.world().resource::<Finished>().0,
            vec![Err(DeadwoodBuildError::NoGround)]
        );
    }

    #[test]
    fn levels_hand_over_and_fade_out_at_the_cull_distance() {
        let lod = default_lod();
        let cull = lod.cull_distance.unwrap();
        // A piece twice the reference size, drawn at low quality.
        let size = 2.0 * lod.reference_size;
        let scale = 2.0 * Quality::Low.distance_scale();
        let ranges = variant_ranges(&lod, size, Quality::Low);
        assert_eq!(ranges.len(), lod.levels.len());
        assert_eq!(ranges[0].start_margin, 0.0..0.0);
        for pair in ranges.windows(2) {
            assert_eq!(pair[0].end_margin, pair[1].start_margin);
        }
        assert!(
            ranges[1]
                .start_margin
                .contains(&(lod.levels[1].distance * scale))
        );
        assert!(ranges.last().unwrap().end_margin.contains(&(cull * scale)));
    }
}
