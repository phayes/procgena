//! Grow clumps on their host entity in background tasks, and display them
//! as child entities.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use bevy::transform::helper::TransformHelper;
use procgena_clump::{
    BuildStats, Clump, ClumpError, ClumpMesh, ClumpOptions, ClumpPreset, MaterialOptions,
    TextureCompression, TextureSize, texture_set,
};
use procgena_specimen::{GeneratedSpecimen, Specimen};

use crate::material::{ClumpMaterial, ClumpTextures, clump_material};
use crate::mesh::clump_mesh;

/// Grows a clump, such as snow or moss, on this entity's parent.
///
/// Spawn it as a child of a host: an entity that gets a [`GeneratedSpecimen`],
/// such as a rock from procgena_stone_bevy, a tree from procgena_tree_bevy,
/// or your own mesh with [`ObjectTags`](crate::ObjectTags). The clump grows on the host's most
/// detailed level, on the parts that [`ClumpOptions::grows_on`] picks, and
/// appears when background generation finishes, as a [`ClumpPart`] child.
///
/// Keep this entity's own transform at its default: the clump is built in
/// the host's coordinates. The host's world rotation and scale are taken
/// into account, so directions such as falling snow point the same way in
/// the world however the host is turned, and lengths stay in world metres.
/// The host's scale must be the same along every axis.
///
/// The clump rebuilds when this component, the host's specimen, or the host's
/// world rotation or scale changes; moving the host doesn't rebuild it, and
/// neither does a change to [`ClumpOptions::material`] alone, which only
/// swaps the clump's material. The previous clump stays visible until its
/// replacement is ready. Removing
/// this component despawns the clump's parts. Every finished build triggers
/// [`ClumpFinished`].
///
/// Once built, this entity also gets a [`GeneratedSpecimen`] for the clump
/// itself, tagged with [`ClumpOptions::tags`].
#[derive(Component, Debug, Clone, PartialEq)]
#[require(Transform, Visibility)]
pub struct ClumpParams(pub Arc<ClumpOptions>);

impl ClumpParams {
    /// Creates a clump request from options.
    pub fn new(options: ClumpOptions) -> Self {
        Self(Arc::new(options))
    }

    /// Creates a clump request from a preset's options.
    pub fn preset(preset: ClumpPreset) -> Self {
        Self::new(preset.options())
    }
}

/// Identifies the child entity that draws a [`ClumpParams`] clump.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClumpPart;

/// Present on a [`ClumpParams`] entity while its clump is waiting for its
/// host or being built.
///
/// Added when the clump needs building and removed when the build finishes,
/// at the same moment [`ClumpFinished`] is triggered.
#[derive(Component, Debug, Default)]
pub struct ClumpGenerating;

/// Triggered on a [`ClumpParams`] entity when a clump build finishes.
///
/// Builds dropped because their settings or host changed meanwhile don't
/// trigger it. On success the clump's [`ClumpPart`] child is queued with the
/// same commands, so it exists once this frame's commands have run. An empty
/// clump spawns no part.
#[derive(EntityEvent, Debug, Clone, PartialEq)]
pub struct ClumpFinished {
    /// The clump entity.
    pub entity: Entity,
    /// What was built, or why the clump couldn't be built.
    pub result: Result<ClumpBuilt, ClumpBuildError>,
}

/// Measurements from a finished clump build.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClumpBuilt {
    /// Measurements from procgena_clump.
    pub stats: BuildStats,
    /// Time spent building, not counting the wait for a free thread.
    pub build_time: Duration,
    /// Vertices in the clump mesh.
    pub vertices: usize,
    /// Triangles in the clump mesh.
    pub triangles: usize,
}

/// Why a clump couldn't be built.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ClumpBuildError {
    /// The settings were rejected, or the host's specimen can't carry a clump.
    Clump(ClumpError),
    /// The host is scaled differently along different axes.
    NonUniformScale,
}

impl fmt::Display for ClumpBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Clump(e) => write!(f, "{e}"),
            Self::NonUniformScale => {
                write!(f, "the host must be scaled the same along every axis")
            }
        }
    }
}

impl std::error::Error for ClumpBuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Clump(e) => Some(e),
            Self::NonUniformScale => None,
        }
    }
}

/// Texture location, format and size copied from the plugin configuration.
#[derive(Resource, Debug, Clone)]
pub(crate) struct SpawnSettings {
    pub texture_root: Option<String>,
    pub texture_compression: TextureCompression,
    pub texture_size: TextureSize,
}

