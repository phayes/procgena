//! Why a preset file could not be read.

use std::fmt;

/// Why a preset file could not be read.
///
/// Every variant's message says what to fix. Settings are named by their
/// path in the file, such as `tree.branch.length`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PresetFileError {
    /// The text is not valid YAML. The message gives the line and column.
    Yaml {
        /// What the YAML reader reported.
        message: String,
    },
    /// The file, or the settings under its kind, is not a map of settings.
    NotAMap {
        /// The part that should be a map: the file's kind, or `"file"`.
        path: String,
    },
    /// The file has a top-level key it doesn't know.
    UnknownKey {
        /// The key the file gave.
        key: String,
        /// The keys a file may have.
        expected: Vec<String>,
    },
    /// `base` names no built-in preset.
    UnknownBase {
        /// The key the file gave.
        key: String,
        /// Every built-in preset's key.
        known: Vec<String>,
    },
    /// A setting is unknown, missing or of the wrong type.
    Setting {
        /// Where the setting is, such as `tree.branch.length`.
        path: String,
        /// What is wrong with it.
        message: String,
    },
    /// The settings were read, but break a rule of the options, such as a
    /// value out of range.
    Invalid {
        /// What the options' own check reported.
        message: String,
    },
    /// A reference names no preset of its kind in a
    /// [`Registry`](super::Registry).
    UnknownPreset {
        /// The kind of preset looked for, such as `"tree"`.
        kind: String,
        /// The reference as written.
        key: String,
        /// Every key a reference of this kind may name.
        known: Vec<String>,
    },
    /// A project preset and a built-in preset share this key, so it names
    /// neither until the project preset is renamed; see
    /// [`Problem::KeyClash`](super::Problem::KeyClash).
    KeyClash {
        /// The kind of both presets.
        kind: String,
        /// The key they share.
        key: String,
    },
    /// A chain of `base` presets leads back to a preset already in it.
    BaseCycle {
        /// The kind of the presets.
        kind: String,
        /// The keys of the chain, ending with the one seen twice.
        keys: Vec<String>,
    },
    /// The [`Registry`](super::Registry) has no preset type registered for
    /// this kind.
    UnknownKind {
        /// The kind asked for.
        kind: String,
    },
}

impl fmt::Display for PresetFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Yaml { message } => write!(f, "{message}"),
            Self::NotAMap { path } => write!(f, "{path} must be a map of settings"),
            Self::UnknownKey { key, expected } => write!(
                f,
                "unknown key {key:?}; a preset file may have {}",
                expected.join(", ")
            ),
            Self::UnknownBase { key, known } => write!(
                f,
                "unknown base preset {key:?}; valid keys are {}",
                known.join(", ")
            ),
            Self::Setting { path, message } => write!(f, "{path}: {message}"),
            Self::Invalid { message } => write!(f, "{message}"),
            Self::UnknownPreset { kind, key, known } => write!(
                f,
                "unknown {kind} preset {key:?}; valid keys are {}",
                known.join(", ")
            ),
            Self::KeyClash { kind, key } => write!(
                f,
                "the project's {kind} preset {key:?} has a built-in preset's key; rename it"
            ),
            Self::BaseCycle { kind, keys } => write!(
                f,
                "the {kind} presets' bases form a loop: {}",
                keys.join(" → ")
            ),
            Self::UnknownKind { kind } => write!(f, "no preset kind is named {kind:?}"),
        }
    }
}

impl std::error::Error for PresetFileError {}
