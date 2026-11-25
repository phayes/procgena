//! A tree Scatter document's editor: one row per species, and the Scatter's
//! shared settings.

use std::collections::HashMap;

use bevy::ecs::system::SystemParam;
use bevy::feathers::constants::icons;
use bevy::feathers::controls::{
    ButtonVariant, FeathersButton, FeathersSelect, OptionIndex, list_rows_from_strings,
};
use bevy::feathers::display::{caption, icon, label_small};
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{Activate, ValueChange};
use procgena_tree::{
    ChildKey, MAX_SCATTER_TREES, ScatterTree, TreeOptions, TreePreset, TreeScatter,
    TreeScatterPreset,
};

use super::{
    CUSTOM, RowStatus, added_tree, choice_of, custom_choice, per_ha_text, read_grove, row_statuses,
};
use crate::export::{Done, FileOps, read_preset, save_preset};
use crate::params::{Group, ParamPanel, ParamsChanged, Section, spawn_param_panel};
use crate::studio::Dormant;
use crate::studio::inspector::{
    InspectorPlugin, NestedPanel, Settings, item_params, spawn_meta_panel,
};
use crate::tree::editor::TreePanel;
use crate::ui::SelectOptions;

/// The panel holding a tree Scatter's shared settings.
pub type GrovePanel = ParamPanel<Settings<TreeScatter>>;
/// The panel holding one species' share of a tree Scatter.
type SharePanel = ParamPanel<Settings<ScatterTree>>;

/// Triggered after any tree Scatter editor's Scatter changes. Read the
/// selected tree Scatter document's with [`GroveReader::grove`], and any
/// editor's with [`GroveReader::grove_of`].
#[derive(Event, Debug)]
pub struct GroveEdited;

/// A tree Scatter editor's state: one per open tree Scatter document, or
/// one spawned inside another editor with [`spawn_tree_scatter_editor`].
#[derive(Component)]
pub struct GroveEditor {
    /// The preset edited, which saved files name as their base, on a tree
    /// Scatter document's editor; `None` on other editors.
    preset: Option<&'static TreeScatterPreset>,
    /// The panel holding the Scatter's shared settings. Its Scatter has no
    /// species; they come from the rows.
    panel: Entity,
    /// The node whose children are the [`SpeciesRow`]s, in the grove's order.
    list: Entity,
}

/// One species of the grove: a tree preset picker, its density and share,
/// and buttons to edit or remove it, above its share settings.
#[derive(Component)]
struct SpeciesRow {
    /// The panel holding the row's [`ScatterTree`].
    share: Entity,
    /// The panel with every setting of the tree, once it has been shown.
    tree: Option<Entity>,
    /// Whether the tree's settings are open while the tree is a preset.
    customising: bool,
    /// Whether the row fills the gaps between the other species, which hides
    /// its clustering setting.
    takes_rest: bool,
}

/// The parts of a row's header that show its state.
#[derive(Component, Clone, Copy, Default, PartialEq, Eq)]
enum RowPart {
    /// The tree preset picker.
    #[default]
    Preset,
    /// The trees per hectare.
    PerHa,
    /// The percentage of the grove's trees.
    Share,
    /// The button showing or hiding the tree's settings.
    Edit,
    /// The button removing the row.
    Remove,
}

/// Marks the button adding a species.
#[derive(Component, Default, Clone)]
struct AddTree;

/// Makes the grove editor work. Needs the param panel plugins and
/// [`crate::tree::editor::TreeEditorPlugin`], which edits tree settings.
pub struct GroveEditorPlugin;

impl Plugin for GroveEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            InspectorPlugin::<TreeScatter>::default(),
            InspectorPlugin::<ScatterTree>::default(),
        ))
        .add_systems(
            Update,
            (refresh_rows, show_row_parts, show_tree_settings).chain(),
        );
    }
}

/// Reads the edited tree Scatters.
#[derive(SystemParam)]
pub struct GroveReader<'w, 's> {
    editors: Query<'w, 's, (Entity, &'static GroveEditor, Has<Dormant>)>,
    children: Query<'w, 's, &'static Children>,
    rows: Query<'w, 's, &'static SpeciesRow>,
    shares: Query<'w, 's, &'static SharePanel>,
    groves: Query<'w, 's, &'static GrovePanel>,
}

impl GroveReader<'_, '_> {
    /// The selected tree Scatter document's editor entity, and its state.
    fn selected(&self) -> Option<(Entity, &GroveEditor)> {
        self.editors
            .iter()
            .find(|(_, editor, dormant)| editor.preset.is_some() && !dormant)
            .map(|(entity, editor, _)| (entity, editor))
    }

