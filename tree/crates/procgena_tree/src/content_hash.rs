//! Content hashes and ids for tree settings.
//!
//! A tree's settings, without its seed, have an [`OptionsHash`]. Together
//! with [`TREE_KIND`] they name a [`PoolId`]: every tree grown from the same
//! settings. [`TreeOptions::specimen_id`] then names one specimen of that
//! pool, and [`TreeOptions::cache_key`] names the shape a set of settings
//! actually grows.
//!
//! Hashes are the same on every run and platform. They change when this
//! crate's settings or their encoding change.

use procgena_core::{ContentHash, ContentHasher, GeneratorKind, OptionsHash, PoolId, SpecimenId};

use crate::{
    AttachedFruit, BarkOptions, BarkUvMode, Billboard, BranchOptions, ClusterPlacement,
    GrowthForce, LeafOptions, LodDetail, PerLevel, Tree, TreeOptions, TreeType, TrellisForce,
    TrellisOptions,
};

/// The generator kind of every tree: the first half of every tree
/// [`PoolId`].
///
/// Its key, `"tree.generator.kind"`, is frozen: changing it would give
/// every tree a new pool and a new shape inside a World.
pub const TREE_KIND: GeneratorKind = GeneratorKind::from_key("tree.generator.kind");

/// Names the shape that one set of tree settings grows: its pool and the
/// seed it is grown from.
///
/// Two settings with equal keys grow the same tree, so the key can name
/// cached meshes and baked impostors. Get it from
/// [`TreeOptions::cache_key`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TreeCacheKey {
    /// The pool of the settings, without their seed.
    pub pool: PoolId,
    /// The seed the tree is grown from, [`TreeOptions::default_seed`].
    pub seed: i32,
    /// A hash of what picks the tree's fruit and flower specimens besides
    /// its settings: each cluster settings' own seed and the number of
    /// slots ([`AttachedFruit::default_slots`](crate::AttachedFruit::default_slots)).
    pub attached: u64,
    /// A hash of the detail the tree is built at: its
    /// [`game_level`](TreeOptions::game_level) and its fruit's and flowers'
    /// own game levels. Two keys that differ only here grow the same shape
    /// at different detail.
    pub detail: u64,
}

impl TreeCacheKey {
    /// The key as one 64-bit number, the same on every run and platform.
    /// Use it where a single number is needed, such as an impostor key.
    pub fn value(&self) -> u64 {
        procgena_core::content_hash(self)
    }
}

impl ContentHash for TreeCacheKey {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            pool,
            seed,
            attached,
            detail,
        } = self;
        hasher.write(pool);
        hasher.write_i32(*seed);
        hasher.write_u64(*attached);
        hasher.write_u64(*detail);
    }
}

impl TreeOptions {
    /// The content hash of these settings, leaving out
    /// [`default_seed`](Self::default_seed).
    ///
    /// Every other setting counts, including colours and texture names.
    /// `0.0` and `-0.0` hash the same, as do all NaNs. Different settings
    /// can share a hash only by rare chance.
    ///
    /// ```
    /// use procgena_tree::TreePreset;
    ///
    /// let a = TreePreset::oak_small().tree.clone();
    /// let mut b = a.clone();
    /// b.default_seed += 1;
    /// assert_eq!(a.options_hash(), b.options_hash());
    /// b.leaves.count += 1;
    /// assert_ne!(a.options_hash(), b.options_hash());
    /// ```
    pub fn options_hash(&self) -> OptionsHash {
        OptionsHash::of(self)
    }

    /// The pool of these settings: every tree grown from them, whatever its
    /// seed.
    pub fn pool_id(&self) -> PoolId {
        PoolId::new(TREE_KIND, self.options_hash())
    }

    /// Specimen `slot` of this pool in a World with seed `world_seed`.
    ///
    /// Its [`SpecimenId::seed`] is the seed that specimen grows from. A tree
    /// built on its own uses its [`default_seed`](Self::default_seed) as the
    /// World seed.
    pub fn specimen_id(&self, world_seed: u32, slot: u32) -> SpecimenId {
        SpecimenId::new(world_seed, self.pool_id(), slot)
    }

    /// The key of the shape these settings grow, and the detail it is built
    /// at: their pool, their [`default_seed`](Self::default_seed) and their
    /// [`game_level`](Self::game_level).
    ///
    /// ```
    /// use procgena_tree::TreePreset;
    ///
    /// let a = TreePreset::oak_small().tree.clone();
    /// let mut b = a.clone();
    /// assert_eq!(a.cache_key(), b.cache_key());
    /// b.default_seed += 1;
    /// assert_ne!(a.cache_key(), b.cache_key());
    /// ```
    pub fn cache_key(&self) -> TreeCacheKey {
        let mut attached = ContentHasher::new();
        let mut detail = ContentHasher::new();
        detail.write(&self.game_level);
        for part in [&self.fruit, &self.flowers] {
            attached.write_bool(part.is_some());
            detail.write_bool(part.is_some());
            if let Some(part) = part {
                attached.write_u32(part.fruit.default_seed);
                attached.write_u32(part.default_slots);
                detail.write(&part.fruit.game_level);
            }
        }
        TreeCacheKey {
            pool: self.pool_id(),
            seed: self.default_seed,
            attached: attached.finish(),
            detail: detail.finish(),
        }
    }
}

impl Tree {
    /// The key of this tree's shape; see [`TreeOptions::cache_key`].
    pub fn cache_key(&self) -> TreeCacheKey {
        self.options().cache_key()
    }
}

// Each enum variant writes a fixed number, so reordering the variants in
// their declarations leaves every hash unchanged.

fn tree_type_number(value: TreeType) -> u32 {
    match value {
        TreeType::Deciduous => 0,
        TreeType::Evergreen => 1,
    }
}

