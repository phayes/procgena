//! The Plant Field tab's controls: preset, one row per plant, and the
//! field's shared appearance.

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
use procgena_plant::MAX_PLANTS;
use procgena_plant::prelude::{FieldPlant, FieldPreset, PlantField, PlantParams, PlantPreset};

use super::{
    ADDED_PLANT, CUSTOM, START_PRESET, choice_of, params, per_m2_text, read_field, row_statuses,
};
use crate::export::{Done, FileOps, shown_name, write_file};
use crate::params::{ParamPanel, ParamPanelPlugin, ParamsChanged, spawn_param_panel};
use crate::plant::read_plant;
use crate::ui::{SelectOptions, labelled_row};

/// Triggered after the field changes. Read it with [`FieldReader::field`].
#[derive(Event, Debug)]
pub struct FieldEdited;

/// The field editor's state. There is one, on the Plant Field tab.
#[derive(Component)]
pub struct FieldEditor {
    /// The panel holding the field's shared settings.
    panel: Entity,
    /// The node whose children are the [`PlantRow`]s, in the field's order.
    list: Entity,
}

/// One plant of the field: a preset picker, its density and share, and
/// buttons to edit or remove it, above its density settings.
#[derive(Component)]
struct PlantRow {
    /// The panel holding the row's [`FieldPlant`].
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

/// Makes the field editor work. Needs the param panel plugins and
/// [`crate::plant::editor::PlantEditorPlugin`], which edits plants.
pub struct FieldEditorPlugin;

impl Plugin for FieldEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ParamPanelPlugin::<PlantField>::default(),
            ParamPanelPlugin::<FieldPlant>::default(),
        ))
        .add_systems(
            Update,
            (refresh_rows, show_row_parts, show_plant_settings).chain(),
        );
    }
}

/// Reads the edited field.
#[derive(SystemParam)]
pub struct FieldReader<'w, 's> {
    editors: Query<'w, 's, &'static FieldEditor>,
    children: Query<'w, 's, &'static Children>,
    rows: Query<'w, 's, &'static PlantRow>,
    shares: Query<'w, 's, &'static ParamPanel<FieldPlant>>,
    fields: Query<'w, 's, &'static ParamPanel<PlantField>>,
}

impl FieldReader<'_, '_> {
    /// The edited field: its plants, in row order, with its shared settings.
    /// `None` before the editor is spawned.
    pub fn field(&self) -> Option<PlantField> {
        let editor = self.editors.single().ok()?;
        let field = self.fields.get(editor.panel).ok()?;
        let plants: Vec<FieldPlant> = self
            .children
            .get(editor.list)
            .into_iter()
            .flatten()
            .filter_map(|row| self.rows.get(*row).ok())
            .filter_map(|row| self.shares.get(row.share).ok())
            .map(|share| *share.value())
            .collect();
        Some(PlantField {
            plants: plants.into(),
            ..field.value().clone()
        })
    }
}

/// The field shown when the studio opens.
///
/// # Panics
///
/// Panics if [`START_PRESET`] names no field preset.
pub fn start_field() -> PlantField {
    FieldPreset::named(START_PRESET)
        .expect("START_PRESET names a field preset")
        .field
        .clone()
}

/// Spawns the field editor, showing [`START_PRESET`], as the last child of `parent`.
pub fn spawn_field_editor(commands: &mut Commands, parent: Entity) {
    let field = start_field();
    let names: Vec<String> = FieldPreset::all().map(|p| p.name.to_string()).collect();
    let selected = FieldPreset::all().position(|p| p.key == START_PRESET);
    let preset: Box<dyn Scene> = Box::new(bsn! {
        @FeathersSelect { @options: list_rows_from_strings(names, selected) }
        Node { flex_grow: 1.0 }
        on(choose_field_preset)
    });

    // Preset, the plant rows, and the Add Plant button, above the panel.
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
    commands.entity(editor).insert(FieldEditor { panel, list });

    // Saving and loading, below the settings.
    commands
        .spawn_scene(bsn! {
            Node { display: Display::Flex, column_gap: px(6), flex_shrink: 0.0 }
            Children [
                @FeathersButton { @caption: bsn! { @caption("Save Field…") } }
                on(save_field)
                --
                @FeathersButton { @caption: bsn! { @caption("Load Field…") } }
                on(load_field_file)
            ]
        })
        .insert(ChildOf(editor));
}