    /// The tree Scatter of the selected tree Scatter document: its species,
    /// in row order, with its shared settings. `None` while no tree Scatter
    /// document is selected.
    pub fn grove(&self) -> Option<TreeScatter> {
        self.grove_of(self.selected()?.0)
    }

    /// The tree Scatter edited by the editor `entity`, as returned by
    /// [`spawn_grove_editor`] or [`spawn_tree_scatter_editor`], selected or
    /// not. `None` if `entity` is not a tree Scatter editor.
    pub fn grove_of(&self, entity: Entity) -> Option<TreeScatter> {
        let (_, editor, _) = self.editors.get(entity).ok()?;
        let panel = self.groves.get(editor.panel).ok()?;
        let trees: Vec<ScatterTree> = self
            .children
            .get(editor.list)
            .into_iter()
            .flatten()
            .filter_map(|row| self.rows.get(*row).ok())
            .filter_map(|row| self.shares.get(row.share).ok())
            .map(|share| share.value().options().clone())
            .collect();
        Some(panel.value().options().clone().with_trees(trees))
    }

    /// The preset of the selected tree Scatter document, which saved files
    /// name as their base. `None` while no tree Scatter document is
    /// selected.
    pub fn preset(&self) -> Option<&'static TreeScatterPreset> {
        self.selected()?.1.preset
    }
}

/// The tree Scatter editor that `entity` is part of.
fn editor_of(
    entity: Entity,
    parents: &Query<&ChildOf>,
    editors: &Query<&mut GroveEditor>,
) -> Option<Entity> {
    parents
        .iter_ancestors(entity)
        .find(|ancestor| editors.contains(*ancestor))
}

/// The tree Scatter preset [`START_PRESET`] names.
#[cfg(test)]
pub fn start_preset() -> &'static TreeScatterPreset {
    TreeScatterPreset::named(super::START_PRESET).expect("the start preset is built in")
}

/// The tree Scatter of [`start_preset`].
#[cfg(test)]
pub fn start_grove() -> TreeScatter {
    start_preset().tree_scatter.clone()
}

/// Spawns a tree Scatter document's editor for the built-in preset `key`,
/// with file buttons, as the last child of `parent`, and returns the
/// editor's entity.
///
/// # Panics
///
/// Panics if `key` names no built-in tree Scatter preset.
pub fn spawn_grove_editor(commands: &mut Commands, parent: Entity, key: &str) -> Entity {
    let preset = TreeScatterPreset::named(key).expect("the tree Scatter preset is built in");

    // The species rows, and the Add Tree button, above the panel.
    let editor = commands
        .spawn(Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            row_gap: px(8),
            ..default()
        })
        .insert(ChildOf(parent))
        .id();
    let (panel, list) = spawn_body(commands, editor, preset.tree_scatter.clone());
    commands.entity(editor).insert(GroveEditor {
        preset: Some(preset),
        panel,
        list,
    });

    // Saving and loading, below the settings.
    commands
        .spawn_scene(bsn! {
            Node { display: Display::Flex, column_gap: px(6), flex_shrink: 0.0 }
            Children [
                @FeathersButton { @caption: bsn! { @caption("Save Preset…") } }
                on(save_grove)
                --
                @FeathersButton { @caption: bsn! { @caption("Load Preset…") } }
                on(load_grove_file)
            ]
        })
        .insert(ChildOf(editor));
    editor
}

/// Spawns an editor of `grove`'s species rows and shared settings, without
/// file buttons, as the last child of `parent`, and
/// returns it. Each edit triggers [`GroveEdited`]; read the Scatter with
/// [`GroveReader::grove_of`] and the returned entity.
pub fn spawn_tree_scatter_editor(
    commands: &mut Commands,
    parent: Entity,
    grove: TreeScatter,
) -> Entity {
    let editor = commands
        .spawn(Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            row_gap: px(8),
            ..default()
        })
        .insert(ChildOf(parent))
        .id();
    let (panel, list) = spawn_body(commands, editor, grove);
    commands.entity(editor).insert(GroveEditor {
        preset: None,
        panel,
        list,
    });
    editor
}

