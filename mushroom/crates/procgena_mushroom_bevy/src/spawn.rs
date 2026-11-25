//! Components that request mushrooms, and the systems that build them on
//! background threads and draw them: a mushroom or cluster as child
//! entities, one per mesh step and member plus one impostor, and a scatter
//! through the shared instanced renderer.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::VisibilityRange;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use bevy::transform::helper::TransformHelper;
use procgena_core::chain::{DistanceChain, Lod, Step, ViewScale};
use procgena_core::scatter::{Mask, ScatterArea, ScatterInput};
use procgena_core::{ContentHasher, Ground, LodRange, Quality, SpecimenId, content_hash};
use procgena_core_bevy::instanced::{
    InstancedCopy, InstancedPart, InstancedScatter, InstancedSlot, PartMesh, SpecimenCache,
    chain_lod,
};
use procgena_core_bevy::{LodQuality, visibility_range};
use procgena_impostor::BakeSettings;
use procgena_mushroom::{
    BodyId, Mushroom, MushroomBody, MushroomDetail, MushroomInstance, MushroomOptions,
    MushroomPreset, MushroomScatter, OptionsError, ScatterError, ScatterOptions,
    TextureCompression, TexturePart, TextureSize,
};
use procgena_specimen::{GeneratedSpecimen, Specimen};

use crate::impostor::{BakedAs, ScatterImpostors, SingleImpostor, impostor_key};
use crate::material::{MushroomMaterial, TextureSet, mushroom_material};
use crate::mesh::mushroom_mesh;

/// Asks for one mushroom, or a cluster of them, standing at the entity's
/// position.
///
/// Spawn it after adding [`MushroomPlugin`](crate::MushroomPlugin). The
/// mushroom is built in the background and appears as [`MushroomPart`]
/// children: each member of a cluster (see [`MushroomOptions::cluster`]) at
/// its game level and its lower mesh, and, once baked, one impostor of the
/// whole mushroom or cluster. Which is drawn depends on how large the
/// mushroom or cluster looks on screen, the same for every kind of object
/// (see [`procgena_core::chain`]). Mushrooms with the same options share
/// meshes and materials, so many identical ones are cheap.
///
/// Once the parts appear, the entity also gets a [`GeneratedSpecimen`]
/// describing the mushroom, with every member of a cluster combined, so
/// other generators can grow on it. Changing the options rebuilds the
/// mushroom; removing this component removes the parts and the specimen.
#[derive(Component, Debug, Clone, PartialEq)]
#[require(Transform, Visibility)]
pub enum MushroomParams {
    /// Options held in memory. [`MushroomParams::new`] wraps them for you.
    Options(Arc<MushroomOptions>),
    /// A preset loaded from a `.mushroom.yml` file. The mushroom waits for
    /// the file to load, and rebuilds when the file changes if Bevy is
    /// watching asset files.
    Asset(Handle<MushroomPresetAsset>),
}

impl MushroomParams {
    /// Asks for a mushroom made from `options`.
    pub fn new(options: MushroomOptions) -> Self {
        Self::Options(Arc::new(options))
    }

    /// Asks for a mushroom made from a preset's options.
    pub fn preset(preset: &MushroomPreset) -> Self {
        Self::new(preset.mushroom.clone())
    }
}

/// Spreads mushrooms over this entity's parent.
///
/// Spawn it as a child of a host: an entity with a [`GeneratedSpecimen`],
/// such as a rock from `procgena_stone_bevy`. Leave this entity's own
/// transform at its default, so the mushrooms line up with the host.
///
/// It waits until the host has its specimen, and rebuilds when the specimen, the
/// host's rotation or scale, or these settings change. If the host loses its
/// specimen, the mushrooms are removed. [`ScatterOptions::up`] is in world
/// coordinates, and lengths stay in world metres however the host is scaled.
/// A host stretched differently along different axes is rejected with
/// [`MushroomBuildError::NonUniformScale`].
#[derive(Component, Debug, Clone, PartialEq)]
#[require(Transform, Visibility)]
pub struct MushroomScatterParams {
    /// The species to scatter.
    pub mushroom: Arc<MushroomOptions>,
    /// Where and how densely mushrooms grow.
    pub scatter: ScatterOptions,
}

/// Spreads whole mushrooms, or whole clusters such as fairy rings, over a
/// [`Ground`]; see [`MushroomScatter::new`].
///
/// Put the entity at the area's centre, at height 0 and without rotation
/// or scale: parts are placed relative to it, across from the centre and
/// up to the ground's own heights. [`ScatterOptions::default_seed`] seeds
/// the scatter, as its input seed. It rebuilds when the settings, the area or
/// the ground or mask (compared as shared copies) change.
#[derive(Component, Clone)]
#[require(Transform, Visibility)]
pub struct MushroomGroundScatterParams {
    /// The species to scatter.
    pub mushroom: Arc<MushroomOptions>,
    /// How densely mushrooms grow, and how many different ones.
    pub scatter: ScatterOptions,
    /// The ground to grow on.
    pub ground: Arc<dyn Ground>,
    /// Where on the ground to scatter, in World metres.
    pub area: ScatterArea,
    /// How much of the density is allowed where, or `None` for all of it.
    pub mask: Option<Arc<dyn Mask>>,
}

impl fmt::Debug for MushroomGroundScatterParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MushroomGroundScatterParams")
            .field("mushroom", &self.mushroom)
            .field("scatter", &self.scatter)
            .field("area", &self.area)
            .field("mask", &self.mask.is_some())
            .finish_non_exhaustive()
    }
}

/// A mushroom preset loaded from a `.mushroom.yml` file for
/// [`MushroomParams::Asset`].
///
/// The file names a built-in preset as its `base` and gives only what
/// differs; see [`MushroomPreset::from_yaml`]. Its key is the file name
/// without `.mushroom.yml`. Files with unknown or out-of-range settings
/// fail to load.
#[derive(Asset, TypePath, Debug, Clone, PartialEq)]
pub struct MushroomPresetAsset(pub MushroomPreset);

/// Marks a child entity that draws one step of a single mushroom or
/// cluster: one member's game-level or lower mesh, or the impostor of the
/// whole mushroom or cluster. The plugin spawns and removes these; don't
/// add them yourself. Scatters draw their mushrooms through an
/// [`InstancedScatter`] instead.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MushroomPart {
    /// Which mushroom: 0 for a single mushroom, or the member's number in a
    /// cluster. 0 for the impostor, which shows every member.
    pub instance: usize,
    /// Which step of the drawing chain this part draws.
    pub step: Step,
}

/// Present while a mushroom or scatter is being prepared, including while
/// it waits for its options file or its host.
#[derive(Component, Debug, Default)]
pub struct MushroomGenerating;

