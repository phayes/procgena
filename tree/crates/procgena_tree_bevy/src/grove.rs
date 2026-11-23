//! Groves: many generated trees spread over ground as children of one entity.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use procgena_mesh::{GeneratedObject, Object, TagFilter};
use procgena_tree::{
    FlatGround, Ground, GroveError, GroveInstance, GrovePlacement, GrovePreset, HeightGrid,
    TreeGrove, TreeOptions,
};

use crate::spawn::{TreeGenerating, TreeParams};

/// A grove of trees, spread over a rectangle of ground centred on this
/// entity.
///
/// Add it after installing [`TreePlugin`](crate::TreePlugin). Each placed
/// tree becomes a child entity with a [`TreeParams`], a `Transform` and a
/// [`GroveMember`], and is generated and drawn like any other tree. Trees of
/// the same species and shape variant share their meshes, so a grove of
/// hundreds of trees builds only a handful of meshes. Each tree is still a
/// few entities (one per part and detail level), so keep groves to hundreds
/// of trees rather than thousands.
///
/// Changing the component updates the grove in place: trees that keep their
/// spot, species and shape stay as they are, and only trees that are new,
/// gone or changed are spawned, despawned or rebuilt. Lowering the density
/// only removes trees. Removing the component removes the trees.
#[derive(Component, Debug, Clone, PartialEq)]
#[require(Transform, Visibility)]
pub struct TreeGroveParams {
    /// Which trees grow, and how they vary.
    pub grove: GroveSource,
    /// Width (X) and depth (Z) of the ground, in metres. Each side must be
    /// above 0 and at most [`MAX_GROVE_SIZE`](procgena_tree::MAX_GROVE_SIZE).
    pub size: Vec2,
    /// The ground the trees stand on.
    pub ground: GroveGround,
}

impl TreeGroveParams {
    /// A grove on flat ground at the entity's height, `size` metres across.
    pub fn new(grove: TreeGrove, size: Vec2) -> Self {
        Self {
            grove: GroveSource::Options(Arc::new(grove)),
            size,
            ground: GroveGround::Flat,
        }
    }

    /// A preset's grove on flat ground, `size` metres across.
    pub fn preset(preset: GrovePreset, size: Vec2) -> Self {
        Self::new(preset.grove(), size)
    }

    /// The same grove, standing on its parent's surface instead; see
    /// [`GroveGround::Host`].
    pub fn on_host(mut self, filter: TagFilter, cell: f32) -> Self {
        self.ground = GroveGround::Host { filter, cell };
        self
    }
}

/// Where a [`TreeGroveParams`] gets its grove.
#[derive(Debug, Clone, PartialEq)]
pub enum GroveSource {
    /// A grove held in memory.
    Options(Arc<TreeGrove>),
    /// A grove loaded from a `.grove.json` file. The trees wait for the file
    /// to load, and update when the file changes if Bevy is watching asset
    /// files.
    Asset(Handle<TreeGroveAsset>),
}

/// The ground a [`TreeGroveParams`] stands its trees on.
#[derive(Debug, Clone, PartialEq)]
pub enum GroveGround {
    /// Flat ground at the entity's height.
    Flat,
    /// The surface of the entity's parent, which must have a
    /// [`GeneratedObject`], such as terrain or a rock from another procgena
    /// generator.
    ///
    /// Spawn the grove as a child of that host and leave its own transform
    /// at its default. The ground is the highest surface of the object's
    /// first level that `filter` matches, read in the host's own
    /// coordinates with its Y as up and sampled every `cell` metres. Trees
    /// are left out where it has no surface. Sizes and spacings stay in
    /// world metres however the host is scaled, but a host stretched
    /// differently along different axes is rejected with
    /// [`TreeGroveBuildError::NonUniformScale`].
    ///
    /// The grove waits until the host has its object, and updates when the
    /// object, the host's scale or these settings change. If the host loses
    /// its object, the trees are removed.
    Host {
        /// Which parts of the host's object count as ground.
        filter: TagFilter,
        /// Spacing of the ground samples in metres. Must be positive.
        cell: f32,
    },
}

