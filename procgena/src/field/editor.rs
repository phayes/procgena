//! A Plant Scatter document's editor: one row per plant, and the Scatter's
//! shared appearance.

use bevy::ecs::system::SystemParam;
use bevy::feathers::constants::icons;
use bevy::feathers::controls::{
    ButtonVariant, FeathersButton, FeathersSelect, OptionIndex, list_rows_from_strings,
};
use bevy::feathers::display::{caption, icon, label_small};
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{Activate, ValueChange};
use procgena_plant::MAX_PLANTS;
use procgena_plant::prelude::{
    PlantParams, PlantPreset, PlantScatter, PlantScatterPreset, ScatterPlant,
};

use super::{
    ADDED_PLANT, CUSTOM, START_PRESET, choice_of, params, per_m2_text, read_field, row_statuses,
    save_field_preset,
};
use crate::export::{Done, FileOps};
use crate::params::{ParamPanel, ParamPanelPlugin, ParamsChanged, spawn_param_panel};
use crate::plant::read_plant;
use crate::studio::Dormant;
use crate::ui::SelectOptions;

/// Triggered after the selected Plant Scatter changes. Read it with
/// [`FieldReader::field`].
#[derive(Event, Debug)]
pub struct FieldEdited;

/// A Plant Scatter editor's state, one per open Plant Scatter.
#[derive(Component)]
pub struct FieldEditor {
    /// The preset edited, which saved files name as their base.
    preset: &'static PlantScatterPreset,
    /// The panel holding the field's shared settings.
    panel: Entity,
    /// The node whose children are the [`PlantRow`]s, in the field's order.
    list: Entity,
}

/// One plant of the field: a preset picker, its density and share, and
/// buttons to edit or remove it, above its density settings.
#[derive(Component)]
struct PlantRow {
    /// The panel holding the row's [`ScatterPlant`].
    share: Entity,
    /// The panel with every setting of the plant, once it has been shown.
    plant: Option<Entity>,
    /// Whether the plant's settings are open while the plant is a preset.
    customising: bool,
    /// Whether the row fills the gaps between the other plants, which hides
    /// its clustering setting.
    takes_rest: bool,
}

/// The parts of a row's header that show its state.
#[derive(Component, Clone, Copy, Default, PartialEq, Eq)]
enum RowPart {
    /// The preset picker.
    #[default]
    Preset,
    /// The plants per square metre.
    PerM2,
    /// The percentage of the field.
    Share,
    /// The button showing or hiding the plant's settings.
    Edit,
    /// The button removing the row.
    Remove,
}

/// Marks the button adding a plant.
#[derive(Component, Default, Clone)]
struct AddPlant;

/// Makes Plant Scatter editors work. Needs the param panel plugins and
/// [`crate::plant::editor::PlantEditorPlugin`], which edits plants.
pub struct FieldEditorPlugin;

impl Plugin for FieldEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ParamPanelPlugin::<PlantScatter>::default(),
            ParamPanelPlugin::<ScatterPlant>::default(),
        ))
        .add_systems(
            Update,
            (refresh_rows, show_row_parts, show_plant_settings).chain(),
        );
    }
}

/// Reads the fields Plant Scatter editors hold.
#[derive(SystemParam)]
pub struct FieldReader<'w, 's> {
    editors: Query<'w, 's, &'static FieldEditor>,
    selected: Query<'w, 's, Entity, (With<FieldEditor>, Without<Dormant>)>,
    children: Query<'w, 's, &'static Children>,
    rows: Query<'w, 's, &'static PlantRow>,
    shares: Query<'w, 's, &'static ParamPanel<ScatterPlant>>,
    fields: Query<'w, 's, &'static ParamPanel<PlantScatter>>,
}

impl FieldReader<'_, '_> {
    /// The selected Plant Scatter's field: its plants, in row order, with
    /// its shared settings. `None` unless exactly one Plant Scatter editor
    /// is selected and ready.
    pub fn field(&self) -> Option<PlantScatter> {
        self.field_of(self.selected.single().ok()?)
    }