/// Event triggered on a mushroom or scatter entity when a build ends,
/// successfully or not.
///
/// Observe it on the entity, or globally, to react when mushrooms appear.
/// On success, the new [`MushroomPart`] children have been queued with the
/// same commands, so they exist once this frame's commands have run.
#[derive(EntityEvent, Debug, Clone, PartialEq)]
pub struct MushroomFinished {
    /// The entity that was built.
    pub entity: Entity,
    /// `Ok` when the parts were spawned, or why the build failed.
    pub result: Result<(), MushroomBuildError>,
}

/// Why a mushroom or scatter could not be built.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MushroomBuildError {
    /// A setting is out of range.
    Options(OptionsError),
    /// The scatter could not grow on its host, for example because no part
    /// of the host matched its tag filter, or a ground scatter's area is too
    /// large.
    Scatter(ScatterError),
    /// The scatter's host is scaled differently along different axes.
    NonUniformScale,
}

impl fmt::Display for MushroomBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MushroomBuildError::Options(e) => write!(f, "{e}"),
            MushroomBuildError::Scatter(e) => write!(f, "{e}"),
            MushroomBuildError::NonUniformScale => {
                write!(f, "the scatter's host has a non-uniform scale")
            }
        }
    }
}

impl std::error::Error for MushroomBuildError {}

impl From<OptionsError> for MushroomBuildError {
    fn from(e: OptionsError) -> Self {
        MushroomBuildError::Options(e)
    }
}

/// Loads `.mushroom.yml` preset files, rejecting text that isn't a preset
/// file and unknown or out-of-range settings.
#[derive(Debug, Default, TypePath)]
pub(crate) struct MushroomPresetLoader;

impl AssetLoader for MushroomPresetLoader {
    type Asset = MushroomPresetAsset;
    type Settings = ();
    type Error = BevyError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        context: &mut LoadContext<'_>,
    ) -> Result<MushroomPresetAsset, BevyError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let file_name = context
            .path()
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let key = file_name.strip_suffix(".mushroom.yml").unwrap_or(file_name);
        let preset = MushroomPreset::from_yaml(key, std::str::from_utf8(&bytes)?)?;
        Ok(MushroomPresetAsset(preset))
    }

    fn extensions(&self) -> &[&str] {
        &["mushroom.yml"]
    }
}

/// The plugin's settings, for the build systems.
#[derive(Resource, Debug, Clone)]
pub(crate) struct SpawnSettings {
    /// How impostors are baked, or `None` to draw each mushroom's lower
    /// mesh until it is culled.
    pub impostor: Option<BakeSettings>,
    /// Where to load cap textures from, or `None` for vertex colours only.
    pub textures: Option<TextureSettings>,
}

/// Where and in what form to load cap and stem textures.
#[derive(Debug, Clone)]
pub(crate) struct TextureSettings {
    /// Folder relative to Bevy's asset folder that holds `cap/` and
    /// `stem/`.
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
    /// One mushroom, or one cluster.
    Single(Arc<MushroomOptions>),
    /// A scatter over a host's specimen, with the host's world rotation and
    /// (uniform) scale.
    Scatter {
        mushroom: Arc<MushroomOptions>,
        scatter: ScatterOptions,
        host: Arc<Specimen>,
        rotation: Quat,
        scale: f32,
    },
    /// A scatter of whole mushrooms over a ground.
    Ground(MushroomGroundScatterParams),
}

// Host specimens can be large, so they are compared by identity (the same
// shared copy) rather than by content, as are grounds and masks.
impl PartialEq for Input {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Input::Single(a), Input::Single(b)) => a == b,
            (
                Input::Scatter {
                    mushroom: a,
                    scatter: s,
                    host: h,
                    rotation: r,
                    scale: k,
                },
                Input::Scatter {
                    mushroom: b,
                    scatter: t,
                    host: i,
                    rotation: q,
                    scale: l,
                },
            ) => a == b && s == t && Arc::ptr_eq(h, i) && r == q && k == l,
            (Input::Ground(a), Input::Ground(b)) => {
                let same_mask = match (&a.mask, &b.mask) {
                    (Some(m), Some(n)) => Arc::ptr_eq(m, n),
                    (None, None) => true,
                    _ => false,
                };
                a.mushroom == b.mushroom
                    && a.scatter == b.scatter
                    && a.area == b.area
                    && Arc::ptr_eq(&a.ground, &b.ground)
                    && same_mask
            }
            _ => false,
        }
    }
}

impl Input {
    /// Returns how much the parent entity enlarges the built meshes: the
    /// host's scale for a scatter, 1 otherwise. Used to scale switching
    /// distances.
    fn world_scale(&self) -> f32 {
        match self {
            Input::Scatter { scale, .. } => *scale,
            _ => 1.0,
        }
    }
}

/// On an entity while its build runs on a background thread.
#[derive(Component)]
pub(crate) struct Building {
    /// What is being built.
    input: Input,
    /// The background work.
    task: Task<Result<Built, MushroomBuildError>>,
}

/// The input of the entity's last finished build, successful or not, so it
/// isn't built again until the input changes.
#[derive(Component)]
pub(crate) struct Attempted(Input);

/// What a background build produces, before anything is handed to Bevy's
/// asset storage.
struct Built {
    /// The different mushrooms to draw.
    variants: Vec<BuiltVariant>,
    /// Where to draw copies of them.
    instances: Vec<MushroomInstance>,
    /// What only a single mushroom or cluster has; `None` for scatters.
    single: Option<BuiltSingle>,
    /// For a scatter, each variant's specimen id and its piece (its body's
    /// place among the specimen's bodies); empty for a mushroom or cluster.
    pieces: Vec<(SpecimenId, u32)>,
}

/// What a background build of a single mushroom or cluster produces besides
/// its bodies' meshes.
struct BuiltSingle {
    /// The combined specimen, every member included.
    specimen: Specimen,
    /// The diameter of a sphere around every member, which sets how large
    /// the mushroom or cluster looks on screen.
    diameter: f32,
    /// For each variant, the game-level meshes of every member drawn with it,
    /// placed as in the cluster, for the impostor bake; `None` for a variant
    /// with no triangles.
    bake_meshes: Vec<Option<Mesh>>,
    /// The specimen the impostor is baked for, and the hash of the game
    /// level it is baked from.
    impostor: (SpecimenId, u64),
}

/// Names one body meshed at one game level: its id, and the content hash of
/// its game level. Bodies with equal keys share uploaded meshes.
type BodyKey = (BodyId, u64);

