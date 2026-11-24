//! Saves and loads settings files and exports tree or rock models for other 3D tools.
//!
//! [`FileOps`] asks the user for a file with the system's file dialog, then
//! does the work in the background and reports the outcome as a toast.

pub mod deadwood;
pub mod images;
pub mod mushroom;
pub mod stone;
pub mod stone_panel;
pub mod tree;
pub mod tree_panel;

#[cfg(any(test, debug_assertions))]
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use bevy::winit::{EventLoopProxyWrapper, WinitUserEvent};
use procgena_core::preset::{
    FILE_GROUP, Preset, PresetMeta, read as read_preset_file, write as write_preset,
};

use crate::ui::toast::{ShowToast, ToastKind};

/// Runs file operations started with [`FileOps`] and reports their outcomes.
pub struct FileOpsPlugin;

impl Plugin for FileOpsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LastFolder>()
            .init_resource::<FileDialogs>()
            .insert_resource(ExportImages {
                tree: images::ImageStore::new(images::TREE_IMAGES, "tree").map(Arc::new),
                stone: images::ImageStore::new(images::STONE_IMAGES, "stone").map(Arc::new),
            })
            .add_plugins((
                tree_panel::TreeExportPlugin,
                stone_panel::StoneExportPlugin,
                mushroom::MushroomExportPlugin,
                deadwood::DeadwoodExportPlugin,
            ))
            .add_systems(Update, finish_file_ops);
    }
}

/// Where exported models get their texture images. Each store is `None`
/// when the system has no cache folder; exports then leave textures out.
#[derive(Resource)]
pub struct ExportImages {
    /// Bark and leaf images.
    pub tree: Option<Arc<images::ImageStore>>,
    /// Rock images.
    pub stone: Option<Arc<images::ImageStore>>,
}

/// The folder of the last file chosen in a file dialog, where the next dialog starts.
#[derive(Resource, Default)]
pub struct LastFolder(Option<PathBuf>);

/// Where file dialogs get their answers.
#[derive(Resource, Default)]
pub enum FileDialogs {
    /// The system's file dialogs.
    #[default]
    Native,
    /// Answers given in advance, in order: a path, or `None` to cancel. Dialogs
    /// asked once the list is empty are cancelled.
    #[cfg(any(test, debug_assertions))]
    Scripted(Arc<Mutex<VecDeque<Option<PathBuf>>>>),
}

impl FileDialogs {
    /// The scripted answer for the next dialog, or `None` to show a real one.
    fn scripted_answer(&self) -> Option<Option<PathBuf>> {
        match self {
            Self::Native => None,
            #[cfg(any(test, debug_assertions))]
            Self::Scripted(answers) => Some(
                answers
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .pop_front()
                    .flatten(),
            ),
        }
    }
}

/// Reads a chosen file's text into the app. Returns what to tell the user.
type ApplyFn = Box<dyn FnOnce(&Path, &str, &mut World) -> Result<Done, String> + Send>;

/// How a file operation ended.
enum Outcome {
    /// The user cancelled the dialog.
    Cancelled,
    /// A file was written, or writing it failed.
    Saved(PathBuf, Result<Done, String>),
    /// A file was read, or reading it failed.
    Read(PathBuf, Result<String, String>),
}

/// A file operation still waiting for its dialog or its background work.
#[derive(Component)]
struct PendingFileOp {
    task: Task<Outcome>,
    /// What to do with the text of a file being opened.
    apply: Mutex<Option<ApplyFn>>,
}

/// Starts saving and opening files.
#[derive(SystemParam)]
pub struct FileOps<'w, 's> {
    commands: Commands<'w, 's>,
    last: Res<'w, LastFolder>,
    dialogs: Res<'w, FileDialogs>,
    wake: Option<Res<'w, EventLoopProxyWrapper>>,
}

impl FileOps<'_, '_> {
    /// Asks where to save, suggesting the file name `name`, then calls `write`
    /// with the chosen path in the background and shows its result.
    /// Cancelling the dialog leaves files untouched.
    pub fn save(
        &mut self,
        name: &str,
        write: impl FnOnce(&Path) -> Result<Done, String> + Send + 'static,
    ) {
        let scripted = self.dialogs.scripted_answer();
        let dialog = rfd::AsyncFileDialog::new()
            .set_directory(start_folder(self.last.0.as_deref()))
            .set_file_name(name);
        let wake = self.waker();
        let task = AsyncComputeTaskPool::get().spawn(async move {
            let chosen = match scripted {
                Some(answer) => answer,
                None => dialog.save_file().await.map(|file| file.path().to_owned()),
            };
            let outcome = match chosen {
                Some(path) => {
                    let result = write(&path);
                    Outcome::Saved(path, result)
                }
                None => Outcome::Cancelled,
            };
            wake();
            outcome
        });
        self.commands.spawn(PendingFileOp {
            task,
            apply: Mutex::new(None),
        });
    }

