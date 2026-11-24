//! The Inspector's settings, built from each kind's metadata file.
//!
//! A kind with metadata edits its preset as [`Settings`]: the settings
//! tree its preset file writes, with the typed options read back after each
//! edit. [`sections`] lays the settings out as the metadata says, with
//! labels, ranges, conditions, revive values and tooltips, and
//! [`spawn_part_chips`] adds a row of chips that show one part's settings
//! at a time. Register an [`InspectorPlugin`] for each edited options type.
//! Each user edit triggers [`InspectorEdited`] with what changed.

mod generate;
mod settings;
#[cfg(test)]
mod tests;

use std::marker::PhantomData;

use bevy::feathers::controls::{ButtonVariant, FeathersButton, FeathersDisclosureToggle};
use bevy::feathers::display::caption;
use bevy::prelude::*;
use bevy::ui::Checked;
use bevy::ui_widgets::{Activate, ValueChange};
use procgena_core::meta::{Edit, KindMeta};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub use generate::{CustomGroup, MetaKind, item_params, sections};
pub use settings::Settings;

use super::documents::{Document, Selection};
use crate::params::{ParamPanel, ParamPanelPlugin, ParamsChanged};

/// Triggered after a user edit in the Inspector of a document: the edit
/// itself and any settings it revealed that took revive values, oldest
/// first. Undo them together.
#[derive(Event, Debug, Clone)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "ST2's undo and unsaved state read it; not merged yet"
    )
)]
pub struct InspectorEdited {
    /// The document edited.
    pub document: Entity,
    /// What changed.
    pub edits: Vec<Edit>,
    /// False while a slider is still being dragged; true for the last edit
    /// of a drag and every other edit.
    pub is_final: bool,
}

/// Makes Inspector panels of `ParamPanel<Settings<T>>` work: their
/// controls, their [`InspectorEdited`] reports, and their part filter
/// following [`Selection::part`]. Add once for each `T`.
pub struct InspectorPlugin<T>(PhantomData<T>);

impl<T> Default for InspectorPlugin<T> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<T> Plugin for InspectorPlugin<T>
where
    T: Serialize + DeserializeOwned + Clone + PartialEq + Send + Sync + 'static,
{
    fn build(&self, app: &mut App) {
        app.add_plugins(ParamPanelPlugin::<Settings<T>>::default())
            .add_observer(report_edits::<T>)
            .add_systems(Update, follow_selected_part::<T>);
        if !app.world().contains_resource::<PartChipsAdded>() {
            app.insert_resource(PartChipsAdded)
                .add_observer(choose_part)
                .add_observer(open_part_sections)
                .add_systems(Update, show_chosen_chip);
        }
    }
}

/// Sets the setting at `path` of the panel `panel` to `new` as a user edit
/// would: the change is reported with [`ParamsChanged`] and
/// [`InspectorEdited`] like an edit made with the panel's own controls.
/// Does nothing when the panel is gone or the setting holds `new` already.
pub fn edit_setting<T>(
    commands: &mut Commands,
    panels: &mut Query<&mut ParamPanel<Settings<T>>>,
    panel: Entity,
    path: &str,
    new: serde_json::Value,
) where
    T: Serialize + DeserializeOwned + Clone + PartialEq + Send + Sync + 'static,
{
    let Ok(mut shown) = panels.get_mut(panel) else {
        return;
    };
    let mut settings = shown.value().clone();
    settings.set(&procgena_core::meta::SettingPath::parse(path), new);
    if settings != *shown.value() {
        shown.set_value(settings);
        commands.trigger(ParamsChanged {
            entity: panel,
            is_final: true,
        });
    }
}