/// One variant's game-level and lower meshes.
struct BuiltVariant {
    /// Which body this is.
    key: BodyKey,
    /// The variant's options.
    options: MushroomOptions,
    /// The game-level mesh, then the lower mesh.
    levels: [BuiltLevel; 2],
    /// A sphere around both meshes, as a centre and radius.
    bounds: (Vec3, f32),
    /// The body's bounding diameter (see
    /// [`MushroomBody::bounding_diameter`]), before a copy's scale.
    diameter: f32,
}

/// One mesh step's surfaces.
struct BuiltLevel {
    /// The surfaces, or `None` if the mesh has no triangles.
    surfaces: Option<Mesh>,
    /// True if the surfaces must be drawn two-sided.
    double_sided: bool,
}

/// One variant's meshes and materials, after adding them to Bevy's assets.
struct VariantAssets {
    /// The game-level mesh, then the lower mesh.
    levels: [LevelAssets; 2],
}

/// One mesh step's mesh and material handles: `None` if it has no
/// triangles.
type LevelAssets = Option<(Handle<Mesh>, Handle<MushroomMaterial>)>;

/// Uploaded variants and materials, shared by every entity that needs the
/// same ones.
#[derive(Resource, Default)]
pub(crate) struct MushroomCache {
    /// Variants by [`BuiltVariant::key`].
    variants: HashMap<BodyKey, Arc<VariantAssets>>,
    /// Materials by the content hash of their settings (see
    /// [`material_hash`]) and two-sidedness.
    materials: HashMap<(u64, bool), Handle<MushroomMaterial>>,
    /// Cap and stem texture images by part and set name, kept for reuse.
    texture_sets: HashMap<(TexturePart, String), TextureSet>,
}

// --------------------------------------------------------------------------
// Systems

/// Removes the parts, markers and specimen of entities whose mushroom or
/// scatter component was removed.
pub(crate) fn remove_mushrooms(
    mut commands: Commands,
    mut singles: RemovedComponents<MushroomParams>,
    mut scatters: RemovedComponents<MushroomScatterParams>,
    mut grounds: RemovedComponents<MushroomGroundScatterParams>,
    children: Query<&Children>,
    parts: Query<(), With<MushroomPart>>,
) {
    let removed: Vec<Entity> = singles
        .read()
        .chain(scatters.read())
        .chain(grounds.read())
        .collect();
    for entity in removed {
        despawn_parts(&mut commands, children.get(entity).ok(), &parts);
        if let Ok(mut entity) = commands.get_entity(entity) {
            entity.remove::<(MushroomGenerating, Building, Attempted, Drawn)>();
        }
    }
}

/// What a finished build adds to a mushroom or scatter entity, besides its
/// parts.
type Drawn = (
    GeneratedSpecimen,
    InstancedScatter,
    SingleImpostor,
    ScatterImpostors,
);

/// Despawns the [`MushroomPart`]s among `kids`, leaving other children
/// alone.
fn despawn_parts(
    commands: &mut Commands,
    kids: Option<&Children>,
    parts: &Query<(), With<MushroomPart>>,
) {
    for &child in kids.into_iter().flatten() {
        if parts.contains(child) {
            commands.entity(child).despawn();
        }
    }
}

/// What an entity with a mushroom or scatter component needs this frame.
enum Wanted {
    /// Nothing yet: its options file or its host's specimen isn't available.
    Wait,
    /// The input can't be built, for this reason.
    Fail(Input, MushroomBuildError),
    /// The input to build.
    Build(Input),
}