    /// The field the Plant Scatter editor `editor` holds: its plants, in row
    /// order, with its shared settings. `None` if `editor` is not a Plant
    /// Scatter editor or is not ready yet.
    pub fn field_of(&self, editor: Entity) -> Option<PlantScatter> {
        let editor = self.editors.get(editor).ok()?;
        let field = self.fields.get(editor.panel).ok()?;
        let plants: Vec<ScatterPlant> = self
            .children
            .get(editor.list)
            .into_iter()
            .flatten()
            .filter_map(|row| self.rows.get(*row).ok())
            .filter_map(|row| self.shares.get(row.share).ok())
            .map(|share| *share.value())
            .collect();
        Some(PlantScatter {
            plants: plants.into(),
            ..field.value().clone()
        })
    }

    /// The selected Plant Scatter's preset, which saved files name as their
    /// base. `None` unless exactly one Plant Scatter editor is selected.
    pub fn preset(&self) -> Option<&'static PlantScatterPreset> {
        let editor = self.selected.single().ok()?;
        Some(self.editors.get(editor).ok()?.preset)
    }
}

/// The Plant Scatter preset shown when the studio opens.
///
/// # Panics
///
/// Panics if [`START_PRESET`] names no Plant Scatter preset.
pub fn start_preset() -> &'static PlantScatterPreset {
    PlantScatterPreset::named(START_PRESET).expect("START_PRESET names a Plant Scatter preset")
}

/// The field shown when the studio opens.
///
/// # Panics
///
/// Panics if [`START_PRESET`] names no Plant Scatter preset.
pub fn start_field() -> PlantScatter {
    start_preset().plant_scatter.clone()
}

/// Spawns a Plant Scatter editor for the built-in preset `key` as the last
/// child of `parent`, and returns the editor's entity.
///
/// # Panics
///
/// Panics if `key` names no built-in Plant Scatter preset.
pub fn spawn_field_editor(commands: &mut Commands, parent: Entity, key: &str) -> Entity {
    let preset = PlantScatterPreset::named(key).expect("the Plant Scatter preset is built in");
    let field = preset.plant_scatter.clone();

    // The plant rows and the Add Plant button, above the panel.
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
                @FeathersButton { @caption: bsn! { @caption("+ Add Plant") } }
                AddPlant
                on(add_plant)
            ]
        })
        .insert(ChildOf(editor));
    for entry in field.plants.iter() {
        spawn_row(commands, list, *entry);
    }

    // The field's shared settings, all sections closed.
    let panel = spawn_param_panel(commands, editor, field, params::sections(), &[]);
    commands
        .entity(panel)
        .observe(|_: On<ParamsChanged>, mut commands: Commands| {
            commands.trigger(FieldEdited);
        });
    commands.entity(editor).insert(FieldEditor {
        preset,
        panel,
        list,
    });

    // Saving and loading, below the settings.
    commands
        .spawn_scene(bsn! {
            Node { display: Display::Flex, column_gap: px(6), flex_shrink: 0.0 }
            Children [
                @FeathersButton { @caption: bsn! { @caption("Save Preset…") } }
                on(save_field)
                --
                @FeathersButton { @caption: bsn! { @caption("Load Preset…") } }
                on(load_field_file)
            ]
        })
        .insert(ChildOf(editor));
    editor
}

