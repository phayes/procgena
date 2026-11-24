//! The preset registry: the built-in presets and a project's presets,
//! looked up together by key.

use std::fmt;
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use super::document::{Document, FILE_GROUP};
use super::merge::{diff, from_value, merge, to_value};
use super::{Preset, PresetFileError, PresetMeta};
use crate::{ChildKey, KeyError};

/// Where a preset in a [`Registry`] comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Layer {
    /// A preset file in the open project.
    Project,
    /// One of procgena's built-in presets.
    BuiltIn,
}

impl Layer {
    /// Every layer, in the order [`Registry::entries`] lists them.
    pub const ALL: [Self; 2] = [Self::Project, Self::BuiltIn];
}

/// A preset found in a [`Registry`]: a built-in one, or one read from a
/// project file. Use it as a `&P`.
#[derive(Debug)]
pub enum PresetRef<P: 'static> {
    /// A built-in preset.
    BuiltIn(&'static P),
    /// A preset read from a project file.
    Owned(Arc<P>),
}

impl<P> PresetRef<P> {
    /// The layer the preset comes from.
    #[must_use]
    pub fn layer(&self) -> Layer {
        match self {
            Self::BuiltIn(_) => Layer::BuiltIn,
            Self::Owned(_) => Layer::Project,
        }
    }
}

impl<P> Clone for PresetRef<P> {
    fn clone(&self) -> Self {
        match self {
            Self::BuiltIn(preset) => Self::BuiltIn(preset),
            Self::Owned(preset) => Self::Owned(Arc::clone(preset)),
        }
    }
}

impl<P> Deref for PresetRef<P> {
    type Target = P;

    fn deref(&self) -> &P {
        match self {
            Self::BuiltIn(preset) => preset,
            Self::Owned(preset) => preset,
        }
    }
}

/// How one kind's settings refer to presets of other kinds by key, such as
/// a Scatter naming its trees. Returned by [`Preset::references`].
#[derive(Debug, Clone, Copy)]
pub struct References {
    /// Lists the references in a full settings tree of this kind (its
    /// file's settings laid over its base), with where each one is.
    pub slots: fn(&Value) -> Vec<ReferenceSlot>,
    /// Replaces every reference in a full settings tree of this kind with
    /// the settings it names, looked up in the registry, so the tree reads
    /// with [`from_value`] without a registry. Errors name the setting's
    /// path within the tree, such as `trees.0.tree` (see
    /// [`PresetFileError::at`]); the registry puts the kind in front.
    pub expand: fn(&mut Value, &Registry) -> Result<(), PresetFileError>,
}

impl References {
    /// For kinds whose settings refer to no other preset.
    pub const NONE: Self = Self {
        slots: |_| Vec::new(),
        expand: |_, _| Ok(()),
    };
}

/// One reference to a preset inside another preset's settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceSlot {
    /// Where the reference is, as a dotted path under the kind, such as
    /// `trees.0.tree`.
    pub path: String,
    /// The kind of preset referred to, such as `"tree"`.
    pub kind: &'static str,
    /// The key referred to, such as `pine_small`.
    pub reference: String,
}

/// The reference a settings slot holds: the slot's text, or the `base` of
/// a map of settings. `None` for a map without a `base` and anything else.
#[must_use]
pub fn slot_reference(slot: &Value) -> Option<&str> {
    match slot {
        Value::String(text) => Some(text),
        Value::Object(map) => map.get("base").and_then(Value::as_str),
        _ => None,
    }
}

