//! Draw many rocks that share a small set of shapes generated on the GPU.
//!
//! Requires the `gpu` feature.

mod render;

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use bevy::camera::visibility::VisibilitySystems;
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::render::sync_world::SyncToRenderWorld;
use bevy::transform::TransformSystems;
use procgena_stone::gpu::MAX_GPU_RESOLUTION;
use procgena_stone::{OptionsError, StoneOptions};

use crate::material::StoneTextures;
use crate::spawn::SpawnSettings;

/// Most detail levels a pool can have; sizes the per-view level tables.
pub(crate) const MAX_POOL_LEVELS: usize = procgena_stone::MAX_DETAIL_LEVELS as usize;

pub(crate) use render::{PoolRenderPlugin, PoolShaders};

/// A set of rock shapes generated on the GPU and shared by [`PooledStone`] entities.
///
/// Add a pool to `Assets<StonePool>`, then spawn [`PooledStone`] entities that
/// name the pool and one of its variants. The plugin generates every variant at
/// every detail level on the GPU once, and draws all the pool's rocks with a few
/// GPU-driven draw calls per camera and shadow map. The meshes never reach the
/// CPU, so they cannot be read back for physics. For a collider, build the same
/// shape on the CPU from [`StonePool::variant_options`].
///
/// Changing the pool in `Assets<StonePool>` regenerates its shapes.
///
/// # Memory
///
/// Every variant reserves GPU memory for each detail level. With the default
/// levels (96, 48 and 24 cells per axis), each variant reserves about 2.6 MB of
/// vertex and index data, so 64 variants reserve about 170 MB.
#[derive(Asset, TypePath, Debug, Clone, PartialEq)]
pub struct StonePool {
    /// Settings shared by every variant. Variant `v` uses seed
    /// `options.seed + v`. [`MeshOptions::detail`](procgena_stone::MeshOptions::detail)
    /// sets the detail levels, as for a [`StoneParams`](crate::StoneParams); their
    /// resolutions must not exceed [`MAX_GPU_RESOLUTION`], and every level uses
    /// the nearest level's texture size. The material's colour, roughness,
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
}

impl StonePool {
    /// Creates a pool of `variants` shapes from `options`, with no stretch jitter.
    pub fn new(options: StoneOptions, variants: u32) -> Self {
        Self {
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
    /// [`MeshOptions::detail`](procgena_stone::MeshOptions::detail), and apply the
    /// [`PooledStone`] entity's transform. Each mesh approximates the drawn
    /// rock at the same resolution. The `Stone` also gives the rock's
    /// suggested [`embed_depth`](procgena_stone::Stone::embed_depth). Rarely, a rock
    /// with nearly equally stable resting sides settles onto a different side
    /// on the GPU than on the CPU.
    pub fn variant_options(&self, variant: u32) -> StoneOptions {
        let mut options = self.options.clone();
        options.seed = self.options.seed.wrapping_add(variant);
        for (axis, stretch) in options.stretch.iter_mut().enumerate() {
            *stretch *= 1.0 - self.stretch_jitter * unit_hash(options.seed, axis as u32);
        }
        options
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
        for level in self.options.mesh.detail.levels() {
            if level.resolution > MAX_GPU_RESOLUTION {
                return Err(StonePoolError::ResolutionTooHigh(level.resolution));
            }
        }
        Ok(())
    }
}

/// Returns a value in `[0, 1)` that depends only on `seed` and `axis`.
fn unit_hash(seed: u32, axis: u32) -> f32 {
    // Mix the inputs with the "lowbias32" integer hash.
    let mut x = seed ^ axis.wrapping_mul(0x9e37_79b9);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
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

/// Every pool's settings, copied to the render world when they change.
#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct PoolDefinitions {
    /// Increased whenever any pool is added, changed or removed.
    pub version: u64,
    /// Valid pools by asset.
    pub pools: HashMap<AssetId<StonePool>, PoolDefinition>,
}

/// One valid pool and the textures it draws with.
#[derive(Debug, Clone)]
pub(crate) struct PoolDefinition {
    /// The [`PoolDefinitions::version`] this pool last changed at.
    pub version: u64,
    /// The validated pool.
    pub pool: Arc<StonePool>,
    /// The pool's texture set at its nearest level's size, or `None` when the
    /// plugin has no texture folder.
    pub textures: Option<StoneTextures>,
}

/// The transforms of every drawable [`PooledStone`], copied to the render world
/// when they change.
#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct PoolInstances {
    /// Increased whenever any pooled rock is added, changed or removed.
    pub version: u64,
    /// Rocks by pool, sorted by variant.
    pub pools: Arc<HashMap<AssetId<StonePool>, Vec<PoolInstance>>>,
}

/// One pooled rock: its variant and world transform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PoolInstance {
    pub variant: u32,
    pub world_from_local: Affine3A,
}

