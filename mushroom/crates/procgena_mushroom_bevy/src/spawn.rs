//! Components that request mushrooms, and the systems that build them on
//! background threads and spawn their detail levels as child entities.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::NoAutoAabb;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use bevy::transform::helper::TransformHelper;
use procgena_core::scatter::{Mask, ScatterArea, ScatterInput};
use procgena_core::{ContentHasher, Ground};
use procgena_core_bevy::{LodQuality, visibility_range};
use procgena_mushroom::{
    BodyId, BodyShape, Mushroom, MushroomBody, MushroomError, MushroomInstance, MushroomLod,
    MushroomOptions, MushroomPreset, MushroomScatter, OptionsError, ScatterError, ScatterOptions,
    TextureCompression, TexturePart, TextureSize,
};
use procgena_specimen::{GeneratedSpecimen, Specimen};

use crate::material::{MushroomMaterial, TextureSet, billboard_material, mushroom_material};
use crate::mesh::{billboard_aabb, billboard_mesh, mushroom_mesh};

/// Asks for one mushroom, or a cluster of them, standing at the entity's
/// position.
///
/// Spawn it after adding [`MushroomPlugin`](crate::MushroomPlugin). The
/// mushroom is built in the background and appears as [`MushroomPart`]
/// children, one or two per detail level and member of a cluster (see
/// [`MushroomOptions::cluster`]). Mushrooms with the same options share
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
    /// Options loaded from a `.mushroom.json` file. The mushroom waits for
    /// the file to load, and rebuilds when the file changes if Bevy is
    /// watching asset files.
    Asset(Handle<MushroomOptionsAsset>),
}

impl MushroomParams {
    /// Asks for a mushroom made from `options`.
    pub fn new(options: MushroomOptions) -> Self {
        Self::Options(Arc::new(options))
    }

    /// Asks for a mushroom made from a preset.
    pub fn preset(preset: MushroomPreset) -> Self {
        Self::new(preset.options())
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
/// up to the ground's own heights. [`ScatterOptions::seed`] seeds the
/// scatter, as its input seed. It rebuilds when the settings, the area or
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

/// Mushroom options loaded from a `.mushroom.json` file.
///
/// The file may leave settings out to use their defaults; see
/// [`MushroomOptions::from_json`]. Files with out-of-range settings fail to
/// load.
#[derive(Asset, TypePath, Debug, Clone, PartialEq)]
pub struct MushroomOptionsAsset(pub MushroomOptions);

/// Marks a child entity that draws one detail level of one mushroom. The
/// plugin spawns and removes these; don't add them yourself.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MushroomPart {
    /// Which mushroom: 0 for a single mushroom, or the member's or
    /// instance's number in a cluster or scatter.
    pub instance: usize,
    /// Detail level, from 0 for the nearest.
    pub lod: usize,
    /// True for shapes that turn toward the camera: a flat stem strip, or
    /// the stem and cap triangles of a billboard level.
    pub billboard: bool,
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

/// Loads `.mushroom.json` files, rejecting malformed JSON and out-of-range
/// settings.
#[derive(Debug, Default, TypePath)]
pub(crate) struct MushroomOptionsLoader;

impl AssetLoader for MushroomOptionsLoader {
    type Asset = MushroomOptionsAsset;
    type Settings = ();
    type Error = BevyError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _context: &mut LoadContext<'_>,
    ) -> Result<MushroomOptionsAsset, BevyError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let options = MushroomOptions::from_json(std::str::from_utf8(&bytes)?)
            .map_err(MushroomError::Json)?;
        options.validate().map_err(MushroomError::Options)?;
        Ok(MushroomOptionsAsset(options))
    }

    fn extensions(&self) -> &[&str] {
        &["mushroom.json"]
    }
}

/// The plugin's settings, for the build systems.
#[derive(Resource, Debug, Clone)]
pub(crate) struct SpawnSettings {
    /// Detail levels, already validated. Every body is meshed at each
    /// level, and each part's distance band is scaled to its mushroom's
    /// size and the [`LodQuality`].
    pub lod: MushroomLod,
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
    /// The combined specimen for a mushroom or cluster; `None` for scatters.
    specimen: Option<Specimen>,
}

/// Names one body. Every body is meshed at the plugin's detail levels,
/// which never change while the app runs, so bodies with equal ids share
/// uploaded meshes.
type BodyKey = BodyId;

/// One variant's meshes, one per detail level.
struct BuiltVariant {
    /// Which body this is.
    key: BodyKey,
    /// The variant's options.
    options: MushroomOptions,
    /// One entry per detail level of [`SpawnSettings::lod`].
    levels: Vec<BuiltLevel>,
}