/// Replaces the reference in `slot` with the full settings it names, and
/// returns the preset it names.
///
/// A key gives the preset's settings. A map with a `base` key gives the
/// rest of the map laid over the base preset's settings. A map without a
/// `base`, and anything else, is left as it is and gives `None`.
///
/// # Errors
///
/// Returns the registry's error when the reference names no preset, or
/// names one that can't be read, and [`PresetFileError::Setting`] at
/// `base` when `base` isn't text.
pub fn expand_slot<P: Preset>(
    slot: &mut Value,
    registry: &Registry,
) -> Result<Option<PresetRef<P>>, PresetFileError> {
    match slot {
        Value::String(text) => {
            let preset = registry.resolve::<P>(text)?;
            *slot = to_value(preset.options());
            Ok(Some(preset))
        }
        Value::Object(map) => match map.remove("base") {
            None => Ok(None),
            Some(Value::String(text)) => {
                let preset = registry.resolve::<P>(&text)?;
                let mut full = to_value(preset.options());
                merge(&mut full, slot.take());
                *slot = full;
                Ok(Some(preset))
            }
            Some(_) => Err(PresetFileError::Setting {
                path: "base".to_owned(),
                message: format!("must be a {} preset key", P::KIND),
            }),
        },
        _ => Ok(None),
    }
}

/// One preset as the registry lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The layer it comes from.
    pub layer: Layer,
    /// Its key.
    pub key: String,
    /// Its display name.
    pub name: String,
    /// What it looks like, or empty.
    pub description: String,
    /// The group it is listed under.
    pub group: String,
    /// The reference to its base preset, as its file writes it. `None` for
    /// built-in presets and files without a base.
    pub base: Option<String>,
}

/// A preset file in a registry's project layer.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectPreset {
    /// The kind, such as `"tree"`.
    pub kind: &'static str,
    /// The key, from the file's name.
    pub key: String,
    /// The file it was read from, or `None` for text added without one.
    pub path: Option<PathBuf>,
    /// The file's parts.
    pub document: Document,
}

/// How one preset uses another, as [`Registry::used_by`] reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Use {
    /// The layer of the preset that uses it.
    pub layer: Layer,
    /// The kind of the preset that uses it.
    pub kind: &'static str,
    /// The key of the preset that uses it.
    pub key: String,
    /// `None` when the preset is written over it as its `base`; otherwise
    /// the path of the setting that refers to it, such as `trees.0.tree`.
    pub path: Option<String>,
}

/// Something wrong with a project's presets, found while reading the
/// folder or by [`Registry::problems`]. The rest of the project still
/// reads.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Problem {
    /// A file or folder could not be read.
    Io {
        /// The file or folder.
        path: PathBuf,
        /// What the system reported.
        message: String,
    },
    /// A kind folder names no kind of preset this registry knows.
    UnknownKind {
        /// The folder.
        path: PathBuf,
    },
    /// A file in a kind folder isn't named `<key>.<kind>.yml` for that
    /// folder's kind, or isn't in a kind folder at all.
    Misplaced {
        /// The file.
        path: PathBuf,
        /// What its name and place should be, such as
        /// `presets/tree/<key>.tree.yml`.
        expected: String,
    },
    /// A file's name gives a key that isn't a valid key.
    BadKey {
        /// The file, if the preset came from one.
        path: Option<PathBuf>,
        /// The key.
        key: String,
        /// What is wrong with it.
        error: KeyError,
    },
    /// A file could not be read as a preset file. Presets that only fail
    /// when their settings are read, such as an unknown `base`, are listed
    /// and report their error when resolved.
    Unreadable {
        /// The file, if the preset came from one.
        path: Option<PathBuf>,
        /// The preset's kind.
        kind: &'static str,
        /// The preset's key.
        key: String,
        /// Why it could not be read.
        error: Box<PresetFileError>,
    },
    /// A project preset has a built-in preset's key, which happens when a
    /// newer procgena adds a built-in preset with a key the project already
    /// used. Neither preset resolves by that key until the project preset
    /// is renamed.
    KeyClash {
        /// The kind of both presets.
        kind: &'static str,
        /// The key they share.
        key: String,
    },
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, message } => write!(f, "{}: {message}", path.display()),
            Self::UnknownKind { path } => {
                write!(f, "{}: no kind of preset has this name", path.display())
            }
            Self::Misplaced { path, expected } => write!(
                f,
                "{}: not a preset file here; preset files are {expected}",
                path.display()
            ),
            Self::BadKey { path, key, error } => match path {
                Some(path) => write!(f, "{}: key {key:?}: {error}", path.display()),
                None => write!(f, "key {key:?}: {error}"),
            },
            Self::Unreadable {
                path,
                kind,
                key,
                error,
            } => match path {
                Some(path) => write!(f, "{}: {error}", path.display()),
                None => write!(f, "{kind} preset {key:?}: {error}"),
            },
            Self::KeyClash { kind, key } => write!(
                f,
                "the project's {kind} preset {key:?} has a built-in preset's key; rename it"
            ),
        }
    }
}

