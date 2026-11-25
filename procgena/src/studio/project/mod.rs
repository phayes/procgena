//! The project folder: where the studio keeps the user's own presets.
//!
//! A project is a folder holding `procgena.yml` (its format, namespace id
//! and name), `presets/<kind>/<key>.<kind>.yml` files and an ignored
//! `.cache/`. Its presets join the built-in ones in a [`Registry`], where
//! they can be the base of other project presets and be used by the
//! project's Scatters and Ecotopes. When a project preset shares a
//! built-in preset's key, references name it with the project's id, as in
//! `my_forest:pine_small`, and the built-in one with `builtin:`. Saving
//! writes whole files, so comments added by hand to a file the studio
//! updates are lost.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Save and Save as new call these once documents hold their settings (ST2's \
                  document half, after ST1)"
    )
)]

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bevy::prelude::*;
use bevy::time::common_conditions::on_real_timer;
use procgena_biome::EcotopePreset;
use procgena_clump::ClumpPreset;
use procgena_core::preset::{
    Document, PresetFileError, Problem, Registry, check_namespace, preset_path,
};
use procgena_core::{CHILD_KEY_MAX_LEN, ChildKey};
use procgena_deadwood::DeadwoodPreset;
use procgena_fruit::{FlowerPreset, FruitPreset};
use procgena_mushroom::MushroomPreset;
use procgena_stone::StonePreset;
use procgena_terrain::TerrainPreset;
use procgena_tree::{TreePreset, TreeScatterPreset};
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests;

/// The project file format this studio reads and writes.
pub const FORMAT: u32 = 1;

/// The project file's name, at the project folder's root.
pub const MANIFEST: &str = "procgena.yml";

/// What `.gitignore` holds in a new project.
const GITIGNORE: &str = ".cache/\n";

/// A project's `procgena.yml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The project format; only [`FORMAT`] is read.
    pub format: u32,
    /// The project's namespace: what references write before a key to
    /// name this project's preset, as in `my_forest:tall_pine`. Lower
    /// case, digits and underscores; never `builtin`.
    pub id: String,
    /// The project's display name.
    pub name: String,
    /// The author's version of the project, in any form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// What the project holds, in a sentence or two.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The licence its presets are shared under, as an SPDX id such as
    /// `CC0-1.0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licence: Option<String>,
    /// Who made it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    /// The procgena version that last exported it as a bundle, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub made_with: Option<String>,
    /// The presets an exported bundle was made from, such as
    /// `biome/alpine`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roots: Vec<String>,
}

