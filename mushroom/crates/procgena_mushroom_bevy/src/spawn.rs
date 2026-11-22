//! Components that request mushrooms, and the systems that build them on
//! background threads and spawn their detail levels as child entities.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::{NoAutoAabb, VisibilityRange};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use bevy::transform::helper::TransformHelper;
use procgena_mesh::{GeneratedObject, Object};
use procgena_mushroom::{
    ClusterOptions, DEFAULT_LOD_SIZE, Mushroom, MushroomCluster, MushroomError, MushroomInstance,
    MushroomLod, MushroomOptions, MushroomPreset, MushroomScatter, OptionsError, ScatterError,
    ScatterOptions, TextureCompression, TextureSize,
};

use crate::material::{CapTextures, MushroomMaterial, mushroom_material};
use crate::mesh::{mushroom_mesh, ribbon_aabb, ribbon_mesh};

/// Asks for one mushroom, standing at the entity's position.
///
/// Spawn it after adding [`MushroomPlugin`](crate::MushroomPlugin). The
/// mushroom is built in the background and appears as [`MushroomPart`]
/// children, one or two per detail level. Mushrooms with the same options
/// share meshes and materials, so many identical ones are cheap.
///
/// Once the parts appear, the entity also gets a [`GeneratedObject`]
/// describing the mushroom, so other generators can grow on it. Changing the
/// options rebuilds the mushroom; removing this component removes the parts
/// and the object.
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

/// Asks for a cluster of mushrooms like [`mushroom`](Self::mushroom),
/// centred on the entity's position.
///
/// Members are drawn from a few shared meshes. Once the parts appear, the
/// entity also gets a [`GeneratedObject`] with every member combined.
#[derive(Component, Debug, Clone, PartialEq)]
#[require(Transform, Visibility)]
pub struct MushroomClusterParams {
    /// The species the members are variations of.
    pub mushroom: Arc<MushroomOptions>,
    /// Member count, sizes and arrangement.
    pub cluster: ClusterOptions,
}

/// Spreads mushrooms over this entity's parent.
///
/// Spawn it as a child of a host: an entity with a [`GeneratedObject`],
/// such as a rock from `procgena_stone_bevy`. Leave this entity's own
/// transform at its default, so the mushrooms line up with the host.
///
/// It waits until the host has its object, and rebuilds when the object, the
/// host's rotation or scale, or these settings change. If the host loses its
/// object, the mushrooms are removed. [`ScatterOptions::up`] is in world
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
    /// True for a flat stem strip that turns toward the camera.
    pub ribbon: bool,
}

/// Present while a mushroom, cluster or scatter is being prepared,
/// including while it waits for its options file or its host.
#[derive(Component, Debug, Default)]
pub struct MushroomGenerating;

/// Event triggered on a mushroom, cluster or scatter entity when a build
/// ends, successfully or not.
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

/// Why a mushroom, cluster or scatter could not be built.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MushroomBuildError {
    /// A setting is out of range.
    Options(OptionsError),
    /// The scatter could not grow on its host, for example because no part
    /// of the host matched its tag filter.
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
    /// Detail levels for a mushroom of [`DEFAULT_LOD_SIZE`]; each build
    /// scales them to the mushroom's size.
    pub lod: MushroomLod,
    /// Where to load cap textures from, or `None` for vertex colours only.
    pub textures: Option<TextureSettings>,
}

/// Where and in what form to load cap textures.
#[derive(Debug, Clone)]
pub(crate) struct TextureSettings {
    /// Folder relative to Bevy's asset folder that holds `cap/`.
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
    /// One mushroom.
    Single(Arc<MushroomOptions>),
    /// A cluster of a species.
    Cluster(Arc<MushroomOptions>, ClusterOptions),
    /// A scatter over a host's object, with the host's world rotation and
    /// (uniform) scale.
    Scatter {
        mushroom: Arc<MushroomOptions>,
        scatter: ScatterOptions,
        host: Arc<Object>,
        rotation: Quat,
        scale: f32,
    },
}

// Host objects can be large, so they are compared by identity (the same
// shared copy) rather than by content.
impl PartialEq for Input {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Input::Single(a), Input::Single(b)) => a == b,
            (Input::Cluster(a, c), Input::Cluster(b, d)) => a == b && c == d,
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
    /// The combined object for single mushrooms and clusters; `None` for
    /// scatters.
    object: Option<Object>,
}

/// One variant's meshes, one per detail level.
struct BuiltVariant {
    /// The options and detail levels as JSON. Variants with the same key
    /// share uploaded meshes.
    key: String,
    /// The variant's options.
    options: MushroomOptions,
    /// Detail levels scaled for the variant's size.
    lod: MushroomLod,
    /// One entry per detail level.
    levels: Vec<BuiltLevel>,
}