/// What the registry knows about one kind of preset.
#[derive(Clone, Copy)]
struct KindEntry {
    kind: &'static str,
    builtin_entries: fn() -> Vec<Entry>,
    builtin_value: fn(&str) -> Option<Value>,
    default_value: fn() -> Value,
    references: References,
}

/// The built-in presets of `P`, as registry entries.
fn builtin_entries<P: Preset>() -> Vec<Entry> {
    P::all()
        .map(|preset| Entry {
            layer: Layer::BuiltIn,
            key: preset.key().to_owned(),
            name: preset.name().to_owned(),
            description: preset.description().to_owned(),
            group: preset.group().to_owned(),
            base: None,
        })
        .collect()
}

/// The settings tree of the built-in preset `key` of `P`.
fn builtin_value<P: Preset>(key: &str) -> Option<Value> {
    P::named(key).map(|preset| to_value(preset.options()))
}

/// The settings tree of `P`'s default options.
fn default_value<P: Preset>() -> Value {
    to_value(&P::Options::default())
}

/// The built-in presets of every registered kind, and one project's preset
/// files over them, looked up by kind and key.
///
/// Register each preset type with [`register`](Self::register), then add a
/// project with [`load_folder`](Self::load_folder) or
/// [`add`](Self::add). Project presets may name other project presets as
/// their `base`, and refer to them from their settings. Built-in presets
/// only ever refer to built-in presets.
///
/// One key names one preset of a kind: adding a project preset with a
/// built-in preset's key is refused. A project saved before a newer
/// procgena added a built-in preset with the same key still loads; the
/// clash is listed in [`problems`](Self::problems), and the key resolves to
/// neither preset until the project preset is renamed.
#[derive(Default)]
pub struct Registry {
    kinds: Vec<KindEntry>,
    project: Vec<ProjectPreset>,
    load_problems: Vec<Problem>,
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registry")
            .field("kinds", &self.kinds().collect::<Vec<_>>())
            .field("project", &self.project)
            .field("load_problems", &self.load_problems)
            .finish()
    }
}

impl Registry {
    /// A registry with no kinds and no project.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the preset type `P`, with its built-in presets.
    ///
    /// # Panics
    ///
    /// Panics if a type of the same kind is already registered.
    pub fn register<P: Preset>(&mut self) -> &mut Self {
        assert!(
            self.kind(P::KIND).is_none(),
            "preset kind {} is registered twice",
            P::KIND
        );
        self.kinds.push(KindEntry {
            kind: P::KIND,
            builtin_entries: builtin_entries::<P>,
            builtin_value: builtin_value::<P>,
            default_value: default_value::<P>,
            references: P::references(),
        });
        self
    }

    /// Every registered kind, in the order registered.
    pub fn kinds(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.kinds.iter().map(|kind| kind.kind)
    }

    fn kind(&self, kind: &str) -> Option<&KindEntry> {
        self.kinds.iter().find(|entry| entry.kind == kind)
    }

    fn kind_or_error(&self, kind: &str) -> Result<&KindEntry, PresetFileError> {
        self.kind(kind).ok_or_else(|| PresetFileError::UnknownKind {
            kind: kind.to_owned(),
        })
    }

    /// Adds the text of a preset file to the project layer, replacing a
    /// project preset of the same kind and key.
    ///
    /// `path` is the file it came from, if any, kept for messages and
    /// saving. The file is split into its parts now; its settings are read
    /// when the preset is resolved.
    ///
    /// # Errors
    ///
    /// As for [`add_preset`](Self::add_preset), and [`Problem::Unreadable`]
    /// for text that isn't a preset file. The project is unchanged then.
    pub fn add(
        &mut self,
        kind: &str,
        key: &str,
        text: &str,
        path: Option<PathBuf>,
    ) -> Result<(), Problem> {
        let preset = self.parse_file(kind, key, text, path)?;
        self.add_preset(preset)
    }