    /// Asks for a file to open, reads it as text in the background, then
    /// passes the text to `apply` and shows its result. Cancelling the dialog
    /// changes nothing.
    pub fn open(
        &mut self,
        apply: impl FnOnce(&str, &mut World) -> Result<Done, String> + Send + 'static,
    ) {
        self.open_file(move |_, text, world| apply(text, world));
    }

    /// Like [`open`](Self::open), but also passes `apply` the path of the
    /// file that was read.
    pub fn open_file(
        &mut self,
        apply: impl FnOnce(&Path, &str, &mut World) -> Result<Done, String> + Send + 'static,
    ) {
        let scripted = self.dialogs.scripted_answer();
        let dialog =
            rfd::AsyncFileDialog::new().set_directory(start_folder(self.last.0.as_deref()));
        let wake = self.waker();
        let task = AsyncComputeTaskPool::get().spawn(async move {
            let chosen = match scripted {
                Some(answer) => answer,
                None => dialog.pick_file().await.map(|file| file.path().to_owned()),
            };
            let outcome = match chosen {
                Some(path) => {
                    let text = std::fs::read_to_string(&path)
                        .map_err(|error| format!("Cannot read {}: {error}", path.display()));
                    Outcome::Read(path, text)
                }
                None => Outcome::Cancelled,
            };
            wake();
            outcome
        });
        self.commands.spawn(PendingFileOp {
            task,
            apply: Mutex::new(Some(Box::new(apply))),
        });
    }

    /// A function that wakes the window, which sleeps while idle, so a
    /// finished operation is reported straight away.
    fn waker(&self) -> impl FnOnce() + Send + 'static {
        let proxy = self.wake.as_ref().map(|proxy| (***proxy).clone());
        move || {
            if let Some(proxy) = proxy {
                let _ = proxy.send_event(WinitUserEvent::WakeUp);
            }
        }
    }
}

/// Whether any file operation is still waiting for its dialog or work.
#[cfg(any(test, debug_assertions))]
pub fn file_ops_pending(world: &mut World) -> bool {
    world.query::<&PendingFileOp>().iter(world).next().is_some()
}

/// Finishes file operations whose work is done: remembers the folder,
/// applies opened files, and shows the outcome.
fn finish_file_ops(
    mut ops: Query<(Entity, &mut PendingFileOp)>,
    mut last: ResMut<LastFolder>,
    mut commands: Commands,
) {
    for (entity, mut op) in &mut ops {
        let Some(outcome) = block_on(poll_once(&mut op.task)) else {
            continue;
        };
        commands.entity(entity).despawn();
        let (path, result) = match outcome {
            Outcome::Cancelled => continue,
            Outcome::Saved(path, result) => (path, Ok(result)),
            Outcome::Read(path, text) => (path, Err(text)),
        };
        last.0 = path.parent().map(Path::to_owned);
        match result {
            Ok(result) => commands.trigger(toast(result)),
            Err(text) => {
                // Apply the text with access to the whole app, then report.
                let apply = op
                    .apply
                    .get_mut()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                commands.queue(move |world: &mut World| {
                    let result = match (text, apply) {
                        (Ok(text), Some(apply)) => apply(&path, &text, world),
                        (Ok(_), None) => return,
                        (Err(error), _) => Err(error),
                    };
                    world.trigger(toast(result));
                });
            }
        }
    }
}

/// The toast reporting a save or load: warnings and errors stay until closed.
fn toast(result: Result<Done, String>) -> ShowToast {
    match result {
        Ok(done) if done.warnings.is_empty() => ShowToast {
            kind: ToastKind::Success,
            message: done.message,
        },
        Ok(done) => ShowToast {
            kind: ToastKind::Warning,
            message: format!("{}\n{}", done.message, done.warnings.join("\n")),
        },
        Err(error) => ShowToast {
            kind: ToastKind::Error,
            message: error,
        },
    }
}

/// Chooses a dialog's starting folder: `last` (the folder of the last file
/// chosen), Documents, or the home folder, whichever is available.
pub fn start_folder(last: Option<&Path>) -> PathBuf {
    last.map(Path::to_owned)
        .or_else(dirs::document_dir)
        .or_else(dirs::home_dir)
        .unwrap_or_default()
}

/// A completed save or load, including any limitations the user should know about.
pub struct Done {
    /// A brief description of what was saved or loaded.
    pub message: String,
    /// Warnings about missing content or other limitations, one message per entry.
    pub warnings: Vec<String>,
}

impl Done {
    /// Creates a successful result with no warnings.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            warnings: Vec::new(),
        }
    }
}

