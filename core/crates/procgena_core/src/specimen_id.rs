//! Ids for pools of specimens and for one specimen in a pool.

use crate::content_hash::{ContentHash, ContentHasher, OptionsHash};
use crate::hash::{derive_seed, salt, seed_for};
use crate::seed::SPECIMEN_SEED;

/// Which generator builds a specimen: tree, stone, mushroom and so on.
///
/// It is the salt of a frozen code seed key, such as `"tree.generator.kind"`.
/// It keeps pools of different generators apart even if their options
/// hashes happen to be equal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GeneratorKind(u32);

impl GeneratorKind {
    /// The kind named by seed key `key` (`<crate>.generator.kind`). Each
    /// generator has exactly one key, and must never change it.
    pub const fn from_key(key: &str) -> Self {
        Self(salt(key))
    }

    /// The key's salt.
    pub const fn salt(self) -> u32 {
        self.0
    }
}

/// Names a pool: every specimen that one generator builds from one set of
/// options.
///
/// A pool holds a budget of distinct specimens, its slots, and every copy
/// with these options draws one of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PoolId {
    /// The generator that builds the pool's specimens.
    pub kind: GeneratorKind,
    /// The hash of the options the specimens are built from.
    pub options: OptionsHash,
}

impl PoolId {
    /// The pool of generator `kind` with options hashed to `options`.
    pub const fn new(kind: GeneratorKind, options: OptionsHash) -> Self {
        Self { kind, options }
    }
}

/// Names one specimen: one generated variant of an asset, which many placed
/// copies can share.
///
/// Equal ids always mean the same specimen, so a cache of built specimens
/// can be keyed by it (with the detail level added for meshes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SpecimenId {
    /// The World seed, or the asset's `default_seed` when it is built on its
    /// own, outside any World. Each World grows its own specimens.
    pub world_seed: u32,
    /// The pool the specimen belongs to.
    pub pool: PoolId,
    /// The specimen's slot in its pool, from 0 up to the pool's budget.
    pub slot: u32,
}

impl SpecimenId {
    /// The id of the specimen in `slot` of `pool`, in a World seeded with
    /// `world_seed`.
    pub const fn new(world_seed: u32, pool: PoolId, slot: u32) -> Self {
        Self {
            world_seed,
            pool,
            slot,
        }
    }

    /// The seed the generator builds this specimen from.
    ///
    /// Every input of the id changes it: the World seed, the generator kind,
    /// the options hash and the slot.
    pub fn seed(&self) -> u32 {
        let Self {
            world_seed,
            pool: PoolId { kind, options },
            slot,
        } = *self;
        let mut seed = seed_for(world_seed, SPECIMEN_SEED);
        seed = derive_seed(seed, kind.salt());
        seed = derive_seed(seed, options.0 as u32);
        seed = derive_seed(seed, (options.0 >> 32) as u32);
        derive_seed(seed, slot)
    }
}

/// Writes the kind's salt.
impl ContentHash for GeneratorKind {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_u32(self.0);
    }
}

/// Writes the kind, then the options hash.
impl ContentHash for PoolId {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self { kind, options } = self;
        hasher.write(kind);
        hasher.write(options);
    }
}

/// Writes the World seed, the pool, then the slot.
impl ContentHash for SpecimenId {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            world_seed,
            pool,
            slot,
        } = self;
        hasher.write_u32(*world_seed);
        hasher.write(pool);
        hasher.write_u32(*slot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TREE: GeneratorKind = GeneratorKind::from_key("tree.generator.kind");
    const STONE: GeneratorKind = GeneratorKind::from_key("stone.generator.kind");

    fn id(world_seed: u32, kind: GeneratorKind, options: u64, slot: u32) -> SpecimenId {
        SpecimenId::new(world_seed, PoolId::new(kind, OptionsHash(options)), slot)
    }

    /// Reference values computed outside this crate, in Python, from the
    /// published FNV-1a and PCG3D definitions. A failure means every
    /// specimen in every World would change shape.
    #[test]
    fn seeds_match_reference_values() {
        assert_eq!(id(0, TREE, 0, 0).seed(), 0xaae9_5777);
        assert_eq!(id(42, TREE, 0x0123_4567_89ab_cdef, 3).seed(), 0xa1a1_b69f);
    }

    #[test]
    fn every_input_changes_the_seed() {
        let base = id(42, TREE, 0x0123_4567_89ab_cdef, 3);
        let others = [
            id(43, TREE, 0x0123_4567_89ab_cdef, 3),
            id(42, STONE, 0x0123_4567_89ab_cdef, 3),
            id(42, TREE, 0x0123_4567_89ab_cdee, 3),
            id(42, TREE, 0x0123_4566_89ab_cdef, 3),
            id(42, TREE, 0x0123_4567_89ab_cdef, 4),
        ];
        let mut seeds = vec![base.seed()];
        for other in others {
            assert!(!seeds.contains(&other.seed()), "{other:?}");
            seeds.push(other.seed());
        }
    }

    #[test]
    fn slots_of_one_pool_get_distinct_seeds() {
        let mut seeds: Vec<u32> = (0..256).map(|slot| id(42, TREE, 9, slot).seed()).collect();
        seeds.sort_unstable();
        seeds.dedup();
        assert_eq!(seeds.len(), 256);
    }

    #[test]
    fn ids_compare_by_their_fields() {
        assert_eq!(id(1, TREE, 2, 3), id(1, TREE, 2, 3));
        assert_ne!(id(1, TREE, 2, 3), id(1, STONE, 2, 3));
        assert!(id(1, TREE, 2, 3) < id(2, TREE, 2, 3));
        assert_eq!(TREE.salt(), salt("tree.generator.kind"));
    }
}