/// Changes the panel `panel`'s settings to `options` as user edits would:
/// one edit for each setting that differs, reported together with
/// [`ParamsChanged`] and [`InspectorEdited`]. Does nothing when the panel
/// is gone or nothing differs.
pub fn edit_options<T>(
    commands: &mut Commands,
    panels: &mut Query<&mut ParamPanel<Settings<T>>>,
    panel: Entity,
    options: &T,
) where
    T: Serialize + DeserializeOwned + Clone + PartialEq + Send + Sync + 'static,
{
    let Ok(mut shown) = panels.get_mut(panel) else {
        return;
    };
    let mut settings = shown.value().clone();
    let new = procgena_core::preset::to_value(options);
    for path in procgena_core::meta::leaves(&new) {
        if let Some(value) = path.get(&new)
            && path.get(settings.value()) != Some(value)
        {
            settings.set(&path, value.clone());
        }
    }
    if settings != *shown.value() {
        shown.set_value(settings);
        commands.trigger(ParamsChanged {
            entity: panel,
            is_final: true,
        });
    }
}

/// Marks that the part chips' observer is registered, so that several
/// [`InspectorPlugin`]s add it once.
#[derive(Resource)]
struct PartChipsAdded;

/// Marks a panel that edits part of another preset's settings, such as one
/// species of a tree Scatter. Its edits trigger no [`InspectorEdited`],
/// since their paths are not the document's.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct NestedPanel;

/// Sends the edits a panel recorded as one [`InspectorEdited`], naming the
/// document whose page holds the panel. Nested panels report nothing.
fn report_edits<T>(
    changed: On<ParamsChanged>,
    mut panels: Query<&mut ParamPanel<Settings<T>>, Without<NestedPanel>>,
    parents: Query<&ChildOf>,
    documents: Query<(Entity, &Document)>,
    mut commands: Commands,
) where
    T: Serialize + DeserializeOwned + Clone + PartialEq + Send + Sync + 'static,
{
    let Ok(mut panel) = panels.get_mut(changed.entity) else {
        return;
    };
    let mut settings = panel.value().clone();
    let edits = settings.take_edits();
    panel.set_value(settings);
    let pages: Vec<Entity> = parents.iter_ancestors(changed.entity).collect();
    let document = documents
        .iter()
        .find(|(_, document)| pages.contains(&document.page));
    if let (Some((document, _)), false) = (document, edits.is_empty()) {
        commands.trigger(InspectorEdited {
            document,
            edits,
            is_final: changed.is_final,
        });
    }
}

/// Shows only the selected part's settings in the selected document's
/// panels, and every setting in other documents' panels. A part the
/// panel's kind doesn't have shows every setting.
fn follow_selected_part<T>(
    selection: Res<Selection>,
    documents: Query<&Document>,
    chips: Query<&PartChips>,
    mut panels: Query<&mut ParamPanel<Settings<T>>>,
    parents: Query<&ChildOf>,
) where
    T: Serialize + DeserializeOwned + Clone + PartialEq + Send + Sync + 'static,
{
    let page = selection
        .document
        .and_then(|document| documents.get(document).ok())
        .map(|document| document.page);
    for chips in &chips {
        let Ok(mut panel) = panels.get_mut(chips.panel) else {
            continue;
        };
        let selected =
            page.is_some_and(|page| parents.iter_ancestors(chips.panel).any(|e| e == page));
        let part = selection
            .part
            .as_deref()
            .filter(|part| selected && chips.meta.part(part).is_some());
        if panel.part() != part {
            panel.set_part(part.map(str::to_owned));
        }
    }
}

/// The row of part chips above a kind's settings.
#[derive(Component)]
struct PartChips {
    /// The panel whose settings the chips filter.
    panel: Entity,
    /// The kind whose parts the chips name.
    meta: &'static KindMeta,
}