/// One detail level's meshes.
struct BuiltLevel {
    /// The surfaces, or `None` if the level has no triangles.
    surfaces: Option<Mesh>,
    /// Camera-facing shapes and their bounds, for flat and billboard
    /// levels.
    billboard: Option<(Mesh, Aabb)>,
    /// True if the surfaces must be drawn two-sided.
    double_sided: bool,
    /// True for a billboard level, drawn with vertex colours only.
    plain: bool,
}

/// One variant's meshes and materials, after adding them to Bevy's assets.
struct VariantAssets {
    /// The variant's cap diameter in its own units, before an instance's
    /// scale and the parent's scale, for scaling the detail levels.
    size: f32,
    /// One entry per detail level of [`SpawnSettings::lod`].
    levels: Vec<LevelAssets>,
}

/// One detail level's mesh and material handles.
struct LevelAssets {
    /// The surfaces and their material.
    surfaces: Option<(Handle<Mesh>, Handle<MushroomMaterial>)>,
    /// Camera-facing shapes, their material and their bounds.
    billboard: Option<(Handle<Mesh>, Handle<MushroomMaterial>, Aabb)>,
}

/// Uploaded variants and materials, shared by every entity that needs the
/// same ones.
#[derive(Resource, Default)]
pub(crate) struct MushroomCache {
    /// Variants by [`BuiltVariant::key`].
    variants: HashMap<BodyKey, Arc<VariantAssets>>,
    /// Materials by the content hash of their settings (see
    /// [`material_hash`]), two-sidedness, and whether they draw vertex
    /// colours only (for billboard levels).
    materials: HashMap<(u64, bool, bool), Handle<MushroomMaterial>>,
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
            entity.remove::<(MushroomGenerating, Building, Attempted, GeneratedSpecimen)>();
        }
    }
}

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
    assets: Res<Assets<MushroomOptionsAsset>>,
    parts: Query<(), With<MushroomPart>>,
    settings: Res<SpawnSettings>,
) {
    // Work out what each entity needs.
    let mut wanted: Vec<(Entity, Wanted)> = Vec::new();
    for (entity, params) in &singles {
        let options = match params {
            MushroomParams::Options(o) => Some(o.clone()),
            MushroomParams::Asset(handle) => assets.get(handle).map(|a| Arc::new(a.0.clone())),
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
                        .remove::<(Building, Attempted, GeneratedSpecimen)>();
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
                    .remove::<(Building, MushroomGenerating, GeneratedSpecimen)>()
                    .insert(Attempted(input));
                commands.trigger(MushroomFinished {
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
            .remove::<GeneratedSpecimen>()
            .insert((MushroomGenerating, Building { input, task }));
    }
}

/// Returns true if `input` is already being built, or was the last thing
/// built.
fn is_current(input: &Input, building: Option<&Building>, attempted: Option<&Attempted>) -> bool {
    building.is_some_and(|b| b.input == *input) || attempted.is_some_and(|a| a.0 == *input)
}

/// Builds the meshes and placements for `input` at every level of `lod`.
/// Runs on a background thread.
fn build(input: &Input, lod: &MushroomLod) -> Result<Built, MushroomBuildError> {
    match input {
        Input::Single(options) => {
            let mushroom = Mushroom::new((**options).clone())?;
            let variants = mushroom
                .bodies()
                .iter()
                .map(|b| build_variant(b, lod))
                .collect::<Result<Vec<_>, _>>()?;
            let specimen = mushroom.specimen(lod)?;
            Ok(Built {
                variants,
                instances: mushroom.members().to_vec(),
                specimen: Some(specimen),
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
                .map(|b| build_variant(b, lod))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Built {
                variants,
                instances: built.instances().to_vec(),
                specimen: None,
            })
        }
        Input::Ground(params) => {
            let mut input = ScatterInput::new(&*params.ground, params.area, params.scatter.seed);
            if let Some(mask) = &params.mask {
                input = input.with_mask(&**mask);
            }
            let built = MushroomScatter::new(&params.mushroom, &params.scatter, &input)
                .map_err(MushroomBuildError::Scatter)?;
            let variants = built
                .bodies()
                .iter()
                .map(|b| build_variant(b, lod))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Built {
                variants,
                instances: built.instances().to_vec(),
                specimen: None,
            })
        }
    }
}

/// Meshes `body` at every level of `lod` and converts the meshes for Bevy.
/// Warts are left out of the vertex colours, because the material draws
/// them.
fn build_variant(body: &MushroomBody, lod: &MushroomLod) -> Result<BuiltVariant, OptionsError> {
    let options = body.options().clone();
    let mut lod = lod.clone();
    for level in &mut lod.levels {
        level.detail.paint_warts = false;
    }
    let levels = body
        .lod_meshes(&lod)?
        .into_iter()
        .map(|(level, mesh)| BuiltLevel {
            surfaces: (mesh.surfaces.triangle_count() > 0).then(|| mushroom_mesh(&mesh.surfaces)),
            billboard: mesh
                .billboard
                .as_ref()
                .map(|b| (billboard_mesh(b), billboard_aabb(b))),
            double_sided: mesh.double_sided,
            plain: level.detail.shape == BodyShape::Billboard,
        })
        .collect();
    Ok(BuiltVariant {
        key: body.id(),
        options,
        levels,
    })
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

        let built = match result {
            Ok(built) => built,
            Err(error) => {
                error!("procgena_mushroom_bevy: cannot build mushrooms: {error}");
                commands.trigger(MushroomFinished {
                    entity,
                    result: Err(error),
                });
                continue;
            }
        };

        // Add each variant to Bevy's assets, reusing identical variants that
        // other entities already added.
        let variants: Vec<Arc<VariantAssets>> = built
            .variants
            .into_iter()
            .map(|v| {
                let textures = (&*asset_server, settings.textures.as_ref());
                upload(v, &mut cache, &mut meshes, &mut materials, textures)
            })
            .collect();

        // Spawn one child per mushroom and level (two for a flat level), each
        // shown only within its own distance band, which grows with the
        // mushroom's size in the world and with the quality setting.
        let world_scale = input.world_scale();
        for (i, instance) in built.instances.iter().enumerate() {
            let variant = &variants[instance.variant as usize];
            let size = variant.size * instance.scale * world_scale;
            let ranges = settings.lod.ranges(settings.lod.factor(size, quality.0));
            let transform = Transform {
                translation: instance.translation,
                rotation: instance.rotation,
                scale: Vec3::splat(instance.scale),
            };
            let levels = variant.levels.iter().zip(&settings.lod.levels);
            for (lod, ((level, lod_level), range)) in levels.zip(&ranges).enumerate() {
                let range = visibility_range(range);
                let mut spawned = Vec::with_capacity(2);
                if let Some((mesh, material)) = &level.surfaces {
                    let part = MushroomPart {
                        instance: i,
                        lod,
                        billboard: false,
                    };
                    spawned.push(
                        commands
                            .spawn((
                                part,
                                Mesh3d(mesh.clone()),
                                MeshMaterial3d(material.clone()),
                                transform,
                                range.clone(),
                                ChildOf(entity),
                            ))
                            .id(),
                    );
                }
                if let Some((mesh, material, aabb)) = &level.billboard {
                    let part = MushroomPart {
                        instance: i,
                        lod,
                        billboard: true,
                    };
                    spawned.push(
                        commands
                            .spawn((
                                part,
                                Mesh3d(mesh.clone()),
                                MeshMaterial3d(material.clone()),
                                transform,
                                range.clone(),
                                *aabb,
                                NoAutoAabb,
                                ChildOf(entity),
                            ))
                            .id(),
                    );
                }
                if !lod_level.casts_shadows {
                    for part in spawned {
                        commands.entity(part).insert(NotShadowCaster);
                    }
                }
            }
        }
        if let Some(specimen) = built.specimen {
            commands
                .entity(entity)
                .insert(GeneratedSpecimen(Arc::new(specimen)));
        }
        commands.trigger(MushroomFinished {
            entity,
            result: Ok(()),
        });
    }
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
    (asset_server, textures): (&AssetServer, Option<&TextureSettings>),
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
    let mut material = |double_sided: bool, plain: bool| {
        cache
            .materials
            .entry((material_key, double_sided, plain))
            .or_insert_with(|| {
                materials.add(if plain {
                    billboard_material(o)
                } else {
                    mushroom_material(o, double_sided, cap.as_ref(), stem.as_ref())
                })
            })
            .clone()
    };
    let levels = variant
        .levels
        .into_iter()
        .map(|level| LevelAssets {
            surfaces: level
                .surfaces
                .map(|m| (meshes.add(m), material(level.double_sided, level.plain))),
            billboard: level
                .billboard
                .map(|(m, aabb)| (meshes.add(m), material(true, level.plain), aabb)),
        })
        .collect();
    let assets = Arc::new(VariantAssets {
        size: variant.options.size,
        levels,
    });
    cache.variants.insert(variant.key, assets.clone());
    assets
}

/// Returns the content hash of every setting in `o` that a mushroom's
/// materials depend on: the material settings, the size, the stem's width
/// (for flat stems), and the warts and cap outline (for warts). Mushrooms
/// with equal hashes share materials.
fn material_hash(o: &MushroomOptions) -> u64 {
    let mut h = ContentHasher::new();
    h.write(&o.material);
    h.write(&o.size);
    h.write(&o.stem.radius);
    h.write(&o.stem.base);
    h.write(&o.warts);
    h.write(&o.cap.family);
    h.write(&o.cap.height);
    h.finish()
}

/// After parts are removed, forgets cached variants and materials that no
/// remaining part draws, so Bevy can free them.
pub(crate) fn prune_cache(
    mut removed: RemovedComponents<MushroomPart>,
    parts: Query<&Mesh3d, With<MushroomPart>>,
    mut cache: ResMut<MushroomCache>,
) {
    if removed.read().count() == 0 {
        return;
    }
    // Keep variants that some part still draws, then the materials those
    // variants use.
    let drawn: HashSet<AssetId<Mesh>> = parts.iter().map(|m| m.0.id()).collect();
    cache.variants.retain(|_, v| {
        v.levels.iter().any(|l| {
            l.surfaces
                .iter()
                .map(|(m, _)| m.id())
                .chain(l.billboard.iter().map(|(m, _, _)| m.id()))
                .any(|id| drawn.contains(&id))
        })
    });
    let used: HashSet<AssetId<MushroomMaterial>> = cache
        .variants
        .values()
        .flat_map(|v| v.levels.iter())
        .flat_map(|l| {
            l.surfaces
                .iter()
                .map(|(_, m)| m.id())
                .chain(l.billboard.iter().map(|(_, m, _)| m.id()))
        })
        .collect();
    cache.materials.retain(|_, m| used.contains(&m.id()));
}

#[cfg(test)]
mod tests {
    use std::ops::Range;
    use std::time::{Duration, Instant};

    use bevy::camera::visibility::VisibilityRange;
    use procgena_core::Quality;
    use procgena_mushroom::default_lod;
    use procgena_specimen::{Part, SurfaceMesh, tags};

    use super::*;

    /// Results of every [`MushroomFinished`] event, in order.
    #[derive(Resource, Default)]
    struct Finished(Vec<Result<(), MushroomBuildError>>);

    /// An app running only the build systems, without rendering.
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<MushroomMaterial>()
            .init_resource::<Finished>()
            .add_observer(
                |event: On<MushroomFinished>, mut finished: ResMut<Finished>| {
                    finished.0.push(event.result.clone());
                },
            );
        crate::add_systems(&mut app, default_lod());
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
    fn a_mushroom_gets_every_level_and_its_specimen() {
        let mut app = app();
        let entity = app
            .world_mut()
            .spawn(MushroomParams::preset(MushroomPreset::FlyAgaric))
            .id();
        run_until_finished(&mut app, entity);

        let parts = parts(&mut app, entity);
        let surfaces: Vec<usize> = parts
            .iter()
            .filter(|p| !p.billboard)
            .map(|p| p.lod)
            .collect();
        let mut billboards: Vec<usize> = parts
            .iter()
            .filter(|p| p.billboard)
            .map(|p| p.lod)
            .collect();
        billboards.sort();
        // The flat level has a turning stem; the billboard level is turning
        // shapes only.
        assert_eq!(surfaces, [0, 1, 2, 3]);
        assert_eq!(billboards, [3, 4]);
        assert!(app.world().entity(entity).contains::<GeneratedSpecimen>());
        assert_eq!(app.world().resource::<Finished>().0, vec![Ok(())]);

        // Removing the request removes the parts and the specimen.
        app.world_mut()
            .entity_mut(entity)
            .remove::<MushroomParams>();
        app.update();
        assert!(self::parts(&mut app, entity).is_empty());
        assert!(!app.world().entity(entity).contains::<GeneratedSpecimen>());
    }

    #[test]
    fn identical_mushrooms_share_meshes_and_rebuild_on_change() {
        let mut app = app();
        let a = app
            .world_mut()
            .spawn(MushroomParams::preset(MushroomPreset::Button))
            .id();
        let b = app
            .world_mut()
            .spawn(MushroomParams::preset(MushroomPreset::Button))
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
        let mut options = MushroomPreset::Button.options();
        options.seed = 9;
        *app.world_mut().get_mut::<MushroomParams>(a).unwrap() = MushroomParams::new(options);
        run_until_finished(&mut app, a);
        assert_ne!(meshes(&mut app, a), meshes(&mut app, b));
        assert_eq!(app.world().resource::<Finished>().0.len(), 3);
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
        let mut options = MushroomPreset::Button.options();
        options.cluster.stems = 4;
        let entity = app.world_mut().spawn(MushroomParams::new(options)).id();
        run_until_finished(&mut app, entity);
        let instances: HashSet<usize> =
            parts(&mut app, entity).iter().map(|p| p.instance).collect();
        assert!(instances.len() >= 3, "{instances:?}");
        assert!(app.world().entity(entity).contains::<GeneratedSpecimen>());
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
            mushroom: Arc::new(MushroomPreset::Porcini.options()),
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
        assert!(!parts(&mut app, entity).is_empty());
        assert_eq!(app.world().resource::<Finished>().0, vec![Ok(())]);

        // Losing the host's specimen removes the mushrooms.
        app.world_mut()
            .entity_mut(host)
            .remove::<GeneratedSpecimen>();
        app.update();
        assert!(parts(&mut app, entity).is_empty());
        assert!(app.world().entity(entity).contains::<MushroomGenerating>());
    }

    #[test]
    fn a_ground_scatter_spawns_each_member_at_its_ground_height() {
        let mut app = app();
        let ground: Arc<dyn Ground> =
            Arc::new(|p: bevy::math::DVec2| 0.3 * (p.x as f32 * 0.8).sin());
        let params = MushroomGroundScatterParams {
            mushroom: Arc::new(MushroomPreset::Button.options()),
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

        // Every part stands near the ground under it (sunk a little).
        let mut query = app
            .world_mut()
            .query::<(&MushroomPart, &Transform, &ChildOf)>();
        let mut count = 0;
        for (_, transform, parent) in query.iter(app.world()) {
            assert_eq!(parent.parent(), entity);
            let t = transform.translation;
            let h = ground.height(bevy::math::DVec2::new(t.x.into(), t.z.into()));
            assert!((t.y - h).abs() < 0.05, "part at {t} over ground {h}");
            count += 1;
        }
        assert!(count > 0);

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

    /// Each part under `entity` with its level, distance band and whether it
    /// casts shadows.
    fn ranges(app: &mut App, entity: Entity) -> Vec<(usize, VisibilityRange, bool)> {
        let mut query = app.world_mut().query::<(
            &MushroomPart,
            &VisibilityRange,
            Has<NotShadowCaster>,
            &ChildOf,
        )>();
        query
            .iter(app.world())
            .filter(|(.., parent)| parent.parent() == entity)
            .map(|(part, range, shadowless, _)| (part.lod, range.clone(), !shadowless))
            .collect()
    }

    #[test]
    fn levels_hand_over_and_fade_out_at_the_cull_distance() {
        let mut app = app();
        let options = MushroomPreset::Porcini.options();
        let entity = app
            .world_mut()
            .spawn(MushroomParams::new(options.clone()))
            .id();
        run_until_finished(&mut app, entity);

        // Each part shows over its level's band, scaled to the mushroom's
        // size, and only the card level drops its shadow.
        let lod = default_lod();
        let expected = lod.ranges(lod.factor(options.size, Quality::High));
        let parts = ranges(&mut app, entity);
        assert!(!parts.is_empty());
        for (level, range, shadows) in &parts {
            let want = visibility_range(&expected[*level]);
            assert_eq!(margins(range), margins(&want), "level {level}");
            assert!(!range.use_aabb);
            assert_eq!(*shadows, lod.levels[*level].casts_shadows, "level {level}");
        }
        assert!(parts.iter().any(|(level, ..)| *level == 4));
        let last = &expected[4];
        assert!(
            last.fade_out
                .contains(&(40.0 * lod.factor(options.size, Quality::High)))
        );
    }

    #[test]
    fn the_quality_setting_scales_every_band() {
        let mut app = app();
        app.insert_resource(LodQuality(Quality::Low));
        let options = MushroomPreset::Porcini.options();
        let entity = app
            .world_mut()
            .spawn(MushroomParams::new(options.clone()))
            .id();
        run_until_finished(&mut app, entity);

        let lod = default_lod();
        let expected = lod.ranges(lod.factor(options.size, Quality::Low));
        for (level, range, _) in ranges(&mut app, entity) {
            let want = visibility_range(&expected[level]);
            assert_eq!(margins(&range), margins(&want), "level {level}");
        }
    }
}