/// Everything a clump build depends on.
#[derive(Debug, Clone)]
struct BuildInput {
    options: Arc<ClumpOptions>,
    specimen: Arc<Specimen>,
    /// The host's world rotation.
    rotation: Quat,
    /// The host's world scale, the same along every axis.
    scale: f32,
}

impl PartialEq for BuildInput {
    /// Compares specimens by identity: hosts replace their specimen when it
    /// changes. The options' material is ignored: it changes how the clump
    /// looks, not its mesh.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.specimen, &other.specimen)
            && self.rotation == other.rotation
            && self.scale == other.scale
            && same_mesh_options(&self.options, &other.options)
    }
}

/// Returns true if `a` and `b` build the same clump mesh: every setting but
/// the material matches.
fn same_mesh_options(a: &ClumpOptions, b: &ClumpOptions) -> bool {
    // Name every field, so a new setting can't be left out of the comparison.
    let ClumpOptions {
        seed,
        directions,
        scale,
        mask,
        placement,
        shape,
        mesh,
        grows_on,
        tags,
        material: _,
    } = a;
    *seed == b.seed
        && *directions == b.directions
        && *scale == b.scale
        && *mask == b.mask
        && *placement == b.placement
        && *shape == b.shape
        && *mesh == b.mesh
        && *grows_on == b.grows_on
        && *tags == b.tags
}

/// A finished build's mesh, its specimen description and its measurements.
type Grown = Result<(ClumpMesh, Specimen, ClumpBuilt), ClumpError>;

/// A clump build running in the background.
#[derive(Component)]
pub(crate) struct Building {
    input: BuildInput,
    task: Task<Grown>,
}

/// The input of the last finished build, whether it succeeded or not.
#[derive(Component)]
pub(crate) struct Attempted(BuildInput);

/// Clump materials and textures, shared by clumps that look the same.
#[derive(Resource, Default)]
pub(crate) struct ClumpMaterials {
    textures: HashMap<String, ClumpTextures>,
    materials: HashMap<MaterialKey, Handle<ClumpMaterial>>,
}

/// Identifies matching clump looks, so they can share a material.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MaterialKey {
    /// The texture set in use; `None` without textures.
    texture: Option<String>,
    /// Colour, roughness and texture size, as number bits for exact comparison.
    color: [u32; 3],
    roughness: u32,
    texture_size: u32,
}

impl MaterialKey {
    fn new(material: &MaterialOptions, texture: Option<String>) -> Self {
        Self {
            texture,
            color: material.color.map(f32::to_bits),
            roughness: material.roughness.to_bits(),
            texture_size: material.texture_size.to_bits(),
        }
    }
}

/// Despawns the parts of clumps that lost their [`ClumpParams`], and
/// forgets their builds.
pub(crate) fn remove_clumps(
    mut commands: Commands,
    mut removed: RemovedComponents<ClumpParams>,
    children: Query<&Children>,
    parts: Query<(), With<ClumpPart>>,
) {
    for entity in removed.read() {
        despawn_parts(&mut commands, children.get(entity).ok(), &parts);
        if let Ok(mut entity) = commands.get_entity(entity) {
            entity.remove::<(ClumpGenerating, Building, Attempted, GeneratedSpecimen)>();
        }
    }
}

