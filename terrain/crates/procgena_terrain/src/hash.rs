//! Repeatable pseudo-random values from integer coordinates.

use crate::region::RegionId;

/// Multiplier applied to each input before mixing in [`pcg3d`].
const PCG_MULTIPLIER: u32 = 1_664_525;
/// Increment added to each input before mixing in [`pcg3d`].
const PCG_INCREMENT: u32 = 1_013_904_223;

/// Turns three integers into three repeatable pseudo-random integers.
///
/// Uses the PCG3D hash from Jarzynski and Olano, "Hash Functions for GPU
/// Rendering" (2020), with wrapping 32-bit arithmetic.
pub(crate) fn pcg3d(v: [u32; 3]) -> [u32; 3] {
    let [mut x, mut y, mut z] =
        v.map(|c| c.wrapping_mul(PCG_MULTIPLIER).wrapping_add(PCG_INCREMENT));

    x = x.wrapping_add(y.wrapping_mul(z));
    y = y.wrapping_add(z.wrapping_mul(x));
    z = z.wrapping_add(x.wrapping_mul(y));

    x ^= x >> 16;
    y ^= y >> 16;
    z ^= z >> 16;

    x = x.wrapping_add(y.wrapping_mul(z));
    y = y.wrapping_add(z.wrapping_mul(x));
    z = z.wrapping_add(x.wrapping_mul(y));

    [x, y, z]
}

/// Folds a 64-bit lattice coordinate into 32 bits, keeping nearby
/// coordinates distinct.
fn fold(v: i64) -> u32 {
    let bits = v as u64;
    (bits as u32) ^ ((bits >> 32) as u32).wrapping_mul(0x9e37_79b9)
}

/// Hashes a 2D lattice cell and a seed.
pub(crate) fn hash2(x: i64, z: i64, seed: u32) -> u32 {
    pcg3d([fold(x), fold(z), seed])[0]
}

/// Derives a seed for one use (`salt`) from a parent seed, so separate noise
/// layers don't repeat each other. Seeds for a role in the world come from
/// a key with [`seed_for`]; a numeric salt is for a second level only, such
/// as each octave of one layer's own seed.
pub(crate) fn derive_seed(seed: u32, salt: u32) -> u32 {
    pcg3d([seed, salt, 0x7e44_a1d5])[0]
}

/// The salt for seed key `key`: the 32-bit FNV-1a hash of its bytes.
pub(crate) const fn salt(key: &str) -> u32 {
    let bytes = key.as_bytes();
    let mut hash = 0x811c_9dc5u32;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u32;
        hash = hash.wrapping_mul(0x0100_0193);
        i += 1;
    }
    hash
}

/// The seed for role `key` (one of [`KEYS`]) in a world seeded with `seed`.
pub(crate) fn seed_for(seed: u32, key: &str) -> u32 {
    derive_seed(seed, salt(key))
}

// Seed keys, named `<crate>.<area>.<role>`. They are frozen: renaming one
// changes every world built from it. Add a new key for a new role, and
// list it in `KEYS`.

/// The relief's broad swells.
pub(crate) const RELIEF_SWELLS: &str = "terrain.relief.swells";
/// The relief's hills.
pub(crate) const RELIEF_HILLS: &str = "terrain.relief.hills";
/// Where the relief's mountain ranges lie.
pub(crate) const RELIEF_MOUNTAIN_MASK: &str = "terrain.relief.mountain_mask";
/// The relief's mountain ridges.
pub(crate) const RELIEF_RIDGES: &str = "terrain.relief.ridges";
/// The world elevation's continents.
pub(crate) const ELEVATION_CONTINENTS: &str = "terrain.elevation.continents";
/// The world elevation's uplands.
pub(crate) const ELEVATION_UPLIFT: &str = "terrain.elevation.uplift";
/// How far region sites move along x.
pub(crate) const GRAPH_SITE_X: &str = "terrain.graph.site_x";
/// How far region sites move along z.
pub(crate) const GRAPH_SITE_Z: &str = "terrain.graph.site_z";
/// The irregularities of region borders made by the border warp.
pub(crate) const GRAPH_BORDER_WARP: &str = "terrain.graph.border_warp";
/// How far each corner of a lattice graph drifts from the lattice.
pub(crate) const GRAPH_CORNER_DRIFT: &str = "terrain.graph.corner_drift";
/// The gully cells' jitter. A fixed seed, the same in every world, so
/// gullies match across borders whatever the neighbours' seeds.
pub(crate) const GULLY_CELLS: &str = "terrain.gully.cells";
/// The irregularities along every border. A fixed seed, the same in every
/// world, so both sides of a border share them whatever their seeds.
pub(crate) const SEAM_DETAIL: &str = "terrain.seam.detail";
/// A Terrain's own choices for its region, made once per region rather
/// than sampled at world positions. See [`region_local_seed`].
pub(crate) const REGION_LOCAL: &str = "terrain.region.local";

/// The seed a Terrain uses for its own choices in `region` of a world
/// seeded with `world_seed`: choices made once for the region, rather than
/// noise sampled at world positions, which always uses the world seed.
///
/// It depends only on the world seed and the region's id, never on the
/// Biome that fills the region, so the same land comes back when a region
/// switches to another Biome on the same Terrain.
pub fn region_local_seed(world_seed: u32, region: RegionId) -> u32 {
    procgena_core::region_seed(world_seed, REGION_LOCAL, region)
}

/// Every seed key in this crate, for the test that they are all different.
#[cfg(test)]
pub(crate) const KEYS: &[&str] = &[
    RELIEF_SWELLS,
    RELIEF_HILLS,
    RELIEF_MOUNTAIN_MASK,
    RELIEF_RIDGES,
    ELEVATION_CONTINENTS,
    ELEVATION_UPLIFT,
    GRAPH_SITE_X,
    GRAPH_SITE_Z,
    GRAPH_BORDER_WARP,
    GRAPH_CORNER_DRIFT,
    GULLY_CELLS,
    SEAM_DETAIL,
    REGION_LOCAL,
];

/// Mixes a 64-bit value into a 64-bit hash, for building identifiers.
pub(crate) fn mix64(state: u64, value: u64) -> u64 {
    // SplitMix64 finaliser over the combined value.
    let mut z = state
        ^ value
            .wrapping_add(0x9e37_79b9_7f4a_7c15)
            .wrapping_add(state << 6);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_repeat_and_differ() {
        assert_eq!(hash2(3, -7, 1), hash2(3, -7, 1));
        assert_ne!(hash2(3, -7, 1), hash2(3, -7, 2));
        assert_ne!(hash2(3, -7, 1), hash2(-7, 3, 1));
        // Coordinates differing only in their upper 32 bits stay distinct.
        assert_ne!(hash2(1 << 33, 0, 1), hash2(0, 0, 1));
        assert_ne!(derive_seed(1, 2), derive_seed(1, 3));
        assert_ne!(mix64(1, 2), mix64(2, 1));
    }

    #[test]
    fn seed_keys_and_their_salts_are_all_different() {
        for (i, a) in KEYS.iter().enumerate() {
            assert!(a.starts_with("terrain."), "{a}");
            assert_eq!(a.split('.').count(), 3, "{a} isn't <crate>.<area>.<role>");
            for b in &KEYS[i + 1..] {
                assert_ne!(a, b);
                assert_ne!(salt(a), salt(b), "{a} and {b} share a salt");
            }
        }
        // FNV-1a's published value for "a", so the salts never drift.
        assert_eq!(salt("a"), 0xe40c_292c);
    }
}
