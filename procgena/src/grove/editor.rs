//! The Tree Grove tab's controls: preset, one row per species, and the
//! grove's shared settings.

use bevy::ecs::system::SystemParam;
use bevy::feathers::constants::icons;
use bevy::feathers::controls::{
    ButtonVariant, FeathersButton, FeathersSelect, OptionIndex, list_rows_from_strings,
};
use bevy::feathers::display::{caption, icon, label_small};
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{Activate, ValueChange};
use procgena_tree::{GrovePreset, GroveTree, MAX_GROVE_TREES, TreeGrove, TreeOptions, TreePreset};

use super::{
    CUSTOM, RowStatus, START_PRESET, START_SIZE, added_tree, choice_of, params, per_ha_text,
    read_grove, row_statuses,
};
use crate::export::{Done, FileOps, shown_name, write_file};
use crate::params::{ParamPanel, ParamPanelPlugin, ParamsChanged, spawn_param_panel};
use crate::scene::GroveSettings;
use crate::ui::{SelectOptions, labelled_row};

/// Triggered after the grove changes. Read it with [`GroveReader::settings`].
#[derive(Event, Debug)]
pub struct GroveEdited;

/// The grove editor's state. There is one, on the Tree Grove tab.
#[derive(Component)]
pub struct GroveEditor {
    /// The panel holding the grove's shared settings and size. Its grove has
    /// no species; they come from the rows.
    panel: Entity,
    /// The node whose children are the [`SpeciesRow`]s, in the grove's order.
    list: Entity,
}

/// One species of the grove: a tree preset picker, its density and share,
/// and buttons to edit or remove it, above its share settings.
#[derive(Component)]
struct SpeciesRow {
    /// The panel holding the row's [`GroveTree`].
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
            ParamPanelPlugin::<GroveSettings>::default(),
            ParamPanelPlugin::<GroveTree>::default(),
        ))
        .add_systems(
            Update,
            (refresh_rows, show_row_parts, show_tree_settings).chain(),
        );
    }
}

/// Reads the edited grove.
#[derive(SystemParam)]
pub struct GroveReader<'w, 's> {
    editors: Query<'w, 's, &'static GroveEditor>,
    children: Query<'w, 's, &'static Children>,
    rows: Query<'w, 's, &'static SpeciesRow>,
    shares: Query<'w, 's, &'static ParamPanel<GroveTree>>,
    groves: Query<'w, 's, &'static ParamPanel<GroveSettings>>,
}

impl GroveReader<'_, '_> {
    /// The edited grove: its species, in row order, with its shared settings
    /// and size. `None` before the editor is spawned.
    pub fn settings(&self) -> Option<GroveSettings> {
        let editor = self.editors.single().ok()?;
        let panel = self.groves.get(editor.panel).ok()?;
        let trees: Vec<GroveTree> = self
            .children
            .get(editor.list)
            .into_iter()
            .flatten()
            .filter_map(|row| self.rows.get(*row).ok())
            .filter_map(|row| self.shares.get(row.share).ok())
            .map(|share| share.value().clone())
            .collect();
        let mut settings = panel.value().clone();
        settings.grove.trees = trees;
        Some(settings)
    }
}

/// The grove shown when the studio opens.
pub fn start_grove() -> GroveSettings {
    GroveSettings {
        grove: START_PRESET.grove(),
        size: START_SIZE,
    }
}

/// Spawns the grove editor, showing [`START_PRESET`], as the last child of
/// `parent`.
pub fn spawn_grove_editor(commands: &mut Commands, parent: Entity) {
    let start = start_grove();
    let names = GrovePreset::ALL.map(GrovePreset::name);
    let selected = GrovePreset::ALL.iter().position(|p| *p == START_PRESET);
    let preset: Box<dyn Scene> = Box::new(bsn! {
        @FeathersSelect { @options: list_rows_from_strings(names, selected) }
        Node { flex_grow: 1.0 }
        on(choose_grove_preset)
    });

    // Preset, the species rows, and the Add Tree button, above the panel.
    let editor = commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0,
                row_gap: px(8),
            }
            Children [ @labelled_row("Preset", preset) ]
        })
        .insert(ChildOf(parent))
        .id();
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
    for tree in &start.grove.trees {
        spawn_row(commands, list, tree.clone());
    }

    // The grove's shared settings, with Grove open. Its species live in
    // the rows.
    let shared = GroveSettings {
        grove: TreeGrove {
            trees: Vec::new(),
            ..start.grove
        },
        size: start.size,
    };
    let panel = spawn_param_panel(commands, editor, shared, params::sections(), &[0]);
    commands
        .entity(panel)
        .observe(|_: On<ParamsChanged>, mut commands: Commands| {
            commands.trigger(GroveEdited);
        });
    commands.entity(editor).insert(GroveEditor { panel, list });

    // Saving and loading, below the settings.
    commands
        .spawn_scene(bsn! {
            Node { display: Display::Flex, column_gap: px(6), flex_shrink: 0.0 }
            Children [
                @FeathersButton { @caption: bsn! { @caption("Save Grove…") } }
                on(save_grove)
                --
                @FeathersButton { @caption: bsn! { @caption("Load Grove…") } }
                on(load_grove_file)
            ]
        })
        .insert(ChildOf(editor));
}