/// Spawns `grove`'s species rows, the Add Tree button and the shared
/// settings panel as the last children of `editor`. Returns the panel and
/// the node holding the rows.
fn spawn_body(commands: &mut Commands, editor: Entity, grove: TreeScatter) -> (Entity, Entity) {
    // The species rows, and the Add Tree button, above the panel.
    let list = commands
        .spawn(Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            row_gap: px(10),
            ..default()
        })
        .insert(ChildOf(editor))
        .id();
    commands
        .spawn_scene(bsn! {
            Node { flex_shrink: 0.0 }
            Children [
                @FeathersButton { @caption: bsn! { @caption("+ Add Tree") } }
                AddTree
                on(add_tree)
            ]
        })
        .insert(ChildOf(editor));
    for tree in &grove.trees {
        spawn_row(commands, list, tree.clone());
    }

    // The Scatter's shared settings, with Scatter open. Its species live in
    // the rows.
    let shared = grove.with_trees(Vec::new());
    let panel = spawn_meta_panel(
        commands,
        editor,
        &super::META,
        Settings::new(shared),
        &[0],
        &|_, _| Vec::new(),
    );
    commands
        .entity(panel)
        .observe(|_: On<ParamsChanged>, mut commands: Commands| {
            commands.trigger(GroveEdited);
        });
    (panel, list)
}

/// Spawns a row showing `tree` as the last child of `list` and returns it.
fn spawn_row(commands: &mut Commands, list: Entity, tree: ScatterTree) -> Entity {
    let names: Vec<String> = TreePreset::all()
        .map(|p| p.name.to_string())
        .chain([CUSTOM.to_string()])
        .collect();
    let selected = choice_of(&tree);
    let row = commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0,
                row_gap: px(4),
            }
            Children [
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    column_gap: px(6),
                }
                Children [
                    @FeathersSelect { @options: list_rows_from_strings(names, Some(selected)) }
                    Node { flex_grow: 1.0, min_width: px(0) }
                    RowPart::Preset
                    on(choose_tree_preset)
                    --
                    Node { width: px(56), flex_shrink: 0.0, justify_content: JustifyContent::End }
                    Children [ @label_small("") RowPart::PerHa ]
                    --
                    Node { width: px(34), flex_shrink: 0.0, justify_content: JustifyContent::End }
                    Children [ @label_small("") RowPart::Share ]
                    --
                    @FeathersButton {
                        @caption: bsn! { @caption("Edit") },
                        @variant: ButtonVariant::Plain,
                    }
                    RowPart::Edit
                    on(toggle_tree_settings)
                    --
                    @FeathersButton {
                        @caption: bsn! { @icon(icons::X) },
                        @variant: ButtonVariant::Plain,
                    }
                    RowPart::Remove
                    on(remove_row)
                ]
            ]
        })
        .insert(ChildOf(list))
        .id();

    // The species' share settings, open.
    let share = Settings::new(tree);
    let sections = share_sections(share.value());
    let share = spawn_param_panel(commands, row, share, sections, &[0]);
    commands.entity(share).insert(NestedPanel).observe(
        |_: On<ParamsChanged>, mut commands: Commands| {
            commands.trigger(GroveEdited);
        },
    );
    commands.entity(row).insert(SpeciesRow {
        share,
        tree: None,
        customising: false,
        takes_rest: false,
    });
    row
}

/// One species' share settings, `item`, laid out from the tree Scatter
/// metadata's species list.
fn share_sections(item: &serde_json::Value) -> Vec<Section<Settings<ScatterTree>>> {
    let species = super::META
        .meta
        .sections
        .iter()
        .flat_map(|section| &section.groups)
        .find(|group| group.custom.as_deref() == Some("species_list"))
        .expect("tree_scatter.meta.yml has a species list");
    let prefix = procgena_core::meta::SettingPath::parse("trees.0");
    vec![Section {
        title: "Share",
        groups: vec![Group::plain(item_params(
            &super::META,
            species,
            &prefix,
            item,
        ))],
    }]
}

/// The row that `entity` belongs to.
fn row_of(
    entity: Entity,
    parents: &Query<&ChildOf>,
    rows: &Query<&mut SpeciesRow>,
) -> Option<Entity> {
    parents
        .iter_ancestors(entity)
        .find(|ancestor| rows.contains(*ancestor))
}

/// Shows a loaded tree Scatter in the selected tree Scatter document's
/// editor, replacing every row. The preset, and so the suggested file name,
/// stays as it was. Run it with `World::run_system_cached_with`.
pub fn load_grove(
    In(grove): In<TreeScatter>,
    editors: Query<&GroveEditor, Without<Dormant>>,
    children: Query<&Children>,
    mut groves: Query<&mut GrovePanel>,
    mut commands: Commands,
) {
    if let Some(editor) = editors.iter().find(|editor| editor.preset.is_some()) {
        show_grove(grove, editor, &children, &mut groves, &mut commands);
    }
}