/// Why a project could not be opened or a preset could not be saved.
#[derive(Debug)]
pub enum ProjectError {
    /// A file or folder could not be read or written.
    Io {
        /// The file or folder.
        path: PathBuf,
        /// What the system reported.
        error: io::Error,
    },
    /// `procgena.yml` is not a project file this studio reads.
    Manifest {
        /// What is wrong with it.
        message: String,
    },
    /// The folder to make a new project in is not empty.
    NotEmpty(PathBuf),
    /// The key is not a valid preset key.
    BadKey(String),
    /// A project preset of the kind already has the key. Choose another,
    /// such as [`Project::free_key`] gives.
    KeyTaken {
        /// The kind.
        kind: String,
        /// The key.
        key: String,
    },
    /// The project has no preset of the kind with the key to update.
    NotInProject {
        /// The kind.
        kind: String,
        /// The key.
        key: String,
    },
    /// The preset's settings could not be written over its base.
    Preset(PresetFileError),
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, error } => write!(f, "{}: {error}", path.display()),
            Self::Manifest { message } => write!(f, "{MANIFEST}: {message}"),
            Self::NotEmpty(path) => write!(
                f,
                "{} is not empty; choose an empty folder for a new project",
                path.display()
            ),
            Self::BadKey(key) => write!(
                f,
                "{key:?} is not a valid key: use lower-case letters, digits and underscores, \
                 starting with a letter, at most {CHILD_KEY_MAX_LEN} bytes"
            ),
            Self::KeyTaken { kind, key } => {
                write!(f, "the project already has a {kind} preset named {key:?}")
            }
            Self::NotInProject { kind, key } => {
                write!(f, "the project has no {kind} preset {key:?}")
            }
            Self::Preset(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ProjectError {}

/// An open project: its folder, its `procgena.yml`, and its presets over
/// the built-in ones.
#[derive(Debug)]
pub struct Project {
    root: PathBuf,
    manifest: Manifest,
    registry: Registry,
    /// When each preset file was last changed, as last read.
    stamps: Vec<(PathBuf, Option<SystemTime>)>,
}

/// A registry with every kind of preset the studio can save.
fn registry() -> Registry {
    let mut registry = Registry::new();
    registry
        .register::<TreePreset>()
        .register::<TreeScatterPreset>()
        .register::<StonePreset>()
        .register::<ClumpPreset>()
        .register::<MushroomPreset>()
        .register::<FruitPreset>()
        .register::<FlowerPreset>()
        .register::<DeadwoodPreset>()
        .register::<EcotopePreset>()
        .register::<TerrainPreset>();
    registry
}

impl Project {
    /// Makes a new project in the folder `root`, which may be missing or
    /// empty, and opens it. `id` is its namespace (see [`Manifest::id`])
    /// and `name` its display name.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError::Manifest`] for an `id` that can't be a
    /// namespace, such as `builtin`; [`ProjectError::NotEmpty`] if `root`
    /// holds anything; and [`ProjectError::Io`] if the files can't be
    /// written.
    pub fn create(root: &Path, id: &str, name: &str) -> Result<Self, ProjectError> {
        let io = |path: &Path| {
            let path = path.to_owned();
            move |error| ProjectError::Io { path, error }
        };
        check_namespace(id).map_err(|error| ProjectError::Manifest {
            message: format!("id: {error}"),
        })?;

        // An empty folder, or none yet.
        if root.exists() && std::fs::read_dir(root).map_err(io(root))?.next().is_some() {
            return Err(ProjectError::NotEmpty(root.to_owned()));
        }

        // The project file, the presets folder and what git ignores.
        let manifest = Manifest {
            format: FORMAT,
            id: id.to_owned(),
            name: name.to_owned(),
            version: None,
            description: None,
            licence: None,
            authors: Vec::new(),
            made_with: None,
            roots: Vec::new(),
        };
        let presets = root.join("presets");
        std::fs::create_dir_all(&presets).map_err(io(&presets))?;
        let text = serde_saphyr::to_string(&manifest).expect("a manifest always writes as YAML");
        write_file(&root.join(MANIFEST), &text)?;
        write_file(&root.join(".gitignore"), GITIGNORE)?;
        Self::open(root)
    }

    /// Opens the project in the folder `root`.
    ///
    /// Preset files that can't be read are listed in
    /// [`problems`](Self::problems); the rest of the project opens.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError::Io`] if `procgena.yml` or the presets folder
    /// can't be read, and [`ProjectError::Manifest`] if `procgena.yml` isn't
    /// a project file of [`FORMAT`] or its `id` can't be a namespace, such
    /// as `builtin`.
    pub fn open(root: &Path) -> Result<Self, ProjectError> {
        let path = root.join(MANIFEST);
        let text = std::fs::read_to_string(&path).map_err(|error| ProjectError::Io {
            path: path.clone(),
            error,
        })?;
        let manifest: Manifest =
            serde_saphyr::from_str(&text).map_err(|error| ProjectError::Manifest {
                message: error.to_string(),
            })?;
        if manifest.format != FORMAT {
            return Err(ProjectError::Manifest {
                message: format!(
                    "format {} is not one this studio reads; it reads format {FORMAT}",
                    manifest.format
                ),
            });
        }
        let mut registry = registry();
        registry
            .set_namespace(&manifest.id)
            .map_err(|error| ProjectError::Manifest {
                message: format!("id: {error}"),
            })?;
        let mut project = Self {
            root: root.to_owned(),
            manifest,
            registry,
            stamps: Vec::new(),
        };
        project.reload()?;
        Ok(project)
    }

    /// Opens the project in `root`, or makes one there with `id` and `name`
    /// if the folder has no `procgena.yml`.
    ///
    /// # Errors
    ///
    /// As for [`create`](Self::create) and [`open`](Self::open).
    pub fn open_or_create(root: &Path, id: &str, name: &str) -> Result<Self, ProjectError> {
        if root.join(MANIFEST).exists() {
            Self::open(root)
        } else {
            Self::create(root, id, name)
        }
    }

    /// The project's folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The project's `procgena.yml`.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The project's `presets` folder.
    pub fn presets(&self) -> PathBuf {
        self.root.join("presets")
    }

    /// The project's presets over the built-in ones.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// What is wrong with the project's presets: files that can't be read
    /// or are misplaced, and keys a newer built-in preset now has too.
    pub fn problems(&self) -> Vec<Problem> {
        self.registry.problems()
    }

    /// Reads the presets folder again.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError::Io`] if the presets folder can't be read.
    pub fn reload(&mut self) -> Result<(), ProjectError> {
        let presets = self.presets();
        self.registry
            .load_folder(&presets)
            .map_err(|error| ProjectError::Io {
                path: presets.clone(),
                error,
            })?;
        self.stamps = stamps(&presets);
        Ok(())
    }

    /// Whether a preset file was added, removed or changed since the
    /// project last read or wrote its presets, such as by a text editor or
    /// `git pull`.
    pub fn changed_on_disk(&self) -> bool {
        stamps(&self.presets()) != self.stamps
    }

    /// Whether `key` is a valid preset key that no preset of kind `kind`
    /// has, in the project or built in, so a bare reference to it stays
    /// unambiguous.
    pub fn is_free(&self, kind: &str, key: &str) -> bool {
        ChildKey::new(key).is_ok()
            && self
                .registry
                .entries(kind)
                .iter()
                .all(|entry| entry.key != key)
    }

    /// The first free key of kind `kind` made from `stem`: `stem` itself,
    /// then `stem_2`, `stem_3` and so on, shortened to fit a key's length.
    /// A stem that can't start a key, such as an empty one, gives keys
    /// made from `preset`.
    pub fn free_key(&self, kind: &str, stem: &str) -> String {
        let stem = if ChildKey::new(stem).is_ok() {
            stem
        } else {
            "preset"
        };
        if self.is_free(kind, stem) {
            return stem.to_owned();
        }
        (2..)
            .map(|n| {
                let suffix = format!("_{n}");
                let keep = stem.len().min(CHILD_KEY_MAX_LEN - suffix.len());
                format!("{}{suffix}", stem[..keep].trim_end_matches('_'))
            })
            .find(|key| self.is_free(kind, key))
            .expect("some numbered key is free")
    }

    /// Saves a new project preset of kind `kind` named `key`, holding
    /// `document`, whose settings are the preset's full settings, and
    /// returns its file.
    ///
    /// The file holds only what differs from the document's `base`. The key
    /// may be a built-in preset's; bare references to it then become
    /// ambiguous, and [`problems`](Self::problems) lists it.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError::BadKey`] for a key that isn't valid,
    /// [`ProjectError::KeyTaken`] for a project preset's key,
    /// [`ProjectError::Preset`] if the base can't be found, and
    /// [`ProjectError::Io`] if the file can't be written.
    pub fn save_new(
        &mut self,
        kind: &str,
        key: &str,
        document: &Document,
    ) -> Result<PathBuf, ProjectError> {
        if ChildKey::new(key).is_err() {
            return Err(ProjectError::BadKey(key.to_owned()));
        }
        if self.registry.project_preset(kind, key).is_some() {
            return Err(ProjectError::KeyTaken {
                kind: kind.to_owned(),
                key: key.to_owned(),
            });
        }
        self.write(kind, key, document)
    }

    /// Saves the project preset `key` of kind `kind` again, holding
    /// `document`, whose settings are the preset's full settings, and
    /// returns its file. The file is written whole: comments and layout it
    /// had are lost.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError::NotInProject`] if the project has no such
    /// preset, [`ProjectError::Preset`] if the base can't be found, and
    /// [`ProjectError::Io`] if the file can't be written.
    pub fn update(
        &mut self,
        kind: &str,
        key: &str,
        document: &Document,
    ) -> Result<PathBuf, ProjectError> {
        if self.registry.project_preset(kind, key).is_none() {
            return Err(ProjectError::NotInProject {
                kind: kind.to_owned(),
                key: key.to_owned(),
            });
        }
        self.write(kind, key, document)
    }

    /// Writes the file of the preset `key` and adds it to the registry.
    fn write(
        &mut self,
        kind: &str,
        key: &str,
        document: &Document,
    ) -> Result<PathBuf, ProjectError> {
        let text = self
            .registry
            .file_text(kind, key, document)
            .map_err(ProjectError::Preset)?;
        let path = preset_path(&self.presets(), kind, key);
        write_file(&path, &text)?;
        self.registry
            .add(kind, key, &text, Some(path.clone()))
            .expect("a file the registry wrote reads back");
        self.stamps = stamps(&self.presets());
        Ok(path)
    }
}

/// The folder of the project the studio opens at start, making a project
/// there if it has none. Without this resource the studio opens no
/// project, as in headless runs.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct ProjectFolder(pub PathBuf);

impl ProjectFolder {
    /// The default project's folder: `procgena/default` in the user's
    /// Documents folder, or in the home folder if there is none. `None`
    /// if neither is known.
    pub fn default_project() -> Option<Self> {
        let documents = dirs::document_dir().or_else(dirs::home_dir)?;
        Some(Self(documents.join("procgena").join(DEFAULT_ID)))
    }
}

/// The id and name of the project made in the default folder.
const DEFAULT_ID: &str = "default";
const DEFAULT_NAME: &str = "Default";

/// Every preset the studio can open: the open project's, if any, over the
/// built-in ones.
#[derive(Resource, Debug)]
pub struct Presets {
    project: Option<Project>,
    built_in: Registry,
}

impl Default for Presets {
    fn default() -> Self {
        Self {
            project: None,
            built_in: registry(),
        }
    }
}

impl Presets {
    /// The open project, if any.
    pub fn project(&self) -> Option<&Project> {
        self.project.as_ref()
    }

    /// The open project, if any, to save into.
    pub fn project_mut(&mut self) -> Option<&mut Project> {
        self.project.as_mut()
    }

    /// Opens `project`, closing any other.
    pub fn open(&mut self, project: Project) {
        self.project = Some(project);
    }

    /// The registry to look presets up in: the open project's, or the
    /// built-in presets alone.
    pub fn registry(&self) -> &Registry {
        self.project
            .as_ref()
            .map_or(&self.built_in, Project::registry)
    }
}

/// How often the studio looks for preset files changed outside it.
const RELOAD_EVERY: Duration = Duration::from_secs(1);

/// Keeps [`Presets`]: opens the project in [`ProjectFolder`] at start, and
/// reloads its presets when their files change outside the studio.
pub struct ProjectPlugin;

impl Plugin for ProjectPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Presets>()
            .add_systems(Startup, open_project)
            .add_systems(Update, reload_changed.run_if(on_real_timer(RELOAD_EVERY)));
    }
}

