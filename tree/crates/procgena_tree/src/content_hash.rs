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
    BarkOptions, BarkUvMode, Billboard, BranchOptions, ClusterKind, ClusterPlacement, ClusterShape,
    Facing, FlowerOptions, FlowerShape, FruitOptions, FruitShape, GrowthForce, LeafOptions,
    PerLevel, Tree, TreeOptions, TreeType, TrellisForce, TrellisOptions,
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
    /// The seed the tree is grown from, [`TreeOptions::seed`].
    pub seed: i32,
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
        let Self { pool, seed } = self;
        hasher.write(pool);
        hasher.write_i32(*seed);
    }
}

impl TreeOptions {
    /// The content hash of these settings, leaving out [`seed`](Self::seed).
    ///
    /// Every other setting counts, including colours and texture names.
    /// `0.0` and `-0.0` hash the same, as do all NaNs. Different settings
    /// can share a hash only by rare chance.
    ///
    /// ```
    /// use procgena_tree::TreePreset;
    ///
    /// let a = TreePreset::OakSmall.options();
    /// let mut b = a.clone();
    /// b.seed += 1;
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
    /// built on its own uses its [`seed`](Self::seed) as the World seed.
    pub fn specimen_id(&self, world_seed: u32, slot: u32) -> SpecimenId {
        SpecimenId::new(world_seed, self.pool_id(), slot)
    }

    /// The key of the shape these settings grow: their pool and their
    /// [`seed`](Self::seed).
    ///
    /// ```
    /// use procgena_tree::TreePreset;
    ///
    /// let a = TreePreset::OakSmall.options();
    /// let mut b = a.clone();
    /// assert_eq!(a.cache_key(), b.cache_key());
    /// b.seed += 1;
    /// assert_ne!(a.cache_key(), b.cache_key());
    /// ```
    pub fn cache_key(&self) -> TreeCacheKey {
        TreeCacheKey {
            pool: self.pool_id(),
            seed: self.seed,
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

fn facing_number(value: Facing) -> u32 {
    match value {
        Facing::Hang => 0,
        Facing::Up => 1,
        Facing::Outward => 2,
    }
}

fn cluster_kind_number(value: ClusterKind) -> u32 {
    match value {
        ClusterKind::Single => 0,
        ClusterKind::Umbel => 1,
        ClusterKind::Raceme => 2,
        ClusterKind::Panicle => 3,
        ClusterKind::Catkin => 4,
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

/// Every setting except the seed.
impl ContentHash for TreeOptions {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            seed: _,
            tree_type,
            bark,
            branch,
            leaves,
            trellis,
            fruit,
            flowers,
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

impl ContentHash for FruitOptions {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            placement,
            cluster,
            shape,
            maturity,
        } = self;
        hasher.write(placement);
        hasher.write(cluster);
        hasher.write(shape);
        hasher.write_f32(*maturity);
    }
}

impl ContentHash for FlowerOptions {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            placement,
            cluster,
            shape,
            maturity,
        } = self;
        hasher.write(placement);
        hasher.write(cluster);
        hasher.write(shape);
        hasher.write_f32(*maturity);
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
            facing,
            facing_jitter,
            size,
            size_variance,
        } = self;
        hasher.write_u32(*min_level);
        hasher.write_f32(*tips);
        hasher.write_u32(*count);
        hasher.write_f32(*density);
        hasher.write_f32(*start);
        hasher.write_u32(facing_number(*facing));
        hasher.write_f32(*facing_jitter);
        hasher.write_f32(*size);
        hasher.write_f32(*size_variance);
    }
}

impl ContentHash for ClusterShape {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            kind,
            items,
            items_variance,
            spread,
            axis_length,
            droop,
            taper,
            side_stalks,
            side_length,
            stalk_length,
            stalk_radius,
            stalk_color,
        } = self;
        hasher.write_u32(cluster_kind_number(*kind));
        hasher.write_u32(*items);
        hasher.write_f32(*items_variance);
        hasher.write_f32(*spread);
        hasher.write_f32(*axis_length);
        hasher.write_f32(*droop);
        hasher.write_f32(*taper);
        hasher.write_u32(*side_stalks);
        hasher.write_f32(*side_length);
        hasher.write_f32(*stalk_length);
        hasher.write_f32(*stalk_radius);
        hasher.write_u32(*stalk_color);
    }
}

impl ContentHash for FruitShape {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            elongation,
            top_dimple,
            bottom_taper,
            lobes,
            lobe_depth,
            segments,
            rings,
            stem_length,
            stem_radius,
            stem_curve,
            calyx,
            color,
            unripe_color,
            blush_color,
            blush_amount,
        } = self;
        hasher.write_f32(*elongation);
        hasher.write_f32(*top_dimple);
        hasher.write_f32(*bottom_taper);
        hasher.write_u32(*lobes);
        hasher.write_f32(*lobe_depth);
        hasher.write_u32(*segments);
        hasher.write_u32(*rings);
        hasher.write_f32(*stem_length);
        hasher.write_f32(*stem_radius);
        hasher.write_f32(*stem_curve);
        hasher.write_bool(*calyx);
        hasher.write_u32(*color);
        hasher.write_u32(*unripe_color);
        hasher.write_u32(*blush_color);
        hasher.write_f32(*blush_amount);
    }
}

impl ContentHash for FlowerShape {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            petal_count,
            rings,
            petal_length,
            petal_width,
            cup,
            curl,
            notch,
            petal_segments,
            center_size,
            center_color,
            petal_color,
            base_color,
            bud_color,
            fade_color,
        } = self;
        hasher.write_u32(*petal_count);
        hasher.write_u32(*rings);
        hasher.write_f32(*petal_length);
        hasher.write_f32(*petal_width);
        hasher.write_f32(*cup);
        hasher.write_f32(*curl);
        hasher.write_f32(*notch);
        hasher.write_u32(*petal_segments);
        hasher.write_f32(*center_size);
        hasher.write_u32(*center_color);
        hasher.write_u32(*petal_color);
        hasher.write_u32(*base_color);
        hasher.write_u32(*bud_color);
        hasher.write_u32(*fade_color);
    }
}
