//! Content hashes and ids for fruit settings.
//!
//! A cluster's settings, without their seed, have an [`OptionsHash`].
//! Together with [`FRUIT_KIND`] they name a [`PoolId`]: every cluster grown
//! from the same settings. [`FruitOptions::specimen_id`] then names one
//! specimen of that pool.

use procgena_core::{ContentHash, ContentHasher, GeneratorKind, OptionsHash, PoolId, SpecimenId};

use crate::{
    ClusterKind, ClusterShape, Facing, FlowerShape, FruitOptions, FruitShape, ItemShape, Variation,
};

/// The generator kind of every fruit and flower cluster: the first half of
/// every fruit [`PoolId`].
///
/// Its key, `"fruit.generator.kind"`, is frozen: changing it would give
/// every cluster a new pool and a new shape inside a World.
pub const FRUIT_KIND: GeneratorKind = GeneratorKind::from_key("fruit.generator.kind");

impl FruitOptions {
    /// The content hash of these settings, leaving out
    /// [`default_seed`](Self::default_seed). It is the same on every run and
    /// platform.
    pub fn options_hash(&self) -> OptionsHash {
        OptionsHash::of(self)
    }

    /// The pool of these settings: every cluster grown from them, whatever
    /// its seed.
    pub fn pool_id(&self) -> PoolId {
        PoolId::new(FRUIT_KIND, self.options_hash())
    }

    /// Specimen `slot` of this pool in a World with seed `world_seed`.
    ///
    /// A cluster built on its own, or grown on a host outside a World, uses
    /// [`default_seed`](Self::default_seed) as the World seed.
    pub fn specimen_id(&self, world_seed: u32, slot: u32) -> SpecimenId {
        SpecimenId::new(world_seed, self.pool_id(), slot)
    }
}

// Each enum variant writes a fixed number, so reordering the variants in
// their declarations leaves every hash unchanged. Each impl lists every
// field without `..`, so a new setting fails to compile until it is hashed.

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

/// Every setting except the seed.
impl ContentHash for FruitOptions {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            default_seed: _,
            facing,
            cluster,
            size,
            size_variance,
            item,
            maturity,
            variation,
        } = self;
        hasher.write_u32(facing_number(*facing));
        hasher.write(cluster);
        hasher.write_f32(*size);
        hasher.write_f32(*size_variance);
        hasher.write(item);
        hasher.write_f32(*maturity);
        hasher.write(variation);
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

/// The item kind's number, then its shape.
impl ContentHash for ItemShape {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        match self {
            Self::Fruit(shape) => {
                hasher.write_u32(0);
                hasher.write(shape);
            }
            Self::Flower(shape) => {
                hasher.write_u32(1);
                hasher.write(shape);
            }
        }
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

impl ContentHash for Variation {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            maturity_spread,
            hue_spread,
            blush_spread,
            shape_jitter,
            stem_curve_jitter,
        } = self;
        hasher.write_f32(*maturity_spread);
        hasher.write_f32(*hue_spread);
        hasher.write_f32(*blush_spread);
        hasher.write_f32(*shape_jitter);
        hasher.write_f32(*stem_curve_jitter);
    }
}