/// Returns a file name such as `oak.glb` for messages, or the full path if it has
/// no file name.
pub fn shown_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Saves the supplied bytes and, on failure, reports which path could not be written.
pub fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|error| format!("Cannot save {}: {error}", path.display()))
}

/// Writes `files` beside `chosen`, creating subfolders as needed. Supply file
/// names relative to that folder; `chosen` determines the folder, not the names.
/// Existing files are overwritten. Returns the first directory or write error;
/// files already written remain on disk.
pub fn write_files(chosen: &Path, files: &[(String, Vec<u8>)]) -> Result<(), String> {
    let folder = chosen.parent().unwrap_or(Path::new("."));
    for (name, bytes) in files {
        let file = folder.join(name);
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|error| format!("Cannot make the folder {}: {error}", dir.display()))?;
        }
        write_file(&file, bytes)?;
    }
    Ok(())
}

/// Returns the chosen file name without its final extension or parent folders.
/// Uses `suggested` if the path has no file stem, and replaces invalid text
/// characters with the Unicode replacement character.
pub fn chosen_stem(chosen: &Path, suggested: &str) -> String {
    chosen.file_stem().map_or_else(
        || suggested.to_owned(),
        |s| s.to_string_lossy().into_owned(),
    )
}

/// The key a preset file at `path` loads as: its file name without the
/// `.<kind>.yml` ending, such as `mossy` for `mossy.stone.yml`. A file
/// without that ending gives its name without the last extension.
pub fn preset_key<P: Preset>(path: &Path) -> String {
    file_key(path, P::KIND)
}

/// The key a preset file of `kind` at `path` loads as: its file name without
/// the `.<kind>.yml` ending, such as `meadow` for `meadow.plant_scatter.yml`
/// with kind `plant_scatter`. A file without that ending gives its name
/// without the last extension.
pub fn file_key(path: &Path, kind: &str) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ending = format!(".{kind}.yml");
    match name.strip_suffix(&ending) {
        Some(key) => key.to_owned(),
        None => chosen_stem(path, &name),
    }
}

/// Writes `options` to `path` as a `.<kind>.yml` preset file that names
/// `base` as its base and holds only the settings that differ from it. The
/// file's name is its key, as [`preset_key`] reads it.
pub fn save_preset<P: Preset>(
    options: P::Options,
    base: &'static P,
    path: &Path,
) -> Result<Done, String> {
    let key = preset_key::<P>(path);
    let meta = PresetMeta {
        key: key.clone(),
        name: key,
        description: String::new(),
        group: FILE_GROUP.to_owned(),
    };
    let preset = P::from_parts(meta, options, Some(base));
    write_file(path, write_preset(&preset, Some(base)).as_bytes())?;
    Ok(Done::new(format!("Saved {}", shown_name(path))))
}

/// Reads the options of a `.<kind>.yml` preset file's text, checked by the
/// preset type. `what` names the settings in the error, such as `"rock"`.
pub fn read_preset<P: Preset>(text: &str, what: &str) -> Result<P::Options, String> {
    read_preset_file::<P>("file", text)
        .map(|preset| preset.into_options())
        .map_err(|error| format!("Not {what} settings: {error}"))
}

#[cfg(test)]
mod tests {
    //! These start the whole studio on the GPU, so they are ignored by default.
    //! File dialogs are answered in advance, and texture downloads are off.

    use crate::scene::{OrbitCamera, ViewportCamera};
    use bevy::math::Vec2;
    use procgena_plant_bevy::prelude::{PlantParams, PlantScatterOptions, ScatterPlant};
    use procgena_stone::StoneOptions;
    use procgena_tree::TreeOptions;

    use super::*;
    use crate::field::editor::FieldReader;
    use crate::headless::{DEFAULT_SIZE, HeadlessStudio};

