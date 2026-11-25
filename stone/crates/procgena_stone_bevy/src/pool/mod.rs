//! Draw many rocks that share a small set of shapes generated on the GPU.
//!
//! Requires the `gpu` feature.

mod impostor;
mod render;

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use bevy::camera::visibility::VisibilitySystems;
use bevy::math::Affine3A;
use bevy::pbr::StandardMaterialKey;
use bevy::prelude::*;
use bevy::render::RenderApp;
use bevy::render::combined_bind_group::CombinedBindGroupData;
use bevy::transform::TransformSystems;
use procgena_core::chain::DistanceChain;
use procgena_core::{Lod, OptionsHash, PoolId, SpecimenId};
use procgena_core_bevy::instanced::{
    ExternalMeshId, InstancedCopy, InstancedMaterialPlugin, InstancedPart, InstancedScatter,
    InstancedSlot, PartMesh, SpecimenCache, chain_lod,
};
use procgena_impostor::{ImpostorMaterial, ImpostorPipelineKey};
use procgena_stone::gpu::MAX_GPU_RESOLUTION;
use procgena_stone::{OptionsError, STONE_KIND, StoneDetail, StoneOptions};

use crate::material::{StoneMaterial, StoneMaterialKey, StoneTextures, stone_material};
use crate::spawn::SpawnSettings;

use render::PoolRenderPlugin;

/// A set of rock shapes generated on the GPU and shared by [`PooledStone`] entities.
///
/// Add a pool to `Assets<StonePool>`, then spawn [`PooledStone`] entities that
/// name the pool and one of its variants. The plugin generates every variant's
/// game-level and lower meshes on the GPU once, and draws all the pool's rocks with a few
/// GPU-driven draw calls per camera and shadow map. The meshes never reach the
/// CPU, so they cannot be read back for physics. For a collider, build the same
/// shape on the CPU from [`StonePool::variant_options`].
///
/// Changing the pool in `Assets<StonePool>` regenerates its shapes.
///
/// # Memory
///
/// Every variant reserves GPU memory for its two meshes, growing with the
/// square of the game level's resolution: doubling the resolution reserves
/// about four times as much vertex and index data.
#[derive(Asset, TypePath, Debug, Clone, PartialEq)]
pub struct StonePool {
    /// Settings shared by every variant; their own seed is not used. See
    /// [`variant_options`](Self::variant_options) for each variant's.
    /// [`MeshOptions::game_level`](procgena_stone::MeshOptions::game_level)
    /// sets the game level, as for a [`StoneParams`](crate::StoneParams); its
    /// resolution must not exceed [`MAX_GPU_RESOLUTION`], and both meshes use
    /// its texture size. The material's colour, roughness,
    /// metalness, texture set and texture repeat size apply to every variant.
    pub options: StoneOptions,
    /// Number of different shapes, at least 1. [`PooledStone::variant`] must be
    /// below this number.
    pub variants: u32,
    /// How much each variant's [`StoneOptions::stretch`] is randomly reduced on
    /// each axis, from 0 (all variants keep the same proportions) to below 1.
    /// For example, 0.3 shrinks each axis to between 70% and 100% of its
    /// stretch, chosen per variant and axis.
    pub stretch_jitter: f32,
    /// The seed of the World the pool's rocks grow in. Pools with equal
    /// settings and World seeds draw the same shapes, and so does a
    /// [`StoneScatter`](procgena_stone::StoneScatter) whose input has this
    /// seed. For a pool used on its own, outside any World, use the options'
    /// seed (as [`new`](Self::new) does).
    pub world_seed: u32,
}

impl StonePool {
    /// Creates a pool of `variants` shapes from `options`, with no stretch
    /// jitter, used on its own: its World seed is `options.default_seed`.
    pub fn new(options: StoneOptions, variants: u32) -> Self {
        Self {
            world_seed: options.default_seed,
            options,
            variants,
            stretch_jitter: 0.0,
        }
    }

    /// Returns settings for building pool variant `variant` on the CPU,
    /// for example to create a collider. Use an index below [`Self::variants`]
    /// to select a drawn variant; this method does not check the index.
    ///
    /// Create a [`procgena_stone::Stone`] from the result, optionally with a simpler
    /// [`MeshOptions::game_level`](procgena_stone::MeshOptions::game_level), and apply the
    /// [`PooledStone`] entity's transform. Each mesh approximates the drawn
    /// rock at the same resolution. The `Stone` also gives the rock's
    /// suggested [`embed_depth`](procgena_stone::Stone::embed_depth). Rarely, a rock
    /// with nearly equally stable resting sides settles onto a different side
    /// on the GPU than on the CPU.
    pub fn variant_options(&self, variant: u32) -> StoneOptions {
        self.options
            .variant(self.world_seed, variant, self.stretch_jitter)
    }