    /// Adds a preset to the project layer, replacing a project preset of
    /// the same kind and key.
    ///
    /// # Errors
    ///
    /// Returns [`Problem::UnknownKind`] for an unregistered kind,
    /// [`Problem::BadKey`] for a key that isn't a valid [`ChildKey`], and
    /// [`Problem::KeyClash`] for a built-in preset's key. The project is
    /// unchanged then.
    pub fn add_preset(&mut self, preset: ProjectPreset) -> Result<(), Problem> {
        if self.kind(preset.kind).is_none() {
            return Err(Problem::UnknownKind {
                path: preset.path.unwrap_or_else(|| PathBuf::from(preset.kind)),
            });
        }
        if let Err(error) = ChildKey::new(preset.key.as_str()) {
            return Err(Problem::BadKey {
                path: preset.path,
                key: preset.key,
                error,
            });
        }
        if self.has_builtin(preset.kind, &preset.key) {
            return Err(Problem::KeyClash {
                kind: preset.kind,
                key: preset.key,
            });
        }
        self.insert(preset);
        Ok(())
    }

    /// Splits the text of a preset file into a project preset, checking
    /// its kind and key but not whether the key clashes.
    fn parse_file(
        &self,
        kind: &str,
        key: &str,
        text: &str,
        path: Option<PathBuf>,
    ) -> Result<ProjectPreset, Problem> {
        let Some(entry) = self.kind(kind) else {
            return Err(Problem::UnknownKind {
                path: path.unwrap_or_else(|| PathBuf::from(kind)),
            });
        };
        let kind = entry.kind;
        if let Err(error) = ChildKey::new(key) {
            return Err(Problem::BadKey {
                path,
                key: key.to_owned(),
                error,
            });
        }
        let document = Document::parse(kind, text).map_err(|error| Problem::Unreadable {
            path: path.clone(),
            kind,
            key: key.to_owned(),
            error: Box::new(error),
        })?;
        Ok(ProjectPreset {
            kind,
            key: key.to_owned(),
            path,
            document,
        })
    }

    /// Adds a preset to the project layer, replacing one of the same kind
    /// and key.
    fn insert(&mut self, preset: ProjectPreset) {
        match self
            .project
            .iter_mut()
            .find(|old| old.kind == preset.kind && old.key == preset.key)
        {
            Some(old) => *old = preset,
            None => self.project.push(preset),
        }
    }

    /// Removes the project preset `key` of kind `kind`, and returns it.
    pub fn remove(&mut self, kind: &str, key: &str) -> Option<ProjectPreset> {
        let index = self
            .project
            .iter()
            .position(|preset| preset.kind == kind && preset.key == key)?;
        Some(self.project.remove(index))
    }