/// Marks the one entity that stands for all pooled rocks in Bevy's draw lists.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct PoolDrawer;

/// Registers the pool asset and copies pools and pooled rocks to the render world.
pub(crate) struct StonePoolPlugin {
    pub shaders: PoolShaders,
}

impl Plugin for StonePoolPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<StonePool>()
            .init_resource::<PoolDefinitions>()
            .init_resource::<PoolInstances>()
            .add_systems(Startup, spawn_drawer)
            .add_systems(
                PostUpdate,
                (update_definitions, collect_instances)
                    .chain()
                    .after(TransformSystems::Propagate)
                    .after(VisibilitySystems::VisibilityPropagate),
            )
            .add_plugins(PoolRenderPlugin {
                shaders: self.shaders.clone(),
            });
    }
}

fn spawn_drawer(mut commands: Commands) {
    commands.spawn((PoolDrawer, SyncToRenderWorld::default()));
}

/// Records added, changed and removed pools, and requests their textures.
/// Invalid pools are left out with a warning.
fn update_definitions(
    mut events: MessageReader<AssetEvent<StonePool>>,
    assets: Res<Assets<StonePool>>,
    asset_server: Res<AssetServer>,
    settings: Res<SpawnSettings>,
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
                            pool.options.mesh.detail.levels()[0].texture_size,
                            settings.texture_compression,
                        )
                    });
                    definitions.pools.insert(
                        id,
                        PoolDefinition {
                            version,
                            pool: Arc::new(pool),
                            textures,
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

/// Rebuilds the list of pooled rocks when any of them is added, changed, hidden
/// or removed, or when a pool's variant count changes.
fn collect_instances(
    stones: Query<(&PooledStone, &GlobalTransform, &InheritedVisibility)>,
    changed: Query<(), ChangedStones>,
    mut removed: RemovedComponents<PooledStone>,
    definitions: Res<PoolDefinitions>,
    mut instances: ResMut<PoolInstances>,
    mut warned: Local<bool>,
) {
    let removed = removed.read().count() > 0;
    if changed.is_empty() && !removed && !definitions.is_changed() {
        return;
    }
    let mut pools: HashMap<AssetId<StonePool>, Vec<PoolInstance>> = HashMap::new();
    for (stone, transform, visibility) in &stones {
        if !visibility.get() {
            continue;
        }
        let id = stone.pool.id();
        if let Some(definition) = definitions.pools.get(&id)
            && stone.variant >= definition.pool.variants
        {
            if !*warned {
                *warned = true;
                warn!(
                    "procgena_stone_bevy: a PooledStone uses variant {}, but its pool has {} variants; it is not drawn",
                    stone.variant, definition.pool.variants
                );
            }
            continue;
        }
        pools.entry(id).or_default().push(PoolInstance {
            variant: stone.variant,
            world_from_local: transform.affine(),
        });
    }
    for list in pools.values_mut() {
        list.sort_by_key(|instance| instance.variant);
    }
    instances.version += 1;
    instances.pools = Arc::new(pools);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_differ_in_seed_and_stay_within_the_jitter() {
        let mut pool = StonePool::new(StoneOptions::default(), 8);
        pool.stretch_jitter = 0.3;
        let base = StoneOptions::default();
        for v in 0..8 {
            let options = pool.variant_options(v);
            assert_eq!(options.seed, base.seed + v);
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
            |p| p.options.mesh.detail = procgena_stone::StoneDetail::new(vec![]),
            StonePoolError::Options(OptionsError::IntOutOfRange {
                field: "mesh.detail",
                value: 0,
                min: 1,
                max: procgena_stone::MAX_DETAIL_LEVELS,
            }),
        );
        check(
            |p| {
                p.options.mesh.detail = procgena_stone::StoneDetail::single(
                    MAX_GPU_RESOLUTION + 1,
                    procgena_stone::TextureSize::Size512,
                )
            },
            StonePoolError::ResolutionTooHigh(MAX_GPU_RESOLUTION + 1),
        );
        // The most detailed preset still fits on the GPU.
        let mut ultra = valid.clone();
        ultra.options.mesh.detail = procgena_stone::StoneDetail::ULTRA;
        assert_eq!(ultra.validate(), Ok(()));
    }
}