/// A grove loaded from a `.grove.json` file; see [`TreeGrove::from_json`].
/// Files with invalid settings fail to load.
#[derive(Asset, TypePath, Debug, Clone, PartialEq)]
pub struct TreeGroveAsset(pub TreeGrove);

/// Marks a tree child spawned by a [`TreeGroveParams`]. The plugin spawns,
/// updates and removes these; don't add them yourself.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroveMember {
    /// The tree's species: its position in [`TreeGrove::trees`].
    pub tree: usize,
    /// Which of the species' shape variants it is.
    pub variant: u32,
    /// The spot it stands on; see [`GroveInstance::id`].
    pub id: u64,
}

/// Present on a grove while its trees are being placed or generated,
/// including while it waits for its file or its host.
#[derive(Component, Debug, Default)]
pub struct TreeGroveGenerating;

/// Triggered on a grove entity when a build ends, successfully or not.
///
/// On success every tree of the grove has been generated and its parts
/// spawned.
#[derive(EntityEvent, Debug, Clone, PartialEq)]
pub struct TreeGroveFinished {
    /// The grove entity.
    pub entity: Entity,
    /// `Ok` when the trees are in place, or why the grove could not be built.
    pub result: Result<(), TreeGroveBuildError>,
}

/// Why a grove could not be built.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TreeGroveBuildError {
    /// A setting is out of range, or the grove asks for too many trees.
    Grove(GroveError),
    /// No part of the host's object matched the ground filter.
    NoGround,
    /// [`GroveGround::Host`]'s `cell` is not positive.
    BadCell,
    /// The grove's host is scaled differently along different axes.
    NonUniformScale,
}

impl fmt::Display for TreeGroveBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Grove(error) => write!(f, "{error}"),
            Self::NoGround => write!(f, "no part of the grove's host matches its ground filter"),
            Self::BadCell => write!(f, "the grove's ground cell size must be positive"),
            Self::NonUniformScale => write!(f, "the grove's host has a non-uniform scale"),
        }
    }
}

impl std::error::Error for TreeGroveBuildError {}

/// Reads `.grove.json` files and checks their settings.
#[derive(Debug, Default, TypePath)]
pub(crate) struct TreeGroveLoader;

impl AssetLoader for TreeGroveLoader {
    type Asset = TreeGroveAsset;
    type Settings = ();
    type Error = BevyError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _context: &mut LoadContext<'_>,
    ) -> Result<TreeGroveAsset, BevyError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let grove = TreeGrove::from_json(std::str::from_utf8(&bytes)?)?;
        grove.validate()?;
        Ok(TreeGroveAsset(grove))
    }

    fn extensions(&self) -> &[&str] {
        &["grove.json"]
    }
}

// --------------------------------------------------------------------------
// Build inputs

/// Everything a grove's placement depends on. When a grove's input differs
/// from its last one, it is placed again.
#[derive(Debug, Clone)]
pub(crate) struct Input {
    grove: Arc<TreeGrove>,
    size: Vec2,
    ground: InputGround,
}

#[derive(Debug, Clone)]
enum InputGround {
    Flat,
    /// A host's object, with the host's (uniform) world scale.
    Host {
        filter: TagFilter,
        cell: f32,
        host: Arc<Object>,
        scale: f32,
    },
}

impl InputGround {
    /// True when both are the same ground. Host objects can be large, so
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
    fn is(&self, grove: &TreeGrove, size: Vec2, ground: &InputGround) -> bool {
        self.size == size && self.ground.same(ground) && *self.grove == *grove
    }
}

/// On a grove while its placement runs on a background thread.
#[derive(Component)]
pub(crate) struct Placing {
    input: Input,
    task: Task<Result<Vec<GroveInstance>, TreeGroveBuildError>>,
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
    /// Nothing yet: its file or its host's object isn't available.
    Wait,
    /// Place `grove` at `size` on `ground`, or fail with `error` if given.
    Place {
        grove: GroveRef<'a>,
        size: Vec2,
        ground: InputGround,
        error: Option<TreeGroveBuildError>,
    },
}