/// Spawns a row showing `entry` as the last child of `list` and returns it.
fn spawn_row(commands: &mut Commands, list: Entity, entry: ScatterPlant) -> Entity {
    let names: Vec<String> = PlantPreset::all()
        .map(|p| p.name.to_string())
        .chain([CUSTOM.to_string()])
        .collect();
    let selected = choice_of(&entry.plant);
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
                    on(choose_plant_preset)
                    --
                    Node { width: px(52), flex_shrink: 0.0, justify_content: JustifyContent::End }
                    Children [ @label_small("") RowPart::PerM2 ]
                    --
                    Node { width: px(34), flex_shrink: 0.0, justify_content: JustifyContent::End }
                    Children [ @label_small("") RowPart::Share ]
                    --
                    @FeathersButton {
                        @caption: bsn! { @caption("Edit") },
                        @variant: ButtonVariant::Plain,
                    }
                    RowPart::Edit
                    on(toggle_plant_settings)
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

    // The plant's density settings, open.
    let share = spawn_param_panel(commands, row, entry, params::share_sections(), &[0]);
    commands
        .entity(share)
        .observe(|_: On<ParamsChanged>, mut commands: Commands| {
            commands.trigger(FieldEdited);
        });
    commands.entity(row).insert(PlantRow {
        share,
        plant: None,
        customising: false,
        takes_rest: false,
    });
    row
}

/// The row that `entity` belongs to.
fn row_of(
    entity: Entity,
    parents: &Query<&ChildOf>,
    rows: &Query<&mut PlantRow>,
) -> Option<Entity> {
    parents
        .iter_ancestors(entity)
        .find(|ancestor| rows.contains(*ancestor))
}

/// Shows a loaded field in the selected Plant Scatter's editor, replacing
/// every row. The preset, and so the suggested file name, stays as it was.
/// Run it with `World::run_system_cached_with`.
pub fn load_field(
    In(field): In<PlantScatter>,
    editor: Single<&FieldEditor, Without<Dormant>>,
    children: Query<&Children>,
    mut fields: Query<&mut ParamPanel<PlantScatter>>,
    mut commands: Commands,
) {
    show_field(&field, &editor, &children, &mut fields, &mut commands);
}

/// Replaces every row and the shared settings with `field`'s, and triggers
/// [`FieldEdited`].
fn show_field(
    field: &PlantScatter,
    editor: &FieldEditor,
    children: &Query<&Children>,
    fields: &mut Query<&mut ParamPanel<PlantScatter>>,
    commands: &mut Commands,
) {
    for row in children.get(editor.list).into_iter().flatten() {
        commands.entity(*row).despawn();
    }
    for entry in field.plants.iter() {
        spawn_row(commands, editor.list, *entry);
    }
    if let Ok(mut panel) = fields.get_mut(editor.panel) {
        panel.set_value(field.clone());
    }
    commands.trigger(FieldEdited);
}

/// Saves the selected Plant Scatter as a `.plant_scatter.yml` preset file
/// based on its preset.
fn save_field(_: On<Activate>, reader: FieldReader, mut files: FileOps) {
    let (Some(field), Some(base)) = (reader.field(), reader.preset()) else {
        return;
    };
    files.save(&format!("{}.plant_scatter.yml", base.key), move |path| {
        save_field_preset(field, base, path)
    });
}

/// Loads a `.plant_scatter.yml` preset file into the selected Plant
/// Scatter's editor.
fn load_field_file(_: On<Activate>, mut files: FileOps) {
    files.open(|text, world| {
        let field = read_field(text)?;
        world
            .run_system_cached_with(load_field, field)
            .map_err(|error| format!("Cannot show the Plant Scatter: {error}"))?;
        Ok(Done::new("Loaded the Plant Scatter"))
    });
}

/// Loads a `.plant.yml` preset file into the row whose button was pressed.
fn load_row_plant(
    activate: On<Activate>,
    parents: Query<&ChildOf>,
    rows: Query<&mut PlantRow>,
    mut files: FileOps,
) {
    let Some(row) = row_of(activate.event_target(), &parents, &rows) else {
        return;
    };
    files.open(move |text, world| {
        let plant = read_plant(text)?;
        world
            .run_system_cached_with(set_row_plant, (row, plant))
            .map_err(|error| format!("Cannot show the plant: {error}"))?;
        Ok(Done::new("Loaded the plant"))
    });
}