    /// Checks that the pool can be generated.
    pub fn validate(&self) -> Result<(), StonePoolError> {
        if self.variants == 0 {
            return Err(StonePoolError::NoVariants);
        }
        if !(0.0..1.0).contains(&self.stretch_jitter) {
            return Err(StonePoolError::StretchJitter(self.stretch_jitter));
        }
        self.options.validate().map_err(StonePoolError::Options)?;
        for level in self.mesh_levels() {
            if level.resolution > MAX_GPU_RESOLUTION {
                return Err(StonePoolError::ResolutionTooHigh(level.resolution));
            }
        }
        Ok(())
    }

    /// The details of each variant's two meshes: the game level, then the
    /// lower mesh derived from it.
    pub(crate) fn mesh_levels(&self) -> [StoneDetail; 2] {
        let lod = self.options.mesh.game_level.lod();
        [lod.game, lod.lower]
    }
}

/// Why a [`StonePool`] cannot be generated.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum StonePoolError {
    /// [`StonePool::variants`] is 0.
    NoVariants,
    /// [`StonePool::stretch_jitter`] is not in `[0, 1)`.
    StretchJitter(f32),
    /// A detail level's resolution is above [`MAX_GPU_RESOLUTION`].
    ResolutionTooHigh(u32),
    /// The shared settings, including the detail levels, are invalid.
    Options(OptionsError),
}

impl fmt::Display for StonePoolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoVariants => write!(f, "a stone pool needs at least one variant"),
            Self::StretchJitter(value) => {
                write!(
                    f,
                    "stretch_jitter is {value}, but must be at least 0 and below 1"
                )
            }
            Self::ResolutionTooHigh(resolution) => write!(
                f,
                "a detail level's resolution is {resolution}, but GPU generation allows at most {MAX_GPU_RESOLUTION}"
            ),
            Self::Options(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for StonePoolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Options(error) => Some(error),
            _ => None,
        }
    }
}

/// Draws one variant of a [`StonePool`] at this entity's transform.
///
/// The rock appears once the pool is loaded and generated. Like a
/// [`StoneParams`](crate::StoneParams), it sits approximately at local ground level
/// (`y = 0`), with settling controlled by the pool's placement settings.
/// Burial is not applied automatically. Hidden entities are not drawn.
/// Entities whose `variant` is not below the pool's
/// [`StonePool::variants`] are not drawn, and a warning is logged.
///
/// Rocks should not move often: any change to a pooled rock re-uploads all
/// pooled rocks. Motion vectors (for temporal anti-aliasing and motion blur)
/// assume the rocks are still. Transforms with a negative scale draw the rock
/// inside out.
#[derive(Component, Debug, Clone, PartialEq)]
#[require(Transform, Visibility)]
pub struct PooledStone {
    /// The pool whose shapes to draw.
    pub pool: Handle<StonePool>,
    /// Which shape, from 0 to [`StonePool::variants`] − 1.
    pub variant: u32,
}

/// Conservative radius around a pool mesh's origin enclosing any generated
/// rock, in the generated unit cube's units (before the pool's scale).
pub(crate) const UNPLACED_RADIUS: f32 = 2.0 * 1.732_050_8;

/// Every valid pool, copied to the render world when any changes.
#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct PoolDefinitions {
    /// Increased whenever any pool is added, changed or removed.
    pub version: u64,
    /// Valid pools by asset.
    pub pools: HashMap<AssetId<StonePool>, PoolDefinition>,
}

/// One valid pool and how the shared renderer draws it.
#[derive(Debug, Clone)]
pub(crate) struct PoolDefinition {
    /// The [`PoolDefinitions::version`] this pool last changed at.
    pub version: u64,
    /// The validated pool.
    pub pool: Arc<StonePool>,
    /// One slot per variant, whose parts are the pool's generated meshes
    /// (see [`external_mesh_id`]).
    pub slots: Arc<[InstancedSlot]>,
}

/// Each pool variant's size as the GPU measured it: its game-level mesh's
/// bounding diameter in metres, which sets where it switches to its lower
/// mesh and impostor. The render world posts sizes as it reads them back;
/// [`receive_pool_sizes`] takes them into the main world.
#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct MeasuredPoolSizes(Arc<Mutex<Vec<PostedSizes>>>);

