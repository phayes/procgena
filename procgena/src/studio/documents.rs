//! Documents: the presets open in the studio, and which one is selected.
//!
//! A [`Document`] is one open preset. Its settings live in its editor in the
//! Inspector; the document names the preset and keeps what the view frame
//! shows for it. [`Selection`] says which document the Inspector, the view
//! and the Outline show. Open one with [`OpenDocument`], close it with
//! [`CloseDocument`], and select one by setting [`Selection::document`].

use bevy::prelude::*;

use super::kinds::Kinds;
use crate::scene::{Detail, OrbitCamera, ViewerCommand, ViewportCamera};

/// Where an open preset comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// One of the studio's built-in presets.
    BuiltIn,
}

impl Origin {
    /// How the Inspector names it, such as `built-in`.
    pub fn label(self) -> &'static str {
        match self {
            Self::BuiltIn => "built-in",
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
}

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
    /// The preset's key, such as `"oak_medium"`.
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
        app.init_resource::<Selection>()
            .init_resource::<Unsent>()
            .add_observer(open_document)
            .add_observer(close_document)
            .add_observer(send_edited)
            .add_systems(Update, (apply_selection, send_selected).chain());
    }
}

/// Opens a preset, or selects it if it is open already.
fn open_document(
    open: On<OpenDocument>,
    kinds: Res<Kinds>,
    documents: Query<(Entity, &Document)>,
    pages: Single<Entity, With<InspectorPages>>,
    mut selection: ResMut<Selection>,
    mut commands: Commands,
) {
    let open_already = documents
        .iter()
        .find(|(_, document)| document.kind == open.kind && document.key == open.key);
    if let Some((document, _)) = open_already {
        select(&mut selection, document);
        return;
    }
    let Some(kind) = kinds.get(open.kind) else {
        error!("no kind {} to open {} with", open.kind, open.key);
        return;
    };
    let Some(entry) = (kind.presets)()
        .into_iter()
        .find(|preset| preset.key == open.key)
    else {
        error!("no {} preset {}", open.kind, open.key);
        return;
    };

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
    let editor = (kind.spawn_editor)(&mut commands, page, &open.key);
    commands.entity(editor).insert(Dormant);
    let document = commands
        .spawn(Document {
            kind: kind.id,
            key: entry.key,
            name: entry.name,
            origin: Origin::BuiltIn,
            page,
            editor,
            view: None,
            detail: DetailChoice::default(),
        })
        .id();
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
        // Documents are spawned in order, so the last other one is the
        // one opened last.
        let next = documents
            .iter()
            .map(|(entity, _)| entity)
            .filter(|entity| *entity != close.0)
            .max();
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