/// Starts a build for every clump whose inputs differ from its running or
/// last finished build. A clump whose host has no specimen yet waits, and
/// loses any clump built on the host's previous specimen.
#[allow(clippy::type_complexity)]
pub(crate) fn start_generation(
    mut commands: Commands,
    clumps: Query<(
        Entity,
        &ClumpParams,
        Option<&ChildOf>,
        Option<&Building>,
        Option<&Attempted>,
        Option<&Children>,
        Has<ClumpGenerating>,
    )>,
    hosts: Query<&GeneratedSpecimen>,
    transforms: TransformHelper,
    parts: Query<(), With<ClumpPart>>,
) {
    for (entity, params, parent, building, attempted, kids, generating) in &clumps {
        // Wait for the host's specimen. A host that is regenerating takes its
        // clump with it. The host's world transform is worked out from the
        // hierarchy, as it may not have been propagated yet this frame.
        let host = parent.map(ChildOf::parent);
        let found = host.and_then(|h| {
            Some((
                hosts.get(h).ok()?,
                transforms.compute_global_transform(h).ok()?,
            ))
        });
        let Some((specimen, transform)) = found else {
            if attempted.is_some() || building.is_some() {
                despawn_parts(&mut commands, kids, &parts);
                commands
                    .entity(entity)
                    .remove::<(Building, Attempted, GeneratedSpecimen)>();
            }
            if !generating {
                commands.entity(entity).insert(ClumpGenerating);
            }
            continue;
        };

        // Skip clumps whose inputs haven't changed.
        let (scale, rotation, _) = transform.to_scale_rotation_translation();
        let input = BuildInput {
            options: params.0.clone(),
            specimen: specimen.0.clone(),
            rotation,
            scale: scale.x,
        };
        if building.is_some_and(|b| b.input == input) || attempted.is_some_and(|a| a.0 == input) {
            continue;
        }

        // The previous clump's specimen no longer describes what will be shown.
        commands
            .entity(entity)
            .remove::<(Building, GeneratedSpecimen)>();

        // A host stretched along one axis has no single scale to divide by.
        if !is_uniform(scale) {
            commands
                .entity(entity)
                .remove::<ClumpGenerating>()
                .insert(Attempted(input));
            commands.trigger(ClumpFinished {
                entity,
                result: Err(ClumpBuildError::NonUniformScale),
            });
            continue;
        }

        // Build in the host's coordinates, with world directions and lengths.
        let options = host_options(&input);
        let task_specimen = input.specimen.clone();
        let task = AsyncComputeTaskPool::get().spawn(async move { grow(&task_specimen, &options) });
        commands
            .entity(entity)
            .insert((ClumpGenerating, Building { input, task }));
    }
}

/// Returns true if `scale` is the same along every axis, to within 0.01%.
fn is_uniform(scale: Vec3) -> bool {
    let tolerance = scale.abs().max_element() * 1e-4;
    (scale.x - scale.y).abs() <= tolerance && (scale.x - scale.z).abs() <= tolerance
}

/// Returns the clump options to build with in the host's coordinates: its
/// directions turned into the host's frame, and its lengths divided by the
/// host's scale, so the clump keeps its world size.
fn host_options(input: &BuildInput) -> ClumpOptions {
    let mut options = (*input.options).clone();
    let to_host = input.rotation.inverse();
    for direction in &mut options.directions {
        direction.vector = (to_host * Vec3::from(direction.vector)).to_array();
    }
    options.scale /= input.scale;
    options
}

/// Grows a clump on `specimen` and measures how long it took.
fn grow(specimen: &Specimen, options: &ClumpOptions) -> Grown {
    let started = Instant::now();
    let clump = Clump::on(specimen, options)?;
    let built = ClumpBuilt {
        stats: clump.stats(),
        build_time: started.elapsed(),
        vertices: clump.mesh().vertex_count(),
        triangles: clump.mesh().triangle_count(),
    };
    Ok((clump.mesh().clone(), clump.specimen(), built))
}

/// Shows finished builds: replaces the clump's part, describes the clump
/// with a [`GeneratedSpecimen`], and triggers [`ClumpFinished`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn finish_generation(
    mut commands: Commands,
    mut clumps: Query<(Entity, &mut Building, &ClumpParams, Option<&Children>)>,
    parts: Query<(), With<ClumpPart>>,
    settings: Res<SpawnSettings>,
    asset_server: Res<AssetServer>,
    mut cache: ResMut<ClumpMaterials>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ClumpMaterial>>,
) {
    for (entity, mut building, params, kids) in &mut clumps {
        let Some(grown) = block_on(poll_once(&mut building.task)) else {
            continue;
        };

        // Replace the previous clump with the new one, in the current look:
        // the material may have changed while the clump was building.
        despawn_parts(&mut commands, kids, &parts);
        let result = grown.map(|(mesh, specimen, built)| {
            if !mesh.is_empty() {
                let material =
                    cache.material(&params.0.material, &settings, &asset_server, &mut materials);
                commands.spawn((
                    ClumpPart,
                    Mesh3d(meshes.add(clump_mesh(&mesh))),
                    MeshMaterial3d(material),
                    ChildOf(entity),
                ));
            }
            commands
                .entity(entity)
                .insert(GeneratedSpecimen(Arc::new(specimen)));
            built
        });
        if let Err(error) = &result {
            error!("procgena_clump_bevy: cannot grow clump: {error}");
        }

        // Record what was built so it isn't built again.
        let input = building.input.clone();
        commands
            .entity(entity)
            .remove::<(Building, ClumpGenerating)>()
            .insert(Attempted(input));
        commands.trigger(ClumpFinished {
            entity,
            result: result.map_err(ClumpBuildError::Clump),
        });
    }
}