    /// Replaces the project layer with the preset files in `presets`, a
    /// project's `presets` folder.
    ///
    /// Files are read from `presets/<kind>/<key>.<kind>.yml`. A file or
    /// folder that can't be read, or doesn't fit that layout, is listed in
    /// [`problems`](Self::problems) and skipped. `.DS_Store`, `Thumbs.db`,
    /// `__MACOSX` and other names starting with a dot are ignored. A
    /// missing folder gives an empty project.
    ///
    /// # Errors
    ///
    /// Returns the error reading `presets` itself; the project layer is
    /// empty then.
    pub fn load_folder(&mut self, presets: &Path) -> io::Result<()> {
        self.project.clear();
        self.load_problems.clear();
        if !presets.exists() {
            return Ok(());
        }

        // Each kind's folder, in name order.
        for kind_path in sorted_entries(presets)? {
            if !kind_path.is_dir() {
                self.load_problems.push(Problem::Misplaced {
                    expected: "presets/<kind>/<key>.<kind>.yml".to_owned(),
                    path: kind_path,
                });
                continue;
            }
            let folder = file_name(&kind_path);
            let Some(kind) = self.kind(&folder).map(|entry| entry.kind) else {
                self.load_problems
                    .push(Problem::UnknownKind { path: kind_path });
                continue;
            };
            let files = match sorted_entries(&kind_path) {
                Ok(files) => files,
                Err(error) => {
                    self.load_problems.push(Problem::Io {
                        path: kind_path,
                        message: error.to_string(),
                    });
                    continue;
                }
            };

            // Each preset file in it.
            let suffix = format!(".{kind}.yml");
            for path in files {
                let name = file_name(&path);
                let key = match name.strip_suffix(&suffix) {
                    Some(key) if path.is_file() => key.to_owned(),
                    _ => {
                        self.load_problems.push(Problem::Misplaced {
                            path,
                            expected: format!("presets/{kind}/<key>{suffix}"),
                        });
                        continue;
                    }
                };
                // A key clash is kept, and reported by `problems`.
                match std::fs::read_to_string(&path) {
                    Ok(text) => match self.parse_file(kind, &key, &text, Some(path)) {
                        Ok(preset) => self.insert(preset),
                        Err(problem) => self.load_problems.push(problem),
                    },
                    Err(error) => self.load_problems.push(Problem::Io {
                        path,
                        message: error.to_string(),
                    }),
                }
            }
        }
        Ok(())
    }

    /// The project layer's presets, in the order added.
    #[must_use]
    pub fn project(&self) -> &[ProjectPreset] {
        &self.project
    }

    /// The project preset `key` of kind `kind`, if any.
    #[must_use]
    pub fn project_preset(&self, kind: &str, key: &str) -> Option<&ProjectPreset> {
        self.project
            .iter()
            .find(|preset| preset.kind == kind && preset.key == key)
    }

    /// What is wrong with the project: the problems found by the last
    /// [`load_folder`](Self::load_folder), then every key clash between a
    /// project preset and a built-in one.
    #[must_use]
    pub fn problems(&self) -> Vec<Problem> {
        let mut problems = self.load_problems.clone();
        for preset in &self.project {
            if self.has_builtin(preset.kind, &preset.key) {
                problems.push(Problem::KeyClash {
                    kind: preset.kind,
                    key: preset.key.clone(),
                });
            }
        }
        problems
    }

    fn has_builtin(&self, kind: &str, key: &str) -> bool {
        self.kind(kind)
            .is_some_and(|entry| (entry.builtin_value)(key).is_some())
    }

    /// Every preset of kind `kind`: the project's, in the order added, then
    /// the built-in ones in catalogue order. Empty for an unregistered
    /// kind.
    ///
    /// A project preset whose file has no `name` is listed by its key, and
    /// one with no `group` under [`FILE_GROUP`].
    #[must_use]
    pub fn entries(&self, kind: &str) -> Vec<Entry> {
        let Some(entry) = self.kind(kind) else {
            return Vec::new();
        };
        let mut entries: Vec<Entry> = self
            .project
            .iter()
            .filter(|preset| preset.kind == kind)
            .map(|preset| Entry {
                layer: Layer::Project,
                key: preset.key.clone(),
                name: preset
                    .document
                    .name
                    .clone()
                    .unwrap_or_else(|| preset.key.clone()),
                description: preset.document.description.clone().unwrap_or_default(),
                group: preset
                    .document
                    .group
                    .clone()
                    .unwrap_or_else(|| FILE_GROUP.to_owned()),
                base: preset.document.base.clone(),
            })
            .collect();
        entries.extend((entry.builtin_entries)());
        entries
    }

