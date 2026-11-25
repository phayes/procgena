//! Documents: the presets open in the studio, and which one is selected.
//!
//! A [`Document`] is one open preset. Its editor in the Inspector shows its
//! settings; the document names the preset, keeps its settings with their
//! undo history ([`DocumentSettings`]), and keeps what the view frame shows
//! for it. [`Selection`] says which document the Inspector, the view and
//! the Outline show. Open one with [`OpenDocument`], close it with
//! [`CloseDocument`], and select one by setting [`Selection::document`].

#[cfg(test)]
mod editing_tests;
pub mod history;
pub mod panels;
pub mod pickers;
pub mod save;

use bevy::prelude::*;
use procgena_core::preset::Layer;

pub use history::DocumentSettings;

use super::kinds::Kinds;
use super::project::Presets;
use crate::scene::{Detail, OrbitCamera, ViewerCommand, ViewportCamera};

/// Where an open preset comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// One of the studio's built-in presets.
    BuiltIn,
    /// A preset file in the open project.
    Project,
}

impl Origin {
    /// How the Inspector names it, such as `built-in`.
    pub fn label(self) -> &'static str {
        match self {
            Self::BuiltIn => "built-in",
            Self::Project => "project",
        }
    }

    /// The registry layer of presets from here.
    pub fn layer(self) -> Layer {
        match self {
            Self::BuiltIn => Layer::BuiltIn,
            Self::Project => Layer::Project,
        }
    }
}

/// Which detail levels the view shows for a document, as the Detail
/// control offers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DetailChoice {
    /// What a game draws: levels chosen by distance, never finer than
    /// level 1 near the camera.
    #[default]
    GameDefault,
    /// Levels chosen by distance, with full detail, level 0, near the
    /// camera.
    Level0Preview,
    /// Only level `n` (0 is the most detailed), at every distance.
    Level(usize),
}

impl DetailChoice {
    /// The scene's detail for this choice.
    pub fn detail(self) -> Detail {
        match self {
            Self::GameDefault => Detail::ByDistance { finest: 1 },
            Self::Level0Preview => Detail::ByDistance { finest: 0 },
            Self::Level(n) => Detail::Level(n),
        }
    }

    /// The choice at `index` in the Detail control's list: the game
    /// default, the level 0 preview, then each level.
    pub fn at(index: usize) -> Self {
        match index {
            0 => Self::GameDefault,
            1 => Self::Level0Preview,
            n => Self::Level(n - 2),
        }
    }

    /// This choice's index in the Detail control's list; see [`at`](Self::at).
    pub fn index(self) -> usize {
        match self {
            Self::GameDefault => 0,
            Self::Level0Preview => 1,
            Self::Level(n) => n + 2,
        }
    }
}

/// One open preset.
#[derive(Component, Debug, Clone)]
pub struct Document {
    /// The id of its [`Kind`](super::kinds::Kind).
    pub kind: &'static str,
    /// The preset's key.
    pub key: String,
    /// The preset's display name.
    pub name: String,
    /// Where the preset comes from.
    pub origin: Origin,
    /// The Inspector page holding its editor.
    pub page: Entity,
    /// Its editor, which holds its settings.
    pub editor: Entity,
    /// The camera view last used on it, kept while another document is
    /// selected.
    pub view: Option<OrbitCamera>,
    /// The detail levels the view shows for it.
    pub detail: DetailChoice,
    /// Counts up as documents are opened: later documents have larger
    /// numbers.
    pub opened: u64,
}

/// How many documents have been opened.
#[derive(Resource, Debug, Default)]
struct OpenedCount(u64);

/// What is selected: a document and, within it, a node and a part.
///
/// The Inspector, the view and the Outline follow it. `node` and `part`
/// narrow the selection inside the document; nothing sets them yet.
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
pub struct Selection {
    /// The selected document, or `None` with none open.
    pub document: Option<Entity>,
    /// Child keys and list items leading to a node inside the document,
    /// such as `["scatters", "pines"]`. Empty for the document itself.
    pub node: Vec<String>,
    /// The part of the node selected, such as `"branch"`, or `None` for
    /// all of it.
    pub part: Option<String>,
}