impl ClumpMaterials {
    /// Returns the shared material for `material`, creating it and loading
    /// its textures on first use. Without a texture folder, or for an
    /// untextured look, the material uses plain colour.
    fn material(
        &mut self,
        material: &MaterialOptions,
        settings: &SpawnSettings,
        asset_server: &AssetServer,
        materials: &mut Assets<ClumpMaterial>,
    ) -> Handle<ClumpMaterial> {
        let set = settings
            .texture_root
            .as_ref()
            .zip(material.texture.as_deref().and_then(texture_set));
        let key = MaterialKey::new(material, set.map(|(_, set)| set.name.to_owned()));
        self.materials
            .entry(key)
            .or_insert_with(|| {
                let textures = set.map(|(root, set)| {
                    self.textures
                        .entry(set.name.to_owned())
                        .or_insert_with(|| {
                            ClumpTextures::load(
                                asset_server,
                                root,
                                set,
                                settings.texture_size,
                                settings.texture_compression,
                            )
                        })
                        .clone()
                });
                materials.add(clump_material(material, textures.as_ref()))
            })
            .clone()
    }
}

/// Gives the parts of clumps whose [`ClumpParams`] changed the material of
/// their current look, without rebuilding their meshes.
pub(crate) fn refresh_materials(
    clumps: Query<(&ClumpParams, &Children), Changed<ClumpParams>>,
    mut parts: Query<&mut MeshMaterial3d<ClumpMaterial>, With<ClumpPart>>,
    settings: Res<SpawnSettings>,
    asset_server: Res<AssetServer>,
    mut cache: ResMut<ClumpMaterials>,
    mut materials: ResMut<Assets<ClumpMaterial>>,
) {
    for (params, kids) in &clumps {
        let wanted = cache.material(&params.0.material, &settings, &asset_server, &mut materials);
        for &child in kids {
            if let Ok(mut material) = parts.get_mut(child)
                && material.0 != wanted
            {
                material.0 = wanted.clone();
            }
        }
    }
}

/// Forgets materials that no clump part uses any more, so they can be freed.
/// Texture images stay cached for reuse.
pub(crate) fn prune_materials(
    mut removed: RemovedComponents<ClumpPart>,
    parts: Query<&MeshMaterial3d<ClumpMaterial>, With<ClumpPart>>,
    mut cache: ResMut<ClumpMaterials>,
) {
    if removed.read().count() == 0 {
        return;
    }
    let used: HashSet<AssetId<ClumpMaterial>> = parts.iter().map(|m| m.0.id()).collect();
    cache
        .materials
        .retain(|_, handle| used.contains(&handle.id()));
}

/// Despawns the [`ClumpPart`] children among `kids`.
fn despawn_parts(
    commands: &mut Commands,
    kids: Option<&Children>,
    parts: &Query<(), With<ClumpPart>>,
) {
    for &child in kids.into_iter().flatten() {
        if parts.contains(child) {
            commands.entity(child).despawn();
        }
    }
}

/// Gives each clump part the distances its host is drawn at: from where the
/// host's nearest level starts to where its farthest level ends.
///
/// A clump has no levels of its own; it grows on the host's most detailed
/// level and shows whenever any of the host's levels does. A host whose
/// meshes have no distance limits (or which has no meshes) leaves its clumps
/// drawn at every distance.
pub(crate) fn follow_host_range(
    mut commands: Commands,
    parts: Query<(Entity, &ChildOf, Option<&VisibilityRange>), With<ClumpPart>>,
    clumps: Query<&ChildOf, With<ClumpParams>>,
    host_children: Query<&Children>,
    meshes: Query<Option<&VisibilityRange>, (With<Mesh3d>, Without<ClumpPart>)>,
) {
    for (part, clump, current) in &parts {
        let Ok(host) = clumps.get(clump.parent()) else {
            continue;
        };

        // The host's drawn levels: its own mesh, or its mesh children.
        let host = host.parent();
        let children = host_children.get(host).into_iter().flatten().copied();
        let levels: Vec<Option<&VisibilityRange>> = std::iter::once(host)
            .chain(children)
            .filter_map(|e| meshes.get(e).ok())
            .collect();
        let wanted = host_range(&levels);
        if wanted.as_ref() != current {
            match wanted {
                Some(range) => commands.entity(part).insert(range),
                None => commands.entity(part).remove::<VisibilityRange>(),
            };
        }
    }
}

