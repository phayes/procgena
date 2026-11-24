//! The seed keys this crate derives its random numbers from.
//!
//! Each key names one use of randomness, such as the shape of the main stem
//! or which leaf sprays a detail level keeps. A piece's seed and a key give
//! that use its own seed (see [`procgena_core::hash::seed_for`]), so adding
//! draws to one use never changes another. Keys are frozen: renaming one
//! changes every piece built with it.

/// The main stem's curve, radii and end kinds.
pub(crate) const STEM_SHAPE: &str = "deadwood.stem.shape";
/// The main stem's surface noise and splinters.
pub(crate) const STEM_SURFACE: &str = "deadwood.stem.surface";
/// The splinters and wear of a stem's base end.
pub(crate) const END_BASE: &str = "deadwood.end.base";
/// The splinters and wear of a stem's tip end.
pub(crate) const END_TIP: &str = "deadwood.end.tip";
/// Where branch stubs grow, and their sizes.
pub(crate) const STUB_SHAPE: &str = "deadwood.stub.shape";
/// Each stub's surface, numbered by stub.
pub(crate) const STUB_SURFACE: &str = "deadwood.stub.surface";
/// Where side branches and twigs grow, and their sizes.
pub(crate) const BRANCH_SHAPE: &str = "deadwood.branch.shape";
/// Each side branch's and twig's surface, numbered in the order they grow.
pub(crate) const BRANCH_SURFACE: &str = "deadwood.branch.surface";
/// Where a stump's roots grow, and their sizes.
pub(crate) const ROOT_SHAPE: &str = "deadwood.root.shape";
/// Each root's surface, numbered by root.
pub(crate) const ROOT_SURFACE: &str = "deadwood.root.surface";
/// Where bark has peeled off a stem.
pub(crate) const BARK_PATCHES: &str = "deadwood.bark.patches";
/// The cracks between a stem's bark plates.
pub(crate) const BARK_CRACKS: &str = "deadwood.bark.cracks";
/// The roughness of a hollow's inner wall.
pub(crate) const HOLLOW_WALL: &str = "deadwood.hollow.wall";
/// Where leaf sprays grow, and how their cards are laid out.
pub(crate) const LEAF_SPRAYS: &str = "deadwood.leaf.sprays";
/// Which leaf sprays a detail level keeps, numbered by spray.
pub(crate) const LEAF_KEEP: &str = "deadwood.leaf.keep";
/// The sizes and decay of a stick scatter's variants.
pub(crate) const SCATTER_VARIANT: &str = "deadwood.scatter.variant";
/// Each scatter cell's candidate stick, numbered by cell.
pub(crate) const SCATTER_CELL: &str = "deadwood.scatter.cell";

/// Every key above, for the uniqueness test.
#[cfg(test)]
pub(crate) const KEYS: &[&str] = &[
    STEM_SHAPE,
    STEM_SURFACE,
    END_BASE,
    END_TIP,
    STUB_SHAPE,
    STUB_SURFACE,
    BRANCH_SHAPE,
    BRANCH_SURFACE,
    ROOT_SHAPE,
    ROOT_SURFACE,
    BARK_PATCHES,
    BARK_CRACKS,
    HOLLOW_WALL,
    LEAF_SPRAYS,
    LEAF_KEEP,
    SCATTER_VARIANT,
    SCATTER_CELL,
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use procgena_core::hash::salt;

    use super::KEYS;

    #[test]
    fn seed_keys_and_their_salts_are_all_different() {
        let keys: HashSet<&str> = KEYS.iter().copied().collect();
        assert_eq!(keys.len(), KEYS.len(), "a seed key is listed twice");
        let salts: HashSet<u32> = KEYS.iter().map(|k| salt(k)).collect();
        assert_eq!(salts.len(), KEYS.len(), "two seed keys share a salt");
    }

    #[test]
    fn seed_keys_belong_to_this_crate() {
        for key in KEYS {
            let parts: Vec<&str> = key.split('.').collect();
            assert_eq!(parts.len(), 3, "{key} is not <crate>.<area>.<role>");
            assert_eq!(parts[0], "deadwood", "{key} is not a deadwood key");
            assert!(
                parts.iter().all(|p| !p.is_empty()),
                "{key} has an empty part"
            );
        }
    }
}