/// Replaces every row and the shared settings with `grove`'s, and triggers
/// [`GroveEdited`].
fn show_grove(
    mut grove: TreeScatter,
    editor: &GroveEditor,
    children: &Query<&Children>,
    groves: &mut Query<&mut GrovePanel>,
    commands: &mut Commands,
) {
    for row in children.get(editor.list).into_iter().flatten() {
        commands.entity(*row).despawn();
    }
    for tree in std::mem::take(&mut grove.trees) {
        spawn_row(commands, editor.list, tree);
    }
    if let Ok(mut panel) = groves.get_mut(editor.panel) {
        panel.set_value(Settings::new(grove));
    }
    commands.trigger(GroveEdited);
}

/// Saves the selected tree Scatter as a `.tree_scatter.yml` preset file
/// based on its document's preset.
fn save_grove(_: On<Activate>, reader: GroveReader, mut files: FileOps) {
    let (Some(grove), Some(base)) = (reader.grove(), reader.preset()) else {
        return;
    };
    files.save(&format!("{}.tree_scatter.yml", base.key), move |path| {
        save_preset(grove, base, path)
    });
}

/// Loads a `.tree_scatter.yml` preset file into the selected tree Scatter
/// document's editor.
fn load_grove_file(_: On<Activate>, mut files: FileOps) {
    files.open(|text, world| {
        let grove = read_grove(text)?;
        world
            .run_system_cached_with(load_grove, grove)
            .map_err(|error| format!("Cannot show the tree Scatter: {error}"))?;
        Ok(Done::new("Loaded the tree Scatter"))
    });
}

/// Loads a tree settings file into the row whose button was pressed.
fn load_row_tree(
    activate: On<Activate>,
    parents: Query<&ChildOf>,
    rows: Query<&mut SpeciesRow>,
    mut files: FileOps,
) {
    let Some(row) = row_of(activate.event_target(), &parents, &rows) else {
        return;
    };
    files.open(move |text, world| {
        let tree = read_preset::<TreePreset>(text, "tree")?;
        world
            .run_system_cached_with(set_row_tree, (row, tree))
            .map_err(|error| format!("Cannot show the tree: {error}"))?;
        Ok(Done::new("Loaded the tree"))
    });
}

/// Makes `tree` the tree of `row`, if the row still exists. Keeps the row's
/// density and spacing.
fn set_row_tree(
    In((row, tree)): In<(Entity, TreeOptions)>,
    rows: Query<&SpeciesRow>,
    mut shares: Query<&mut SharePanel>,
    mut trees: Query<&mut TreePanel>,
    mut commands: Commands,
) {
    let Ok(row) = rows.get(row) else {
        return;
    };
    if let Ok(mut share) = shares.get_mut(row.share) {
        let entry = ScatterTree {
            tree: tree.clone(),
            ..share.value().options().clone()
        };
        share.set_value(Settings::new(entry));
    }
    if let Some(mut panel) = row.tree.and_then(|panel| trees.get_mut(panel).ok()) {
        panel.set_value(Settings::new(tree));
    }
    commands.trigger(GroveEdited);
}

/// Adds [`added_tree`] before the last row, unless the Scatter has
/// [`MAX_SCATTER_TREES`] species.
fn add_tree(
    activate: On<Activate>,
    parents: Query<&ChildOf>,
    editors: Query<&mut GroveEditor>,
    children: Query<&Children>,
    rows: Query<&SpeciesRow>,
    shares: Query<&SharePanel>,
    mut commands: Commands,
) {
    let Some(editor) = editor_of(activate.event_target(), &parents, &editors)
        .and_then(|entity| editors.get(entity).ok())
    else {
        return;
    };
    let row_entities: Vec<Entity> = children
        .get(editor.list)
        .into_iter()
        .flatten()
        .copied()
        .collect();
    let count = row_entities.len();
    if count >= MAX_SCATTER_TREES {
        return;
    }
    // Species keys must differ, so a new row takes the first free `added_<n>`.
    let taken: Vec<String> = row_entities
        .iter()
        .filter_map(|row| rows.get(*row).ok())
        .filter_map(|row| shares.get(row.share).ok())
        .map(|share| share.value().options().key.as_str().to_owned())
        .collect();
    let key = (1..)
        .map(|n| format!("added_{n}"))
        .find(|key| !taken.contains(key))
        .and_then(|key| ChildKey::new(key).ok())
        .expect("a free key");
    let row = spawn_row(
        &mut commands,
        editor.list,
        ScatterTree {
            key,
            ..added_tree()
        },
    );
    commands
        .entity(editor.list)
        .insert_children(count.saturating_sub(1), &[row]);
    commands.trigger(GroveEdited);
}