/// Marks the editors of documents that aren't selected. Editor systems
/// skip editors with it. Their Inspector pages are hidden as well, so
/// anything else on a page can be told apart by its
/// [`InheritedVisibility`].
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct Dormant;

/// Marks the node that holds every document's Inspector page.
#[derive(Component, Default, Clone)]
pub struct InspectorPages;

/// Marks one document's Inspector page.
#[derive(Component, Default, Clone)]
pub struct InspectorPage;

/// Opens the preset `key` of kind `kind` and selects it. A preset already
/// open is selected instead of opened again.
#[derive(Event, Debug, Clone)]
pub struct OpenDocument {
    /// The kind's id, such as `"tree"`.
    pub kind: &'static str,
    /// The preset's key, such as `"oak_medium"`, or a namespace and a key,
    /// such as `builtin:oak_medium`, when the project has a preset with the
    /// same key.
    pub key: String,
}

/// Closes the document `0`. Closing the selected document selects the
/// one opened last before it, if any.
#[derive(Event, Debug, Clone, Copy)]
pub struct CloseDocument(pub Entity);

/// Triggered by an editor after its settings change. The selected
/// document is sent to the scene again, and with `reframe`, the camera is
/// fitted to it.
#[derive(Event, Debug, Clone, Copy, Default)]
pub struct DocumentEdited {
    /// Whether to fit the camera to the edited subject.
    pub reframe: bool,
}

/// Opens and closes documents, and keeps the view and Inspector on the
/// selected one.
pub struct DocumentsPlugin;

impl Plugin for DocumentsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((history::HistoryPlugin, save::SavePlugin))
            .init_resource::<Presets>()
            .init_resource::<Selection>()
            .init_resource::<Unsent>()
            .init_resource::<OpenedCount>()
            .add_observer(open_document)
            .add_observer(close_document)
            .add_observer(send_edited)
            .add_systems(Update, (apply_selection, send_selected).chain());
    }
}

/// The preset an [`OpenDocument`] names, as found.
struct Found {
    origin: Origin,
    key: String,
    name: String,
    /// The built-in preset the editor starts from.
    start: String,
    /// What the document's settings start with once the editor is ready.
    pending: history::PendingLoad,
}

/// Checks that [`OpenDocument`] can open the preset `reference` of `kind`.
///
/// # Errors
///
/// Says why not: no such preset, a key two layers share written without
/// its namespace, or a project preset of a kind that can't open them yet.
pub fn can_open(
    kind: &super::kinds::Kind,
    reference: &str,
    presets: &Presets,
) -> Result<(), String> {
    find_preset(kind, reference, presets).map(|_| ())
}