/// A grove borrowed from its component or its loaded file.
enum GroveRef<'a> {
    Shared(&'a Arc<TreeGrove>),
    Loaded(&'a TreeGrove),
}

impl GroveRef<'_> {
    fn get(&self) -> &TreeGrove {
        match self {
            Self::Shared(grove) => grove,
            Self::Loaded(grove) => grove,
        }
    }

    fn to_arc(&self) -> Arc<TreeGrove> {
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
        &TreeGroveParams,
        Option<&ChildOf>,
        Option<&Placing>,
        Option<&Placed>,
        Option<&Children>,
        Has<TreeGroveGenerating>,
    )>,
    hosts: Query<&GeneratedObject>,
    transforms: TransformHelper,
    assets: Res<Assets<TreeGroveAsset>>,
    members: Query<(), With<GroveMember>>,
) {
    for (entity, params, parent, placing, placed, kids, generating) in &groves {
        let want = wanted(params, parent, &hosts, &transforms, &assets);
        let Wanted::Place {
            grove,
            size,
            ground,
            error,
        } = want
        else {
            // While waiting, remove trees laid on a host that lost its
            // object.
            if placing.is_some() || placed.is_some() {
                despawn_members(&mut commands, kids, &members);
                commands.entity(entity).remove::<(Placing, Placed)>();
            }
            if !generating {
                commands.entity(entity).insert(TreeGroveGenerating);
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
            fail(&mut commands, entity, kids, &members, input, error);
            continue;
        }
        let task_input = input.clone();
        let task = AsyncComputeTaskPool::get().spawn(async move { place(&task_input) });
        commands
            .entity(entity)
            .insert((TreeGroveGenerating, Placing { input, task }));
    }
}

/// Works out what grove, size and ground `params` asks for this frame.
fn wanted<'a>(
    params: &'a TreeGroveParams,
    parent: Option<&ChildOf>,
    hosts: &Query<&GeneratedObject>,
    transforms: &TransformHelper,
    assets: &'a Assets<TreeGroveAsset>,
) -> Wanted<'a> {
    let grove = match &params.grove {
        GroveSource::Options(grove) => GroveRef::Shared(grove),
        GroveSource::Asset(handle) => match assets.get(handle) {
            Some(asset) => GroveRef::Loaded(&asset.0),
            None => return Wanted::Wait,
        },
    };
    let (filter, cell) = match &params.ground {
        GroveGround::Flat => {
            return Wanted::Place {
                grove,
                size: params.size,
                ground: InputGround::Flat,
                error: None,
            };
        }
        GroveGround::Host { filter, cell } => (filter, *cell),
    };

    // Wait for the host's object. Work out the host's world scale from its
    // parents, because Bevy may not have updated it yet this frame.
    let host = parent.map(ChildOf::parent);
    let found = host.and_then(|h| {
        Some((
            hosts.get(h).ok()?,
            transforms.compute_global_transform(h).ok()?,
        ))
    });
    let Some((object, transform)) = found else {
        return Wanted::Wait;
    };
    let (scale, _, _) = transform.to_scale_rotation_translation();
    let tolerance = scale.abs().max_element() * 1e-4;
    let uniform = scale.x > 0.0
        && (scale.x - scale.y).abs() <= tolerance
        && (scale.x - scale.z).abs() <= tolerance;
    let error = if !uniform {
        Some(TreeGroveBuildError::NonUniformScale)
    } else if !(cell > 0.0 && cell.is_finite()) {
        Some(TreeGroveBuildError::BadCell)
    } else {
        None
    };
    Wanted::Place {
        grove,
        size: params.size,
        ground: InputGround::Host {
            filter: filter.clone(),
            cell,
            host: object.0.clone(),
            scale: scale.x,
        },
        error,
    }
}