/// Makes `plant` the plant of `row`, if the row still exists.
fn set_row_plant(
    In((row, plant)): In<(Entity, PlantParams)>,
    rows: Query<&PlantRow>,
    mut shares: Query<&mut ParamPanel<ScatterPlant>>,
    mut plants: Query<&mut ParamPanel<PlantParams>>,
    mut commands: Commands,
) {
    let Ok(row) = rows.get(row) else {
        return;
    };
    if let Ok(mut share) = shares.get_mut(row.share) {
        let entry = ScatterPlant {
            plant,
            ..*share.value()
        };
        share.set_value(entry);
    }
    if let Some(mut panel) = row.plant.and_then(|panel| plants.get_mut(panel).ok()) {
        panel.set_value(plant);
    }
    commands.trigger(FieldEdited);
}

/// Adds [`ADDED_PLANT`] before the last row, unless the field has [`MAX_PLANTS`].
fn add_plant(
    _: On<Activate>,
    editor: Single<&FieldEditor, Without<Dormant>>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    let count = children.get(editor.list).map_or(0, |rows| rows.len());
    if count >= MAX_PLANTS {
        return;
    }
    let row = spawn_row(&mut commands, editor.list, ADDED_PLANT);
    commands
        .entity(editor.list)
        .insert_children(count.saturating_sub(1), &[row]);
    commands.trigger(FieldEdited);
}

/// Removes the row, unless it is the only one.
fn remove_row(
    activate: On<Activate>,
    parents: Query<&ChildOf>,
    rows: Query<&mut PlantRow>,
    editor: Single<&FieldEditor, Without<Dormant>>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    let count = children.get(editor.list).map_or(0, |rows| rows.len());
    if count <= 1 {
        return;
    }
    if let Some(row) = row_of(activate.event_target(), &parents, &rows) {
        commands.entity(row).despawn();
        commands.trigger(FieldEdited);
    }
}

/// Shows or hides the plant's settings.
fn toggle_plant_settings(
    activate: On<Activate>,
    parents: Query<&ChildOf>,
    mut rows: Query<&mut PlantRow>,
) {
    if let Some(entity) = row_of(activate.event_target(), &parents, &rows)
        && let Ok(mut row) = rows.get_mut(entity)
    {
        row.customising = !row.customising;
    }
}

/// Makes the chosen preset the row's plant. Choosing Custom opens the
/// settings of the plant shown.
#[allow(clippy::too_many_arguments)]
fn choose_plant_preset(
    change: On<ValueChange<Entity>>,
    options: Query<&OptionIndex>,
    parents: Query<&ChildOf>,
    mut rows: Query<&mut PlantRow>,
    mut shares: Query<&mut ParamPanel<ScatterPlant>>,
    mut plants: Query<&mut ParamPanel<PlantParams>>,
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
    let current = share.value().plant;

    match PlantPreset::all().nth(choice.0) {
        // The picker also reports choices made to match the plant; ignore those.
        Some(preset) if preset.plant == current => {}
        Some(preset) => {
            let entry = ScatterPlant {
                plant: preset.plant,
                ..*share.value()
            };
            share.set_value(entry);
            if let Some(mut panel) = row.plant.and_then(|panel| plants.get_mut(panel).ok()) {
                panel.set_value(preset.plant);
            }
            commands.trigger(FieldEdited);
        }
        None if choice_of(&current) == choice.0 => {}
        None => row.customising = true,
    }
}

/// Recomputes each row's share of its field, and hides clustering on the
/// row that fills the gaps, in every Plant Scatter editor.
fn refresh_rows(
    editors: Query<&FieldEditor>,
    children: Query<&Children>,
    fields: Query<&ParamPanel<PlantScatter>>,
    mut rows: Query<&mut PlantRow>,
    mut shares: Query<&mut ParamPanel<ScatterPlant>>,
) {
    for editor in &editors {
        // The editor's rows, in order, with their densities.
        let field_scale = fields
            .get(editor.panel)
            .map_or(1.0, |field| field.value().density_scale);
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

        // Hide clustering on the row that fills the gaps, touching panels only on change.
        for (entity, status) in entities.iter().zip(row_statuses(&densities, field_scale)) {
            let Ok(mut row) = rows.get_mut(*entity) else {
                continue;
            };
            if row.takes_rest == status.takes_rest {
                continue;
            }
            row.takes_rest = status.takes_rest;
            if let Ok(mut share) = shares.get_mut(row.share) {
                let hidden: &[&'static str] = if status.takes_rest {
                    &["clustering"]
                } else {
                    &[]
                };
                share.set_hidden_keys(hidden.iter().copied());
            }
        }
    }
}