    /// Finds the preset `key` of kind `kind`, and returns the layer it is
    /// in.
    ///
    /// # Errors
    ///
    /// Returns [`PresetFileError::UnknownKind`] for an unregistered kind,
    /// [`PresetFileError::UnknownPreset`] when no layer has the key, and
    /// [`PresetFileError::KeyClash`] when a project preset and a built-in
    /// preset share it (see [`Problem::KeyClash`]).
    pub fn find(&self, kind: &str, key: &str) -> Result<Layer, PresetFileError> {
        let entry = self.kind_or_error(kind)?;
        let in_project = self.project_preset(kind, key).is_some();
        let built_in = (entry.builtin_value)(key).is_some();
        match (in_project, built_in) {
            (true, false) => Ok(Layer::Project),
            (false, true) => Ok(Layer::BuiltIn),
            (false, false) => Err(PresetFileError::UnknownPreset {
                kind: entry.kind.to_owned(),
                key: key.to_owned(),
                known: self.known_keys(entry.kind),
            }),
            (true, true) => Err(PresetFileError::KeyClash {
                kind: entry.kind.to_owned(),
                key: key.to_owned(),
            }),
        }
    }

    /// Every key of kind `kind` that names one preset, project presets
    /// first.
    fn known_keys(&self, kind: &str) -> Vec<String> {
        let entries = self.entries(kind);
        let mut known = Vec::with_capacity(entries.len());
        for entry in &entries {
            let clashes = entries
                .iter()
                .filter(|other| other.key == entry.key)
                .count()
                > 1;
            if !clashes {
                known.push(entry.key.clone());
            }
        }
        known
    }

    /// The full settings tree of the preset `key` of kind `kind`, in file
    /// form: its file's settings laid over its base's, down to a built-in
    /// preset or the defaults. References to other presets in it are kept
    /// as written.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`find`](Self::find) for the key and for every
    /// `base` in the chain (an unknown base as
    /// [`PresetFileError::UnknownBase`]), and
    /// [`PresetFileError::BaseCycle`] when the chain loops.
    pub fn settings(&self, kind: &str, key: &str) -> Result<Value, PresetFileError> {
        let layer = self.find(kind, key)?;
        self.settings_of(kind, layer, key, &mut Vec::new())
    }

    /// The full settings tree of `document`, a file of kind `kind` named
    /// `key`, laid over its base through this registry the way
    /// [`settings`](Self::settings) does. The document need not be in the
    /// registry, such as one about to be saved.
    ///
    /// # Errors
    ///
    /// As for [`settings`](Self::settings).
    pub fn document_settings(
        &self,
        kind: &str,
        key: &str,
        document: &Document,
    ) -> Result<Value, PresetFileError> {
        self.over_base(kind, key, document, &mut Vec::new())
    }

    fn settings_of(
        &self,
        kind: &str,
        layer: Layer,
        key: &str,
        chain: &mut Vec<String>,
    ) -> Result<Value, PresetFileError> {
        let entry = self.kind_or_error(kind)?;
        match layer {
            Layer::BuiltIn => {
                (entry.builtin_value)(key).ok_or_else(|| PresetFileError::UnknownPreset {
                    kind: entry.kind.to_owned(),
                    key: key.to_owned(),
                    known: self.known_keys(entry.kind),
                })
            }
            Layer::Project => {
                let preset = self.project_preset(kind, key).ok_or_else(|| {
                    PresetFileError::UnknownPreset {
                        kind: entry.kind.to_owned(),
                        key: key.to_owned(),
                        known: self.known_keys(entry.kind),
                    }
                })?;
                self.over_base(kind, key, &preset.document, chain)
            }
        }
    }

    /// `document`'s settings laid over its base's full settings, with the
    /// keys of the project presets being laid in `chain`.
    fn over_base(
        &self,
        kind: &str,
        key: &str,
        document: &Document,
        chain: &mut Vec<String>,
    ) -> Result<Value, PresetFileError> {
        let entry = self.kind_or_error(kind)?;

        // A key already being laid closes a loop.
        if chain.iter().any(|seen| seen == key) {
            let mut keys = chain.clone();
            keys.push(key.to_owned());
            return Err(PresetFileError::BaseCycle {
                kind: entry.kind.to_owned(),
                keys,
            });
        }

        // The base's settings, or the defaults.
        chain.push(key.to_owned());
        let base = match &document.base {
            Some(base) => self
                .find_base(entry.kind, base)
                .and_then(|layer| self.settings_of(entry.kind, layer, base, chain)),
            None => Ok((entry.default_value)()),
        };
        chain.pop();
        let mut value = base?;

        // This file's settings over them.
        if let Some(settings) = &document.settings {
            merge(&mut value, settings.clone());
        }
        Ok(value)
    }