fn billboard_number(value: Billboard) -> u32 {
    match value {
        Billboard::Single => 0,
        Billboard::Double => 1,
    }
}

fn uv_mode_number(value: BarkUvMode) -> u32 {
    match value {
        BarkUvMode::Mirrored => 0,
        BarkUvMode::Continuous => 1,
    }
}

// Each impl lists every field without `..`, so a new setting fails to
// compile until it is hashed too.

/// Writes the four per-level values, as a fixed-length array.
impl<T: ContentHash> ContentHash for PerLevel<T> {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self(levels) = self;
        hasher.write(levels);
    }
}

/// Every setting except the seed and the game level, which say how a tree
/// is built rather than what shape it grows.
impl ContentHash for TreeOptions {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            default_seed: _,
            tree_type,
            bark,
            branch,
            leaves,
            trellis,
            fruit,
            flowers,
            game_level: _,
        } = self;
        hasher.write_u32(tree_type_number(*tree_type));
        hasher.write(bark);
        hasher.write(branch);
        hasher.write(leaves);
        hasher.write(trellis);
        hasher.write(fruit);
        hasher.write(flowers);
    }
}

impl ContentHash for BarkOptions {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            kind,
            tint,
            flat_shading,
            textured,
            texture_scale,
            uv_mode,
        } = self;
        hasher.write_str(kind);
        hasher.write_u32(*tint);
        hasher.write_bool(*flat_shading);
        hasher.write_bool(*textured);
        hasher.write(texture_scale);
        hasher.write_u32(uv_mode_number(*uv_mode));
    }
}

impl ContentHash for BranchOptions {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            levels,
            stems,
            stem_angle,
            angle,
            children,
            force,
            flatten,
            gnarliness,
            length,
            radius,
            sections,
            segments,
            start,
            taper,
            twist,
        } = self;
        hasher.write_u32(*levels);
        hasher.write_u32(*stems);
        hasher.write_f32(*stem_angle);
        hasher.write(angle);
        hasher.write(children);
        hasher.write(force);
        hasher.write(flatten);
        hasher.write(gnarliness);
        hasher.write(length);
        hasher.write(radius);
        hasher.write(sections);
        hasher.write(segments);
        hasher.write(start);
        hasher.write(taper);
        hasher.write(twist);
    }
}

impl ContentHash for GrowthForce {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            direction,
            strength,
        } = self;
        hasher.write(direction);
        hasher.write_f32(*strength);
    }
}

impl ContentHash for LeafOptions {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            kind,
            billboard,
            angle,
            count,
            start,
            size,
            size_variance,
            tint,
            alpha_test,
            rounded_normals,
            flatten,
        } = self;
        hasher.write_str(kind);
        hasher.write_u32(billboard_number(*billboard));
        hasher.write_f32(*angle);
        hasher.write_u32(*count);
        hasher.write_f32(*start);
        hasher.write_f32(*size);
        hasher.write_f32(*size_variance);
        hasher.write_u32(*tint);
        hasher.write_f32(*alpha_test);
        hasher.write_bool(*rounded_normals);
        hasher.write_f32(*flatten);
    }
}

impl ContentHash for TrellisOptions {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            enabled,
            position,
            width,
            height,
            spacing,
            force,
            cylinder_radius,
            visible,
            color,
        } = self;
        hasher.write_bool(*enabled);
        hasher.write(position);
        hasher.write_f32(*width);
        hasher.write_f32(*height);
        hasher.write_f32(*spacing);
        hasher.write(force);
        hasher.write_f32(*cylinder_radius);
        hasher.write_bool(*visible);
        hasher.write_u32(*color);
    }
}

impl ContentHash for TrellisForce {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            strength,
            max_distance,
            falloff,
        } = self;
        hasher.write_f32(*strength);
        hasher.write_f32(*max_distance);
        hasher.write_f32(*falloff);
    }
}

/// The cluster settings' own options hash, then the placement. Leaves out
/// [`AttachedFruit::default_slots`], which [`TreeCacheKey`] holds instead.
impl ContentHash for AttachedFruit {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            fruit,
            placement,
            default_slots: _,
        } = self;
        hasher.write(&fruit.options_hash());
        hasher.write(placement);
    }
}

impl ContentHash for ClusterPlacement {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            min_level,
            tips,
            count,
            density,
            start,
            facing_jitter,
            scale,
            scale_variance,
        } = self;
        hasher.write_u32(*min_level);
        hasher.write_f32(*tips);
        hasher.write_u32(*count);
        hasher.write_f32(*density);
        hasher.write_f32(*start);
        hasher.write_f32(*facing_jitter);
        hasher.write_f32(*scale);
        hasher.write_f32(*scale_variance);
    }
}

/// Every setting. Not part of a tree's name; [`TreeOptions::cache_key`]
/// hashes it separately.
impl ContentHash for LodDetail {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            section_stride,
            segment_factor,
            closed,
            min_branch_radius,
            leaf_stride,
            leaf_scale,
            billboard,
            cluster_stride,
            cluster_scale,
            lower_clusters,
        } = self;
        hasher.write_u32(*section_stride);
        hasher.write_f32(*segment_factor);
        hasher.write_bool(*closed);
        hasher.write_f32(*min_branch_radius);
        hasher.write_u32(*leaf_stride);
        hasher.write_f32(*leaf_scale);
        hasher.write_bool(billboard.is_some());
        if let Some(billboard) = billboard {
            hasher.write_u32(billboard_number(*billboard));
        }
        hasher.write_u32(*cluster_stride);
        hasher.write_f32(*cluster_scale);
        hasher.write_bool(*lower_clusters);
    }
}