/// One pool's sizes, as posted by the render world.
#[derive(Debug)]
struct PostedSizes {
    pool: AssetId<StonePool>,
    /// The [`PoolDefinition::version`] the sizes were measured for.
    version: u64,
    /// One size per variant, or `None` where it was not measured.
    sizes: Vec<Option<f32>>,
}

impl MeasuredPoolSizes {
    /// Posts the sizes measured for `pool` defined at `version`.
    pub(crate) fn post(&self, pool: AssetId<StonePool>, version: u64, sizes: Vec<Option<f32>>) {
        if let Ok(mut posted) = self.0.lock() {
            posted.push(PostedSizes {
                pool,
                version,
                sizes,
            });
        }
    }
}

/// The measured size of each variant of each pool, in the main world.
#[derive(Resource, Debug, Default)]
struct PoolSizes(HashMap<AssetId<StonePool>, (u64, Vec<Option<f32>>)>);

impl PoolSizes {
    /// The measured size of `variant` of the pool `id` defined at
    /// `version`, if the GPU has measured it.
    fn get(&self, id: AssetId<StonePool>, version: u64, variant: u32) -> Option<f32> {
        let (measured_version, sizes) = self.0.get(&id)?;
        if *measured_version != version {
            return None;
        }
        sizes.get(variant as usize).copied().flatten()
    }
}

/// Takes the sizes the render world measured into [`PoolSizes`], keeping
/// only those of pools still defined as they were measured.
fn receive_pool_sizes(
    posted: Res<MeasuredPoolSizes>,
    definitions: Res<PoolDefinitions>,
    mut sizes: ResMut<PoolSizes>,
) {
    let Ok(mut posted) = posted.0.lock() else {
        return;
    };
    if posted.is_empty() {
        return;
    }
    for measured in posted.drain(..) {
        let current = definitions
            .pools
            .get(&measured.pool)
            .is_some_and(|definition| definition.version == measured.version);
        if current {
            sizes
                .0
                .insert(measured.pool, (measured.version, measured.sizes));
        }
    }
    sizes.0.retain(|id, (version, _)| {
        definitions
            .pools
            .get(id)
            .is_some_and(|d| d.version == *version)
    });
}

/// The id under which the render world lists mesh `mesh` (variant-major:
/// `variant * levels + level`) of the pool defined at `version`.
pub(crate) fn external_mesh_id(version: u64, mesh: u32) -> ExternalMeshId {
    ExternalMeshId((version << 32) | u64::from(mesh))
}

/// The pipeline key of an [`ImpostorMaterial`].
type ImpostorMaterialKey = CombinedBindGroupData<StandardMaterialKey, ImpostorPipelineKey>;

/// The entity drawing each pool's rocks.
#[derive(Resource, Debug, Default)]
struct PoolDrawers(HashMap<AssetId<StonePool>, Entity>);

/// Marks an entity that draws one pool's rocks through the shared renderer.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct PoolDrawer;

/// Registers the pool asset, draws pooled rocks through the shared
/// renderer, and generates pools in the render world.
pub(crate) struct StonePoolPlugin;

impl Plugin for StonePoolPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            InstancedMaterialPlugin::<StoneMaterial, StoneMaterialKey>::new(|key| {
                key.extension.instanced = true
            }),
            // Pooled rocks draw their baked impostors as their last level.
            InstancedMaterialPlugin::<ImpostorMaterial, ImpostorMaterialKey>::new(|key| {
                key.extension.instanced = true
            }),
        ))
        .init_asset::<StonePool>()
        .init_resource::<PoolDefinitions>()
        .init_resource::<PoolDrawers>()
        .init_resource::<impostor::PoolImpostors>()
        .init_resource::<PoolSizes>()
        .add_systems(
            PostUpdate,
            (
                update_definitions,
                receive_pool_sizes,
                impostor::bake_pool_impostors,
                collect_instances,
            )
                .chain()
                .after(TransformSystems::Propagate)
                .after(VisibilitySystems::VisibilityPropagate),
        )
        .add_plugins(PoolRenderPlugin);

        // The render world posts measured sizes where the main world reads
        // them.
        let measured = MeasuredPoolSizes::default();
        app.insert_resource(measured.clone());
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app.insert_resource(measured);
        }
    }
}