/// The union of a host's level ranges, or `None` when the host has no levels
/// or any level is drawn at every distance.
fn host_range(levels: &[Option<&VisibilityRange>]) -> Option<VisibilityRange> {
    let mut union: Option<VisibilityRange> = None;
    for level in levels {
        let range = (*level)?;
        union = Some(match union {
            None => range.clone(),
            Some(u) => VisibilityRange {
                start_margin: if range.start_margin.start < u.start_margin.start {
                    range.start_margin.clone()
                } else {
                    u.start_margin
                },
                end_margin: if range.end_margin.end > u.end_margin.end {
                    range.end_margin.clone()
                } else {
                    u.end_margin
                },
                use_aabb: u.use_aabb,
            },
        });
    }
    union
}

#[cfg(test)]
mod tests {
    use std::f32::consts::PI;
    use std::ops::Range;

    use bevy::transform::TransformPlugin;
    use procgena_specimen::{Part, SurfaceMesh, tags};

    use super::*;
    use crate::tagged::ObjectTags;

    /// Results of every [`ClumpFinished`] event, in order.
    #[derive(Resource, Default)]
    struct Finished(Vec<Result<ClumpBuilt, ClumpBuildError>>);

    /// An app running the clump systems and transform propagation, without
    /// rendering or textures.
    fn clump_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), TransformPlugin))
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<ClumpMaterial>()
            .init_resource::<ClumpMaterials>()
            .init_resource::<Finished>()
            .insert_resource(SpawnSettings {
                texture_root: None,
                texture_compression: TextureCompression::Bc7,
                texture_size: TextureSize::Size1k,
            })
            .add_systems(Update, crate::spawn_systems())
            .add_observer(|event: On<ClumpFinished>, mut finished: ResMut<Finished>| {
                finished.0.push(event.result.clone());
            });
        app
    }

    /// A one-metre ground patch, as a host would describe it.
    fn ground() -> GeneratedSpecimen {
        let part = Part::new([tags::GROUND], SurfaceMesh::ground(1.0, 4));
        GeneratedSpecimen(Arc::new(Specimen::single(vec![part])))
    }

    /// Spawns a host with `transform` and a snow clump on it; returns both.
    fn spawn_snow(app: &mut App, transform: Transform) -> (Entity, Entity) {
        let host = app
            .world_mut()
            .spawn((transform, Visibility::default()))
            .id();
        let clump = app
            .world_mut()
            .spawn((
                ClumpParams::preset(ClumpPreset::DirtySnowCap),
                ChildOf(host),
            ))
            .id();
        (host, clump)
    }

    /// Updates the app until `clump` loses [`ClumpGenerating`].
    fn update_until_finished(app: &mut App, clump: Entity) {
        let deadline = Instant::now() + Duration::from_secs(30);
        app.update();
        while app.world().entity(clump).contains::<ClumpGenerating>() {
            assert!(Instant::now() < deadline, "clump generation did not finish");
            std::thread::sleep(Duration::from_millis(1));
            app.update();
        }
    }

    /// The number of [`ClumpPart`] children of `clump`.
    fn part_count(app: &mut App, clump: Entity) -> usize {
        let mut parts = app
            .world_mut()
            .query_filtered::<&ChildOf, With<ClumpPart>>();
        parts
            .iter(app.world())
            .filter(|p| p.parent() == clump)
            .count()
    }

    /// The highest point of `clump`'s mesh, in host coordinates.
    fn top(app: &App, clump: Entity) -> f32 {
        let specimen = app.world().get::<GeneratedSpecimen>(clump).unwrap();
        let positions = &specimen.0.levels[0].parts[0].mesh.positions;
        positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max)
    }

    #[test]
    fn clumps_wait_for_their_host_then_grow() {
        let mut app = clump_app();
        let (host, clump) = spawn_snow(&mut app, Transform::default());
        for _ in 0..3 {
            app.update();
        }
        assert!(app.world().entity(clump).contains::<ClumpGenerating>());
        assert_eq!(part_count(&mut app, clump), 0);

        // The clump grows once the host describes itself.
        app.world_mut().entity_mut(host).insert(ground());
        update_until_finished(&mut app, clump);
        assert_eq!(part_count(&mut app, clump), 1);
        let finished = &app.world().resource::<Finished>().0;
        assert_eq!(finished.len(), 1);
        assert!(finished[0].as_ref().unwrap().triangles > 0);
        let specimen = app.world().get::<GeneratedSpecimen>(clump).unwrap();
        assert_eq!(
            specimen.0.levels[0].parts[0].tags,
            [tags::CLUMP, tags::SNOW].into()
        );

        // A host that regenerates takes its clump with it until it is done.
        app.world_mut()
            .entity_mut(host)
            .remove::<GeneratedSpecimen>();
        app.update();
        assert_eq!(part_count(&mut app, clump), 0);
        assert!(app.world().entity(clump).contains::<ClumpGenerating>());
        assert!(!app.world().entity(clump).contains::<GeneratedSpecimen>());
        app.world_mut().entity_mut(host).insert(ground());
        update_until_finished(&mut app, clump);
        assert_eq!(part_count(&mut app, clump), 1);

        // Removing the request removes the clump.
        app.world_mut().entity_mut(clump).remove::<ClumpParams>();
        app.update();
        assert_eq!(part_count(&mut app, clump), 0);
        assert!(!app.world().entity(clump).contains::<GeneratedSpecimen>());
    }

    #[test]
    fn only_the_latest_settings_finish() {
        let mut app = clump_app();
        let (host, clump) = spawn_snow(&mut app, Transform::default());
        app.world_mut().entity_mut(host).insert(ground());
        update_until_finished(&mut app, clump);

        // Change the settings twice before the first change can finish.
        let set = |app: &mut App, preset: ClumpPreset| {
            *app.world_mut().get_mut::<ClumpParams>(clump).unwrap() = ClumpParams::preset(preset);
            app.update();
        };
        set(&mut app, ClumpPreset::MossCushions);
        set(&mut app, ClumpPreset::MossPatches);
        update_until_finished(&mut app, clump);

        // The previous clump stayed until then, and only one build finished.
        assert_eq!(app.world().resource::<Finished>().0.len(), 2);
        assert_eq!(part_count(&mut app, clump), 1);
        let specimen = app.world().get::<GeneratedSpecimen>(clump).unwrap();
        assert_eq!(
            specimen.0.levels[0].parts[0].tags,
            [tags::CLUMP, tags::MOSS].into()
        );
    }

    #[test]
    fn a_new_look_swaps_the_material_without_rebuilding() {
        let mut app = clump_app();
        let (host, clump) = spawn_snow(&mut app, Transform::default());
        app.world_mut().entity_mut(host).insert(ground());
        update_until_finished(&mut app, clump);
        let material = |app: &mut App| {
            let mut parts = app
                .world_mut()
                .query_filtered::<&MeshMaterial3d<ClumpMaterial>, With<ClumpPart>>();
            parts.single(app.world()).unwrap().0.clone()
        };
        let before = material(&mut app);

        // Tint the snow blue: the material changes, the mesh is kept.
        let mut options = ClumpPreset::DirtySnowCap.options();
        options.material.color = [0.2, 0.3, 1.0];
        *app.world_mut().get_mut::<ClumpParams>(clump).unwrap() = ClumpParams::new(options);
        app.update();
        assert!(!app.world().entity(clump).contains::<ClumpGenerating>());
        assert_eq!(app.world().resource::<Finished>().0.len(), 1);
        let after = material(&mut app);
        assert_ne!(before, after);
        let tint = app
            .world()
            .resource::<Assets<ClumpMaterial>>()
            .get(&after)
            .unwrap();
        assert_eq!(tint.base.base_color, Color::linear_rgb(0.2, 0.3, 1.0));
    }

    #[test]
    fn turning_the_host_rebuilds_and_moving_it_does_not() {
        let mut app = clump_app();
        let (host, clump) = spawn_snow(&mut app, Transform::default());
        app.world_mut().entity_mut(host).insert(ground());
        update_until_finished(&mut app, clump);
        assert_eq!(part_count(&mut app, clump), 1);

        // Moving the host keeps its clump.
        app.world_mut()
            .get_mut::<Transform>(host)
            .unwrap()
            .translation = Vec3::new(5.0, 1.0, 0.0);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(app.world().resource::<Finished>().0.len(), 1);

        // Turned upside down, the ground faces away from falling snow.
        app.world_mut().get_mut::<Transform>(host).unwrap().rotation = Quat::from_rotation_x(PI);
        app.update();
        update_until_finished(&mut app, clump);
        let finished = &app.world().resource::<Finished>().0;
        assert_eq!(finished.len(), 2);
        assert_eq!(finished[1].as_ref().unwrap().triangles, 0);
        assert_eq!(part_count(&mut app, clump), 0);
    }

    #[test]
    fn scaled_hosts_keep_world_sizes() {
        let mut app = clump_app();
        let (host, plain) = spawn_snow(&mut app, Transform::default());
        app.world_mut().entity_mut(host).insert(ground());
        update_until_finished(&mut app, plain);

        let (big_host, big) = spawn_snow(&mut app, Transform::from_scale(Vec3::splat(2.0)));
        app.world_mut().entity_mut(big_host).insert(ground());
        update_until_finished(&mut app, big);

        // On a host drawn twice as large, the snow is about half as deep in
        // the host's own coordinates.
        let ratio = top(&app, big) / top(&app, plain);
        assert!((0.35..0.65).contains(&ratio), "ratio {ratio}");

        // A host stretched along one axis is refused.
        let (stretched_host, stretched) =
            spawn_snow(&mut app, Transform::from_scale(Vec3::new(1.0, 2.0, 1.0)));
        app.world_mut().entity_mut(stretched_host).insert(ground());
        update_until_finished(&mut app, stretched);
        let finished = &app.world().resource::<Finished>().0;
        assert_eq!(
            finished.last(),
            Some(&Err(ClumpBuildError::NonUniformScale))
        );
    }

    #[test]
    fn clumps_grow_on_tagged_meshes() {
        let mut app = clump_app();
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Plane3d::default().mesh().size(1.0, 1.0).subdivisions(3));
        let host = app
            .world_mut()
            .spawn((Mesh3d(mesh), ObjectTags([tags::GROUND].into())))
            .id();
        let clump = app
            .world_mut()
            .spawn((
                ClumpParams::preset(ClumpPreset::DirtySnowCap),
                ChildOf(host),
            ))
            .id();
        update_until_finished(&mut app, clump);
        assert_eq!(part_count(&mut app, clump), 1);

        // Without a matching tag nothing grows.
        app.world_mut()
            .entity_mut(host)
            .insert(ObjectTags([tags::LEAF, tags::CARD].into()));
        app.update();
        update_until_finished(&mut app, clump);
        assert_eq!(part_count(&mut app, clump), 0);
    }

    #[test]
    fn clumps_are_drawn_over_their_hosts_levels() {
        let mut app = clump_app();
        let (host, clump) = spawn_snow(&mut app, Transform::IDENTITY);
        app.world_mut().entity_mut(host).insert(ground());

        // Two host levels: near until 10–12 m, far from there to 30–33 m.
        let level = |start: Range<f32>, end: Range<f32>| VisibilityRange {
            start_margin: start,
            end_margin: end,
            use_aabb: false,
        };
        let near = level(0.0..0.0, 10.0..12.0);
        let far = level(10.0..12.0, 30.0..33.0);
        for range in [near, far] {
            app.world_mut()
                .spawn((Mesh3d(Handle::default()), range, ChildOf(host)));
        }
        update_until_finished(&mut app, clump);
        app.update();
        app.update();

        let mut parts = app
            .world_mut()
            .query_filtered::<(&ChildOf, Option<&VisibilityRange>), With<ClumpPart>>();
        let ranges: Vec<_> = parts
            .iter(app.world())
            .filter(|(p, _)| p.parent() == clump)
            .map(|(_, r)| r.map(|r| (r.start_margin.clone(), r.end_margin.clone())))
            .collect();
        assert_eq!(ranges, [Some((0.0..0.0, 30.0..33.0))]);

        // A host level drawn at every distance keeps the clump on everywhere.
        app.world_mut()
            .spawn((Mesh3d(Handle::default()), ChildOf(host)));
        app.update();
        app.update();
        let mut parts = app
            .world_mut()
            .query_filtered::<(&ChildOf, Option<&VisibilityRange>), With<ClumpPart>>();
        assert!(
            parts
                .iter(app.world())
                .filter(|(p, _)| p.parent() == clump)
                .all(|(_, r)| r.is_none())
        );
    }
}