/// One detail level's meshes.
struct BuiltLevel {
    /// The surfaces, or `None` if the level has no triangles.
    surfaces: Option<Mesh>,
    /// A flat stem and its bounds, for flat levels.
    ribbon: Option<(Mesh, Aabb)>,
    /// True if the surfaces must be drawn two-sided.
    double_sided: bool,
}

/// One variant's meshes and materials, after adding them to Bevy's assets.
struct VariantAssets {
    /// Detail levels scaled for the variant's size, but not for an
    /// instance's own scale.
    lod: MushroomLod,
    /// One entry per detail level.
    levels: Vec<LevelAssets>,
}

/// One detail level's mesh and material handles.
struct LevelAssets {
    /// The surfaces and their material.
    surfaces: Option<(Handle<Mesh>, Handle<MushroomMaterial>)>,
    /// A flat stem, its material and its bounds.
    ribbon: Option<(Handle<Mesh>, Handle<MushroomMaterial>, Aabb)>,
}

/// Uploaded variants and materials, shared by every entity that needs the
/// same ones.
#[derive(Resource, Default)]
pub(crate) struct MushroomCache {
    /// Variants by [`BuiltVariant::key`].
    variants: HashMap<String, Arc<VariantAssets>>,
    /// Materials by material settings (as JSON) and two-sidedness.
    materials: HashMap<(String, bool), Handle<MushroomMaterial>>,
    /// Cap texture images by set name, kept for reuse.
    cap_textures: HashMap<String, CapTextures>,
}

// --------------------------------------------------------------------------
// Systems