/// Shows each row's preset, density and share, and which buttons can be
/// used, in every Plant Scatter editor.
#[allow(clippy::too_many_arguments)]
fn show_row_parts(
    reader: FieldReader,
    editors: Query<(Entity, &FieldEditor)>,
    children: Query<&Children>,
    rows: Query<&PlantRow>,
    shares: Query<&ParamPanel<ScatterPlant>>,
    parts: Query<&RowPart>,
    parents: Query<&ChildOf>,
    adders: Query<(), With<AddPlant>>,
    mut texts: Query<&mut Text>,
    mut variants: Query<&mut ButtonVariant>,
    disabled: Query<Has<InteractionDisabled>>,
    selects: SelectOptions,
    mut commands: Commands,
) {
    let set_disabled = |commands: &mut Commands, entity: Entity, off: bool| {
        if disabled.get(entity).is_ok_and(|was| was != off) {
            if off {
                commands.entity(entity).insert(InteractionDisabled);
            } else {
                commands.entity(entity).remove::<InteractionDisabled>();
            }
        }
    };

    for (editor_entity, editor) in &editors {
        // The editor's rows, and each one's status.
        let Some(field) = reader.field_of(editor_entity) else {
            continue;
        };
        let row_entities: Vec<Entity> = children
            .get(editor.list)
            .into_iter()
            .flatten()
            .copied()
            .collect();
        let densities: Vec<f32> = field.plants.iter().map(ScatterPlant::density).collect();
        let statuses = row_statuses(&densities, field.density_scale);

        for entity in children.iter_descendants(editor_entity) {
            // The editor's Add Plant button.
            if adders.contains(entity) {
                set_disabled(&mut commands, entity, row_entities.len() >= MAX_PLANTS);
                continue;
            }
            let Ok(part) = parts.get(entity) else {
                continue;
            };

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
            let Ok(entry) = shares.get(row.share).map(|share| *share.value()) else {
                continue;
            };
            let custom = choice_of(&entry.plant) == PlantPreset::all().count();

            match part {
                RowPart::Preset => selects.select(&mut commands, entity, choice_of(&entry.plant)),
                RowPart::PerM2 | RowPart::Share => {
                    let shown = match part {
                        RowPart::PerM2 => per_m2_text(status.per_m2),
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
                    // A custom plant's settings are always open.
                    set_disabled(&mut commands, entity, custom);
                    if let Ok(mut variant) = variants.get_mut(entity) {
                        variant.set_if_neq(ButtonVariant::selected(row.customising || custom));
                    }
                }
                RowPart::Remove => set_disabled(&mut commands, entity, !status.removable),
            }
        }
    }
}

/// Shows each row's plant settings while the row is customised or its plant
/// is custom, creating them the first time.
fn show_plant_settings(
    mut rows: Query<(Entity, &mut PlantRow)>,
    shares: Query<&ParamPanel<ScatterPlant>>,
    parents: Query<&ChildOf>,
    mut nodes: Query<&mut Node>,
    mut commands: Commands,
) {
    for (entity, mut row) in &mut rows {
        let Ok(entry) = shares.get(row.share).map(|share| *share.value()) else {
            continue;
        };
        let shown = row.customising || choice_of(&entry.plant) == PlantPreset::all().count();
        match row.plant {
            Some(panel) => {
                // Show or hide the settings together with their Load Plant button.
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
                    entry.plant,
                    crate::plant::params::sections(),
                    &[0],
                );
                commands.entity(panel).observe(edit_row_plant);
                commands
                    .spawn_scene(bsn! {
                        Node { flex_shrink: 0.0, margin: UiRect::top(px(4)) }
                        Children [
                            @FeathersButton { @caption: bsn! { @caption("Load Plant…") } }
                            on(load_row_plant)
                        ]
                    })
                    .insert(ChildOf(holder));
                row.plant = Some(panel);
            }
            None => {}
        }
    }
}