    /// [`find`](Self::find) for a file's `base`, reporting an unknown key
    /// as [`PresetFileError::UnknownBase`].
    fn find_base(&self, kind: &str, base: &str) -> Result<Layer, PresetFileError> {
        self.find(kind, base).map_err(|error| match error {
            PresetFileError::UnknownPreset { key, known, .. } => {
                PresetFileError::UnknownBase { key, known }
            }
            other => other,
        })
    }

    /// The text of a preset file of kind `kind` for `document`, whose
    /// `settings` are the preset's full settings in file form (as
    /// [`settings`](Self::settings) gives them, edited).
    ///
    /// The file names the document's `base`, name, description and group
    /// as given, and holds only the settings that differ from the base's
    /// full settings, or from the defaults without a base, in the order the
    /// settings are declared. The same document always gives the same text.
    /// Comments and layout of a file the text replaces are not kept.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`settings`](Self::settings) for the base.
    pub fn file_text(&self, kind: &str, document: &Document) -> Result<String, PresetFileError> {
        let entry = self.kind_or_error(kind)?;
        let base = match &document.base {
            Some(base) => self
                .settings(entry.kind, base)
                .map_err(|error| match error {
                    PresetFileError::UnknownPreset { key, known, .. } => {
                        PresetFileError::UnknownBase { key, known }
                    }
                    other => other,
                })?,
            None => (entry.default_value)(),
        };
        let differences = Document {
            settings: document
                .settings
                .as_ref()
                .and_then(|settings| diff(&base, settings)),
            ..document.clone()
        };
        Ok(differences.to_yaml(entry.kind))
    }