/// Records added, changed and removed pools, with their materials and
/// slots. Invalid pools are left out with a warning.
fn update_definitions(
    mut events: MessageReader<AssetEvent<StonePool>>,
    assets: Res<Assets<StonePool>>,
    asset_server: Res<AssetServer>,
    settings: Res<SpawnSettings>,
    mut materials: ResMut<Assets<StoneMaterial>>,
    mut cache: ResMut<SpecimenCache>,
    mut definitions: ResMut<PoolDefinitions>,
) {
    for event in events.read() {
        let (AssetEvent::Added { id }
        | AssetEvent::Modified { id }
        | AssetEvent::Removed { id }
        | AssetEvent::Unused { id }
        | AssetEvent::LoadedWithDependencies { id }) = *event;
        // Ignore events that leave a pool as it was, such as the several Bevy
        // sends for one new asset.
        let pool = assets.get(id).cloned();
        let current = definitions.pools.get(&id).map(|d| d.pool.as_ref());
        if pool.is_some() && pool.as_ref() == current {
            continue;
        }
        let version = definitions.version + 1;
        definitions.version = version;
        match pool {
            Some(pool) => match pool.validate() {
                Ok(()) => {
                    let textures = settings.texture_root.as_deref().map(|root| {
                        StoneTextures::load(
                            &asset_server,
                            root,
                            &pool.options.material.texture,
                            pool.options.mesh.game_level.texture_size,
                            settings.texture_compression,
                        )
                    });
                    let material = materials.add(pool_material(&pool, textures.as_ref()));
                    let slots = pool_slots(&pool, version, &material, &mut cache);
                    definitions.pools.insert(
                        id,
                        PoolDefinition {
                            version,
                            pool: Arc::new(pool),
                            slots,
                        },
                    );
                }
                Err(error) => {
                    warn!("procgena_stone_bevy: a stone pool is not drawn: {error}");
                    definitions.pools.remove(&id);
                }
            },
            None => {
                definitions.pools.remove(&id);
            }
        }
    }
}

/// Pooled rocks whose pool, variant, transform or visibility changed.
type ChangedStones = (
    With<PooledStone>,
    Or<(
        Changed<PooledStone>,
        Changed<GlobalTransform>,
        Changed<InheritedVisibility>,
    )>,
);

/// The material a pool's rocks are drawn with. Pooled rocks are drawn in the
/// generated unit cube's units, so the texture repeat size is divided by
/// the pool's scale.
fn pool_material(pool: &StonePool, textures: Option<&StoneTextures>) -> StoneMaterial {
    let mut material = pool.options.material.clone();
    material.texture_size /= pool.options.scale;
    stone_material(&material, textures)
}

/// The shared instanced renderer's level table for pooled rocks: the shared
/// [`DistanceChain`]'s game-level mesh, lower mesh and, with `impostor`,
/// impostor (see [`chain_lod`]). Each copy's [`InstancedCopy::size`] is its
/// rock's bounding diameter.
pub(crate) fn renderer_lod(impostor: bool) -> Lod {
    chain_lod(&DistanceChain::DEFAULT, impostor)
}

/// One slot per variant of `pool`, defined at `version`, each drawing the
/// variant's generated meshes with `material`. Caches each variant's parts
/// by specimen and level.
fn pool_slots(
    pool: &StonePool,
    version: u64,
    material: &Handle<StoneMaterial>,
    cache: &mut SpecimenCache,
) -> Arc<[InstancedSlot]> {
    let options = &pool.options;
    let lod = renderer_lod(false);
    let levels = pool.mesh_levels().len() as u32;
    let pool_id = PoolId::new(STONE_KIND, OptionsHash::of(&(options, pool.stretch_jitter)));
    (0..pool.variants)
        .map(|variant| {
            let specimen = SpecimenId::new(pool.world_seed, pool_id, variant);
            let parts: Vec<InstancedPart> = (0..levels)
                .map(|level| InstancedPart {
                    level,
                    mesh: PartMesh::External(external_mesh_id(version, variant * levels + level)),
                    material: material.clone().untyped(),
                })
                .collect();
            for part in &parts {
                cache.insert(specimen, 0, part.level, vec![part.clone()]);
            }
            InstancedSlot {
                specimen,
                piece: 0,
                lod: lod.clone(),
                parts,
                bounds: (Vec3::ZERO, UNPLACED_RADIUS),
            }
        })
        .collect()
}

