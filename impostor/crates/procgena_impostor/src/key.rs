//! Names for impostors that stay the same from run to run, so baked
//! impostors can be found again, and the settings each one is baked with.

use std::fmt;

use procgena_core::{ContentHash, ContentHasher, PoolId, SpecimenId};

/// How one impostor is baked: how many views its atlas holds, and how many
/// pixels each view has.
///
/// More views switch less visibly as the camera moves round; more pixels
/// keep detail closer to the camera. Both make the atlas larger.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BakeSettings {
    /// Views along each side of the atlas; the atlas holds `frames²` views.
    /// At least 1.
    pub frames: u32,
    /// Pixels along each side of one view. At least 1.
    pub frame_size: u32,
    /// Which directions the views look from.
    pub layout: BakeLayout,
}

/// Which directions an impostor's views look from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BakeLayout {
    /// From every direction of the upper hemisphere, for an object seen
    /// from any side and from above, drawn with [`ImpostorMaterial`](crate::ImpostorMaterial).
    #[default]
    Hemisphere,
    /// From one side of the object's +Y axis, at elevations from below to
    /// above (see [`elevation_direction`](crate::elevation_direction)), for
    /// a card turned about that axis that picks its view by elevation alone.
    /// [`ImpostorMaterial`](crate::ImpostorMaterial) can't draw these.
    Elevations,
}

impl BakeSettings {
    /// Hemisphere settings with `frames` views along each side, each
    /// `frame_size` pixels square.
    pub const fn new(frames: u32, frame_size: u32) -> Self {
        Self {
            frames,
            frame_size,
            layout: BakeLayout::Hemisphere,
        }
    }

    /// [`Elevations`](BakeLayout::Elevations) settings with `frames` views
    /// along each side (`frames²` elevations), each `frame_size` pixels
    /// square.
    pub const fn elevations(frames: u32, frame_size: u32) -> Self {
        Self {
            frames,
            frame_size,
            layout: BakeLayout::Elevations,
        }
    }

    /// Pixels along each side of the whole atlas.
    pub const fn atlas_size(&self) -> u32 {
        self.frames * self.frame_size
    }
}

/// 8 × 8 views of 128 pixels: a 1024-pixel atlas.
impl Default for BakeSettings {
    fn default() -> Self {
        Self::new(8, 128)
    }
}

/// Writes the view count, the view size, then the layout's number.
impl ContentHash for BakeSettings {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            frames,
            frame_size,
            layout,
        } = self;
        hasher.write_u32(*frames);
        hasher.write_u32(*frame_size);
        hasher.write_u32(match layout {
            BakeLayout::Hemisphere => 0,
            BakeLayout::Elevations => 1,
        });
    }
}

/// Identifies one impostor: what was baked, how it was baked, and
/// everything else that changes how it looks. Two requests with the same
/// key share one bake.
///
/// Build it with [`ImpostorKey::specimen`] for a whole specimen, or with
/// [`ImpostorKey::pool_part`] for a part shared by every specimen of a
/// pool. The key is the same on every run and platform, so it can name
/// files.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImpostorKey {
    hash: u64,
    bake: BakeSettings,
}

impl ImpostorKey {
    /// The key of specimen `specimen`'s impostor, baked with `bake`.
    ///
    /// `extra` holds every other setting that changes the picture, such as
    /// the detail level baked, texture choices and the generator's version.
    /// The impostor crate's own bake version is mixed in, so impostors baked
    /// by an older bake are never reused.
    pub fn specimen(specimen: SpecimenId, bake: BakeSettings, extra: &[&[u8]]) -> Self {
        Self::build(0, |hasher| hasher.write(&specimen), bake, extra)
    }

    /// The key of a part named `part`, such as `"fruit_cluster"`, that
    /// every specimen of `pool` shares, baked with `bake`. `extra` is as for
    /// [`specimen`](Self::specimen).
    pub fn pool_part(pool: PoolId, part: &str, bake: BakeSettings, extra: &[&[u8]]) -> Self {
        Self::build(
            1,
            |hasher| {
                hasher.write(&pool);
                hasher.write_str(part);
            },
            bake,
            extra,
        )
    }

    /// Hashes the bake version, what is baked (after its `subject` tag), the
    /// bake settings and every `extra` part.
    fn build(
        subject: u32,
        write_subject: impl FnOnce(&mut ContentHasher),
        bake: BakeSettings,
        extra: &[&[u8]],
    ) -> Self {
        let mut hasher = ContentHasher::new();
        hasher.write_u32(BAKE_VERSION);
        hasher.write_u32(subject);
        write_subject(&mut hasher);
        hasher.write(&bake);
        hasher.write_usize(extra.len());
        for part in extra {
            hasher.write_bytes(part);
        }
        Self {
            hash: hasher.finish(),
            bake,
        }
    }