/// Makes a plant settings panel's edits the plant of the row it is in.
fn edit_row_plant(
    changed: On<ParamsChanged>,
    parents: Query<&ChildOf>,
    rows: Query<&PlantRow>,
    plants: Query<&ParamPanel<PlantParams>>,
    mut shares: Query<&mut ParamPanel<ScatterPlant>>,
    mut commands: Commands,
) {
    let row = parents
        .iter_ancestors(changed.entity)
        .find_map(|ancestor| rows.get(ancestor).ok());
    let (Some(row), Ok(plant)) = (row, plants.get(changed.entity)) else {
        return;
    };
    if let Ok(mut share) = shares.get_mut(row.share) {
        let entry = ScatterPlant {
            plant: *plant.value(),
            ..*share.value()
        };
        share.set_value(entry);
        commands.trigger(FieldEdited);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headless::{DEFAULT_SIZE, HeadlessStudio};

    /// The visible buttons of kind `part`, top to bottom, with whether each is disabled.
    fn buttons(studio: &mut HeadlessStudio, part: RowPart) -> Vec<(Vec2, bool)> {
        let world = studio.app_mut().world_mut();
        let mut query = world.query::<(Entity, &RowPart, Has<InteractionDisabled>)>();
        let found: Vec<(Entity, bool)> = query
            .iter(world)
            .filter(|(_, kind, _)| **kind == part)
            .map(|(entity, _, disabled)| (entity, disabled))
            .collect();
        let mut shown: Vec<(Vec2, bool)> = found
            .into_iter()
            .filter_map(|(entity, disabled)| Some((studio.center_of(entity)?, disabled)))
            .collect();
        shown.sort_by(|a, b| a.0.y.total_cmp(&b.0.y));
        shown
    }

    /// How many times `text` is visible.
    fn count(studio: &mut HeadlessStudio, text: &str) -> usize {
        studio.texts().iter().filter(|t| t.text == text).count()
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn plant_rows_can_be_added_customised_and_removed() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        studio.click("Plant Scatter").unwrap();
        studio.click("Meadow").unwrap();
        assert_eq!(buttons(&mut studio, RowPart::Remove).len(), 1);
        assert!(
            buttons(&mut studio, RowPart::Remove)[0].1,
            "the only row can't be removed"
        );

        // A clover row goes before the last row, which fills the gaps and so
        // has no clustering.
        studio.click("+ Add Plant").unwrap();
        let removes = buttons(&mut studio, RowPart::Remove);
        assert_eq!(removes.len(), 2);
        assert!(removes.iter().all(|(_, disabled)| !disabled));
        let clover = studio.find("Clover").unwrap();
        assert!(clover.y < studio.find("Meadow grass").unwrap().y);
        assert_eq!(count(&mut studio, "Clustering"), 1);

        // Edit shows the clover's settings; changing one makes it custom.
        let edit = buttons(&mut studio, RowPart::Edit)[0].0;
        studio.click_at(edit);
        let per_m2 = studio.find("Per m²").unwrap();
        studio.drag(Vec2::new(1240.0, per_m2.y), Vec2::new(1300.0, per_m2.y));
        assert!(studio.find("Custom").is_ok());
        assert!(
            buttons(&mut studio, RowPart::Edit)[0].1,
            "custom settings stay open"
        );

        // Removing the custom row leaves the meadow grass alone.
        let remove = buttons(&mut studio, RowPart::Remove)[0].0;
        studio.click_at(remove);
        assert_eq!(buttons(&mut studio, RowPart::Remove).len(), 1);
        assert!(studio.find("Custom").is_err() && studio.find("Per m²").is_err());
        assert_eq!(count(&mut studio, "100%"), 1);
    }
}