/// Removes a grove's trees, records `input` as attempted, and reports
/// `error`.
fn fail(
    commands: &mut Commands,
    entity: Entity,
    kids: Option<&Children>,
    members: &Query<(), With<GroveMember>>,
    input: Input,
    error: TreeGroveBuildError,
) {
    error!("procgena_tree_bevy: cannot build grove: {error}");
    despawn_members(commands, kids, members);
    commands
        .entity(entity)
        .remove::<(Placing, TreeGroveGenerating)>()
        .insert(Placed {
            input,
            failed: true,
        });
    commands.trigger(TreeGroveFinished {
        entity,
        result: Err(error),
    });
}

/// Ground read from a host's object in the host's units, given in world
/// metres by multiplying by the host's scale.
struct ScaledGround {
    grid: HeightGrid,
    scale: f32,
}

impl Ground for ScaledGround {
    fn height(&self, x: f32, z: f32) -> f32 {
        self.grid.height(x / self.scale, z / self.scale) * self.scale
    }
}

/// Places the grove described by `input`, in world metres. Runs on a
/// background thread.
fn place(input: &Input) -> Result<Vec<GroveInstance>, TreeGroveBuildError> {
    let size = input.size.to_array();
    let placement = match &input.ground {
        InputGround::Flat => GrovePlacement::new(&input.grove, size, &FlatGround, None),
        InputGround::Host {
            filter,
            cell,
            host,
            scale,
        } => {
            let grid = HeightGrid::from_object(host, 0, filter, cell / scale)
                .ok_or(TreeGroveBuildError::NoGround)?;
            let ground = ScaledGround {
                grid,
                scale: *scale,
            };
            GrovePlacement::new(&input.grove, size, &ground, None)
        }
    };
    placement
        .map(|p| p.instances().to_vec())
        .map_err(TreeGroveBuildError::Grove)
}

/// Brings each grove whose placement finished up to date: spawns new trees,
/// despawns trees that are gone, and updates the rest in place.
pub(crate) fn finish_placing(
    mut commands: Commands,
    mut placing: Query<(Entity, &mut Placing, Option<&Children>)>,
    members: Query<(&GroveMember, &TreeParams, &Transform)>,
    member_marker: Query<(), With<GroveMember>>,
) {
    for (entity, mut p, kids) in &mut placing {
        let Some(result) = block_on(poll_once(&mut p.task)) else {
            continue;
        };
        let input = p.input.clone();
        let instances = match result {
            Ok(instances) => instances,
            Err(error) => {
                fail(&mut commands, entity, kids, &member_marker, input, error);
                continue;
            }
        };
        commands.entity(entity).remove::<Placing>().insert(Placed {
            input: input.clone(),
            failed: false,
        });

        // Index the grove's current trees by the spot they stand on.
        let mut existing: HashMap<u64, Entity> = HashMap::new();
        for &child in kids.into_iter().flatten() {
            if let Ok((member, _, _)) = members.get(child) {
                existing.insert(member.id, child);
            }
        }

        // Share one copy of each variant's options between its trees.
        let mut variants: HashMap<(usize, u32), Arc<TreeOptions>> = HashMap::new();
        let shrink = 1.0 / input.ground.scale();
        for instance in instances {
            let options = variants
                .entry((instance.tree, instance.variant))
                .or_insert_with(|| {
                    Arc::new(input.grove.trees[instance.tree].variant(instance.variant))
                });
            let member = GroveMember {
                tree: instance.tree,
                variant: instance.variant,
                id: instance.id,
            };
            let transform = Transform::from_matrix(
                Mat4::from_scale(Vec3::splat(shrink)) * Mat4::from(instance.transform),
            );

            // Update a tree already on this spot, touching only what changed
            // so unchanged trees keep their parts.
            if let Some(child) = existing.remove(&instance.id) {
                let Ok((old_member, old_params, old_transform)) = members.get(child) else {
                    continue;
                };
                let mut child = commands.entity(child);
                if *old_member != member {
                    child.insert(member);
                }
                let same_tree =
                    matches!(old_params, TreeParams::Options(old) if **old == **options);
                if !same_tree {
                    child.insert(TreeParams::Options(Arc::clone(options)));
                }
                if *old_transform != transform {
                    child.insert(transform);
                }
                continue;
            }
            commands.spawn((
                TreeParams::Options(Arc::clone(options)),
                transform,
                member,
                ChildOf(entity),
            ));
        }
        for (_, child) in existing {
            commands.entity(child).despawn();
        }
    }
}

