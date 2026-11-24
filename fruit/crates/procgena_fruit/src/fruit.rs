//! One generated fruit or flower cluster: a specimen of a fruit pool.

use procgena_core::{LodLevel, SpecimenId};

use crate::mesh::mesh_cluster;
use crate::{ClusterDetail, FruitLod, FruitMesh, FruitOptions, FruitPreset, FruitSkeleton, OptionsError};

/// One fruit or flower cluster, grown from its settings and its
/// [`SpecimenId`], ready to mesh at any level of detail.
///
/// ```
/// use procgena_fruit::{ClusterDetail, Fruit, FruitPreset};
///
/// let apple = Fruit::from_preset(FruitPreset::apple())?;
/// let mesh = apple.mesh(&ClusterDetail::default());
/// assert!(mesh.triangle_count() > 0);
/// # Ok::<(), procgena_fruit::OptionsError>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Fruit {
    options: FruitOptions,
    id: SpecimenId,
    skeleton: FruitSkeleton,
}

impl Fruit {
    /// Grows specimen `slot` of `options`' pool in a World seeded with
    /// `world_seed`. The same inputs always grow the same cluster.
    ///
    /// # Errors
    ///
    /// Returns the first setting out of range; see [`FruitOptions::validate`].
    pub fn new(options: FruitOptions, world_seed: u32, slot: u32) -> Result<Self, OptionsError> {
        options.validate()?;
        let id = options.specimen_id(world_seed, slot);
        let skeleton = FruitSkeleton::generate(&options, id.seed());
        Ok(Self {
            options,
            id,
            skeleton,
        })
    }

    /// Grows the first specimen of a preset on its own, using its
    /// [`default_seed`](FruitOptions::default_seed) as the World seed.
    ///
    /// # Errors
    ///
    /// Returns the first setting out of range, which a built-in preset
    /// never has.
    pub fn from_preset(preset: &FruitPreset) -> Result<Self, OptionsError> {
        let options = preset.fruit.clone();
        let seed = options.default_seed;
        Self::new(options, seed, 0)
    }

    /// The settings the cluster was grown from.
    pub fn options(&self) -> &FruitOptions {
        &self.options
    }

    /// The specimen's id.
    pub fn id(&self) -> SpecimenId {
        self.id
    }

    /// The cluster's stalks and items.
    pub fn skeleton(&self) -> &FruitSkeleton {
        &self.skeleton
    }

    /// The radius of the ball around the cluster's attachment that holds
    /// all of it, in metres; see [`FruitSkeleton::radius`].
    pub fn radius(&self) -> f32 {
        self.skeleton.radius()
    }

    /// The cluster's size for choosing its level of detail: the diameter of
    /// the ball [`radius`](Self::radius) describes. Pass it, times the
    /// copy's scale, to [`Lod::factor`](procgena_core::Lod::factor).
    pub fn size(&self) -> f32 {
        2.0 * self.radius()
    }

    /// The cluster's mesh at `detail`, in the cluster frame.
    pub fn mesh(&self, detail: &ClusterDetail) -> FruitMesh {
        mesh_cluster(&self.skeleton, &self.options, detail)
    }

    /// One mesh per level of `lod`, in level order, with its level.
    pub fn lod_meshes(&self, lod: &FruitLod) -> Vec<(LodLevel<ClusterDetail>, FruitMesh)> {
        lod.levels
            .iter()
            .map(|level| (*level, self.mesh(&level.detail)))
            .collect()
    }
}
