//! Presets and preset files for every procgena level.
//!
//! A preset is a named, ready-made set of options: a tree, a stone, a
//! Scatter of trees, a Terrain. Each generator crate lists its built-in
//! presets in a catalogue and implements [`Preset`] for its preset type.
//! This module reads and writes those presets as YAML files, so people can
//! keep their own variants next to their game and comment them.
//!
//! # The file format
//!
//! A file is named `<key>.<kind>.yml`, such as `tall_pine.tree.yml`. The key
//! comes from the file name; the kind says what the file holds. A file
//! names a built-in preset as its `base` and gives only what differs:
//!
//! ```yaml
//! # tall_pine.tree.yml
//! base: pine_medium        # optional: a built-in preset's key
//! name: Tall pine          # optional: the key when not given
//! description: A pine stretched to twice its height.   # optional
//! group: My trees          # optional: "File" when not given
//! tree:                    # the options, under the file's kind
//!   branch:
//!     length:
//!       "0": 10.0
//! ```
//!
//! How the settings under the kind are laid over the base:
//!
//! - **Groups of settings merge key by key.** Setting `branch.length."0"`
//!   changes only that one value and keeps the rest of the base's branch
//!   settings.
//! - **Lists replace the base's list whole.** To change one item of a list,
//!   such as one tree in a Scatter, write out the whole list.
//! - **A group with a different `kind` replaces the base's group whole.**
//!   Switching a setting from one kind to another, such as a sawn end to an
//!   axe-cut one, starts that group afresh, so settings of the old kind do
//!   not linger. The key `kind` is kept for this: an options type whose
//!   setting is a plain value named `kind` writes it under another name.
//! - Without a `base`, the settings are laid over the options' defaults.
//!
//! A setting the options don't have, or a value of the wrong type, is an
//! error that names the setting's path, such as `tree.branch.lenght`.
//!
//! # Reading and writing
//!
//! [`catalogue!`](crate::catalogue) declares a crate's built-in presets and implements
//! [`Preset`] for them. [`read`] turns a file's text into a preset, and
//! [`write`](fn@write) turns a preset into a file's text, writing only what differs
//! from a base.
//! Formats that need more than this, such as Scatter files that refer to
//! asset presets by key, build on [`Document`], [`merge`], [`to_value`],
//! [`from_value`], [`diff`] and [`to_yaml`].

mod catalogue;
mod document;
mod error;
mod file;
mod merge;

pub use document::{Document, FILE_GROUP};
pub use error::PresetFileError;
pub use file::{Preset, PresetMeta, groups, read, write};
pub use merge::{diff, from_value, merge, to_value, to_yaml};

#[cfg(test)]
mod tests;
