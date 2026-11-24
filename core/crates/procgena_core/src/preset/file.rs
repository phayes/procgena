//! The [`Preset`] trait, and reading and writing whole presets.

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::PresetFileError;
use super::document::{Document, FILE_GROUP};
use super::merge::{diff, from_value, merge, to_value};
use super::registry::References;

/// A named, ready-made set of options in a catalogue of built-in presets.
///
/// Each generator crate implements this for its preset type, such as a
/// tree preset, so its presets can be read from and written to
/// `.<kind>.yml` files with [`read`] and [`write`](fn@write), and listed with
/// [`groups`].
pub trait Preset: Sized + 'static {
    /// The options the preset holds.
    type Options: Serialize + DeserializeOwned + Default;

    /// The file kind, such as `"tree"`: files are named
    /// `<key>.<kind>.yml`, and hold the options under this key.
    const KIND: &'static str;

    /// The preset's key, such as `"pine_medium"`: lower case with
    /// underscores, unique in its catalogue.
    fn key(&self) -> &str;

    /// A short display name, such as `"Pine Medium"`.
    fn name(&self) -> &str;

    /// What the preset looks like, in a sentence or two.
    fn description(&self) -> &str;

    /// The group the preset is listed under, such as `"Conifer"`.
    fn group(&self) -> &str;

    /// The preset's options.
    fn options(&self) -> &Self::Options;

    /// The preset's options, taken out of the preset.
    fn into_options(self) -> Self::Options;

    /// Builds a preset from its name parts and options, as read from a file.
    ///
    /// `base` is the built-in preset the file named as its `base`, if any.
    /// A preset type with fields of its own besides the options takes them
    /// from it.
    fn from_parts(meta: PresetMeta, options: Self::Options, base: Option<&'static Self>) -> Self;

    /// Every built-in preset, in catalogue order: grouped, and in display
    /// order within each group.
    fn all() -> impl Iterator<Item = &'static Self>;

    /// The built-in preset with this exact key, if any.
    fn named(key: &str) -> Option<&'static Self> {
        Self::all().find(|preset| preset.key() == key)
    }

    /// Checks options read from a file, such as values that must be in
    /// range. The default accepts everything.
    ///
    /// # Errors
    ///
    /// Returns a message saying what is wrong; [`read`] reports it as
    /// [`PresetFileError::Invalid`].
    fn check(options: &Self::Options) -> Result<(), String> {
        let _ = options;
        Ok(())
    }

    /// How this kind's settings refer to presets of other kinds by key,
    /// such as a Scatter naming its trees. A [`Registry`](super::Registry)
    /// uses it to read presets that refer to project presets, and to answer
    /// which presets use a preset. The default refers to nothing.
    fn references() -> References {
        References::NONE
    }
}

/// A preset's name parts, as read from a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetMeta {
    /// The key, from the file's name.
    pub key: String,
    /// The display name: the file's `name`, or the key.
    pub name: String,
    /// The file's `description`, or empty.
    pub description: String,
    /// The file's `group`, or [`FILE_GROUP`].
    pub group: String,
}

/// Reads a preset from the text of a `.<kind>.yml` file.
///
/// `key` is the preset's key, usually the file name without `.<kind>.yml`;
/// it is not read from the text. The file's settings are laid over its
/// `base` preset's options, or over the options' defaults when it names no
/// base (see the crate docs for how), and then checked with
/// [`Preset::check`].
///
/// # Errors
///
/// Returns a [`PresetFileError`] for text that isn't YAML, keys or settings
/// the file can't have, a `base` that names no built-in preset, and
/// settings that fail [`Preset::check`].
pub fn read<P: Preset>(key: &str, text: &str) -> Result<P, PresetFileError> {
    let document = Document::parse(P::KIND, text)?;

    // Start from the base preset's options, or the defaults.
    let base = match &document.base {
        Some(base) => Some(P::named(base).ok_or_else(|| PresetFileError::UnknownBase {
            key: base.clone(),
            known: P::all().map(|preset| preset.key().to_owned()).collect(),
        })?),
        None => None,
    };
    let mut value = match base {
        Some(base) => to_value(base.options()),
        None => to_value(&P::Options::default()),
    };

    // Lay the file's settings over it, then read and check the options.
    if let Some(settings) = document.settings {
        merge(&mut value, settings);
    }
    let options: P::Options = from_value(value, P::KIND)?;
    P::check(&options).map_err(|message| PresetFileError::Invalid { message })?;

    let meta = PresetMeta {
        key: key.to_owned(),
        name: document.name.unwrap_or_else(|| key.to_owned()),
        description: document.description.unwrap_or_default(),
        group: document.group.unwrap_or_else(|| FILE_GROUP.to_owned()),
    };
    Ok(P::from_parts(meta, options, base))
}

/// Writes a preset as the text of a `.<kind>.yml` file that [`read`] reads
/// back to equal options, name, description and group.
///
/// With a `base`, the file names it and holds only the settings that
/// differ from it; without one, only those that differ from the options'
/// defaults. The key is not written: it is the file's name. The group is
/// left out when it is [`FILE_GROUP`].
#[must_use]
pub fn write<P: Preset>(preset: &P, base: Option<&P>) -> String {
    let base_value = match base {
        Some(base) => to_value(base.options()),
        None => to_value(&P::Options::default()),
    };
    let document = Document {
        base: base.map(|base| base.key().to_owned()),
        name: Some(preset.name().to_owned()),
        description: (!preset.description().is_empty()).then(|| preset.description().to_owned()),
        group: (preset.group() != FILE_GROUP).then(|| preset.group().to_owned()),
        settings: diff(&base_value, &to_value(preset.options())),
    };
    document.to_yaml(P::KIND)
}

/// The built-in presets of `P`, grouped for a menu: each group's name and
/// its presets, in catalogue order.
#[must_use]
pub fn groups<P: Preset>() -> Vec<(&'static str, Vec<&'static P>)> {
    let mut groups: Vec<(&'static str, Vec<&'static P>)> = Vec::new();
    for preset in P::all() {
        match groups.iter_mut().find(|(name, _)| *name == preset.group()) {
            Some((_, presets)) => presets.push(preset),
            None => groups.push((preset.group(), vec![preset])),
        }
    }
    groups
}