/// Rebuilds each pool's copies when any pooled rock is added, changed,
/// hidden or removed, or when a pool changes: one drawer entity per pool
/// with rocks, holding an [`InstancedScatter`] of its visible rocks. Also
/// rebuilds them when the GPU has measured a pool's variants.
#[expect(clippy::too_many_arguments, reason = "one Bevy system")]
fn collect_instances(
    mut commands: Commands,
    stones: Query<(&PooledStone, &GlobalTransform, &InheritedVisibility)>,
    changed: Query<(), ChangedStones>,
    mut removed: RemovedComponents<PooledStone>,
    definitions: Res<PoolDefinitions>,
    sizes: Res<PoolSizes>,
    mut drawers: ResMut<PoolDrawers>,
    mut warned: Local<bool>,
) {
    let removed = removed.read().count() > 0;
    if changed.is_empty() && !removed && !definitions.is_changed() && !sizes.is_changed() {
        return;
    }

    // Each defined pool's visible rocks, in the generated unit cube's units.
    let mut pools: HashMap<AssetId<StonePool>, Vec<InstancedCopy>> = HashMap::new();
    for (stone, transform, visibility) in &stones {
        let id = stone.pool.id();
        let Some(definition) = definitions.pools.get(&id) else {
            continue;
        };
        if !visibility.get() {
            continue;
        }
        if stone.variant >= definition.pool.variants {
            if !*warned {
                *warned = true;
                warn!(
                    "procgena_stone_bevy: a PooledStone uses variant {}, but its pool has {} variants; it is not drawn",
                    stone.variant, definition.pool.variants
                );
            }
            continue;
        }
        let options = &definition.pool.options;
        let scale = options.scale;
        // Until the GPU has measured the variant, the bound of any rock from
        // the pool's settings stands in for its size.
        let size = sizes
            .get(id, definition.version, stone.variant)
            .unwrap_or_else(|| options.bounding_diameter());
        pools.entry(id).or_default().push(InstancedCopy {
            slot: stone.variant,
            transform: transform.affine() * Affine3A::from_scale(Vec3::splat(scale)),
            size,
            wind_phase: 0.0,
        });
    }

    // One drawer per pool with rocks; drawers of other pools go.
    drawers.0.retain(|id, entity| {
        let keep = pools.contains_key(id);
        if !keep {
            commands.entity(*entity).despawn();
        }
        keep
    });
    for (id, copies) in pools {
        let slots = definitions.pools[&id].slots.clone();
        let entity = *drawers
            .0
            .entry(id)
            .or_insert_with(|| commands.spawn(PoolDrawer).id());
        commands.entity(entity).insert(InstancedScatter {
            slots,
            copies: copies.into(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_differ_in_seed_and_stay_within_the_jitter() {
        let mut pool = StonePool::new(StoneOptions::default(), 8);
        pool.stretch_jitter = 0.3;
        let base = StoneOptions::default();
        let mut seeds = std::collections::HashSet::new();
        for v in 0..8 {
            let options = pool.variant_options(v);
            assert!(
                seeds.insert(options.default_seed),
                "variant {v} repeats a seed"
            );
            assert_eq!(options, base.variant(base.default_seed, v, 0.3));
            for (stretch, original) in options.stretch.iter().zip(base.stretch) {
                assert!(*stretch <= original && *stretch > original * 0.7 - 1e-6);
            }
        }
        assert_ne!(
            pool.variant_options(0).stretch,
            pool.variant_options(1).stretch
        );
        assert_eq!(pool.variant_options(3), pool.variant_options(3));
    }

    #[test]
    fn validation_rejects_unusable_pools() {
        let valid = StonePool::new(StoneOptions::default(), 4);
        assert_eq!(valid.validate(), Ok(()));
        let check = |change: fn(&mut StonePool), expected: StonePoolError| {
            let mut pool = valid.clone();
            change(&mut pool);
            assert_eq!(pool.validate(), Err(expected));
        };
        check(|p| p.variants = 0, StonePoolError::NoVariants);
        check(
            |p| p.stretch_jitter = 1.0,
            StonePoolError::StretchJitter(1.0),
        );
        check(
            |p| p.options.scale = 0.0,
            StonePoolError::Options(OptionsError::FloatOutOfRange {
                field: "scale",
                value: 0.0,
                requirement: "positive",
            }),
        );
        check(
            |p| {
                p.options.mesh.game_level =
                    StoneDetail::new(MAX_GPU_RESOLUTION + 1, procgena_stone::TextureSize::Size512)
            },
            StonePoolError::ResolutionTooHigh(MAX_GPU_RESOLUTION + 1),
        );
        // The most detailed named detail still fits on the GPU.
        let mut ultra = valid.clone();
        ultra.options.mesh.game_level = StoneDetail::ULTRA;
        assert_eq!(ultra.validate(), Ok(()));
    }
}