/// Removes the row, unless it is the only one.
fn remove_row(
    activate: On<Activate>,
    parents: Query<&ChildOf>,
    rows: Query<&mut SpeciesRow>,
    editors: Query<&mut GroveEditor>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    let Some(editor) = editor_of(activate.event_target(), &parents, &editors)
        .and_then(|entity| editors.get(entity).ok())
    else {
        return;
    };
    let count = children.get(editor.list).map_or(0, |rows| rows.len());
    if count <= 1 {
        return;
    }
    if let Some(row) = row_of(activate.event_target(), &parents, &rows) {
        commands.entity(row).despawn();
        commands.trigger(GroveEdited);
    }
}

/// Shows or hides the tree's settings.
fn toggle_tree_settings(
    activate: On<Activate>,
    parents: Query<&ChildOf>,
    mut rows: Query<&mut SpeciesRow>,
) {
    if let Some(entity) = row_of(activate.event_target(), &parents, &rows)
        && let Ok(mut row) = rows.get_mut(entity)
    {
        row.customising = !row.customising;
    }
}

/// Makes the chosen preset the row's tree, at the preset's own density and
/// spacing. Choosing Custom opens the settings of the tree shown.
#[allow(clippy::too_many_arguments)]
fn choose_tree_preset(
    change: On<ValueChange<Entity>>,
    options: Query<&OptionIndex>,
    parents: Query<&ChildOf>,
    mut rows: Query<&mut SpeciesRow>,
    mut shares: Query<&mut SharePanel>,
    mut trees: Query<&mut TreePanel>,
    mut commands: Commands,
) {
    let Ok(choice) = options.get(change.value) else {
        return;
    };
    let Some(entity) = row_of(change.source, &parents, &rows) else {
        return;
    };
    let Ok(mut row) = rows.get_mut(entity) else {
        return;
    };
    let Ok(mut share) = shares.get_mut(row.share) else {
        return;
    };
    let current = choice_of(share.value().options());

    match TreePreset::all().nth(choice.0) {
        // The picker also reports choices made to match the tree; ignore those.
        Some(_) if current == choice.0 => {}
        Some(preset) => {
            // The preset's tree, density and spacing; the row keeps the rest.
            let fresh = ScatterTree::new(preset);
            let entry = ScatterTree {
                tree: fresh.tree,
                density: fresh.density,
                spacing: fresh.spacing,
                ..share.value().options().clone()
            };
            if let Some(mut panel) = row.tree.and_then(|panel| trees.get_mut(panel).ok()) {
                panel.set_value(Settings::new(entry.tree.clone()));
            }
            share.set_value(Settings::new(entry));
            commands.trigger(GroveEdited);
        }
        None if current == choice.0 => {}
        None => row.customising = true,
    }
}

/// Hides clustering on the row that fills the gaps, in every editor.
fn refresh_rows(
    editors: Query<&GroveEditor>,
    children: Query<&Children>,
    mut rows: Query<&mut SpeciesRow>,
    mut shares: Query<&mut SharePanel>,
) {
    for editor in &editors {
        refresh_editor_rows(editor, &children, &mut rows, &mut shares);
    }
}