/// Finds the preset `reference` of `kind`: in the registry for kinds it
/// holds, and among the kind's built-in presets otherwise.
fn find_preset(
    kind: &super::kinds::Kind,
    reference: &str,
    presets: &Presets,
) -> Result<Found, String> {
    let registry = presets.registry();

    // Kinds the registry doesn't hold open only their built-in presets.
    if !registry.kinds().any(|id| id == kind.id) {
        let entry = (kind.presets)()
            .into_iter()
            .find(|preset| preset.key == reference)
            .ok_or_else(|| format!("no {} preset {reference}", kind.id))?;
        return Ok(Found {
            origin: Origin::BuiltIn,
            start: entry.key.clone(),
            key: entry.key,
            name: entry.name,
            pending: history::PendingLoad::default(),
        });
    }

    // The preset, its name, and its settings in file form.
    let (layer, key) = registry
        .find(kind.id, reference)
        .map_err(|error| error.to_string())?;
    let name = registry
        .entries(kind.id)
        .into_iter()
        .find(|entry| entry.layer == layer && entry.key == key)
        .map_or_else(|| key.clone(), |entry| entry.name);
    let file = registry
        .settings(kind.id, reference)
        .map_err(|error| error.to_string())?;

    // A built-in preset's editor starts with it; a project preset's starts
    // from its built-in root and then shows its settings.
    if layer == Layer::BuiltIn {
        return Ok(Found {
            origin: Origin::BuiltIn,
            start: key.clone(),
            key,
            name,
            pending: history::PendingLoad {
                load: None,
                file: Some(file),
            },
        });
    }
    let Some(settings) = kind.settings else {
        return Err(format!(
            "{} presets from the project can't be opened yet",
            kind.label
        ));
    };
    let load = (settings.resolve)(registry, reference)?;
    let start = registry
        .builtin_root(kind.id, reference)
        .unwrap_or_else(|| kind.start.to_owned());
    Ok(Found {
        origin: Origin::Project,
        key,
        name,
        start,
        pending: history::PendingLoad {
            load: Some(load),
            file: Some(file),
        },
    })
}

/// Opens a preset, or selects it if it is open already.
#[allow(clippy::too_many_arguments)]
fn open_document(
    open: On<OpenDocument>,
    kinds: Res<Kinds>,
    presets: Res<Presets>,
    documents: Query<(Entity, &Document)>,
    pages: Single<Entity, With<InspectorPages>>,
    mut selection: ResMut<Selection>,
    mut opened: ResMut<OpenedCount>,
    mut commands: Commands,
) {
    let Some(kind) = kinds.get(open.kind) else {
        error!("no kind {} to open {} with", open.kind, open.key);
        return;
    };
    let found = match find_preset(kind, &open.key, &presets) {
        Ok(found) => found,
        Err(error) => {
            error!("could not open {}: {error}", open.key);
            return;
        }
    };
    let open_already = documents.iter().find(|(_, document)| {
        document.kind == open.kind && document.origin == found.origin && document.key == found.key
    });
    if let Some((document, _)) = open_already {
        select(&mut selection, document);
        return;
    }

    // A hidden page for the editor; selecting the document shows it. Hidden
    // pages are also invisible, so panels on screen can be told apart.
    let page = commands
        .spawn((
            Node {
                display: Display::None,
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0,
                row_gap: px(8),
                ..default()
            },
            Visibility::Hidden,
            InspectorPage,
            ChildOf(*pages),
        ))
        .id();
    let editor = (kind.spawn_editor)(&mut commands, page, &found.start);
    commands.entity(editor).insert(Dormant);
    let document = commands
        .spawn(Document {
            kind: kind.id,
            key: found.key,
            name: found.name,
            origin: found.origin,
            page,
            editor,
            view: None,
            detail: DetailChoice::default(),
            opened: opened.0,
        })
        .id();
    commands.entity(document).insert(found.pending);
    opened.0 += 1;
    select(&mut selection, document);
}

/// Selects `document` as a whole.
fn select(selection: &mut ResMut<Selection>, document: Entity) {
    selection.set_if_neq(Selection {
        document: Some(document),
        ..default()
    });
}

/// Closes a document, selecting another if it was selected.
fn close_document(
    close: On<CloseDocument>,
    documents: Query<(Entity, &Document)>,
    mut selection: ResMut<Selection>,
    mut commands: Commands,
) {
    let Ok((_, document)) = documents.get(close.0) else {
        return;
    };
    commands.entity(document.page).despawn();
    commands.entity(close.0).despawn();
    if selection.document == Some(close.0) {
        let next = documents
            .iter()
            .filter(|(entity, _)| *entity != close.0)
            .max_by_key(|(_, document)| document.opened)
            .map(|(entity, _)| entity);
        *selection = Selection {
            document: next,
            ..default()
        };
    }
}

