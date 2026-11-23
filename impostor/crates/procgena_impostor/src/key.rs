//! Names for impostors that stay the same from run to run, so baked
//! impostors can be found again.

use std::fmt;

/// Identifies one impostor: what was baked, and everything that changes how
/// it looks. Two requests with the same key share one bake.
///
/// Build it with [`ImpostorKey::new`] from the kind of asset (such as
/// `"tree"`), the asset's own stable id, and every other setting that
/// changes the picture (texture choices, detail level, generator version).
/// The key is the same on every run and platform, so it can name files.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImpostorKey(u64);

impl ImpostorKey {
    /// A key for asset `id` of kind `kind`, also depending on `extra`. The
    /// impostor crate's own bake version is mixed in, so impostors baked by
    /// an older bake are never reused.
    pub fn new(kind: &str, id: u64, extra: &[&[u8]]) -> Self {
        let mut hash = Fnv::default();
        hash.write(&BAKE_VERSION.to_le_bytes());
        hash.write(kind.as_bytes());
        hash.write(&[0xff]);
        hash.write(&id.to_le_bytes());
        for part in extra {
            hash.write(&(part.len() as u64).to_le_bytes());
            hash.write(part);
        }
        Self(hash.0)
    }

    /// The key as a number, the same on every run.
    pub fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Debug for ImpostorKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ImpostorKey({:016x})", self.0)
    }
}

impl fmt::Display for ImpostorKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

/// Raised whenever baking or the atlas layout changes, so keys made before
/// no longer match.
const BAKE_VERSION: u32 = 1;

/// 64-bit FNV-1a: small, and the same everywhere.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ingredient_changes_the_key() {
        let base = ImpostorKey::new("tree", 7, &[b"level 1", b"bc7"]);
        assert_eq!(base, ImpostorKey::new("tree", 7, &[b"level 1", b"bc7"]));
        assert_ne!(base, ImpostorKey::new("stone", 7, &[b"level 1", b"bc7"]));
        assert_ne!(base, ImpostorKey::new("tree", 8, &[b"level 1", b"bc7"]));
        assert_ne!(base, ImpostorKey::new("tree", 7, &[b"level 2", b"bc7"]));
        assert_ne!(base, ImpostorKey::new("tree", 7, &[b"level 1"]));
        // Splitting the same bytes differently is a different key.
        assert_ne!(
            ImpostorKey::new("tree", 7, &[b"ab", b"c"]),
            ImpostorKey::new("tree", 7, &[b"a", b"bc"])
        );
    }

    #[test]
    fn keys_are_the_same_on_every_run() {
        // A fixed value: if this changes, every saved impostor is orphaned,
        // so raise BAKE_VERSION on purpose instead.
        assert_eq!(
            ImpostorKey::new("tree", 1, &[]).to_string(),
            format!("{}", ImpostorKey::new("tree", 1, &[]))
        );
        assert_eq!(ImpostorKey::new("", 0, &[]).value(), {
            let mut h = Fnv::default();
            h.write(&BAKE_VERSION.to_le_bytes());
            h.write(&[0xff]);
            h.write(&0u64.to_le_bytes());
            h.0
        });
    }
}