/// One part chip: the part it shows, or every part for `None`.
#[derive(Component, Default, Clone)]
struct PartChip(Option<&'static str>);

/// Spawns the Inspector panel of `kind` editing `settings` as the last
/// child of `parent`, and returns it. For kinds with two or more parts the
/// panel starts with a row of part chips. Sections whose indices are in
/// `open` start expanded. `custom` builds the controls of groups that name
/// one (see [`sections`]).
pub fn spawn_meta_panel<T>(
    commands: &mut Commands,
    parent: Entity,
    kind: &'static MetaKind,
    settings: Settings<T>,
    open: &[usize],
    custom: &CustomGroup<T>,
) -> Entity
where
    T: Serialize + DeserializeOwned + Clone + PartialEq + Send + Sync + 'static,
{
    let sections = sections(kind, settings.value(), custom);
    let panel = crate::params::spawn_param_panel(commands, parent, settings, sections, open);
    spawn_part_chips(commands, panel, &kind.meta);
    panel
}

/// Puts a row of chips first in the panel `panel`, one for each of
/// `meta`'s parts plus "All", that select which part's settings the
/// Inspector shows (see [`Selection::part`]). Adds nothing for kinds with
/// fewer than two parts.
fn spawn_part_chips(commands: &mut Commands, panel: Entity, meta: &'static KindMeta) {
    if meta.parts.len() < 2 {
        return;
    }
    let chips: Vec<(Option<&'static str>, &'static str)> = std::iter::once((None, "All"))
        .chain(
            meta.parts
                .iter()
                .map(|(key, part)| (Some(key.as_str()), part.name.as_str())),
        )
        .collect();
    let row = commands
        .spawn((
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                flex_shrink: 0.0,
                column_gap: px(4),
                row_gap: px(4),
                ..default()
            },
            PartChips { panel, meta },
        ))
        .id();
    commands.entity(panel).insert_children(0, &[row]);
    for (part, name) in chips {
        let tip = part
            .and_then(|key| meta.part(key))
            .map_or("Every setting.", |p| p.description.as_str());
        commands
            .spawn_scene(bsn! {
                @FeathersButton {
                    @caption: bsn! { @caption(name) },
                    @variant: ButtonVariant::Normal,
                }
                PartChip({part})
                crate::ui::Tooltip({tip})
            })
            .insert(ChildOf(row));
    }
}

/// Selects the clicked chip's part.
fn choose_part(activate: On<Activate>, chips: Query<&PartChip>, mut selection: ResMut<Selection>) {
    if let Ok(PartChip(part)) = chips.get(activate.event_target()) {
        let part = part.map(str::to_owned);
        if selection.part != part {
            selection.part = part;
        }
    }
}

/// Opens every section of the panel whose part chip (other than "All") was
/// clicked, so that part's settings show; sections without them are hidden
/// anyway.
fn open_part_sections(
    activate: On<Activate>,
    chips: Query<&PartChip>,
    parents: Query<&ChildOf>,
    rows: Query<&PartChips>,
    children: Query<&Children>,
    toggles: Query<Has<Checked>, With<FeathersDisclosureToggle>>,
    mut commands: Commands,
) {
    let chip = activate.event_target();
    if !chips.get(chip).is_ok_and(|PartChip(part)| part.is_some()) {
        return;
    }
    let panel = parents
        .iter_ancestors(chip)
        .find_map(|ancestor| rows.get(ancestor).ok())
        .map(|row| row.panel);
    for toggle in panel
        .into_iter()
        .flat_map(|panel| children.iter_descendants(panel))
    {
        if toggles.get(toggle).is_ok_and(|open| !open) {
            commands.trigger(ValueChange {
                source: toggle,
                value: true,
                is_final: true,
            });
        }
    }
}

/// Marks the chip of the selected part as the primary button.
fn show_chosen_chip(selection: Res<Selection>, mut chips: Query<(&PartChip, &mut ButtonVariant)>) {
    if !selection.is_changed() {
        return;
    }
    for (PartChip(part), mut variant) in &mut chips {
        let chosen = *part == selection.part.as_deref();
        let wanted = if chosen {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Normal
        };
        if *variant != wanted {
            *variant = wanted;
        }
    }
}