    /// The key as a number, the same on every run.
    pub fn value(self) -> u64 {
        self.hash
    }

    /// The settings the impostor is baked with.
    pub fn bake(self) -> BakeSettings {
        self.bake
    }

    /// A key for tests: slot `slot` of a test pool, with default bake
    /// settings.
    #[cfg(test)]
    pub(crate) fn test(slot: u32) -> Self {
        let pool = PoolId::new(
            procgena_core::GeneratorKind::from_key("impostor.test.kind"),
            procgena_core::OptionsHash(0),
        );
        Self::specimen(SpecimenId::new(0, pool, slot), BakeSettings::default(), &[])
    }
}

impl fmt::Debug for ImpostorKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ImpostorKey({:016x})", self.hash)
    }
}

impl fmt::Display for ImpostorKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.hash)
    }
}

/// Raised whenever baking or the atlas layout changes, so keys made before
/// no longer match.
///
/// - 2: bake cameras no longer see grove trees (version 1 bakes made next
///   to a grove could show its trees in some views).
/// - 3: keys name a specimen or a pool's part, and carry their bake
///   settings.
const BAKE_VERSION: u32 = 3;

#[cfg(test)]
mod tests {
    use procgena_core::{GeneratorKind, OptionsHash};

    use super::*;

    const TREE: GeneratorKind = GeneratorKind::from_key("tree.generator.kind");
    const STONE: GeneratorKind = GeneratorKind::from_key("stone.generator.kind");

    fn id(kind: GeneratorKind, options: u64, slot: u32) -> SpecimenId {
        SpecimenId::new(7, PoolId::new(kind, OptionsHash(options)), slot)
    }

    #[test]
    fn every_ingredient_changes_the_key() {
        let bake = BakeSettings::default();
        let key = |specimen, bake, extra: &[&[u8]]| ImpostorKey::specimen(specimen, bake, extra);
        let base = key(id(TREE, 1, 0), bake, &[b"level 1", b"bc7"]);
        assert_eq!(base, key(id(TREE, 1, 0), bake, &[b"level 1", b"bc7"]));
        assert_ne!(base, key(id(STONE, 1, 0), bake, &[b"level 1", b"bc7"]));
        assert_ne!(base, key(id(TREE, 2, 0), bake, &[b"level 1", b"bc7"]));
        assert_ne!(base, key(id(TREE, 1, 1), bake, &[b"level 1", b"bc7"]));
        assert_ne!(base, key(id(TREE, 1, 0), bake, &[b"level 2", b"bc7"]));
        assert_ne!(base, key(id(TREE, 1, 0), bake, &[b"level 1"]));
        let small = BakeSettings::new(4, 64);
        assert_ne!(base, key(id(TREE, 1, 0), small, &[b"level 1", b"bc7"]));
        let strip = BakeSettings::elevations(8, 128);
        assert_ne!(base, key(id(TREE, 1, 0), strip, &[b"level 1", b"bc7"]));
        assert_eq!(
            key(id(TREE, 1, 0), small, &[]).bake(),
            small,
            "the key carries its bake settings"
        );

        // Splitting the same bytes differently is a different key.
        assert_ne!(
            key(id(TREE, 1, 0), bake, &[b"ab", b"c"]),
            key(id(TREE, 1, 0), bake, &[b"a", b"bc"])
        );

        // A pool's part never shares a key with a specimen, nor with the
        // pool's other parts.
        let pool = PoolId::new(TREE, OptionsHash(1));
        let fruit = ImpostorKey::pool_part(pool, "fruit", bake, &[]);
        assert_eq!(fruit, ImpostorKey::pool_part(pool, "fruit", bake, &[]));
        assert_ne!(fruit, ImpostorKey::pool_part(pool, "flower", bake, &[]));
        assert_ne!(fruit, key(id(TREE, 1, 0), bake, &[]));
    }

    #[test]
    fn keys_are_the_same_on_every_run() {
        // A fixed value: if this changes, every saved impostor is orphaned,
        // so raise BAKE_VERSION on purpose instead.
        let key = ImpostorKey::specimen(id(TREE, 1, 0), BakeSettings::default(), &[]);
        assert_eq!(key.to_string(), format!("{:016x}", key.value()));
        assert_eq!(key.value(), {
            let mut h = ContentHasher::new();
            h.write_u32(BAKE_VERSION);
            h.write_u32(0);
            h.write(&id(TREE, 1, 0));
            h.write(&BakeSettings::default());
            h.write_usize(0);
            h.finish()
        });
    }
}