/// Ends the build of each placed grove once none of its trees is still
/// generating, and reports it with [`TreeGroveFinished`].
#[expect(clippy::type_complexity, reason = "Bevy system parameters")]
pub(crate) fn finish_groves(
    mut commands: Commands,
    groves: Query<
        (Entity, &Placed, Option<&Children>),
        (With<TreeGroveGenerating>, Without<Placing>),
    >,
    generating: Query<(), (With<GroveMember>, With<TreeGenerating>)>,
) {
    for (entity, placed, kids) in &groves {
        if placed.failed || kids.into_iter().flatten().any(|&c| generating.contains(c)) {
            continue;
        }
        commands.entity(entity).remove::<TreeGroveGenerating>();
        commands.trigger(TreeGroveFinished {
            entity,
            result: Ok(()),
        });
    }
}

/// Removes the trees and build state of groves that lost their
/// [`TreeGroveParams`].
pub(crate) fn remove_groves(
    mut commands: Commands,
    mut removed: RemovedComponents<TreeGroveParams>,
    children: Query<&Children>,
    members: Query<(), With<GroveMember>>,
) {
    for entity in removed.read() {
        despawn_members(&mut commands, children.get(entity).ok(), &members);
        if let Ok(mut entity) = commands.get_entity(entity) {
            entity.remove::<(Placing, Placed, TreeGroveGenerating)>();
        }
    }
}