/// Starts a background build for every entity whose input differs from its
/// running or last finished build, and marks entities that are waiting.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn start_generation(
    mut commands: Commands,
    singles: Query<(Entity, &MushroomParams)>,
    scatters: Query<(Entity, &MushroomScatterParams, Option<&ChildOf>)>,
    grounds: Query<(Entity, &MushroomGroundScatterParams)>,
    state: Query<(
        Option<&Building>,
        Option<&Attempted>,
        Option<&Children>,
        Has<MushroomGenerating>,
    )>,
    hosts: Query<&GeneratedSpecimen>,
    transforms: TransformHelper,
    assets: Res<Assets<MushroomPresetAsset>>,
    parts: Query<(), With<MushroomPart>>,
) {
    // Work out what each entity needs.
    let mut wanted: Vec<(Entity, Wanted)> = Vec::new();
    for (entity, params) in &singles {
        let options = match params {
            MushroomParams::Options(o) => Some(o.clone()),
            MushroomParams::Asset(handle) => {
                assets.get(handle).map(|a| Arc::new(a.0.mushroom.clone()))
            }
        };
        wanted.push((
            entity,
            options.map_or(Wanted::Wait, |o| Wanted::Build(Input::Single(o))),
        ));
    }
    for (entity, params) in &grounds {
        wanted.push((entity, Wanted::Build(Input::Ground(params.clone()))));
    }
    for (entity, params, parent) in &scatters {
        // Wait for the host's specimen. Work out the host's world transform
        // from its parents, because Bevy may not have updated it yet this
        // frame.
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
        let (scale, rotation, _) = transform.to_scale_rotation_translation();
        let input = Input::Scatter {
            mushroom: params.mushroom.clone(),
            scatter: params.scatter.clone(),
            host: specimen.0.clone(),
            rotation,
            scale: scale.x,
        };
        let tolerance = scale.abs().max_element() * 1e-4;
        let uniform =
            (scale.x - scale.y).abs() <= tolerance && (scale.x - scale.z).abs() <= tolerance;
        wanted.push((
            entity,
            if uniform {
                Wanted::Build(input)
            } else {
                Wanted::Fail(input, MushroomBuildError::NonUniformScale)
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
                // If the host lost its specimen, remove the mushrooms grown
                // on it.
                if building.is_some() || attempted.is_some() {
                    despawn_parts(&mut commands, kids, &parts);
                    commands
                        .entity(entity)
                        .remove::<(Building, Attempted, Drawn)>();
                }
                if !generating {
                    commands.entity(entity).insert(MushroomGenerating);
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
                    .remove::<(Building, MushroomGenerating, Drawn)>()
                    .insert(Attempted(input));
                commands.trigger(MushroomFinished {
                    entity,
                    result: Err(error),
                });
                continue;
            }
            Wanted::Build(input) => input,
        };
        let task_input = input.clone();
        let task = AsyncComputeTaskPool::get().spawn(async move { build(&task_input) });
        commands
            .entity(entity)
            .remove::<GeneratedSpecimen>()
            .insert((MushroomGenerating, Building { input, task }));
    }
}

/// Returns true if `input` is already being built, or was the last thing
/// built.
fn is_current(input: &Input, building: Option<&Building>, attempted: Option<&Attempted>) -> bool {
    building.is_some_and(|b| b.input == *input) || attempted.is_some_and(|a| a.0 == *input)
}

/// Builds the meshes and placements for `input`, each body at its game
/// level and its lower mesh. Runs on a background thread.
fn build(input: &Input) -> Result<Built, MushroomBuildError> {
    match input {
        Input::Single(options) => {
            let mushroom = Mushroom::new((**options).clone())?;
            let variants = mushroom
                .bodies()
                .iter()
                .map(build_variant)
                .collect::<Result<Vec<_>, _>>()?;
            let options = mushroom.options();
            let game = drawn_lod(options).game;
            let single = BuiltSingle {
                specimen: mushroom.specimen(),
                diameter: mushroom.bounding_diameter(),
                bake_meshes: placed_game_meshes(&variants, mushroom.members()),
                impostor: (
                    SpecimenId::new(options.default_seed, options.pool(), 0),
                    content_hash(&game),
                ),
            };
            Ok(Built {
                variants,
                instances: mushroom.members().to_vec(),
                single: Some(single),
                pieces: Vec::new(),
            })
        }
        Input::Scatter {
            mushroom,
            scatter,
            host,
            rotation,
            scale,
        } => {
            // Work in the host's own coordinates: turn the world "up" into
            // them, and adjust lengths and sizes for the host's scale so
            // they stay right in world metres.
            let mut options = scatter.clone();
            options.up = rotation.inverse() * scatter.up;
            options.min_spacing /= scale;
            options.density *= scale * scale;
            let mut species = (**mushroom).clone();
            species.size /= scale;
            let built = MushroomScatter::on(host, &species, &options)
                .map_err(MushroomBuildError::Scatter)?;
            let variants = built
                .bodies()
                .iter()
                .map(build_variant)
                .collect::<Result<Vec<_>, _>>()?;
            // The scatter grows slots 0, 1, … of the species' pool.
            let slots = species.cluster.variants;
            let pieces = built
                .bodies()
                .iter()
                .map(|b| body_piece(b.id(), options.default_seed, slots))
                .collect();
            Ok(Built {
                variants,
                instances: built.instances().to_vec(),
                single: None,
                pieces,
            })
        }
        Input::Ground(params) => {
            let mut input =
                ScatterInput::new(&*params.ground, params.area, params.scatter.default_seed);
            if let Some(mask) = &params.mask {
                input = input.with_mask(&**mask);
            }
            let built = MushroomScatter::new(&params.mushroom, &params.scatter, &input)
                .map_err(MushroomBuildError::Scatter)?;
            let variants = built
                .bodies()
                .iter()
                .map(build_variant)
                .collect::<Result<Vec<_>, _>>()?;
            // The scatter grows slots 0, 1, … of the mushroom's pool, in the
            // World of the input's seed.
            let slots = params.scatter.specimens;
            let pieces = built
                .bodies()
                .iter()
                .map(|b| body_piece(b.id(), input.seed, slots))
                .collect();
            Ok(Built {
                variants,
                instances: built.instances().to_vec(),
                single: None,
                pieces,
            })
        }
    }
}

/// The specimen a scattered body belongs to, and its piece (its place among
/// the specimen's bodies): the body's pool in the World seeded with
/// `world_seed`, at the slot below `slots` whose specimen seed grew it.
///
/// A body that no such slot grew (which a scatter never makes) gets slot
/// `u32::MAX`, past any real slot.
fn body_piece(body: BodyId, world_seed: u32, slots: u32) -> (SpecimenId, u32) {
    let slot = (0..slots)
        .find(|&slot| SpecimenId::new(world_seed, body.pool, slot).seed() == body.seed)
        .unwrap_or(u32::MAX);
    (SpecimenId::new(world_seed, body.pool, slot), body.index)
}

/// The details a mushroom made from `options` is drawn at: its game level
/// and lower mesh, with warts left out of the vertex colours, because the
/// material draws them.
fn drawn_lod(options: &MushroomOptions) -> Lod<MushroomDetail> {
    options.game_level.lod().map(|detail| MushroomDetail {
        paint_warts: false,
        ..detail
    })
}

/// Meshes `body` at its game level and its lower mesh, and converts the
/// meshes for Bevy.
fn build_variant(body: &MushroomBody) -> Result<BuiltVariant, OptionsError> {
    let options = body.options().clone();
    let lod = drawn_lod(&options);
    let levels = body.lod_meshes(&lod)?.map(|(_, mesh)| BuiltLevel {
        surfaces: (mesh.surfaces.triangle_count() > 0).then(|| mushroom_mesh(&mesh.surfaces)),
        double_sided: mesh.double_sided,
    });
    let bounds = sphere_around(
        levels
            .iter()
            .filter_map(|level| level.surfaces.as_ref().and_then(mesh_box)),
    );
    Ok(BuiltVariant {
        key: (body.id(), content_hash(&lod.game)),
        options,
        levels,
        bounds,
        diameter: body.bounding_diameter(),
    })
}

/// For each of `variants`, its game-level mesh copied to every member in
/// `members` drawn with it, in one mesh; `None` for a variant with no
/// triangles or no members.
fn placed_game_meshes(
    variants: &[BuiltVariant],
    members: &[MushroomInstance],
) -> Vec<Option<Mesh>> {
    variants
        .iter()
        .enumerate()
        .map(|(index, variant)| {
            let game = variant.levels[Step::Game.index()].surfaces.as_ref()?;
            let mut placed: Option<Mesh> = None;
            for member in members.iter().filter(|m| m.variant as usize == index) {
                let copy = game.clone().transformed_by(instance_transform(member));
                match &mut placed {
                    // Every copy comes from the same mesh, so they always
                    // have the same attributes and merge.
                    Some(mesh) => mesh.merge(&copy).ok()?,
                    None => placed = Some(copy),
                }
            }
            placed
        })
        .collect()
}

/// Where `instance` puts its mushroom, relative to its cluster or scatter.
fn instance_transform(instance: &MushroomInstance) -> Transform {
    Transform {
        translation: instance.translation,
        rotation: instance.rotation,
        scale: Vec3::splat(instance.scale),
    }
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
/// the new ones, adds its [`GeneratedSpecimen`] if it has one, and triggers
/// [`MushroomFinished`]. A failed build is logged and leaves no parts.
#[expect(clippy::too_many_arguments, reason = "Bevy system parameters")]
pub(crate) fn finish_generation(
    mut commands: Commands,
    mut building: Query<(Entity, &mut Building, Option<&Children>)>,
    parts: Query<(), With<MushroomPart>>,
    mut cache: ResMut<MushroomCache>,
    mut specimens: ResMut<SpecimenCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<MushroomMaterial>>,
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
            .remove::<(Building, MushroomGenerating)>()
            .insert(Attempted(input.clone()));

        let mut built = match result {
            Ok(built) => built,
            Err(error) => {
                error!("procgena_mushroom_bevy: cannot build mushrooms: {error}");
                commands.entity(entity).remove::<Drawn>();
                commands.trigger(MushroomFinished {
                    entity,
                    result: Err(error),
                });
                continue;
            }
        };

        let textures = (&*asset_server, settings.textures.as_ref());
        let Some(single) = built.single.take() else {
            // A scatter draws through the shared instanced renderer, and
            // waits for each body's impostor when impostors are on.
            let game_levels: Vec<u64> = built.variants.iter().map(|v| v.key.1).collect();
            let mut uploads = (&mut *cache, &mut *meshes, &mut *materials, textures);
            let scatter = scatter_draw(built, input.world_scale(), &mut specimens, &mut uploads);
            let mut drawn = commands.entity(entity);
            drawn.remove::<(GeneratedSpecimen, SingleImpostor, ScatterImpostors)>();
            if let Some(bake) = settings.impostor {
                drawn.insert(ScatterImpostors::new(
                    &scatter.slots,
                    &game_levels,
                    bake,
                    &settings,
                ));
            }
            drawn.insert(scatter);
            commands.trigger(MushroomFinished {
                entity,
                result: Ok(()),
            });
            continue;
        };

        // Add each variant to Bevy's assets, reusing identical variants that
        // other entities already added.
        let variants: Vec<Arc<VariantAssets>> = built
            .variants
            .into_iter()
            .map(|v| upload(v, &mut cache, &mut meshes, &mut materials, textures))
            .collect();

        // Spawn one child per member and mesh step. The whole mushroom or
        // cluster switches together, by its size on screen, so every member
        // shares the same bands. Until an impostor is drawn, the lower mesh
        // is drawn on until the cull.
        let chain = chain_ranges(single.diameter, quality.0);
        let mesh_ranges = mesh_ranges(&chain);
        for (i, instance) in built.instances.iter().enumerate() {
            let variant = &variants[instance.variant as usize];
            let transform = instance_transform(instance);
            for (step, level) in [Step::Game, Step::Lower].into_iter().zip(&variant.levels) {
                let Some((mesh, material)) = level else {
                    continue;
                };
                let mut part = commands.spawn((
                    MushroomPart { instance: i, step },
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    transform,
                    ChildOf(entity),
                ));
                if let Some(range) = bevy_range(&mesh_ranges[step.index()]) {
                    part.insert(range);
                }
            }
        }

        let mut drawn = commands.entity(entity);
        drawn
            .remove::<(InstancedScatter, ScatterImpostors, SingleImpostor)>()
            .insert(GeneratedSpecimen(Arc::new(single.specimen)));
        if let Some(bake) = settings.impostor {
            // Each variant's placed members bake with that variant's
            // game-level material.
            let parts = single
                .bake_meshes
                .into_iter()
                .zip(&variants)
                .filter_map(|(mesh, variant)| {
                    let (_, material) = variant.levels[Step::Game.index()].as_ref()?;
                    Some((mesh?, material.clone()))
                })
                .collect();
            let (specimen, game_level) = single.impostor;
            drawn.insert(SingleImpostor {
                key: impostor_key(specimen, BakedAs::Single, game_level, bake, &settings),
                parts,
                ranges: chain,
            });
        }
        commands.trigger(MushroomFinished {
            entity,
            result: Ok(()),
        });
    }
}

/// The fade bands of each step of the shared [`DistanceChain`], by
/// [`Step::index`], for an object whose bounding diameter in the world is
/// `diameter`, seen through a [`ViewScale::DEFAULT`] camera at `quality`.
pub(crate) fn chain_ranges(diameter: f32, quality: Quality) -> [LodRange; 3] {
    let factor = DistanceChain::factor(diameter, ViewScale::DEFAULT);
    DistanceChain::DEFAULT.ranges(factor, quality)
}

/// The fade bands of the game-level and lower meshes with no impostor drawn:
/// as in `chain`, but the lower mesh is drawn on until the cull, where the
/// impostor would have ended.
pub(crate) fn mesh_ranges(chain: &[LodRange; 3]) -> [LodRange; 2] {
    let [game, lower, impostor] = chain.clone();
    let lower = LodRange {
        fade_in: lower.fade_in,
        fade_out: impostor.fade_out,
    };
    [game, lower]
}

/// Bevy's visibility range for a step drawn over `range`, or `None` for a
/// step drawn at every distance.
pub(crate) fn bevy_range(range: &LodRange) -> Option<VisibilityRange> {
    (!range.is_always()).then(|| visibility_range(range))
}

/// The asset server, and where to load textures from when they are wanted.
type TextureSource<'a> = (&'a AssetServer, Option<&'a TextureSettings>);

/// Builds the shared renderer's view of a built scatter: a slot per body,
/// with its game-level and lower parts from `specimens` (uploading bodies
/// that aren't cached yet with `uploads`), and a copy per mushroom. Each
/// copy switches steps by its size in the world: the body's bounding
/// diameter times the copy's scale and `world_scale`, the parent's. Until a
/// body's impostor is added, its lower mesh is drawn until the cull.
fn scatter_draw(
    built: Built,
    world_scale: f32,
    specimens: &mut SpecimenCache,
    (cache, meshes, materials, textures): &mut (
        &mut MushroomCache,
        &mut Assets<Mesh>,
        &mut Assets<MushroomMaterial>,
        TextureSource,
    ),
) -> InstancedScatter {
    let slot_lod = chain_lod(&DistanceChain::DEFAULT, false);
    let levels = slot_lod.levels.len() as u32;
    let mut diameters = Vec::with_capacity(built.variants.len());
    let slots: Vec<InstancedSlot> = built
        .variants
        .into_iter()
        .zip(&built.pieces)
        .map(|(variant, &(specimen, piece))| {
            let bounds = variant.bounds;
            diameters.push(variant.diameter);
            let parts = specimens
                .get_levels(specimen, piece, levels)
                .unwrap_or_else(|| {
                    let assets = upload(variant, cache, meshes, materials, *textures);
                    let mut parts = Vec::new();
                    for (level, assets) in assets.levels.iter().enumerate() {
                        let level_parts = level_parts(level as u32, assets.as_ref());
                        specimens.insert(specimen, piece, level as u32, level_parts.clone());
                        parts.extend(level_parts);
                    }
                    parts
                });
            InstancedSlot {
                specimen,
                piece,
                lod: slot_lod.clone(),
                parts,
                bounds,
            }
        })
        .collect();
    let copies: Vec<InstancedCopy> = built
        .instances
        .iter()
        .map(|instance| InstancedCopy {
            slot: instance.variant,
            transform: instance_transform(instance).compute_affine(),
            size: diameters[instance.variant as usize] * instance.scale * world_scale,
            wind_phase: 0.0,
        })
        .collect();
    InstancedScatter {
        slots: slots.into(),
        copies: copies.into(),
    }
}

/// One mesh step's parts for the shared renderer: its surfaces, or nothing
/// if the step has no triangles.
fn level_parts(
    level: u32,
    assets: Option<&(Handle<Mesh>, Handle<MushroomMaterial>)>,
) -> Vec<InstancedPart> {
    assets
        .into_iter()
        .map(|(mesh, material)| InstancedPart {
            level,
            mesh: PartMesh::Mesh(mesh.clone()),
            material: material.clone().untyped(),
        })
        .collect()
}

/// Adds a variant's meshes and materials to Bevy's assets and returns their
/// handles, or returns the cached handles for an identical variant.
/// Materials are shared by every variant with the same material settings.
///
/// `textures` is the asset server and, when textures are wanted, where to
/// load them from; the variant's cap and stem texture sets, if it names
/// them, are loaded once and then reused.
fn upload(
    variant: BuiltVariant,
    cache: &mut MushroomCache,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<MushroomMaterial>,
    (asset_server, textures): TextureSource,
) -> Arc<VariantAssets> {
    if let Some(found) = cache.variants.get(&variant.key) {
        return found.clone();
    }

    // The cap and stem texture sets, if this mushroom names them and
    // textures are on.
    let m = &variant.options.material;
    let mut texture_set = |part: TexturePart, name: &Option<String>| match (name, textures) {
        (Some(set), Some(t)) => Some(
            cache
                .texture_sets
                .entry((part, set.clone()))
                .or_insert_with(|| {
                    TextureSet::load(asset_server, &t.root, part, set, t.size, t.compression)
                })
                .clone(),
        ),
        _ => None,
    };
    let cap = texture_set(TexturePart::Cap, &m.cap_texture);
    let stem = texture_set(TexturePart::Stem, &m.stem_texture);

    let o = &variant.options;
    let material_key = material_hash(o);
    let mut material = |double_sided: bool| {
        cache
            .materials
            .entry((material_key, double_sided))
            .or_insert_with(|| {
                materials.add(mushroom_material(
                    o,
                    double_sided,
                    cap.as_ref(),
                    stem.as_ref(),
                ))
            })
            .clone()
    };
    let levels = variant.levels.map(|level| {
        level
            .surfaces
            .map(|mesh| (meshes.add(mesh), material(level.double_sided)))
    });
    let assets = Arc::new(VariantAssets { levels });
    cache.variants.insert(variant.key, assets.clone());
    assets
}

/// Returns the content hash of every setting in `o` that a mushroom's
/// materials depend on: the material settings, the size, and the warts and
/// cap outline (for warts). Mushrooms with equal hashes share materials.
fn material_hash(o: &MushroomOptions) -> u64 {
    let mut h = ContentHasher::new();
    h.write(&o.material);
    h.write(&o.size);
    h.write(&o.warts);
    h.write(&o.cap.family);
    h.write(&o.cap.height);
    h.finish()
}

/// After parts are removed, forgets cached variants and materials that no
/// remaining part draws, so Bevy can free them.
#[expect(clippy::type_complexity, reason = "Bevy system parameters")]
pub(crate) fn prune_cache(
    mut removed: RemovedComponents<MushroomPart>,
    mut removed_scatters: RemovedComponents<InstancedScatter>,
    replaced: Query<
        (),
        (
            Changed<InstancedScatter>,
            Or<(
                With<MushroomScatterParams>,
                With<MushroomGroundScatterParams>,
            )>,
        ),
    >,
    parts: Query<&Mesh3d, With<MushroomPart>>,
    scatters: Query<
        &InstancedScatter,
        Or<(
            With<MushroomScatterParams>,
            With<MushroomGroundScatterParams>,
        )>,
    >,
    mut cache: ResMut<MushroomCache>,
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
        v.levels
            .iter()
            .flatten()
            .any(|(mesh, _)| drawn.contains(&mesh.id()))
    });
    let used: HashSet<AssetId<MushroomMaterial>> = cache
        .variants
        .values()
        .flat_map(|v| v.levels.iter().flatten())
        .map(|(_, material)| material.id())
        .collect();
    cache.materials.retain(|_, m| used.contains(&m.id()));
}