/// Hides clustering on `editor`'s row that fills the gaps.
fn refresh_editor_rows(
    editor: &GroveEditor,
    children: &Query<&Children>,
    rows: &mut Query<&mut SpeciesRow>,
    shares: &mut Query<&mut SharePanel>,
) {
    let entities: Vec<Entity> = children
        .get(editor.list)
        .into_iter()
        .flatten()
        .copied()
        .filter(|entity| rows.contains(*entity))
        .collect();
    let densities: Vec<f32> = entities
        .iter()
        .map(|entity| {
            rows.get(*entity)
                .ok()
                .and_then(|row| shares.get(row.share).ok())
                .map_or(0.0, |share| share.value().options().density())
        })
        .collect();
    let rest = densities.iter().rposition(|density| *density > 0.0);

    // Touch the panels only when a row's role changes.
    for (index, entity) in entities.iter().enumerate() {
        let takes_rest = rest == Some(index);
        let Ok(mut row) = rows.get_mut(*entity) else {
            continue;
        };
        if row.takes_rest == takes_rest {
            continue;
        }
        row.takes_rest = takes_rest;
        if let Ok(mut share) = shares.get_mut(row.share) {
            let hidden: &[&'static str] = if takes_rest { &["clustering"] } else { &[] };
            share.set_hidden_keys(hidden.iter().copied());
        }
    }
}

/// The row statuses last worked out, and the Scatter they were worked out
/// for.
#[derive(Default)]
struct StatusCache {
    grove: Option<TreeScatter>,
    statuses: Vec<RowStatus>,
}

/// Shows each row's preset, placed density and share, and which buttons can
/// be used, in every editor.
#[allow(clippy::too_many_arguments)]
fn show_row_parts(
    reader: GroveReader,
    editors: Query<(Entity, &GroveEditor)>,
    children: Query<&Children>,
    rows: Query<&SpeciesRow>,
    shares: Query<&SharePanel>,
    parts: Query<(Entity, &RowPart)>,
    parents: Query<&ChildOf>,
    adders: Query<Entity, With<AddTree>>,
    mut texts: Query<&mut Text>,
    mut variants: Query<&mut ButtonVariant>,
    disabled: Query<Has<InteractionDisabled>>,
    selects: SelectOptions,
    mut caches: Local<HashMap<Entity, StatusCache>>,
    mut commands: Commands,
) {
    caches.retain(|entity, _| editors.contains(*entity));
    for (entity, editor) in &editors {
        let Some(grove) = reader.grove_of(entity) else {
            continue;
        };
        let in_editor = |part: Entity| parents.iter_ancestors(part).any(|a| a == entity);
        show_editor_row_parts(
            editor,
            grove,
            caches.entry(entity).or_default(),
            &children,
            &rows,
            &shares,
            parts.iter().filter(|(part, _)| in_editor(*part)),
            &parents,
            adders.iter().filter(|adder| in_editor(*adder)),
            &mut texts,
            &mut variants,
            &disabled,
            &selects,
            &mut commands,
        );
    }
}

/// Shows the row parts of `editor`, which edits `grove`.
#[allow(clippy::too_many_arguments)]
fn show_editor_row_parts<'a>(
    editor: &GroveEditor,
    grove: TreeScatter,
    cache: &mut StatusCache,
    children: &Query<&Children>,
    rows: &Query<&SpeciesRow>,
    shares: &Query<&SharePanel>,
    parts: impl Iterator<Item = (Entity, &'a RowPart)>,
    parents: &Query<&ChildOf>,
    adders: impl Iterator<Item = Entity>,
    texts: &mut Query<&mut Text>,
    variants: &mut Query<&mut ButtonVariant>,
    disabled: &Query<Has<InteractionDisabled>>,
    selects: &SelectOptions,
    commands: &mut Commands,
) {
    // Place the Scatter again only when it changed.
    if cache.grove.as_ref() != Some(&grove) {
        cache.statuses = row_statuses(&grove);
        cache.grove = Some(grove);
    }
    let statuses = &cache.statuses;
    let row_entities: Vec<Entity> = children
        .get(editor.list)
        .into_iter()
        .flatten()
        .copied()
        .collect();
    let set_disabled = |commands: &mut Commands, entity: Entity, off: bool| {
        if disabled.get(entity).is_ok_and(|was| was != off) {
            if off {
                commands.entity(entity).insert(InteractionDisabled);
            } else {
                commands.entity(entity).remove::<InteractionDisabled>();
            }
        }
    };

    for adder in adders {
        set_disabled(commands, adder, row_entities.len() >= MAX_SCATTER_TREES);
    }

    for (entity, part) in parts {
        // Find the row this part belongs to, and its status.
        let Some(at) = parents
            .iter_ancestors(entity)
            .find_map(|ancestor| row_entities.iter().position(|row| *row == ancestor))
        else {
            continue;
        };
        let (Ok(row), Some(status)) = (rows.get(row_entities[at]), statuses.get(at)) else {
            continue;
        };
        let Ok(choice) = shares
            .get(row.share)
            .map(|share| choice_of(share.value().options()))
        else {
            continue;
        };
        let custom = choice == custom_choice();

        match part {
            RowPart::Preset => selects.select(commands, entity, choice),
            RowPart::PerHa | RowPart::Share => {
                let shown = match part {
                    RowPart::PerHa => per_ha_text(status.per_ha),
                    _ => format!("{:.0}%", status.share * 100.0),
                };
                let text = children
                    .iter_descendants(entity)
                    .chain([entity])
                    .find(|e| texts.contains(*e));
                if let Some(mut text) = text.and_then(|e| texts.get_mut(e).ok()) {
                    text.set_if_neq(Text(shown));
                }
            }
            RowPart::Edit => {
                // A custom tree's settings are always open.
                set_disabled(commands, entity, custom);
                if let Ok(mut variant) = variants.get_mut(entity) {
                    variant.set_if_neq(ButtonVariant::selected(row.customising || custom));
                }
            }
            RowPart::Remove => set_disabled(commands, entity, !status.removable),
        }
    }
}

/// Shows each row's tree settings while the row is customised or its tree is
/// custom, creating them the first time.
fn show_tree_settings(
    mut rows: Query<(Entity, &mut SpeciesRow)>,
    shares: Query<&SharePanel>,
    parents: Query<&ChildOf>,
    mut nodes: Query<&mut Node>,
    mut commands: Commands,
) {
    for (entity, mut row) in &mut rows {
        let Ok(entry) = shares.get(row.share).map(|share| share.value().options()) else {
            continue;
        };
        let shown = row.customising || choice_of(entry) == custom_choice();
        match row.tree {
            Some(panel) => {
                // Show or hide the settings together with their Load Tree button.
                let holder = parents.get(panel).map_or(panel, ChildOf::parent);
                if let Ok(mut node) = nodes.get_mut(holder) {
                    let display = if shown { Display::Flex } else { Display::None };
                    if node.display != display {
                        node.display = display;
                    }
                }
            }
            None if shown => {
                // Indent the settings under the row.
                let holder = commands
                    .spawn((
                        Node {
                            display: Display::Flex,
                            flex_direction: FlexDirection::Column,
                            flex_shrink: 0.0,
                            padding: UiRect::left(px(12)),
                            ..default()
                        },
                        ChildOf(entity),
                    ))
                    .id();
                let panel = spawn_meta_panel(
                    &mut commands,
                    holder,
                    &crate::tree::META,
                    Settings::new(entry.tree.clone()),
                    &[],
                    &crate::tree::custom,
                );
                commands
                    .entity(panel)
                    .insert(NestedPanel)
                    .observe(edit_row_tree);
                commands
                    .spawn_scene(bsn! {
                        Node { flex_shrink: 0.0, margin: UiRect::top(px(4)) }
                        Children [
                            @FeathersButton { @caption: bsn! { @caption("Load Tree…") } }
                            on(load_row_tree)
                        ]
                    })
                    .insert(ChildOf(holder));
                row.tree = Some(panel);
            }
            None => {}
        }
    }
}

/// Makes a tree settings panel's edits the tree of the row it is in.
fn edit_row_tree(
    changed: On<ParamsChanged>,
    parents: Query<&ChildOf>,
    rows: Query<&SpeciesRow>,
    trees: Query<&TreePanel>,
    mut shares: Query<&mut SharePanel>,
    mut commands: Commands,
) {
    let row = parents
        .iter_ancestors(changed.entity)
        .find_map(|ancestor| rows.get(ancestor).ok());
    let (Some(row), Ok(tree)) = (row, trees.get(changed.entity)) else {
        return;
    };
    if let Ok(mut share) = shares.get_mut(row.share) {
        let entry = ScatterTree {
            tree: tree.value().options().clone(),
            ..share.value().options().clone()
        };
        share.set_value(Settings::new(entry));
        commands.trigger(GroveEdited);
    }
}

#[cfg(test)]
mod tests {
    //! These start the whole studio on the GPU, so they are ignored by default.

    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;
    use crate::headless::{DEFAULT_SIZE, HeadlessStudio, lock};
    use crate::scene::{EnvironmentSettings, GroveSubject, ViewerCommand};

    /// The tree Scatter of the selected document.
    fn edited(studio: &mut HeadlessStudio) -> TreeScatter {
        let world = studio.app_mut().world_mut();
        world
            .run_system_cached(|reader: GroveReader| reader.grove())
            .unwrap()
            .unwrap()
    }

    /// The grove `command` shows, if it shows one.
    fn grove_shown(command: &ViewerCommand) -> Option<&TreeScatter> {
        match command {
            ViewerCommand::Show(show) => show.settings::<GroveSubject>(),
            _ => None,
        }
    }

    /// The grove in the last grove `Show` among `commands`.
    fn last_grove(commands: &[ViewerCommand]) -> TreeScatter {
        commands
            .iter()
            .rev()
            .find_map(grove_shown)
            .cloned()
            .expect("a grove was sent")
    }

    /// The size of the grove the scene shows.
    fn shown_size(studio: &mut HeadlessStudio) -> Vec2 {
        let world = studio.app_mut().world_mut();
        let mut groves = world.query::<&procgena_tree_bevy::TreeScatterParams>();
        groves.single(world).unwrap().size
    }

    /// The tree count in the overlay line `line`.
    fn tree_count(line: &str) -> usize {
        let count = line.split(" trees").next().unwrap();
        count.replace(',', "").parse().unwrap()
    }

    /// Waits up to a minute for the overlay to report a built grove, and
    /// returns its line.
    fn built_line(studio: &mut HeadlessStudio) -> String {
        built_line_other_than(studio, "")
    }

    /// Waits up to a minute for the overlay to report a built grove in a
    /// line other than `old`, and returns it.
    fn built_line_other_than(studio: &mut HeadlessStudio, old: &str) -> String {
        for _ in 0..120 {
            if let Some(shown) = studio
                .texts()
                .into_iter()
                .find(|shown| shown.text.contains(" trees · ") && shown.text != old)
            {
                return shown.text;
            }
            studio.wait(Duration::from_millis(500));
        }
        panic!("the grove never finished: {:?}", studio.texts());
    }

    /// An empty folder for `test`'s files.
    fn folder(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("procgena-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn a_grove_document_builds_its_grove_and_sends_its_edits() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        let sent = studio.record::<ViewerCommand>();
        let start = start_grove();
        studio.click("Tree Scatter").unwrap();
        studio.click(&start_preset().name).unwrap();

        // The opened grove is sent and built, with one row per species.
        assert_eq!(last_grove(&lock(&sent)), start);
        assert!(built_line(&mut studio).ends_with(" ms"));
        let rows = studio
            .texts()
            .into_iter()
            .filter(|shown| shown.text.ends_with("/ha"))
            .count();
        assert_eq!(rows, start.trees.len());

        // Adding a species sends it, ahead of the row that takes the rest.
        studio.click("+ Add Tree").unwrap();
        let added = last_grove(&lock(&sent));
        assert_eq!(added.trees.len(), start.trees.len() + 1);
        // A new species is the added tree under the first free key.
        assert_eq!(
            added.trees[start.trees.len() - 1],
            ScatterTree {
                key: procgena_core::ChildKey::from_static("added_1"),
                ..added_tree()
            }
        );
        assert_eq!(added, edited(&mut studio));

        // Opening another tree Scatter preset from the Library shows it.
        studio.click(&TreeScatterPreset::pine_stand().name).unwrap();
        let pines = last_grove(&lock(&sent));
        assert_eq!(pines, TreeScatterPreset::pine_stand().tree_scatter);
        assert!(built_line(&mut studio).ends_with(" ms"));
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn the_environments_grove_size_sizes_the_grove_shown() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        studio.click("Tree Scatter").unwrap();
        studio.click(&start_preset().name).unwrap();
        let start = EnvironmentSettings::default();
        let first = built_line(&mut studio);
        let small = tree_count(&first);
        assert_eq!(shown_size(&mut studio), Vec2::splat(start.grove_size));

        // A larger grove size grows more trees, without a new grove being sent.
        let sent = studio.record::<ViewerCommand>();
        let larger = EnvironmentSettings {
            grove_size: 2.0 * start.grove_size,
            ..start
        };
        let world = studio.app_mut().world_mut();
        world.write_message(ViewerCommand::SetEnvironment(larger));
        studio.settle();
        assert_eq!(shown_size(&mut studio), Vec2::splat(larger.grove_size));
        let large = tree_count(&built_line_other_than(&mut studio, &first));
        assert!(large > 2 * small, "{small} trees grew to {large}");
        assert!(
            !lock(&sent)
                .iter()
                .any(|command| grove_shown(command).is_some())
        );
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn groves_save_and_load() {
        let dir = folder("grove");
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        studio.click("Tree Scatter").unwrap();
        studio.click(&start_preset().name).unwrap();
        // The file buttons sit below the rows, so scroll down to them over
        // the settings column, here where the Add Tree button starts out.
        // The column stops at its end.
        let panel = studio.find("+ Add Tree").unwrap();
        studio.click_at(panel);
        let saved = edited(&mut studio);
        let scroll = |studio: &mut HeadlessStudio| studio.scroll(panel, -100.0);

        // Saving writes the Scatter; loading it into another document makes
        // that document's Scatter the saved one.
        scroll(&mut studio);
        studio.answer_dialog(Some(dir.join("woods.tree_scatter.yml")));
        studio.click("Save Preset…").unwrap();
        assert!(studio.find("Saved woods.tree_scatter.yml").is_ok());
        studio.click(&TreeScatterPreset::orchard().name).unwrap();
        assert_ne!(edited(&mut studio), saved);
        scroll(&mut studio);
        studio.answer_dialog(Some(dir.join("woods.tree_scatter.yml")));
        studio.click("Load Preset…").unwrap();
        assert_eq!(edited(&mut studio), saved);
        assert!(studio.find("Loaded the tree Scatter").is_ok());

        // A file that is not a tree Scatter changes nothing.
        std::fs::write(dir.join("bad.tree_scatter.yml"), "not a preset").unwrap();
        scroll(&mut studio);
        studio.answer_dialog(Some(dir.join("bad.tree_scatter.yml")));
        studio.click("Load Preset…").unwrap();
        assert!(studio.shows_text_starting("Not tree Scatter settings"));
        assert_eq!(edited(&mut studio), saved);
    }
}