/// Spawns a row showing `tree` as the last child of `list` and returns it.
fn spawn_row(commands: &mut Commands, list: Entity, tree: GroveTree) -> Entity {
    let names: Vec<String> = TreePreset::ALL
        .iter()
        .map(|p| p.name().to_string())
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
    let share = spawn_param_panel(commands, row, tree, params::share_sections(), &[0]);
    commands
        .entity(share)
        .observe(|_: On<ParamsChanged>, mut commands: Commands| {
            commands.trigger(GroveEdited);
        });
    commands.entity(row).insert(SpeciesRow {
        share,
        tree: None,
        customising: false,
        takes_rest: false,
    });
    row
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

/// Replaces every row and the shared settings with the chosen grove
/// preset's. Keeps the grove's size.
fn choose_grove_preset(
    change: On<ValueChange<Entity>>,
    options: Query<&OptionIndex>,
    editor: Single<&GroveEditor>,
    children: Query<&Children>,
    mut groves: Query<&mut ParamPanel<GroveSettings>>,
    mut commands: Commands,
) {
    if let Some(preset) = options
        .get(change.value)
        .ok()
        .and_then(|index| GrovePreset::ALL.get(index.0))
    {
        show_grove(
            preset.grove(),
            &editor,
            &children,
            &mut groves,
            &mut commands,
        );
    }
}

/// Shows a loaded grove in the editor, replacing every row. The preset
/// picker keeps its choice, and the grove keeps its size. Run it with
/// `World::run_system_cached_with`.
pub fn load_grove(
    In(grove): In<TreeGrove>,
    editor: Single<&GroveEditor>,
    children: Query<&Children>,
    mut groves: Query<&mut ParamPanel<GroveSettings>>,
    mut commands: Commands,
) {
    show_grove(grove, &editor, &children, &mut groves, &mut commands);
}

/// Replaces every row and the shared settings with `grove`'s, keeps the
/// size, and triggers [`GroveEdited`].
fn show_grove(
    mut grove: TreeGrove,
    editor: &GroveEditor,
    children: &Query<&Children>,
    groves: &mut Query<&mut ParamPanel<GroveSettings>>,
    commands: &mut Commands,
) {
    for row in children.get(editor.list).into_iter().flatten() {
        commands.entity(*row).despawn();
    }
    for tree in std::mem::take(&mut grove.trees) {
        spawn_row(commands, editor.list, tree);
    }
    if let Ok(mut panel) = groves.get_mut(editor.panel) {
        let size = panel.value().size;
        panel.set_value(GroveSettings { grove, size });
    }
    commands.trigger(GroveEdited);
}

/// Saves the grove as a `.grove.json` file. The size is not saved.
fn save_grove(_: On<Activate>, reader: GroveReader, mut files: FileOps) {
    let Some(settings) = reader.settings() else {
        return;
    };
    files.save("grove.grove.json", move |path| {
        write_file(path, settings.grove.to_json().as_bytes())?;
        Ok(Done::new(format!("Saved {}", shown_name(path))))
    });
}

/// Loads a `.grove.json` file into the editor.
fn load_grove_file(_: On<Activate>, mut files: FileOps) {
    files.open(|text, world| {
        let grove = read_grove(text)?;
        world
            .run_system_cached_with(load_grove, grove)
            .map_err(|error| format!("Cannot show the grove: {error}"))?;
        Ok(Done::new("Loaded the grove"))
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
        let tree = TreeOptions::from_json(text).map_err(|error| format!("Not a tree: {error}"))?;
        tree.validate()
            .map_err(|error| format!("Not a usable tree: {error}"))?;
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
    mut shares: Query<&mut ParamPanel<GroveTree>>,
    mut trees: Query<&mut ParamPanel<TreeOptions>>,
    mut commands: Commands,
) {
    let Ok(row) = rows.get(row) else {
        return;
    };
    if let Ok(mut share) = shares.get_mut(row.share) {
        let entry = GroveTree {
            tree: tree.clone(),
            ..share.value().clone()
        };
        share.set_value(entry);
    }
    if let Some(mut panel) = row.tree.and_then(|panel| trees.get_mut(panel).ok()) {
        panel.set_value(tree);
    }
    commands.trigger(GroveEdited);
}

/// Adds [`added_tree`] before the last row, unless the grove has
/// [`MAX_GROVE_TREES`] species.
fn add_tree(
    _: On<Activate>,
    editor: Single<&GroveEditor>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    let count = children.get(editor.list).map_or(0, |rows| rows.len());
    if count >= MAX_GROVE_TREES {
        return;
    }
    let row = spawn_row(&mut commands, editor.list, added_tree());
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
    editor: Single<&GroveEditor>,
    children: Query<&Children>,
    mut commands: Commands,
) {
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
    mut shares: Query<&mut ParamPanel<GroveTree>>,
    mut trees: Query<&mut ParamPanel<TreeOptions>>,
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
    let current = choice_of(share.value());

    match TreePreset::ALL.get(choice.0) {
        // The picker also reports choices made to match the tree; ignore those.
        Some(_) if current == choice.0 => {}
        Some(preset) => {
            let entry = GroveTree {
                tree: preset.options(),
                density: preset.grove_density(),
                spacing: preset.grove_spacing(),
                ..share.value().clone()
            };
            if let Some(mut panel) = row.tree.and_then(|panel| trees.get_mut(panel).ok()) {
                panel.set_value(entry.tree.clone());
            }
            share.set_value(entry);
            commands.trigger(GroveEdited);
        }
        None if current == choice.0 => {}
        None => row.customising = true,
    }
}

/// Hides clustering on the row that fills the gaps.
fn refresh_rows(
    editors: Query<&GroveEditor>,
    children: Query<&Children>,
    mut rows: Query<&mut SpeciesRow>,
    mut shares: Query<&mut ParamPanel<GroveTree>>,
) {
    let Ok(editor) = editors.single() else {
        return;
    };
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
                .map_or(0.0, |share| share.value().density())
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

/// The row statuses last worked out, and the grove they were worked out for.
#[derive(Default)]
struct StatusCache {
    settings: Option<GroveSettings>,
    statuses: Vec<RowStatus>,
}

/// Shows each row's preset, placed density and share, and which buttons can
/// be used.
#[allow(clippy::too_many_arguments)]
fn show_row_parts(
    reader: GroveReader,
    editors: Query<&GroveEditor>,
    children: Query<&Children>,
    rows: Query<&SpeciesRow>,
    shares: Query<&ParamPanel<GroveTree>>,
    parts: Query<(Entity, &RowPart)>,
    parents: Query<&ChildOf>,
    adders: Query<Entity, With<AddTree>>,
    mut texts: Query<&mut Text>,
    mut variants: Query<&mut ButtonVariant>,
    disabled: Query<Has<InteractionDisabled>>,
    selects: SelectOptions,
    mut cache: Local<StatusCache>,
    mut commands: Commands,
) {
    let (Some(settings), Ok(editor)) = (reader.settings(), editors.single()) else {
        return;
    };

    // Place the grove again only when it changed.
    if cache.settings.as_ref() != Some(&settings) {
        cache.statuses = row_statuses(&settings.grove, settings.size);
        cache.settings = Some(settings);
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

    for adder in &adders {
        set_disabled(&mut commands, adder, row_entities.len() >= MAX_GROVE_TREES);
    }

    for (entity, part) in &parts {
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
        let Ok(choice) = shares.get(row.share).map(|share| choice_of(share.value())) else {
            continue;
        };
        let custom = choice == TreePreset::ALL.len();

        match part {
            RowPart::Preset => selects.select(&mut commands, entity, choice),
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
                set_disabled(&mut commands, entity, custom);
                if let Ok(mut variant) = variants.get_mut(entity) {
                    variant.set_if_neq(ButtonVariant::selected(row.customising || custom));
                }
            }
            RowPart::Remove => set_disabled(&mut commands, entity, !status.removable),
        }
    }
}

/// Shows each row's tree settings while the row is customised or its tree is
/// custom, creating them the first time.
fn show_tree_settings(
    mut rows: Query<(Entity, &mut SpeciesRow)>,
    shares: Query<&ParamPanel<GroveTree>>,
    parents: Query<&ChildOf>,
    mut nodes: Query<&mut Node>,
    mut commands: Commands,
) {
    for (entity, mut row) in &mut rows {
        let Ok(entry) = shares.get(row.share).map(ParamPanel::value) else {
            continue;
        };
        let shown = row.customising || choice_of(entry) == TreePreset::ALL.len();
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
                let panel = spawn_param_panel(
                    &mut commands,
                    holder,
                    entry.tree.clone(),
                    crate::tree::params::sections(),
                    &[],
                );
                commands.entity(panel).observe(edit_row_tree);
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
    trees: Query<&ParamPanel<TreeOptions>>,
    mut shares: Query<&mut ParamPanel<GroveTree>>,
    mut commands: Commands,
) {
    let row = parents
        .iter_ancestors(changed.entity)
        .find_map(|ancestor| rows.get(ancestor).ok());
    let (Some(row), Ok(tree)) = (row, trees.get(changed.entity)) else {
        return;
    };
    if let Ok(mut share) = shares.get_mut(row.share) {
        let entry = GroveTree {
            tree: tree.value().clone(),
            ..share.value().clone()
        };
        share.set_value(entry);
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
    use crate::scene::ViewerCommand;

    /// The grove being edited.
    fn edited(studio: &mut HeadlessStudio) -> GroveSettings {
        let world = studio.app_mut().world_mut();
        world
            .run_system_cached(|reader: GroveReader| reader.settings())
            .unwrap()
            .unwrap()
    }

    /// The grove in the last `SetGrove` among `commands`.
    fn last_grove(commands: &[ViewerCommand]) -> GroveSettings {
        commands
            .iter()
            .rev()
            .find_map(|command| match command {
                ViewerCommand::SetGrove(settings) => Some((**settings).clone()),
                _ => None,
            })
            .expect("a grove was sent")
    }

    /// Waits up to a minute for the overlay to report a built grove, and
    /// returns its line.
    fn built_line(studio: &mut HeadlessStudio) -> String {
        for _ in 0..120 {
            if let Some(shown) = studio
                .texts()
                .into_iter()
                .find(|shown| shown.text.contains(" trees · "))
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
    fn the_grove_tab_builds_its_grove_and_sends_its_edits() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        studio.click("Tree Grove").unwrap();
        let sent = studio.record::<ViewerCommand>();
        let start = start_grove();

        // The starting grove is built, with one row per species.
        assert!(built_line(&mut studio).ends_with(" ms"));
        let rows = studio
            .texts()
            .into_iter()
            .filter(|shown| shown.text.ends_with("/ha"))
            .count();
        assert_eq!(rows, start.grove.trees.len());

        // Adding a species sends it, ahead of the row that takes the rest.
        studio.click("+ Add Tree").unwrap();
        let added = last_grove(&lock(&sent));
        assert_eq!(added.grove.trees.len(), start.grove.trees.len() + 1);
        assert_eq!(added.grove.trees[start.grove.trees.len() - 1], added_tree());
        assert_eq!(added, edited(&mut studio));

        // A grove preset replaces every row and keeps the size.
        studio.click(START_PRESET.name()).unwrap();
        studio.click(GrovePreset::PineStand.name()).unwrap();
        let pines = last_grove(&lock(&sent));
        assert_eq!(pines.grove, GrovePreset::PineStand.grove());
        assert_eq!(pines.size, START_SIZE);
        assert!(built_line(&mut studio).ends_with(" ms"));
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn groves_save_and_load() {
        let dir = folder("grove");
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        studio.click("Tree Grove").unwrap();
        studio.click("+ Add Tree").unwrap();
        let saved = edited(&mut studio);

        // The file buttons sit below the rows, so scroll down to them, and
        // back up to the preset picker. The column stops at either end.
        let panel = studio.find("Preset").unwrap();
        let scroll = |studio: &mut HeadlessStudio, down: bool| {
            studio.scroll(panel, if down { -100.0 } else { 100.0 });
        };

        // Saving writes the grove; loading it undoes later edits.
        scroll(&mut studio, true);
        studio.answer_dialog(Some(dir.join("woods.grove.json")));
        studio.click("Save Grove…").unwrap();
        assert!(studio.find("Saved woods.grove.json").is_ok());
        scroll(&mut studio, false);
        studio.click(START_PRESET.name()).unwrap();
        studio.click(GrovePreset::Orchard.name()).unwrap();
        assert_ne!(edited(&mut studio), saved);
        scroll(&mut studio, true);
        studio.answer_dialog(Some(dir.join("woods.grove.json")));
        studio.click("Load Grove…").unwrap();
        assert_eq!(edited(&mut studio), saved);
        assert!(studio.find("Loaded the grove").is_ok());

        // A file that is not a grove changes nothing.
        std::fs::write(dir.join("bad.grove.json"), "not json").unwrap();
        scroll(&mut studio, true);
        studio.answer_dialog(Some(dir.join("bad.grove.json")));
        studio.click("Load Grove…").unwrap();
        assert!(studio.shows_text_starting("Not a grove"));
        assert_eq!(edited(&mut studio), saved);
    }
}