    /// The built-in preset at the root of the `base` chain of `document`,
    /// if the chain ends in one.
    fn root_builtin<P: Preset>(&self, document: &Document) -> Option<&'static P> {
        let mut base = document.base.clone();
        let mut steps = 0;
        while let Some(key) = base {
            match self.find_base(P::KIND, &key).ok()? {
                Layer::BuiltIn => return P::named(&key),
                Layer::Project => {
                    base = self.project_preset(P::KIND, &key)?.document.base.clone();
                }
            }
            // A loop is reported when the settings are laid; stop here.
            steps += 1;
            if steps > self.project.len() {
                return None;
            }
        }
        None
    }

    /// The preset `key` of type `P`: a built-in preset as it is, or a
    /// project preset read from its file through this registry, with its
    /// base and the presets it refers to found here too.
    ///
    /// A project preset takes the fields its type inherits from a base
    /// (see [`catalogue!`](crate::catalogue)'s `inherit`) from the built-in
    /// preset at the root of its `base` chain.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`settings`](Self::settings), the errors of
    /// expanding the presets it refers to, and the errors of reading and
    /// checking its options, as [`read`](super::read) does.
    pub fn resolve<P: Preset>(&self, key: &str) -> Result<PresetRef<P>, PresetFileError> {
        match self.find(P::KIND, key)? {
            Layer::BuiltIn => P::named(key).map(PresetRef::BuiltIn).ok_or_else(|| {
                PresetFileError::UnknownPreset {
                    kind: P::KIND.to_owned(),
                    key: key.to_owned(),
                    known: self.known_keys(P::KIND),
                }
            }),
            Layer::Project => {
                let preset = self
                    .project_preset(P::KIND, key)
                    .expect("find found the project preset");
                self.build::<P>(key, &preset.document)
                    .map(|preset| PresetRef::Owned(Arc::new(preset)))
            }
        }
    }

    /// Reads a preset of type `P` from the text of a file, like
    /// [`read`](super::read), but with its `base` and the presets it refers
    /// to looked up in this registry. `key` is the preset's key; the file
    /// need not be in the registry.
    ///
    /// # Errors
    ///
    /// As for [`resolve`](Self::resolve), and errors splitting the text as
    /// [`Document::parse`] gives them.
    pub fn read<P: Preset>(&self, key: &str, text: &str) -> Result<P, PresetFileError> {
        let document = Document::parse(P::KIND, text)?;
        self.build(key, &document)
    }

    /// The options of a file of type `P` named `key` holding `document`,
    /// with its base and references found in this registry.
    fn build<P: Preset>(&self, key: &str, document: &Document) -> Result<P, PresetFileError> {
        // The full settings, with references replaced by what they name.
        let mut value = self.document_settings(P::KIND, key, document)?;
        (P::references().expand)(&mut value, self).map_err(|error| error.at(P::KIND))?;

        // Read and check the options.
        let options: P::Options = from_value(value, P::KIND)?;
        P::check(&options).map_err(|message| PresetFileError::Invalid { message })?;
        let meta = PresetMeta {
            key: key.to_owned(),
            name: document.name.clone().unwrap_or_else(|| key.to_owned()),
            description: document.description.clone().unwrap_or_default(),
            group: document
                .group
                .clone()
                .unwrap_or_else(|| FILE_GROUP.to_owned()),
        };
        Ok(P::from_parts(
            meta,
            options,
            self.root_builtin::<P>(document),
        ))
    }

    /// Every preset that uses the preset `key` of kind `kind` directly: as
    /// its `base`, or by a reference in its settings.
    ///
    /// A preset's references are those in its full settings, so references
    /// it takes from its base count as its own. Built-in presets never use
    /// project presets. A project preset whose settings can't be laid is
    /// skipped.
    #[must_use]
    pub fn used_by(&self, kind: &str, key: &str) -> Vec<Use> {
        let mut uses = Vec::new();

        // Project presets: their base, then the references in their settings.
        for preset in &self.project {
            if preset.kind == kind && preset.document.base.as_deref() == Some(key) {
                uses.push(Use {
                    layer: Layer::Project,
                    kind: preset.kind,
                    key: preset.key.clone(),
                    path: None,
                });
            }
            let Some(entry) = self.kind(preset.kind) else {
                continue;
            };
            let Ok(settings) =
                self.settings_of(preset.kind, Layer::Project, &preset.key, &mut Vec::new())
            else {
                continue;
            };
            for slot in (entry.references.slots)(&settings) {
                if slot.kind == kind && slot.reference == key {
                    uses.push(Use {
                        layer: Layer::Project,
                        kind: preset.kind,
                        key: preset.key.clone(),
                        path: Some(slot.path),
                    });
                }
            }
        }

        // Built-in presets refer only to built-in presets.
        if self.has_builtin(kind, key) {
            for entry in &self.kinds {
                for builtin in (entry.builtin_entries)() {
                    let Some(settings) = (entry.builtin_value)(&builtin.key) else {
                        continue;
                    };
                    for slot in (entry.references.slots)(&settings) {
                        if slot.kind == kind && slot.reference == key {
                            uses.push(Use {
                                layer: Layer::BuiltIn,
                                kind: entry.kind,
                                key: builtin.key.clone(),
                                path: Some(slot.path),
                            });
                        }
                    }
                }
            }
        }
        uses
    }
}

/// Where the file of the project preset `key` of kind `kind` goes, in a
/// project's `presets` folder: `presets/<kind>/<key>.<kind>.yml`.
#[must_use]
pub fn preset_path(presets: &Path, kind: &str, key: &str) -> PathBuf {
    presets.join(kind).join(format!("{key}.{kind}.yml"))
}

/// The entries of `folder` that a project reads, in name order.
fn sorted_entries(folder: &Path) -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(folder)? {
        let path = entry?.path();
        if !ignored(&file_name(&path)) {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

/// Whether a project ignores files and folders named `name`: hidden ones
/// and those that operating systems and zip tools leave behind.
fn ignored(name: &str) -> bool {
    name.starts_with('.') || name == "Thumbs.db" || name == "__MACOSX"
}

/// The last part of `path`, or empty.
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}
