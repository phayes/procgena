//! The Outline: the open documents, the selected one highlighted. Clicking
//! one selects it; its × closes it.

use bevy::feathers::controls::{ButtonVariant, FeathersButton};
use bevy::feathers::display::{caption, label_dim};
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui_widgets::Activate;

use super::documents::{CloseDocument, Document, DocumentSettings, Selection};
use super::kinds::Kinds;
use super::layout::InspectorTitle;

/// Marks the list of open documents.
#[derive(Component, Default, Clone)]
struct OutlineList;

/// A button selecting the document it names.
#[derive(Component, Default, Clone)]
struct SelectButton(Option<Entity>);

/// A button closing the document it names.
#[derive(Component, Default, Clone)]
struct CloseButton(Option<Entity>);

/// Keeps the Outline and the Inspector's title in step with the documents.
pub struct OutlinePlugin;

impl Plugin for OutlinePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (list_documents, show_title))
            .add_observer(select_clicked)
            .add_observer(close_clicked);
    }
}

/// The Outline: a title above the list of open documents.
pub fn outline() -> impl Scene {
    bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
        }
        Children [
            @label_dim("Open")
            --
            Node {
                flex_grow: 1.0,
                min_height: px(0),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                overflow: Overflow::scroll_y(),
                row_gap: px(2),
            }
            OutlineList
        ]
    }
}

/// The Outline's text for a document: its name and its kind's label, such
/// as `Oak Medium · Trees`.
pub fn outline_text(document: &Document, kinds: &Kinds) -> String {
    let kind = kinds
        .get(document.kind)
        .map_or(document.kind, |kind| kind.label);
    format!("{} · {kind}", document.name)
}

/// Lists the open documents, in the order they were opened, whenever they
/// or the selection change.
fn list_documents(
    selection: Res<Selection>,
    added: Query<(), Added<Document>>,
    mut removed: RemovedComponents<Document>,
    documents: Query<(Entity, &Document)>,
    kinds: Res<Kinds>,
    list: Single<(Entity, Option<&Children>), With<OutlineList>>,
    mut commands: Commands,
) {
    let removed = removed.read().count() > 0;
    if !selection.is_changed() && added.is_empty() && !removed {
        return;
    }
    let (list, rows) = *list;
    for row in rows.into_iter().flatten() {
        commands.entity(*row).despawn();
    }
    let mut open: Vec<(Entity, &Document)> = documents.iter().collect();
    open.sort_by_key(|(_, document)| document.opened);
    for (entity, document) in open {
        let text = outline_text(document, &kinds);
        let variant = ButtonVariant::selected(selection.document == Some(entity));
        let document = Some(entity);
        commands
            .spawn_scene(bsn! {
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    flex_shrink: 0.0,
                    column_gap: px(2),
                }
                Children [
                    @FeathersButton { @caption: bsn! { @caption(text) }, @variant: variant }
                    Node { flex_grow: 1.0, justify_content: JustifyContent::FlexStart }
                    SelectButton({document})
                    --
                    @FeathersButton { @caption: bsn! { @caption("×") } }
                    CloseButton({document})
                ]
            })
            .insert(ChildOf(list));
    }
}

/// Selects the clicked document.
fn select_clicked(
    activate: On<Activate>,
    buttons: Query<&SelectButton>,
    mut selection: ResMut<Selection>,
) {
    if let Ok(SelectButton(Some(document))) = buttons.get(activate.event_target()) {
        selection.set_if_neq(Selection {
            document: Some(*document),
            ..default()
        });
    }
}

/// Closes the document whose × was clicked.
fn close_clicked(activate: On<Activate>, buttons: Query<&CloseButton>, mut commands: Commands) {
    if let Ok(CloseButton(Some(document))) = buttons.get(activate.event_target()) {
        commands.trigger(CloseDocument(*document));
    }
}

/// Titles the Inspector with the selected document's name and key, such as
/// `Oak Medium (oak_medium) · built-in`, ending with ` · unsaved` while it
/// has unsaved changes.
fn show_title(
    selection: Res<Selection>,
    documents: Query<(&Document, Option<&DocumentSettings>)>,
    mut title: Single<&mut Text, With<InspectorTitle>>,
) {
    let text = selection
        .document
        .and_then(|document| documents.get(document).ok())
        .map_or_else(String::new, |(document, state)| {
            let unsaved = if state.is_some_and(DocumentSettings::is_unsaved) {
                " · unsaved"
            } else {
                ""
            };
            format!(
                "{} ({}) · {}{unsaved}",
                document.name,
                document.key,
                document.origin.label()
            )
        });
    if title.0 != text {
        title.0 = text;
    }
}