/// Removes the parts, markers and object of entities whose mushroom, cluster
/// or scatter component was removed.
pub(crate) fn remove_mushrooms(
    mut commands: Commands,
    mut singles: RemovedComponents<MushroomParams>,
    mut clusters: RemovedComponents<MushroomClusterParams>,
    mut scatters: RemovedComponents<MushroomScatterParams>,
    children: Query<&Children>,
    parts: Query<(), With<MushroomPart>>,
) {
    let removed: Vec<Entity> = singles
        .read()
        .chain(clusters.read())
        .chain(scatters.read())
        .collect();
    for entity in removed {
        despawn_parts(&mut commands, children.get(entity).ok(), &parts);
        if let Ok(mut entity) = commands.get_entity(entity) {
            entity.remove::<(MushroomGenerating, Building, Attempted, GeneratedObject)>();
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

/// What an entity with a mushroom, cluster or scatter component needs this
/// frame.
enum Wanted {
    /// Nothing yet: its options file or its host's object isn't available.
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
    clusters: Query<(Entity, &MushroomClusterParams)>,
    scatters: Query<(Entity, &MushroomScatterParams, Option<&ChildOf>)>,
    state: Query<(
        Option<&Building>,
        Option<&Attempted>,
        Option<&Children>,
        Has<MushroomGenerating>,
    )>,
    hosts: Query<&GeneratedObject>,
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
    for (entity, params) in &clusters {
        wanted.push((
            entity,
            Wanted::Build(Input::Cluster(
                params.mushroom.clone(),
                params.cluster.clone(),
            )),
        ));
    }
    for (entity, params, parent) in &scatters {
        // Wait for the host's object. Work out the host's world transform
        // from its parents, because Bevy may not have updated it yet this
        // frame.
        let host = parent.map(ChildOf::parent);
        let found = host.and_then(|h| {
            Some((
                hosts.get(h).ok()?,
                transforms.compute_global_transform(h).ok()?,
            ))
        });
        let Some((object, transform)) = found else {
            wanted.push((entity, Wanted::Wait));
            continue;
        };
        let (scale, rotation, _) = transform.to_scale_rotation_translation();
        let input = Input::Scatter {
            mushroom: params.mushroom.clone(),
            scatter: params.scatter.clone(),
            host: object.0.clone(),
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
                // If the host lost its object, remove the mushrooms grown
                // on it.
                if building.is_some() || attempted.is_some() {
                    despawn_parts(&mut commands, kids, &parts);
                    commands
                        .entity(entity)
                        .remove::<(Building, Attempted, GeneratedObject)>();
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
                    .remove::<(Building, MushroomGenerating, GeneratedObject)>()
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
            .remove::<GeneratedObject>()
            .insert((MushroomGenerating, Building { input, task }));
    }
}

/// Returns true if `input` is already being built, or was the last thing
/// built.
fn is_current(input: &Input, building: Option<&Building>, attempted: Option<&Attempted>) -> bool {
    building.is_some_and(|b| b.input == *input) || attempted.is_some_and(|a| a.0 == *input)
}

/// Builds the meshes and placements for `input`. Runs on a background
/// thread. `lod` is for a mushroom of [`DEFAULT_LOD_SIZE`].
fn build(input: &Input, lod: &MushroomLod) -> Result<Built, MushroomBuildError> {
    match input {
        Input::Single(options) => {
            let mushroom = Mushroom::new((**options).clone())?;
            let variant = build_variant(&mushroom, lod)?;
            let object = mushroom.object(&variant.lod)?;
            Ok(Built {
                variants: vec![variant],
                instances: vec![MushroomInstance {
                    variant: 0,
                    translation: Vec3::ZERO,
                    rotation: Quat::IDENTITY,
                    scale: 1.0,
                }],
                object: Some(object),
            })
        }
        Input::Cluster(options, cluster) => {
            let built = MushroomCluster::new(options, cluster)?;
            let variants = built
                .variants()
                .iter()
                .map(|m| build_variant(m, lod))
                .collect::<Result<Vec<_>, _>>()?;
            let object = built.object(&size_lod(lod, options.size))?;
            Ok(Built {
                variants,
                instances: built.members().to_vec(),
                object: Some(object),
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
                .variants()
                .iter()
                .map(|m| build_variant(m, lod))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Built {
                variants,
                instances: built.instances().to_vec(),
                object: None,
            })
        }
    }
}

/// Returns `lod`, written for a mushroom of [`DEFAULT_LOD_SIZE`], scaled for
/// a cap of diameter `size` metres.
fn size_lod(lod: &MushroomLod, size: f32) -> MushroomLod {
    lod.clone().scaled(size / DEFAULT_LOD_SIZE)
}

/// Meshes `mushroom` at every level of `lod` (scaled for its size) and
/// converts the meshes for Bevy.
fn build_variant(mushroom: &Mushroom, lod: &MushroomLod) -> Result<BuiltVariant, OptionsError> {
    let options = mushroom.options().clone();
    let lod = size_lod(lod, options.size);
    let levels = mushroom
        .lod_meshes(&lod)?
        .into_iter()
        .map(|(_, mesh)| BuiltLevel {
            surfaces: (mesh.surfaces.triangle_count() > 0).then(|| mushroom_mesh(&mesh.surfaces)),
            ribbon: mesh
                .stem_ribbon
                .as_ref()
                .map(|r| (ribbon_mesh(r), ribbon_aabb(r))),
            double_sided: mesh.double_sided,
        })
        .collect();
    let key = format!(
        "{}{}",
        options.to_json(),
        serde_json::to_string(&lod).unwrap_or_default()
    );
    Ok(BuiltVariant {
        key,
        options,
        lod,
        levels,
    })
}

/// Collects finished background builds: replaces each entity's parts with
/// the new ones, adds its [`GeneratedObject`] if it has one, and triggers
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

        // Spawn one child per mushroom and level (two for a flat stem), each
        // shown only within its own distance band, which grows with the
        // mushroom's size.
        let world_scale = input.world_scale();
        for (i, instance) in built.instances.iter().enumerate() {
            let variant = &variants[instance.variant as usize];
            let ranges =
                visibility_ranges(&variant.lod.clone().scaled(instance.scale * world_scale));
            let transform = Transform {
                translation: instance.translation,
                rotation: instance.rotation,
                scale: Vec3::splat(instance.scale),
            };
            for (lod, (level, range)) in variant.levels.iter().zip(ranges).enumerate() {
                if let Some((mesh, material)) = &level.surfaces {
                    commands.spawn((
                        MushroomPart {
                            instance: i,
                            lod,
                            ribbon: false,
                        },
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        transform,
                        range.clone(),
                        ChildOf(entity),
                    ));
                }
                if let Some((mesh, material, aabb)) = &level.ribbon {
                    commands.spawn((
                        MushroomPart {
                            instance: i,
                            lod,
                            ribbon: true,
                        },
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        transform,
                        range.clone(),
                        *aabb,
                        NoAutoAabb,
                        ChildOf(entity),
                    ));
                }
            }
        }
        if let Some(object) = built.object {
            commands
                .entity(entity)
                .insert(GeneratedObject(Arc::new(object)));
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
/// `textures` is the asset server and, when cap textures are wanted, where
/// to load them from; the variant's cap texture set, if it names one, is
/// loaded once and then reused.
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

    // The cap texture set, if this mushroom names one and textures are on.
    let cap = match (&variant.options.material.cap_texture, textures) {
        (Some(set), Some(t)) => Some(
            cache
                .cap_textures
                .entry(set.clone())
                .or_insert_with(|| {
                    CapTextures::load(asset_server, &t.root, set, t.size, t.compression)
                })
                .clone(),
        ),
        _ => None,
    };

    let material_json = serde_json::to_string(&(&variant.options.material, variant.options.size))
        .unwrap_or_default();
    let mut material = |double_sided: bool| {
        cache
            .materials
            .entry((material_json.clone(), double_sided))
            .or_insert_with(|| {
                materials.add(mushroom_material(
                    &variant.options,
                    double_sided,
                    cap.as_ref(),
                ))
            })
            .clone()
    };
    let levels = variant
        .levels
        .into_iter()
        .map(|level| LevelAssets {
            surfaces: level
                .surfaces
                .map(|m| (meshes.add(m), material(level.double_sided))),
            ribbon: level
                .ribbon
                .map(|(m, aabb)| (meshes.add(m), material(true), aabb)),
        })
        .collect();
    let assets = Arc::new(VariantAssets {
        lod: variant.lod,
        levels,
    });
    cache.variants.insert(variant.key, assets.clone());
    assets
}

/// Returns the distance band to show each level of `lod` in. Neighbouring
/// levels blend over `distance × hysteresis` either side of each switch,
/// and the last level fades out around the cull distance.
pub(crate) fn visibility_ranges(lod: &MushroomLod) -> Vec<VisibilityRange> {
    let margin = |distance: f32, hysteresis: f32| {
        let half = distance * hysteresis;
        (distance - half).max(0.0)..distance + half
    };
    let last_hysteresis = lod.levels.last().map_or(0.0, |l| l.hysteresis);
    (0..lod.levels.len())
        .map(|i| {
            let level = &lod.levels[i];
            VisibilityRange {
                start_margin: if i == 0 {
                    0.0..0.0
                } else {
                    margin(level.distance, level.hysteresis)
                },
                end_margin: match lod.levels.get(i + 1) {
                    Some(next) => margin(next.distance, next.hysteresis),
                    None => margin(lod.cull_distance, last_hysteresis),
                },
                use_aabb: false,
            }
        })
        .collect()
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
                .chain(l.ribbon.iter().map(|(m, _, _)| m.id()))
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
                .chain(l.ribbon.iter().map(|(_, m, _)| m.id()))
        })
        .collect();
    cache.materials.retain(|_, m| used.contains(&m.id()));
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use procgena_mesh::{Part, SurfaceMesh, tags};

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
        crate::add_systems(&mut app, MushroomLod::default());
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
    fn a_mushroom_gets_every_level_and_its_object() {
        let mut app = app();
        let entity = app
            .world_mut()
            .spawn(MushroomParams::preset(MushroomPreset::FlyAgaric))
            .id();
        run_until_finished(&mut app, entity);

        let parts = parts(&mut app, entity);
        let surfaces: Vec<usize> = parts.iter().filter(|p| !p.ribbon).map(|p| p.lod).collect();
        let ribbons: Vec<usize> = parts.iter().filter(|p| p.ribbon).map(|p| p.lod).collect();
        assert_eq!(surfaces.len(), 4);
        assert_eq!(ribbons, [3]);
        assert!(app.world().entity(entity).contains::<GeneratedObject>());
        assert_eq!(app.world().resource::<Finished>().0, vec![Ok(())]);

        // Removing the request removes the parts and the object.
        app.world_mut()
            .entity_mut(entity)
            .remove::<MushroomParams>();
        app.update();
        assert!(self::parts(&mut app, entity).is_empty());
        assert!(!app.world().entity(entity).contains::<GeneratedObject>());
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
        let entity = app
            .world_mut()
            .spawn(MushroomClusterParams {
                mushroom: Arc::new(MushroomPreset::Button.options()),
                cluster: ClusterOptions {
                    count_min: 4,
                    count_max: 4,
                    ..ClusterOptions::default()
                },
            })
            .id();
        run_until_finished(&mut app, entity);
        let instances: HashSet<usize> =
            parts(&mut app, entity).iter().map(|p| p.instance).collect();
        assert!(instances.len() >= 3, "{instances:?}");
        assert!(app.world().entity(entity).contains::<GeneratedObject>());
    }

    /// A host entity with a ground object, at `scale`.
    fn host(app: &mut App, scale: Vec3) -> Entity {
        let part = Part::new([tags::GROUND], SurfaceMesh::ground(1.0, 4));
        app.world_mut()
            .spawn((
                GeneratedObject(Arc::new(Object::single(vec![part]))),
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

        // Losing the host's object removes the mushrooms.
        app.world_mut().entity_mut(host).remove::<GeneratedObject>();
        app.update();
        assert!(parts(&mut app, entity).is_empty());
        assert!(app.world().entity(entity).contains::<MushroomGenerating>());
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

    #[test]
    fn levels_hand_over_and_fade_out_at_the_cull_distance() {
        let lod = MushroomLod::default();
        let ranges = visibility_ranges(&lod);
        assert_eq!(ranges.len(), 4);
        assert_eq!(ranges[0].start_margin, 0.0..0.0);
        assert_eq!(ranges[0].end_margin, ranges[1].start_margin);
        assert!(ranges[3].end_margin.contains(&lod.cull_distance));
    }
}