/// Despawns the grove's tree children, leaving any other children alone.
fn despawn_members(
    commands: &mut Commands,
    kids: Option<&Children>,
    members: &Query<(), With<GroveMember>>,
) {
    for &child in kids.into_iter().flatten() {
        if members.contains(child) {
            commands.entity(child).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::time::{Duration, Instant};

    use procgena_mesh::{Part, SurfaceMesh, tags};
    use procgena_tree::{GroveTree, LodLevel, TextureCompression, TextureSize, TreePreset};

    use super::*;
    use crate::material::TreeMaterial;
    use crate::spawn::{
        BarkTexture, LeafTexture, SpawnSettings, TreeCache, TreeKey, TreeLodLevel,
        TreeOptionsAsset, TreePart, finish_generation, prune_cache, reset_changed_trees,
        spawn_parts, start_generation,
    };
    use crate::wind::Wind;

    /// Results of every [`TreeGroveFinished`] event, in order.
    #[derive(Resource, Default)]
    struct Finished(Vec<Result<(), TreeGroveBuildError>>);

    /// An app with one full-detail level and the grove and tree systems in
    /// the plugin's order, without rendering or loading textures.
    fn grove_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<StandardMaterial>()
            .init_asset::<TreeMaterial>()
            .init_asset::<TreeOptionsAsset>()
            .init_asset::<TreeGroveAsset>()
            .init_resource::<Wind>()
            .init_resource::<TreeCache>()
            .init_resource::<Finished>()
            .insert_resource(SpawnSettings {
                texture_root: None,
                texture_compression: TextureCompression::Bc7,
                lod_levels: vec![TreeLodLevel {
                    level: LodLevel::default(),
                    bark: BarkTexture::Textured(TextureSize::Size1k),
                    leaves: LeafTexture::Leaf,
                }],
            })
            .add_systems(
                Update,
                (
                    remove_groves,
                    finish_placing,
                    start_groves,
                    reset_changed_trees,
                    start_generation,
                    finish_generation,
                    spawn_parts,
                    prune_cache,
                    finish_groves,
                )
                    .chain(),
            )
            .add_observer(
                |event: On<TreeGroveFinished>, mut finished: ResMut<Finished>| {
                    finished.0.push(event.result.clone());
                },
            );
        app
    }

    /// A small grove of two quick-to-build species, two variants each.
    fn small_grove() -> TreeGrove {
        TreeGrove::new(vec![
            GroveTree::new(TreePreset::Bush2)
                .with_variants(2)
                .scaled(0.4),
            GroveTree::new(TreePreset::PineSmall)
                .with_variants(2)
                .scaled(0.2),
        ])
    }

    const SIZE: Vec2 = Vec2::new(12.0, 12.0);

    /// Updates the app until `grove` loses [`TreeGroveGenerating`].
    fn update_until_finished(app: &mut App, grove: Entity) {
        let deadline = Instant::now() + Duration::from_secs(60);
        app.update();
        while app.world().entity(grove).contains::<TreeGroveGenerating>() {
            assert!(Instant::now() < deadline, "grove did not finish");
            std::thread::sleep(Duration::from_millis(1));
            app.update();
        }
    }

    /// The grove's trees: spot id to (entity, member, part entities).
    fn trees(app: &mut App, grove: Entity) -> HashMap<u64, (Entity, GroveMember, Vec<Entity>)> {
        let mut members = app.world_mut().query::<(Entity, &GroveMember, &ChildOf)>();
        let world = app.world();
        members
            .iter(world)
            .filter(|(_, _, parent)| parent.parent() == grove)
            .map(|(entity, member, _)| {
                let parts = world
                    .get::<Children>(entity)
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|&c| world.entity(c).contains::<TreePart>())
                    .collect();
                (member.id, (entity, *member, parts))
            })
            .collect()
    }

    fn set_grove(app: &mut App, entity: Entity, grove: TreeGrove) {
        app.world_mut()
            .get_mut::<TreeGroveParams>(entity)
            .unwrap()
            .grove = GroveSource::Options(Arc::new(grove));
    }

    #[test]
    fn a_grove_spawns_its_placed_trees_sharing_variant_meshes() {
        let mut app = grove_app();
        let grove = small_grove();
        let entity = app
            .world_mut()
            .spawn(TreeGroveParams::new(grove.clone(), SIZE))
            .id();
        update_until_finished(&mut app, entity);
        assert_eq!(app.world().resource::<Finished>().0, vec![Ok(())]);

        let placed = GrovePlacement::new(&grove, SIZE.to_array(), &FlatGround, None).unwrap();
        let spawned = trees(&mut app, entity);
        assert!(spawned.len() > 4);
        assert_eq!(spawned.len(), placed.instances().len());
        assert!(spawned.values().all(|(_, _, parts)| !parts.is_empty()));

        // At most two variants of each of two species: four shared keys.
        let mut keys = app.world_mut().query::<(&TreeKey, &GroveMember)>();
        let distinct: HashSet<_> = keys.iter(app.world()).map(|(k, _)| *k).collect();
        assert!(distinct.len() <= 4);
    }

    #[test]
    fn lowering_density_keeps_the_remaining_trees_untouched() {
        let mut app = grove_app();
        let entity = app
            .world_mut()
            .spawn(TreeGroveParams::new(small_grove(), SIZE))
            .id();
        update_until_finished(&mut app, entity);
        let before = trees(&mut app, entity);

        let sparse = small_grove().with_density(small_grove().density() * 0.5);
        set_grove(&mut app, entity, sparse);
        update_until_finished(&mut app, entity);
        let after = trees(&mut app, entity);

        assert!(!after.is_empty() && after.len() < before.len());
        for (id, tree) in &after {
            assert_eq!(before.get(id), Some(tree), "a kept tree was rebuilt");
        }
        assert_eq!(app.world().resource::<Finished>().0.len(), 2);
    }

    #[test]
    fn changing_one_species_rebuilds_only_its_trees() {
        let mut app = grove_app();
        let entity = app
            .world_mut()
            .spawn(TreeGroveParams::new(small_grove(), SIZE))
            .id();
        update_until_finished(&mut app, entity);
        let before = trees(&mut app, entity);

        let mut changed = small_grove();
        changed.trees[1].tree.seed += 1;
        set_grove(&mut app, entity, changed);
        update_until_finished(&mut app, entity);
        let after = trees(&mut app, entity);

        assert_eq!(after.len(), before.len());
        for (id, (entity, member, parts)) in &after {
            let (old_entity, _, old_parts) = &before[id];
            assert_eq!(entity, old_entity, "trees keep their entities");
            if member.tree == 0 {
                assert_eq!(parts, old_parts, "the unchanged species was rebuilt");
            } else {
                assert_ne!(parts, old_parts, "the changed species kept its old parts");
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
            .spawn(TreeGroveParams::new(sparse(40.0), wide))
            .id();
        update_until_finished(&mut app, entity);
        let first: HashSet<u64> = trees(&mut app, entity).into_keys().collect();

        // Drag the density down a little every frame, as a slider would.
        let mut changed = false;
        for step in 1..=40 {
            set_grove(&mut app, entity, sparse(40.0 - step as f32 * 0.5));
            app.update();
            let now: HashSet<u64> = trees(&mut app, entity).into_keys().collect();
            changed |= now != first;
        }
        assert!(
            changed,
            "the trees never changed while the grove was being edited"
        );
    }

    #[test]
    fn a_new_host_object_moves_the_trees_and_a_missing_surface_is_reported() {
        let mut app = grove_app();
        let level = |height: f32| {
            let mut ground = SurfaceMesh::ground(16.0, 4);
            ground.positions.iter_mut().for_each(|p| p[1] = height);
            GeneratedObject(Arc::new(Object::single(vec![Part::new(
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
                TreeGroveParams::new(small_grove(), SIZE)
                    .on_host(TagFilter::any_of([tags::GROUND]), 0.5),
                ChildOf(host),
            ))
            .id();
        update_until_finished(&mut app, entity);
        let height = |app: &mut App| {
            let (child, _, _) = *trees(app, entity).values().next().unwrap();
            app.world().get::<Transform>(child).unwrap().translation.y
        };
        assert!(height(&mut app) < 0.0);

        // A new object on the host places the trees again, on its surface.
        app.world_mut().entity_mut(host).insert(level(3.0));
        update_until_finished(&mut app, entity);
        assert!((height(&mut app) - (3.0 - small_grove().variation.sink)).abs() < 1e-3);

        // A filter that matches nothing has no ground.
        app.world_mut()
            .get_mut::<TreeGroveParams>(entity)
            .unwrap()
            .ground = GroveGround::Host {
            filter: TagFilter::any_of([tags::BARK]),
            cell: 0.5,
        };
        update_until_finished(&mut app, entity);
        assert_eq!(
            app.world().resource::<Finished>().0.last(),
            Some(&Err(TreeGroveBuildError::NoGround))
        );
        assert!(trees(&mut app, entity).is_empty());
    }

    #[test]
    fn setting_the_same_grove_again_does_nothing() {
        let mut app = grove_app();
        let entity = app
            .world_mut()
            .spawn(TreeGroveParams::new(small_grove(), SIZE))
            .id();
        update_until_finished(&mut app, entity);
        let before = trees(&mut app, entity);

        // An equal grove in a new copy, and a plain change notification.
        set_grove(&mut app, entity, small_grove());
        app.update();
        app.world_mut()
            .get_mut::<TreeGroveParams>(entity)
            .unwrap()
            .set_changed();
        app.update();
        assert!(!app.world().entity(entity).contains::<TreeGroveGenerating>());
        assert_eq!(trees(&mut app, entity), before);
        assert_eq!(app.world().resource::<Finished>().0.len(), 1);
    }

    #[test]
    fn host_ground_waits_for_its_object_and_follows_its_scale() {
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
                TreeGroveParams::new(small_grove(), SIZE)
                    .on_host(TagFilter::any_of([tags::GROUND]), 0.25),
                ChildOf(host),
            ))
            .id();
        app.update();
        assert!(app.world().entity(entity).contains::<TreeGroveGenerating>());
        assert!(trees(&mut app, entity).is_empty());

        // A level square 1 unit up and 8 units across: 16 m wide and 2 m up
        // in the world, enough for the whole 12 m grove.
        let mut ground = SurfaceMesh::ground(8.0, 8);
        for p in &mut ground.positions {
            p[1] += 1.0;
        }
        let object = Object::single(vec![Part::new([tags::GROUND], ground)]);
        app.world_mut()
            .entity_mut(host)
            .insert(GeneratedObject(Arc::new(object)));
        update_until_finished(&mut app, entity);
        let placed = trees(&mut app, entity);
        assert!(!placed.is_empty());

        // Trees stand on the surface, sunk 0.1 m (0.05 host units) into it,
        // at their own world size: in the half-scale host, at most half the
        // largest size variation allows.
        let variation = small_grove().variation;
        let largest = 1.0 + 3.0 * variation.scale_jitter;
        for (child, _, _) in placed.values() {
            let transform = app.world().get::<Transform>(*child).unwrap();
            let sunk = 1.0 - variation.sink / 2.0;
            assert!((transform.translation.y - sunk).abs() < 1e-3);
            assert!(transform.scale.x <= 0.5 * largest + 1e-4);
        }

        // Losing the object removes the trees.
        app.world_mut().entity_mut(host).remove::<GeneratedObject>();
        app.update();
        app.update();
        assert!(trees(&mut app, entity).is_empty());
        assert!(app.world().entity(entity).contains::<TreeGroveGenerating>());
    }

    #[test]
    fn a_bad_ground_cell_is_reported() {
        let mut app = grove_app();
        let ground = SurfaceMesh::ground(8.0, 2);
        let object = Object::single(vec![Part::new([tags::GROUND], ground)]);
        let host = app
            .world_mut()
            .spawn((GeneratedObject(Arc::new(object)), Transform::default()))
            .id();
        let entity = app
            .world_mut()
            .spawn((
                TreeGroveParams::new(small_grove(), SIZE)
                    .on_host(TagFilter::any_of([tags::GROUND]), 0.0),
                ChildOf(host),
            ))
            .id();
        app.update();
        assert!(!app.world().entity(entity).contains::<TreeGroveGenerating>());
        assert_eq!(
            app.world().resource::<Finished>().0,
            vec![Err(TreeGroveBuildError::BadCell)]
        );
    }

    #[test]
    fn removing_the_component_removes_the_trees() {
        let mut app = grove_app();
        let entity = app
            .world_mut()
            .spawn(TreeGroveParams::new(small_grove(), SIZE))
            .id();
        update_until_finished(&mut app, entity);
        assert!(!trees(&mut app, entity).is_empty());

        app.world_mut()
            .entity_mut(entity)
            .remove::<TreeGroveParams>();
        app.update();
        assert!(trees(&mut app, entity).is_empty());
    }

    #[test]
    fn a_grove_asset_places_trees_and_follows_its_changes() {
        let mut app = grove_app();
        let handle = app
            .world_mut()
            .resource_mut::<Assets<TreeGroveAsset>>()
            .add(TreeGroveAsset(small_grove()));
        let entity = app
            .world_mut()
            .spawn(TreeGroveParams {
                grove: GroveSource::Asset(handle.clone()),
                size: SIZE,
                ground: GroveGround::Flat,
            })
            .id();
        update_until_finished(&mut app, entity);
        let full = trees(&mut app, entity).len();
        assert!(full > 0);

        // Editing the loaded grove updates the trees.
        let sparse = small_grove().with_density(small_grove().density() * 0.5);
        app.world_mut()
            .resource_mut::<Assets<TreeGroveAsset>>()
            .get_mut(&handle)
            .unwrap()
            .0 = sparse;
        update_until_finished(&mut app, entity);
        assert!(trees(&mut app, entity).len() < full);
    }
}