    /// An empty folder for `test`'s files.
    fn folder(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("procgena-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The studio, exporting without texture images so nothing is downloaded.
    fn offline_studio() -> HeadlessStudio {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        studio.app_mut().insert_resource(ExportImages {
            tree: None,
            stone: None,
        });
        studio
    }

    /// The field being edited.
    fn field(studio: &mut HeadlessStudio) -> PlantScatterOptions {
        let world = studio.app_mut().world_mut();
        world
            .run_system_cached(|reader: FieldReader| reader.field())
            .unwrap()
            .unwrap()
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn tree_settings_save_load_and_export() {
        let dir = folder("tree");
        let mut studio = offline_studio();
        let saved: TreeOptions = studio.panel_value();

        // Saving writes the settings; loading them undoes later edits.
        studio.answer_dialog(Some(dir.join("tree.tree.yml")));
        studio.click("Save Preset…").unwrap();
        assert!(studio.find("Saved tree.tree.yml").is_ok());
        studio.click("Jiggle").unwrap();
        assert_ne!(studio.panel_value::<TreeOptions>(), saved);
        studio.answer_dialog(Some(dir.join("tree.tree.yml")));
        studio.click("Load Preset…").unwrap();
        assert_eq!(studio.panel_value::<TreeOptions>(), saved);
        assert!(studio.find("Loaded the tree settings").is_ok());

        // Cancelling writes nothing; a file that is not tree settings changes nothing.
        studio.answer_dialog(None);
        studio.click("Save Preset…").unwrap();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::write(dir.join("bad.tree.yml"), "not a preset").unwrap();
        studio.answer_dialog(Some(dir.join("bad.tree.yml")));
        studio.click("Load Preset…").unwrap();
        assert!(studio.shows_text_starting("Not tree settings"));
        assert_eq!(studio.panel_value::<TreeOptions>(), saved);

        // Both model formats are written, the OBJ with its material file.
        studio.answer_dialog(Some(dir.join("tree.glb")));
        studio.click("Export GLB…").unwrap();
        assert!(std::fs::metadata(dir.join("tree.glb")).unwrap().len() > 0);
        assert!(studio.shows_text_starting("Saved tree.glb"));
        studio.answer_dialog(Some(dir.join("tree.obj")));
        studio.click("Export OBJ…").unwrap();
        assert!(dir.join("tree.obj").exists() && dir.join("tree.mtl").exists());
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn loading_rock_settings_rebuilds_the_rock_and_refits_the_camera() {
        let dir = folder("stone");
        let mut studio = offline_studio();
        studio.click("Stones").unwrap();
        studio.click("Boulder").unwrap();
        let saved: StoneOptions = studio.panel_value();
        studio.answer_dialog(Some(dir.join("rock.stone.yml")));
        studio.click("Save Preset…").unwrap();
        studio.click("Random Seed").unwrap();
        studio.drag(Vec2::new(500.0, 400.0), Vec2::new(800.0, 300.0));

        studio.answer_dialog(Some(dir.join("rock.stone.yml")));
        studio.click("Load Preset…").unwrap();
        assert_eq!(studio.panel_value::<StoneOptions>(), saved);
        let camera = |studio: &mut HeadlessStudio| {
            let world = studio.app_mut().world_mut();
            let mut cameras = world.query_filtered::<&OrbitCamera, With<ViewportCamera>>();
            *cameras.single(world).unwrap()
        };
        let loaded = camera(&mut studio);
        studio.click("Reset Camera").unwrap();
        assert_eq!(camera(&mut studio), loaded);

        studio.answer_dialog(Some(dir.join("rock.glb")));
        studio.click("Export GLB…").unwrap();
        assert!(dir.join("rock.glb").exists());
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn plants_and_fields_save_and_load() {
        let dir = folder("plants");
        let mut studio = offline_studio();

        // A plant comes back as saved, loaded into another plant's document.
        studio.click("Plant").unwrap();
        studio.click("Meadow grass").unwrap();
        let plant: PlantParams = studio.panel_value();
        studio.answer_dialog(Some(dir.join("grass.plant.yml")));
        studio.click("Save Preset…").unwrap();
        studio.click("Wheat").unwrap();
        assert_ne!(studio.panel_value::<PlantParams>(), plant);
        studio.answer_dialog(Some(dir.join("grass.plant.yml")));
        studio.click("Load Preset…").unwrap();
        assert_eq!(studio.panel_value::<PlantParams>(), plant);

        // A Plant Scatter comes back with all its rows, loaded into another
        // Plant Scatter's document.
        studio.click("Plant Scatter").unwrap();
        studio.click("Meadow").unwrap();
        studio.click("+ Add Plant").unwrap();
        let saved = field(&mut studio);
        assert_eq!(saved.plants.len(), 2);
        studio.answer_dialog(Some(dir.join("field.plant_scatter.yml")));
        studio.click("Save Preset…").unwrap();
        studio.click("Lawn").unwrap();
        assert_ne!(field(&mut studio), saved);
        studio.answer_dialog(Some(dir.join("field.plant_scatter.yml")));
        studio.click("Load Preset…").unwrap();
        assert_eq!(field(&mut studio), saved);

        // A row's plant can be loaded from a plant file.
        let first_edit = studio
            .texts()
            .into_iter()
            .filter(|shown| shown.text == "Edit")
            .min_by(|a, b| a.center.y.total_cmp(&b.center.y))
            .unwrap();
        studio.click_at(first_edit.center);
        studio.answer_dialog(Some(dir.join("grass.plant.yml")));
        studio.click("Load Plant…").unwrap();
        assert_eq!(
            field(&mut studio).plants[0],
            ScatterPlant {
                plant,
                ..saved.plants[0]
            }
        );
    }
}
