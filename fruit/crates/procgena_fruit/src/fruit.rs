//! One generated fruit or flower cluster: a specimen of a fruit pool.

use procgena_core::chain::Lod;
use procgena_core::{PartStep, SpecimenId};

use crate::mesh::mesh_cluster;
use crate::{
    ClusterDetail, ClusterItem, ClusterStalk, FlowerPreset, FruitMesh, FruitOptions, FruitPreset,
    FruitSkeleton, OptionsError,
};

/// One fruit or flower cluster, grown from its settings and its
/// [`SpecimenId`], ready to mesh at any detail.
///
/// ```
/// use procgena_fruit::{ClusterDetail, Fruit, FruitPreset};
///
/// let apple = Fruit::from_preset(FruitPreset::apple())?;
/// let meshes = apple.meshes();
/// assert!(meshes.lower.triangle_count() < meshes.game.triangle_count());
/// let full = apple.mesh(&ClusterDetail::FULL);
/// assert!(full.triangle_count() > meshes.game.triangle_count());
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

    /// Grows the flower preset `preset` as [`from_preset`](Self::from_preset)
    /// grows a fruit preset.
    ///
    /// # Errors
    ///
    /// Returns the first setting out of range, which a built-in preset
    /// never has.
    pub fn from_flower_preset(preset: &FlowerPreset) -> Result<Self, OptionsError> {
        let options = preset.flower.clone();
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

    /// The cluster's size on screen is measured by: the diameter of the
    /// ball [`radius`](Self::radius) describes. Pass it, times the copy's
    /// scale, to [`DistanceChain::factor`](procgena_core::chain::DistanceChain::factor).
    pub fn size(&self) -> f32 {
        2.0 * self.radius()
    }

    /// The cluster's mesh at `detail`, in the cluster frame.
    pub fn mesh(&self, detail: &ClusterDetail) -> FruitMesh {
        mesh_cluster(&self.skeleton, &self.options, detail)
    }

    /// The details of the cluster's two meshes: its
    /// [`game_level`](FruitOptions::game_level), and the coarser detail
    /// derived from it.
    pub fn lod(&self) -> Lod<ClusterDetail> {
        Lod::from_game(self.options.game_level)
    }

    /// The cluster's game-level mesh and its lower mesh, in the cluster
    /// frame.
    pub fn meshes(&self) -> Lod<FruitMesh> {
        self.lod().map(|detail| self.mesh(&detail))
    }
}

/// The part kind key of a cluster's stalks: step `stalk k` names
/// [`FruitSkeleton::stalks`]`[k]`.
pub const STALK_PART: &str = "fruit.part.stalk";
/// The part kind key of a cluster's items: step `item k` names
/// [`FruitSkeleton::items`]`[k]`.
pub const ITEM_PART: &str = "fruit.part.item";

/// A part of a cluster that a [`PartStep`] names.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FruitPart<'a> {
    /// A supporting stalk or main stem.
    Stalk(&'a ClusterStalk),
    /// One fruit or flower.
    Item(&'a ClusterItem),
}

impl Fruit {
    /// The part that `step` names in this cluster, or `None` when its kind
    /// is not [`STALK_PART`] or [`ITEM_PART`], its index is out of range, or
    /// it is pinned to another specimen. The same step names the same part
    /// at every level of detail.
    pub fn part(&self, step: PartStep) -> Option<FruitPart<'_>> {
        if step.pin != self.id {
            return None;
        }
        let index = step.index as usize;
        if step.kind == PartStep::kind_of(STALK_PART) {
            self.skeleton.stalks.get(index).map(FruitPart::Stalk)
        } else if step.kind == PartStep::kind_of(ITEM_PART) {
            self.skeleton.items.get(index).map(FruitPart::Item)
        } else {
            None
        }
    }

    /// The step naming item `index` of this cluster.
    pub fn item_step(&self, index: u32) -> PartStep {
        PartStep::new(ITEM_PART, index, self.id)
    }
}