/// Shows the selected document's page and wakes its editor, puts the
/// others to sleep, and shows the selected document in the scene with the
/// camera and detail it had.
fn apply_selection(
    selection: Res<Selection>,
    mut documents: Query<(Entity, &mut Document)>,
    mut nodes: Query<(&mut Node, &mut Visibility), With<InspectorPage>>,
    cameras: Query<&OrbitCamera, With<ViewportCamera>>,
    mut shown: Local<Option<Entity>>,
    mut viewer: MessageWriter<ViewerCommand>,
    mut commands: Commands,
) {
    if !selection.is_changed() || *shown == selection.document {
        return;
    }

    // Keep the camera of the document left.
    if let Some(left) = *shown
        && let Ok((_, mut document)) = documents.get_mut(left)
        && let Ok(view) = cameras.single()
    {
        document.view = Some(*view);
    }
    *shown = selection.document;

    // Only the selected document's page shows, and only its editor works.
    for (entity, document) in &documents {
        let selected = selection.document == Some(entity);
        if let Ok((mut node, mut visibility)) = nodes.get_mut(document.page) {
            node.display = if selected {
                Display::Flex
            } else {
                Display::None
            };
            visibility.set_if_neq(if selected {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
        if let Ok(mut editor) = commands.get_entity(document.editor) {
            if selected {
                editor.remove::<Dormant>();
            } else {
                editor.insert(Dormant);
            }
        }
    }

    // Show it, once its editor is awake, with its own camera and detail.
    let Some((entity, document)) = selection
        .document
        .and_then(|selected| documents.get(selected).ok())
    else {
        commands.insert_resource(Unsent(None));
        viewer.write(ViewerCommand::Clear);
        return;
    };
    viewer.write(ViewerCommand::SetDetail(document.detail.detail()));
    commands.insert_resource(Unsent(Some((entity, document.view))));
}

/// A newly selected document not yet sent to the scene, with the camera
/// view to restore once it is.
#[derive(Resource, Default)]
struct Unsent(Option<(Entity, Option<OrbitCamera>)>);

/// Sends the newly selected document to the scene once its editor is
/// ready, then restores its camera view.
fn send_selected(world: &mut World) {
    let Some((document, view)) = world.resource::<Unsent>().0 else {
        return;
    };
    if !send_document(world, document, false) {
        return;
    }
    if let Some(view) = view {
        world.write_message(ViewerCommand::SetCamera(view));
    }
    world.resource_mut::<Unsent>().0 = None;
}

/// Sends the selected document to the scene again after an edit.
fn send_edited(edited: On<DocumentEdited>, selection: Res<Selection>, mut commands: Commands) {
    let Some(document) = selection.document else {
        return;
    };
    let reframe = edited.reframe;
    commands.queue(move |world: &mut World| {
        send_document(world, document, reframe);
    });
}

/// Sends `document` to the scene as its editor has it, fitting the camera
/// to it with `reframe`. Returns whether it was sent: not when the
/// document is gone or its editor isn't ready yet.
pub fn send_document(world: &mut World, document: Entity, reframe: bool) -> bool {
    let Some(found) = world.get::<Document>(document) else {
        return false;
    };
    let (kind, editor) = (found.kind, found.editor);
    let Some(show) = world.resource::<Kinds>().get(kind).map(|kind| kind.show) else {
        return false;
    };
    let Some(show) = show(world, editor, document.to_bits()) else {
        return false;
    };
    world.write_message(ViewerCommand::Show(show));
    if reframe {
        world.write_message(ViewerCommand::ResetCamera);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_choices_follow_the_controls_order() {
        for index in 0..6 {
            assert_eq!(DetailChoice::at(index).index(), index);
        }
        assert_eq!(
            DetailChoice::GameDefault.detail(),
            Detail::ByDistance { finest: 1 }
        );
        assert_eq!(
            DetailChoice::Level0Preview.detail(),
            Detail::ByDistance { finest: 0 }
        );
        assert_eq!(DetailChoice::at(4).detail(), Detail::Level(2));
    }
}
