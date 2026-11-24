//! Clump documents: a tree or rock subject, and a clump such as snow or moss
//! grown on it, each with its own preset and seed.

pub mod editor;

use std::sync::{Arc, LazyLock};

use procgena_clump::ClumpPreset;
use procgena_stone::StonePreset;
use procgena_tree::TreePreset;

use crate::scene::{ClumpHost, ClumpSettings};
use crate::studio::inspector::MetaKind;

/// The clump settings' metadata, from `clump.meta.yml`.
pub static META: LazyLock<MetaKind> = LazyLock::new(|| {
    MetaKind::new(
        procgena_clump::CLUMP_META,
        procgena_core::meta::ChoiceLists::new(),
    )
});

/// The key of the clump preset shown when the app starts.
pub const START_PRESET: &str = "fresh_snow_cap";

/// A subject preset: a tree or a rock to grow a clump on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Subject {
    /// A tree preset.
    Tree(&'static TreePreset),
    /// A rock preset.
    Stone(&'static StonePreset),
}

impl Subject {
    /// The subject a new clump document starts on: the boulder.
    pub fn start() -> Self {
        Self::Stone(StonePreset::boulder())
    }

    /// Every subject, in menu order: the rocks, then the trees.
    pub fn all() -> Vec<Subject> {
        let stones = StonePreset::all().map(Subject::Stone);
        let trees = TreePreset::all().map(Subject::Tree);
        stones.chain(trees).collect()
    }

    /// The menu label, such as `Rock · Boulder` or `Tree · Oak Medium`.
    pub fn label(self) -> String {
        match self {
            Self::Tree(preset) => format!("Tree · {}", preset.name),
            Self::Stone(preset) => format!("Rock · {}", preset.name),
        }
    }

    /// The preset's tree or rock settings, with its own seed.
    pub fn settings(self) -> ClumpHost {
        match self {
            Self::Tree(preset) => ClumpHost::Tree(Arc::new(preset.tree.clone())),
            Self::Stone(preset) => ClumpHost::Stone(Arc::new(preset.stone.clone())),
        }
    }
}

/// The seed of `subject`'s tree or rock.
pub fn subject_seed(subject: &ClumpHost) -> i64 {
    match subject {
        ClumpHost::Tree(options) => i64::from(options.default_seed),
        ClumpHost::Stone(options) => i64::from(options.default_seed),
    }
}

/// `subject` with its tree or rock seed set to `seed`. Returns `None` if the
/// seed is out of range for that kind of subject.
pub fn with_subject_seed(subject: &ClumpHost, seed: i64) -> Option<ClumpHost> {
    Some(match subject {
        ClumpHost::Tree(options) => {
            let mut options = (**options).clone();
            options.default_seed = i32::try_from(seed).ok()?;
            ClumpHost::Tree(Arc::new(options))
        }
        ClumpHost::Stone(options) => {
            let mut options = (**options).clone();
            options.default_seed = u32::try_from(seed).ok()?;
            ClumpHost::Stone(Arc::new(options))
        }
    })
}

/// The clump settings for `preset` grown on `subject`. The preset's own
/// material sets its look: a texture, tinted by its colour.
pub fn settings(subject: ClumpHost, preset: &ClumpPreset) -> ClumpSettings {
    ClumpSettings {
        subject,
        clump: preset.clump.clone(),
    }
}

/// The clump preset [`START_PRESET`] names.
#[cfg(test)]
pub fn start_preset() -> &'static ClumpPreset {
    ClumpPreset::named(START_PRESET).expect("the start preset is built in")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_name_the_kind_and_preset() {
        assert_eq!(
            Subject::Stone(StonePreset::boulder()).label(),
            "Rock · Boulder"
        );
        assert_eq!(
            Subject::Tree(TreePreset::oak_medium()).label(),
            "Tree · Oak Medium"
        );
        assert_eq!(ClumpPreset::dirty_snow_cap().name, "Dirty Snow Cap");
        assert_eq!(start_preset().name, "Fresh Snow Cap");
        assert!(Subject::all().contains(&Subject::start()));
    }

    #[test]
    fn subject_seeds_are_set_within_range() {
        for subject in [
            Subject::Stone(StonePreset::boulder()).settings(),
            Subject::Tree(TreePreset::oak_medium()).settings(),
        ] {
            let reseeded = with_subject_seed(&subject, 1234).unwrap();
            assert_eq!(subject_seed(&reseeded), 1234);
        }
        let stone = Subject::Stone(StonePreset::boulder()).settings();
        assert!(with_subject_seed(&stone, -1).is_none());
        let tree = Subject::Tree(TreePreset::oak_medium()).settings();
        assert!(with_subject_seed(&tree, i64::from(u32::MAX)).is_none());
    }
}