#[cfg(test)]
mod tests {
    use std::ops::Range;
    use std::time::{Duration, Instant};

    use bevy::light::NotShadowCaster;
    use procgena_specimen::{Part, SurfaceMesh, tags};

    use super::*;
    use crate::default_mushroom_impostor;

    /// Results of every [`MushroomFinished`] event, in order.
    #[derive(Resource, Default)]
    struct Finished(Vec<Result<(), MushroomBuildError>>);

    /// An app running only the build systems, without rendering.
    fn app() -> App {
        app_with(AssetPlugin::default())
    }

    /// An app running only the build systems, reading assets with `assets`.
    fn app_with(assets: AssetPlugin) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, assets))
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<MushroomMaterial>()
            .init_resource::<Finished>()
            .add_observer(
                |event: On<MushroomFinished>, mut finished: ResMut<Finished>| {
                    finished.0.push(event.result.clone());
                },
            );
        // Impostors are on, but without the impostor plugin nothing bakes:
        // mushrooms keep waiting for theirs.
        crate::add_systems(&mut app, Some(default_mushroom_impostor()));
        app
    }

    /// Updates until `entity` has finished a build, or panics after 30 s.
    fn run_until_finished(app: &mut App, entity: Entity) {
        let deadline = Instant::now() + Duration::from_secs(30);
        app.update();
        while app.world().entity(entity).contains::<MushroomGenerating>()
            || app.world().entity(entity).contains::<Building>()
        {
            assert!(Instant::now() < deadline, "build did not finish");
            std::thread::sleep(Duration::from_millis(1));
            app.update();
        }
        app.update();
    }

    /// The parts under `entity`.
    fn parts(app: &mut App, entity: Entity) -> Vec<MushroomPart> {
        let mut query = app.world_mut().query::<(&MushroomPart, &ChildOf)>();
        query
            .iter(app.world())
            .filter(|(_, parent)| parent.parent() == entity)
            .map(|(part, _)| *part)
            .collect()
    }

    #[test]
    fn a_mushroom_gets_both_meshes_its_specimen_and_waits_for_its_impostor() {
        let mut app = app();
        let entity = app
            .world_mut()
            .spawn(MushroomParams::preset(MushroomPreset::fly_agaric()))
            .id();
        run_until_finished(&mut app, entity);

        let steps: Vec<Step> = parts(&mut app, entity).iter().map(|p| p.step).collect();
        assert_eq!(steps.len(), 2, "{steps:?}");
        assert!(steps.contains(&Step::Game) && steps.contains(&Step::Lower));
        assert!(app.world().entity(entity).contains::<GeneratedSpecimen>());
        assert_eq!(app.world().resource::<Finished>().0, vec![Ok(())]);

        // The impostor is baked from the game-level mesh, with its material.
        let waiting = app.world().entity(entity).get::<SingleImpostor>().unwrap();
        assert_eq!(waiting.parts.len(), 1);

        // Removing the request removes the parts, the specimen and the wait
        // for the impostor.
        app.world_mut()
            .entity_mut(entity)
            .remove::<MushroomParams>();
        app.update();
        assert!(self::parts(&mut app, entity).is_empty());
        assert!(!app.world().entity(entity).contains::<GeneratedSpecimen>());
        assert!(!app.world().entity(entity).contains::<SingleImpostor>());
    }

    #[test]
    fn without_impostors_nothing_waits_for_one() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<MushroomMaterial>();
        crate::add_systems(&mut app, None);
        let entity = app
            .world_mut()
            .spawn(MushroomParams::preset(MushroomPreset::button()))
            .id();
        run_until_finished(&mut app, entity);
        assert!(!parts(&mut app, entity).is_empty());
        assert!(!app.world().entity(entity).contains::<SingleImpostor>());
    }

    #[test]
    fn identical_mushrooms_share_meshes_and_rebuild_on_change() {
        let mut app = app();
        let a = app
            .world_mut()
            .spawn(MushroomParams::preset(MushroomPreset::button()))
            .id();
        let b = app
            .world_mut()
            .spawn(MushroomParams::preset(MushroomPreset::button()))
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
        let mut options = MushroomPreset::button().mushroom.clone();
        options.default_seed = 9;
        *app.world_mut().get_mut::<MushroomParams>(a).unwrap() = MushroomParams::new(options);
        run_until_finished(&mut app, a);
        assert_ne!(meshes(&mut app, a), meshes(&mut app, b));
        assert_eq!(app.world().resource::<Finished>().0.len(), 3);
    }

    #[test]
    fn preset_files_load_on_their_base() {
        // Write a preset file into a fresh asset folder.
        let dir = std::env::temp_dir().join(format!(
            "procgena_mushroom_bevy_assets_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("pale.mushroom.yml"),
            "base: button\nmushroom:\n  default_seed: 9\n  material:\n    cap_centre: \"#c09060\"\n",
        )
        .unwrap();

        let mut app = app_with(AssetPlugin {
            file_path: dir.to_string_lossy().into_owned(),
            ..default()
        });
        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<MushroomPresetAsset>("pale.mushroom.yml");
        let entity = app
            .world_mut()
            .spawn(MushroomParams::Asset(handle.clone()))
            .id();
        run_until_finished(&mut app, entity);
        std::fs::remove_dir_all(&dir).unwrap();

        // Settings the file leaves out keep the base preset's values.
        let mut expected = MushroomPreset::button().mushroom.clone();
        expected.default_seed = 9;
        expected.material.cap_centre = "#c09060".into();
        let assets = app.world().resource::<Assets<MushroomPresetAsset>>();
        let preset = &assets.get(&handle).unwrap().0;
        assert_eq!(preset.mushroom, expected);
        assert_eq!(preset.key, "pale");
        assert!(!parts(&mut app, entity).is_empty());
        assert_eq!(app.world().resource::<Finished>().0, vec![Ok(())]);
    }

    #[test]
    fn invalid_options_finish_with_an_error() {
        let mut app = app();
        let options = MushroomOptions {
            size: 0.0,
            ..MushroomOptions::default()
        };
        let entity = app.world_mut().spawn(MushroomParams::new(options)).id();
        run_until_finished(&mut app, entity);
        assert!(parts(&mut app, entity).is_empty());
        assert!(matches!(
            app.world().resource::<Finished>().0[..],
            [Err(MushroomBuildError::Options(_))]
        ));
    }

    #[test]
    fn a_cluster_spawns_every_member() {
        let mut app = app();
        let mut options = MushroomPreset::button().mushroom.clone();
        options.cluster.stems = 4;
        let entity = app.world_mut().spawn(MushroomParams::new(options)).id();
        run_until_finished(&mut app, entity);
        let instances: HashSet<usize> =
            parts(&mut app, entity).iter().map(|p| p.instance).collect();
        assert!(instances.len() >= 3, "{instances:?}");
        assert!(app.world().entity(entity).contains::<GeneratedSpecimen>());

        // One impostor shows the whole cluster: it is baked from every
        // member's game-level mesh.
        let mut query = app
            .world_mut()
            .query::<(&MushroomPart, &Mesh3d, &ChildOf)>();
        let meshes = app.world().resource::<Assets<Mesh>>();
        let drawn: usize = query
            .iter(app.world())
            .filter(|(part, _, parent)| part.step == Step::Game && parent.parent() == entity)
            .map(|(_, mesh, _)| meshes.get(&mesh.0).unwrap().count_vertices())
            .sum();
        let waiting = app.world().entity(entity).get::<SingleImpostor>().unwrap();
        let baked: usize = waiting.parts.iter().map(|(m, _)| m.count_vertices()).sum();
        assert_eq!(baked, drawn);
    }

    /// A host entity with a ground specimen, at `scale`.
    fn host(app: &mut App, scale: Vec3) -> Entity {
        let part = Part::new([tags::GROUND], SurfaceMesh::ground(1.0, 4));
        app.world_mut()
            .spawn((
                GeneratedSpecimen(Arc::new(Specimen::single(vec![part]))),
                Transform::from_scale(scale),
            ))
            .id()
    }

    fn scatter() -> MushroomScatterParams {
        MushroomScatterParams {
            mushroom: Arc::new(MushroomPreset::porcini().mushroom.clone()),
            scatter: ScatterOptions {
                density: 20.0,
                ..ScatterOptions::default()
            },
        }
    }

    #[test]
    fn a_scatter_waits_for_its_host_and_grows_on_it() {
        let mut app = app();
        let orphan = app.world_mut().spawn(scatter()).id();
        app.update();
        assert!(app.world().entity(orphan).contains::<MushroomGenerating>());

        let host = host(&mut app, Vec3::ONE);
        let entity = app.world_mut().spawn((scatter(), ChildOf(host))).id();
        run_until_finished(&mut app, entity);
        assert!(!copies(&app, entity).is_empty());
        assert!(parts(&mut app, entity).is_empty(), "no entity per mushroom");
        assert_eq!(app.world().resource::<Finished>().0, vec![Ok(())]);

        // Every body is a slot with a real specimen id, cached by specimen,
        // piece and level.
        let drawn = app
            .world()
            .entity(entity)
            .get::<InstancedScatter>()
            .unwrap();
        let cache = app.world().resource::<SpecimenCache>();
        for slot in drawn.slots.iter() {
            assert!(
                slot.specimen.slot < u32::MAX,
                "a body with no specimen slot"
            );
            assert_eq!(slot.lod, chain_lod(&DistanceChain::DEFAULT, false));
            let levels = slot.lod.levels.len() as u32;
            assert_eq!(levels, 2, "the game-level and lower meshes");
            assert_eq!(
                cache.get_levels(slot.specimen, slot.piece, levels),
                Some(slot.parts.clone())
            );
        }
        // Each body waits for its impostor.
        assert!(app.world().entity(entity).contains::<ScatterImpostors>());

        // Losing the host's specimen removes the mushrooms.
        app.world_mut()
            .entity_mut(host)
            .remove::<GeneratedSpecimen>();
        app.update();
        assert!(copies(&app, entity).is_empty());
        assert!(app.world().entity(entity).contains::<MushroomGenerating>());
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
    fn a_ground_scatter_draws_each_member_at_its_ground_height() {
        let mut app = app();
        let ground: Arc<dyn Ground> =
            Arc::new(|p: bevy::math::DVec2| 0.3 * (p.x as f32 * 0.8).sin());
        let params = MushroomGroundScatterParams {
            mushroom: Arc::new(MushroomPreset::button().mushroom.clone()),
            scatter: ScatterOptions {
                density: 2.0,
                ..ScatterOptions::default()
            },
            ground: ground.clone(),
            area: ScatterArea::new(bevy::math::DVec2::ZERO, Vec2::splat(4.0)),
            mask: None,
        };
        let entity = app.world_mut().spawn(params.clone()).id();
        run_until_finished(&mut app, entity);
        assert_eq!(app.world().resource::<Finished>().0, vec![Ok(())]);

        // Every copy stands near the ground under it (sunk a little), and
        // switches steps by its body's bounding diameter at its scale.
        let copies = copies(&app, entity);
        assert!(!copies.is_empty());
        let built = MushroomScatter::new(
            &params.mushroom,
            &params.scatter,
            &ScatterInput::new(&*ground, params.area, params.scatter.default_seed),
        )
        .unwrap();
        for (copy, instance) in copies.iter().zip(built.instances()) {
            let t = Vec3::from(copy.transform.translation);
            let h = ground.height(bevy::math::DVec2::new(t.x.into(), t.z.into()));
            assert!((t.y - h).abs() < 0.05, "copy at {t} over ground {h}");
            let body = &built.bodies()[instance.variant as usize];
            assert_eq!(copy.size, body.bounding_diameter() * instance.scale);
        }

        // The same ground and settings don't rebuild; a new ground does.
        app.update();
        assert_eq!(app.world().resource::<Finished>().0.len(), 1);
        app.world_mut()
            .entity_mut(entity)
            .insert(MushroomGroundScatterParams {
                ground: Arc::new(procgena_core::FlatGround),
                ..params
            });
        run_until_finished(&mut app, entity);
        assert_eq!(app.world().resource::<Finished>().0.len(), 2);
    }

    #[test]
    fn a_stretched_host_is_rejected() {
        let mut app = app();
        let host = host(&mut app, Vec3::new(1.0, 2.0, 1.0));
        let entity = app.world_mut().spawn((scatter(), ChildOf(host))).id();
        run_until_finished(&mut app, entity);
        assert_eq!(
            app.world().resource::<Finished>().0,
            vec![Err(MushroomBuildError::NonUniformScale)]
        );
    }

    /// The fade-in and fade-out margins of `range`.
    fn margins(range: &VisibilityRange) -> (Range<f32>, Range<f32>) {
        (range.start_margin.clone(), range.end_margin.clone())
    }

    /// Each part under `entity` with its step, distance band and whether it
    /// casts shadows.
    fn ranges(app: &mut App, entity: Entity) -> Vec<(Step, VisibilityRange, bool)> {
        let mut query = app.world_mut().query::<(
            &MushroomPart,
            &VisibilityRange,
            Has<NotShadowCaster>,
            &ChildOf,
        )>();
        query
            .iter(app.world())
            .filter(|(.., parent)| parent.parent() == entity)
            .map(|(part, range, shadowless, _)| (part.step, range.clone(), !shadowless))
            .collect()
    }

    #[test]
    fn steps_follow_the_chain_and_the_lower_mesh_fades_out_at_the_cull() {
        let mut app = app();
        let options = MushroomPreset::porcini().mushroom.clone();
        let entity = app
            .world_mut()
            .spawn(MushroomParams::new(options.clone()))
            .id();
        run_until_finished(&mut app, entity);

        // Each mesh shows over its step's band for the mushroom's size on
        // screen, with the lower mesh drawn until the cull while no impostor
        // is drawn, and both cast shadows.
        let diameter = Mushroom::new(options).unwrap().bounding_diameter();
        let chain = chain_ranges(diameter, Quality::High);
        let expected = mesh_ranges(&chain);
        let parts = ranges(&mut app, entity);
        assert_eq!(parts.len(), 2);
        for (step, range, shadows) in &parts {
            let want = visibility_range(&expected[step.index()]);
            assert_eq!(margins(range), margins(&want), "{step:?}");
            assert!(!range.use_aabb);
            assert!(*shadows, "{step:?}");
        }
        assert_eq!(expected[1].fade_in, chain[1].fade_in);
        assert_eq!(expected[1].fade_out, chain[2].fade_out);

        // Once the impostor is drawn, the steps take the chain's own bands.
        let waiting = app.world().entity(entity).get::<SingleImpostor>().unwrap();
        assert_eq!(waiting.ranges, chain);
    }

    #[test]
    fn the_quality_setting_scales_every_band() {
        let mut app = app();
        app.insert_resource(LodQuality(Quality::Low));
        let options = MushroomPreset::porcini().mushroom.clone();
        let entity = app
            .world_mut()
            .spawn(MushroomParams::new(options.clone()))
            .id();
        run_until_finished(&mut app, entity);

        let diameter = Mushroom::new(options).unwrap().bounding_diameter();
        let expected = mesh_ranges(&chain_ranges(diameter, Quality::Low));
        for (step, range, _) in ranges(&mut app, entity) {
            let want = visibility_range(&expected[step.index()]);
            assert_eq!(margins(&range), margins(&want), "{step:?}");
        }
    }
}