/// Opens or makes the project in [`ProjectFolder`], if one is given.
fn open_project(folder: Option<Res<ProjectFolder>>, mut presets: ResMut<Presets>) {
    let Some(folder) = folder else {
        return;
    };
    match Project::open_or_create(&folder.0, DEFAULT_ID, DEFAULT_NAME) {
        Ok(project) => {
            for problem in project.problems() {
                warn!("{problem}");
            }
            presets.open(project);
        }
        Err(error) => error!("could not open the project: {error}"),
    }
}

/// Reloads the open project's presets when a file changed on disk.
fn reload_changed(mut presets: ResMut<Presets>) {
    if !presets.project().is_some_and(Project::changed_on_disk) {
        return;
    }
    let Some(project) = presets.project_mut() else {
        return;
    };
    match project.reload() {
        Ok(()) => {
            for problem in project.problems() {
                warn!("{problem}");
            }
        }
        Err(error) => error!("could not reload the project: {error}"),
    }
}

/// Writes `text` to `path`, making its folder if needed, through a
/// temporary file so a failed write never leaves half a file.
fn write_file(path: &Path, text: &str) -> Result<(), ProjectError> {
    let io = |error| ProjectError::Io {
        path: path.to_owned(),
        error,
    };
    let parent = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(io)?;

    // A hidden name, which the presets folder's reader ignores.
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temporary = parent.join(format!(".{name}.saving"));
    std::fs::write(&temporary, text).map_err(io)?;
    std::fs::rename(&temporary, path).map_err(io)
}

/// Every file under `presets` two levels down, with when it last changed,
/// in path order.
fn stamps(presets: &Path) -> Vec<(PathBuf, Option<SystemTime>)> {
    let list = |folder: &Path| -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .collect();
        paths.sort();
        paths
    };
    let mut stamps = Vec::new();
    for folder in list(presets) {
        for path in list(&folder) {
            let modified = std::fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .ok();
            stamps.push((path, modified));
        }
    }
    stamps
}
