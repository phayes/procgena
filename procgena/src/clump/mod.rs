//! The Clumps tab: a tree or rock subject, and a clump such as snow or moss
//! grown on it, each with its own preset and seed.

pub mod editor;
pub mod params;

use std::sync::Arc;

use procgena_clump::ClumpPreset;
use procgena_stone::StonePreset;
use procgena_tree::TreePreset;

use crate::scene::{ClumpSettings, ClumpSubject};

/// The subject shown when the app starts.
pub const START_SUBJECT: Subject = Subject::Stone(StonePreset::Boulder);
/// The clump preset shown when the app starts.
pub const START_PRESET: ClumpPreset = ClumpPreset::SnowCap;

/// A subject preset: a tree or a rock to grow a clump on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    /// A tree preset.
    Tree(TreePreset),
    /// A rock preset.
    Stone(StonePreset),
}

impl Subject {
    /// Every subject, in menu order: the rocks, then the trees.
    pub fn all() -> Vec<Subject> {
        let stones = StonePreset::ALL.into_iter().map(Subject::Stone);
        let trees = TreePreset::ALL.into_iter().map(Subject::Tree);
        stones.chain(trees).collect()
    }

    /// The menu label, such as `Rock · Boulder` or `Tree · Oak Medium`.
    pub fn label(self) -> String {
        match self {
            Self::Tree(preset) => format!("Tree · {}", preset.name()),
            Self::Stone(preset) => format!("Rock · {}", preset.name()),
        }
    }

    /// The preset's tree or rock settings, with its own seed.
    pub fn settings(self) -> ClumpSubject {
        match self {
            Self::Tree(preset) => ClumpSubject::Tree(Arc::new(preset.options())),
            Self::Stone(preset) => ClumpSubject::Stone(Arc::new(preset.options())),
        }
    }
}

/// The seed of `subject`'s tree or rock.
pub fn subject_seed(subject: &ClumpSubject) -> i64 {
    match subject {
        ClumpSubject::Tree(options) => i64::from(options.seed),
        ClumpSubject::Stone(options) => i64::from(options.seed),
    }
}

/// `subject` with its tree or rock seed set to `seed`. Returns `None` if the
/// seed is out of range for that kind of subject.
pub fn with_subject_seed(subject: &ClumpSubject, seed: i64) -> Option<ClumpSubject> {
    Some(match subject {
        ClumpSubject::Tree(options) => {
            let mut options = (**options).clone();
            options.seed = i32::try_from(seed).ok()?;
            ClumpSubject::Tree(Arc::new(options))
        }
        ClumpSubject::Stone(options) => {
            let mut options = (**options).clone();
            options.seed = u32::try_from(seed).ok()?;
            ClumpSubject::Stone(Arc::new(options))
        }
    })
}

/// The menu label for a clump preset, such as `Snow Cap`.
pub fn preset_label(preset: ClumpPreset) -> String {
    preset
        .name()
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The clump settings for `preset` grown on `subject`. The preset's own
/// material sets its look: a texture, tinted by its colour.
pub fn settings(subject: ClumpSubject, preset: ClumpPreset) -> ClumpSettings {
    ClumpSettings {
        subject,
        clump: preset.options(),
    }
}

/// The settings the Clumps tab starts with.
pub fn start_settings() -> ClumpSettings {
    settings(START_SUBJECT.settings(), START_PRESET)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_name_the_kind_and_preset() {
        assert_eq!(
            Subject::Stone(StonePreset::Boulder).label(),
            "Rock · Boulder"
        );
        assert_eq!(
            Subject::Tree(TreePreset::OakMedium).label(),
            "Tree · Oak Medium"
        );
        assert_eq!(preset_label(ClumpPreset::SnowCap), "Snow Cap");
        assert_eq!(preset_label(ClumpPreset::MossPatches), "Moss Patches");
        assert!(Subject::all().contains(&START_SUBJECT));
    }

    #[test]
    fn subject_seeds_are_set_within_range() {
        for subject in [
            Subject::Stone(StonePreset::Boulder).settings(),
            Subject::Tree(TreePreset::OakMedium).settings(),
        ] {
            let reseeded = with_subject_seed(&subject, 1234).unwrap();
            assert_eq!(subject_seed(&reseeded), 1234);
        }
        let stone = Subject::Stone(StonePreset::Boulder).settings();
        assert!(with_subject_seed(&stone, -1).is_none());
        let tree = Subject::Tree(TreePreset::OakMedium).settings();
        assert!(with_subject_seed(&tree, i64::from(u32::MAX)).is_none());
    }
}
