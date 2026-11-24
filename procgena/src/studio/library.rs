//! The Library: every built-in preset, filtered by kind. Clicking a preset
//! opens it as a document.

use bevy::feathers::controls::{ButtonVariant, FeathersButton, FeathersScrollbar};
use bevy::feathers::display::{caption, label_dim};
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui_widgets::Activate;

use super::START_KIND;
use super::documents::OpenDocument;
use super::kinds::Kinds;
use super::layout::ScrollColumn;
use crate::ui::{TabSelected, tab_bar};

/// The kind whose presets the Library lists, by id.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LibraryFilter(pub &'static str);

impl Default for LibraryFilter {
    /// The kind of the document opened at start.
    fn default() -> Self {
        Self(START_KIND)
    }
}

/// Marks the kind filter's chips.
#[derive(Component, Default, Clone)]
struct KindChips;

/// Marks the list of presets.
#[derive(Component, Default, Clone)]
struct PresetList;

/// One preset in the list: its kind's id and its key.
#[derive(Component, Default, Clone, Debug)]
struct PresetRow {
    kind: &'static str,
    key: String,
}

/// Fills the Library from the kind registry and opens what is clicked.
pub struct LibraryPlugin;

impl Plugin for LibraryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LibraryFilter>()
            .add_systems(PostStartup, spawn_chips)
            .add_systems(Update, list_presets)
            .add_observer(open_row);
    }
}

/// The Library: a title, the kind chips, and the scrolling preset list.
pub fn library() -> impl Scene {
    bsn! {
        Node {
            flex_grow: 3.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
        }
        Children [
            @label_dim("Library")
            --
            Node { flex_shrink: 0.0 } KindChips
            --
            Node {
                flex_grow: 1.0,
                min_height: px(0),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
            }
            Children [
                #List
                Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::scroll_y(),
                    row_gap: px(2),
                }
                ScrollPosition
                ScrollColumn
                PresetList
                --
                @FeathersScrollbar { @target: #List }
                Node { width: px(8), margin: UiRect::vertical(px(4)) }
            ]
        ]
    }
}

/// Fills the chips with one per registered kind, the filtered one picked.
fn spawn_chips(
    kinds: Res<Kinds>,
    filter: Res<LibraryFilter>,
    slot: Single<Entity, With<KindChips>>,
    mut commands: Commands,
) {
    let labels: Vec<&'static str> = kinds.iter().map(|kind| kind.label).collect();
    let ids: Vec<&'static str> = kinds.iter().map(|kind| kind.id).collect();
    let selected = ids.iter().position(|id| *id == filter.0).unwrap_or(0);
    let chips = commands
        .spawn_scene(bsn! {
            @tab_bar(&labels, selected)
            Node { flex_grow: 1.0, flex_shrink: 1.0 }
        })
        .insert(ChildOf(*slot))
        .id();
    commands.entity(chips).observe(
        move |picked: On<TabSelected>, mut filter: ResMut<LibraryFilter>| {
            if let Some(id) = ids.get(picked.index) {
                filter.0 = id;
            }
        },
    );
}

/// Lists the filtered kind's presets whenever the filter changes.
fn list_presets(
    filter: Res<LibraryFilter>,
    kinds: Res<Kinds>,
    list: Single<(Entity, Option<&Children>), With<PresetList>>,
    mut commands: Commands,
) {
    if !filter.is_changed() {
        return;
    }
    let (list, rows) = *list;
    for row in rows.into_iter().flatten() {
        commands.entity(*row).despawn();
    }
    let Some(kind) = kinds.get(filter.0) else {
        return;
    };
    for preset in (kind.presets)() {
        let (id, key, name) = (kind.id, preset.key, preset.name);
        commands
            .spawn_scene(bsn! {
                @FeathersButton {
                    @caption: bsn! { @caption(name) },
                    @variant: ButtonVariant::Normal,
                }
                Node { flex_shrink: 0.0, justify_content: JustifyContent::FlexStart }
                PresetRow { kind: {id}, key: {key} }
            })
            .insert(ChildOf(list));
    }
}

/// Opens the clicked preset.
fn open_row(activate: On<Activate>, rows: Query<&PresetRow>, mut commands: Commands) {
    if let Ok(row) = rows.get(activate.event_target()) {
        commands.trigger(OpenDocument {
            kind: row.kind,
            key: row.key.clone(),
        });
    }
}
