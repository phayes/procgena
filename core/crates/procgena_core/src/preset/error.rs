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
        }
    }
}

impl std::error::Error for PresetFileError {}