/// Spawns a row showing `entry` as the last child of `list` and returns it.
fn spawn_row(commands: &mut Commands, list: Entity, entry: FieldPlant) -> Entity {
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

/// Replaces every row and the shared settings with the chosen field preset's.
fn choose_field_preset(
    change: On<ValueChange<Entity>>,
    options: Query<&OptionIndex>,
    editor: Single<&FieldEditor>,
    children: Query<&Children>,
    mut fields: Query<&mut ParamPanel<PlantField>>,
    mut commands: Commands,
) {
    if let Some(preset) = options
        .get(change.value)
        .ok()
        .and_then(|index| FieldPreset::all().nth(index.0))
    {
        show_field(
            &preset.field,
            &editor,
            &children,
            &mut fields,
            &mut commands,
        );
    }
}

/// Shows a loaded field in the editor, replacing every row. The preset picker
/// keeps its choice. Run it with `World::run_system_cached_with`.
pub fn load_field(
    In(field): In<PlantField>,
    editor: Single<&FieldEditor>,
    children: Query<&Children>,
    mut fields: Query<&mut ParamPanel<PlantField>>,
    mut commands: Commands,
) {
    show_field(&field, &editor, &children, &mut fields, &mut commands);
}

/// Replaces every row and the shared settings with `field`'s, and triggers
/// [`FieldEdited`].
fn show_field(
    field: &PlantField,
    editor: &FieldEditor,
    children: &Query<&Children>,
    fields: &mut Query<&mut ParamPanel<PlantField>>,
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

/// Saves the field as JSON, or as a `.field.ron` file when the chosen name
/// ends in `.ron`.
fn save_field(_: On<Activate>, reader: FieldReader, mut files: FileOps) {
    let Some(field) = reader.field() else {
        return;
    };
    files.save("field.json", move |path| {
        let ron = path.extension().is_some_and(|ext| ext == "ron");
        let text = if ron { field.to_ron() } else { field.to_json() };
        write_file(path, text.as_bytes())?;
        Ok(Done::new(format!("Saved {}", shown_name(path))))
    });
}

/// Loads a field file into the editor.
fn load_field_file(_: On<Activate>, mut files: FileOps) {
    files.open(|text, world| {
        let field = read_field(text)?;
        world
            .run_system_cached_with(load_field, field)
            .map_err(|error| format!("Cannot show the field: {error}"))?;
        Ok(Done::new("Loaded the field"))
    });
}

/// Loads a plant file into the row whose button was pressed.
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
    mut shares: Query<&mut ParamPanel<FieldPlant>>,
    mut plants: Query<&mut ParamPanel<PlantParams>>,
    mut commands: Commands,
) {
    let Ok(row) = rows.get(row) else {
        return;
    };
    if let Ok(mut share) = shares.get_mut(row.share) {
        let entry = FieldPlant {
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
    editor: Single<&FieldEditor>,
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
    editor: Single<&FieldEditor>,
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
    mut shares: Query<&mut ParamPanel<FieldPlant>>,
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
            let entry = FieldPlant {
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

/// Recomputes each row's share of the field, and hides clustering on the
/// row that fills the gaps.
fn refresh_rows(
    editors: Query<&FieldEditor>,
    children: Query<&Children>,
    fields: Query<&ParamPanel<PlantField>>,
    mut rows: Query<&mut PlantRow>,
    mut shares: Query<&mut ParamPanel<FieldPlant>>,
) {
    let Ok(editor) = editors.single() else {
        return;
    };
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

/// Shows each row's preset, density and share, and which buttons can be used.
#[allow(clippy::too_many_arguments)]
fn show_row_parts(
    reader: FieldReader,
    editors: Query<&FieldEditor>,
    children: Query<&Children>,
    rows: Query<&PlantRow>,
    shares: Query<&ParamPanel<FieldPlant>>,
    parts: Query<(Entity, &RowPart)>,
    parents: Query<&ChildOf>,
    adders: Query<Entity, With<AddPlant>>,
    mut texts: Query<&mut Text>,
    mut variants: Query<&mut ButtonVariant>,
    disabled: Query<Has<InteractionDisabled>>,
    selects: SelectOptions,
    mut commands: Commands,
) {
    let (Some(field), Ok(editor)) = (reader.field(), editors.single()) else {
        return;
    };
    let row_entities: Vec<Entity> = children
        .get(editor.list)
        .into_iter()
        .flatten()
        .copied()
        .collect();
    let densities: Vec<f32> = field.plants.iter().map(FieldPlant::density).collect();
    let statuses = row_statuses(&densities, field.density_scale);
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
        set_disabled(&mut commands, adder, row_entities.len() >= MAX_PLANTS);
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

/// Shows each row's plant settings while the row is customised or its plant
/// is custom, creating them the first time.
fn show_plant_settings(
    mut rows: Query<(Entity, &mut PlantRow)>,
    shares: Query<&ParamPanel<FieldPlant>>,
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
    mut shares: Query<&mut ParamPanel<FieldPlant>>,
    mut commands: Commands,
) {
    let row = parents
        .iter_ancestors(changed.entity)
        .find_map(|ancestor| rows.get(ancestor).ok());
    let (Some(row), Ok(plant)) = (row, plants.get(changed.entity)) else {
        return;
    };
    if let Ok(mut share) = shares.get_mut(row.share) {
        let entry = FieldPlant {
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
        studio.click("Plant Field").unwrap();
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
