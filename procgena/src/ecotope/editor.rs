//! An Ecotope document's editor: seed, ground, one row per Scatter, the
//! selected Scatter's settings, and saving and loading.

use bevy::feathers::constants::icons;
use bevy::feathers::controls::{
    ButtonVariant, FeathersButton, FeathersNumberInput, FeathersSelect, FeathersTextInput,
    FeathersTextInputContainer, HardLimit, NumberInputValue, OptionIndex, list_rows_from_strings,
};
use bevy::feathers::display::{caption, icon, label_small};
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::text::{EditableText, TextEdit};
use bevy::ui_widgets::{Activate, ValueChange};
use procgena_biome::{Ecotope, EcotopePreset, EcotopeScatter, ScatterKind};
use procgena_core::preset::Preset;
use procgena_core::{ChildKey, Rng};
use procgena_deadwood::{
    DeadwoodOptions, DeadwoodPreset, ScatterOptions as DeadwoodScatterOptions,
};
use procgena_mushroom::{
    MushroomOptions, MushroomPreset, ScatterOptions as MushroomScatterOptions,
};
use procgena_plant_bevy::PlantScatterOptions;
use procgena_stone::{StoneClass, StoneOptions, StonePreset};

use super::{
    EcotopeView, KINDS, add_scatter, file_text, kind_index, new_scatter, new_stone_class, params,
    read_ecotope, rename_scatter,
};
use crate::export::{Done, FileOps, file_key, shown_name, write_file};
use crate::grove::editor::{GroveEdited, GroveReader, spawn_tree_scatter_editor};
use crate::params::{ParamPanel, ParamPanelPlugin, ParamsChanged, Section, spawn_param_panel};
use crate::studio::Dormant;
use crate::studio::documents::pickers::{PresetPicked, choice_of, preset_choices};
use crate::studio::project::Presets;
use crate::ui::toast::{ShowToast, ToastKind};
use crate::ui::{LABEL_WIDTH, SelectOptions, labelled_row};
use procgena_core::meta::SettingPath;

/// Picker label for an asset whose settings match no built-in preset.
const CUSTOM: &str = "Custom";

/// Triggered after the selected Ecotope or its view changes. Read them with
/// [`EcotopeEditor::ecotope`] and [`EcotopeEditor::view`].
#[derive(Event, Debug)]
pub struct EcotopeEdited {
    /// Whether the camera should reset to fit the new patch, as after
    /// loading a file.
    pub reframe: bool,
}

/// An Ecotope editor's state, one per open Ecotope.
#[derive(Component)]
pub struct EcotopeEditor {
    /// The preset edited, which saved files name as their base.
    preset: &'static EcotopePreset,
    /// The Ecotope being edited.
    ecotope: Ecotope,
    /// The ground it is shown on.
    view: EcotopeView,
    /// The Scatter whose settings are shown, if any.
    selected: Option<ChildKey>,
    /// The kind `+ Add Scatter` adds, an index in [`KINDS`].
    add_kind: usize,
    /// The node holding one row per Scatter.
    list: Entity,
    /// The node holding the selected Scatter's settings.
    detail: Entity,
    /// The tree Scatter editor shown for a selected tree Scatter.
    tree_editor: Option<Entity>,
    /// Which parts must be spawned again from the edited Ecotope.
    stale: Stale,
    /// Picks seeds for Random Seed.
    rng: Rng,
}

impl EcotopeEditor {
    /// The Ecotope being edited.
    pub fn ecotope(&self) -> &Ecotope {
        &self.ecotope
    }

    /// The ground and patch size the Ecotope is shown with.
    pub fn view(&self) -> EcotopeView {
        self.view
    }

    /// Shows `ecotope`, with its first Scatter selected, and respawns every
    /// part of the editor.
    fn show(&mut self, ecotope: Ecotope) {
        self.selected = ecotope.order().first().map(|(key, _)| (*key).clone());
        self.ecotope = ecotope;
        self.stale = Stale::all();
    }
}

/// Which parts of the editor must be spawned again.
#[derive(Debug, Clone, Copy, Default)]
struct Stale {
    list: bool,
    detail: bool,
}

impl Stale {
    fn all() -> Self {
        Self {
            list: true,
            detail: true,
        }
    }
}

/// Where a settings panel's value belongs in the selected Scatter.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Part {
    /// Its priority and clearance.
    #[default]
    Order,
    /// A stone class's placement, by its place in the list.
    Class(usize),
    /// A stone class's stone shape.
    ClassStone(usize),
    /// A dead wood Scatter's placement.
    Deadwood,
    /// The piece a dead wood Scatter starts from.
    Piece,
    /// A mushroom Scatter's placement.
    MushroomScatter,
    /// A mushroom Scatter's species.
    Species,
    /// A plant Scatter's settings.
    Plants,
}

/// A value a settings panel edits, and how it is stored in a Scatter.
trait Stored: Clone + PartialEq + Send + Sync + 'static {
    /// Stores `self`, edited by the panel for `part`, in `scatter`.
    fn store(self, part: Part, scatter: &mut EcotopeScatter);
}

impl Stored for EcotopeScatter {
    fn store(self, _: Part, scatter: &mut EcotopeScatter) {
        scatter.priority = self.priority;
        scatter.clearance = self.clearance;
    }
}

impl Stored for StoneClass {
    fn store(self, part: Part, scatter: &mut EcotopeScatter) {
        if let (Part::Class(i), ScatterKind::Stone(options)) = (part, &mut scatter.scatter)
            && let Some(class) = options.classes.get_mut(i)
        {
            // The stone's shape is edited in its own panel.
            *class = StoneClass {
                stone: class.stone.clone(),
                ..self
            };
        }
    }
}

impl Stored for StoneOptions {
    fn store(self, part: Part, scatter: &mut EcotopeScatter) {
        if let (Part::ClassStone(i), ScatterKind::Stone(options)) = (part, &mut scatter.scatter)
            && let Some(class) = options.classes.get_mut(i)
        {
            class.stone = self;
        }
    }
}

impl Stored for DeadwoodScatterOptions {
    fn store(self, _: Part, scatter: &mut EcotopeScatter) {
        if let ScatterKind::Deadwood(options) = &mut scatter.scatter {
            // The piece is edited in its own panel.
            *options = DeadwoodScatterOptions {
                piece: options.piece.clone(),
                ..self
            };
        }
    }
}

impl Stored for DeadwoodOptions {
    fn store(self, _: Part, scatter: &mut EcotopeScatter) {
        if let ScatterKind::Deadwood(options) = &mut scatter.scatter {
            options.piece = self;
        }
    }
}

impl Stored for MushroomScatterOptions {
    fn store(self, _: Part, scatter: &mut EcotopeScatter) {
        if let ScatterKind::Mushroom { scatter, .. } = &mut scatter.scatter {
            *scatter = self;
        }
    }
}

impl Stored for PlantScatterOptions {
    fn store(self, _: Part, scatter: &mut EcotopeScatter) {
        if let ScatterKind::Plant(options) = &mut scatter.scatter {
            *options = self;
        }
    }
}

impl Stored for MushroomOptions {
    fn store(self, _: Part, scatter: &mut EcotopeScatter) {
        if let ScatterKind::Mushroom { mushroom, .. } = &mut scatter.scatter {
            *mushroom = self;
        }
    }
}

/// The asset an asset picker chooses: a stone class's stone, a dead wood
/// Scatter's piece or a mushroom Scatter's species.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
struct AssetPicker(Part);

/// Marks a Scatter's row, with its key.
#[derive(Component, Debug, Clone)]
struct ScatterRow(ChildKey);

/// Marks the seed field.
#[derive(Component, Default, Clone)]
struct EcotopeSeed;

/// Marks the key field, with the key to show in it once it is spawned.
#[derive(Component, Default, Clone)]
struct KeyField(Option<String>);

/// Makes Ecotope editors work. Needs the param panel plugins, and the
/// tree Scatter, stone, dead wood, mushroom and Plant Scatter editor
/// plugins, which register the panels it embeds.
pub struct EcotopeEditorPlugin;

impl Plugin for EcotopeEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ParamPanelPlugin::<EcotopeView>::default(),
            ParamPanelPlugin::<EcotopeScatter>::default(),
            ParamPanelPlugin::<StoneClass>::default(),
            ParamPanelPlugin::<DeadwoodScatterOptions>::default(),
            ParamPanelPlugin::<MushroomScatterOptions>::default(),
        ))
        .add_systems(Update, (rebuild, fill_key_field).chain())
        .add_systems(PostUpdate, (show_seed, show_asset_choices))
        .add_observer(edit_tree_scatter);
    }
}

/// Spawns an Ecotope editor for the built-in preset `key` as the last child
/// of `parent`, and returns the editor's entity.
///
/// # Panics
///
/// Panics if `key` names no built-in Ecotope preset.
pub fn spawn_ecotope_editor(commands: &mut Commands, parent: Entity, key: &str) -> Entity {
    let preset = EcotopePreset::named(key).expect("the Ecotope preset is built in");
    let seed = i64::from(preset.ecotope.default_seed);
    let seed_field: Box<dyn Scene> = Box::new(bsn! {
        @FeathersNumberInput
        Node { flex_grow: 1.0 }
        NumberInputValue::I64({seed})
        HardLimit::i64(0..=i64::from(u32::MAX))
        EcotopeSeed
        on(type_seed)
    });
    let column = Node {
        display: Display::Flex,
        flex_direction: FlexDirection::Column,
        flex_shrink: 0.0,
        row_gap: px(8),
        ..default()
    };

    // Seed and the seed button, then the ground.
    let editor = commands.spawn((column.clone(), ChildOf(parent))).id();
    commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0,
                row_gap: px(8),
            }
            Children [
                @labelled_row("Seed", seed_field)
                --
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    flex_shrink: 0.0,
                    padding: UiRect::left(px(LABEL_WIDTH + 8.0)),
                }
                Children [
                    @FeathersButton { @caption: bsn! { @caption("Random Seed") } }
                    on(random_seed)
                ]
            ]
        })
        .insert(ChildOf(editor));
    let view = spawn_param_panel(
        commands,
        editor,
        EcotopeView::default(),
        params::view_sections(),
        &[0],
    );
    commands.entity(view).observe(edit_view);

    // The Scatters, the Add Scatter controls and the selected Scatter.
    let list = commands.spawn((column.clone(), ChildOf(editor))).id();
    let kinds: Vec<String> = KINDS.iter().map(|kind| (*kind).to_owned()).collect();
    let kind: Box<dyn Scene> = Box::new(bsn! {
        @FeathersSelect { @options: list_rows_from_strings(kinds, Some(0)) }
        Node { flex_grow: 1.0, min_width: px(0) }
        on(choose_add_kind)
    });
    commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                column_gap: px(6),
            }
            Children [
                {vec![kind]}
                --
                @FeathersButton { @caption: bsn! { @caption("+ Add Scatter") } }
                on(add_new_scatter)
            ]
        })
        .insert(ChildOf(editor));
    let detail = commands.spawn((column, ChildOf(editor))).id();

    // Saving and loading, below the settings.
    commands
        .spawn_scene(bsn! {
            Node { display: Display::Flex, column_gap: px(6), flex_shrink: 0.0 }
            Children [
                @FeathersButton { @caption: bsn! { @caption("Save Preset…") } }
                on(save_ecotope)
                --
                @FeathersButton { @caption: bsn! { @caption("Load Preset…") } }
                on(load_ecotope_file)
            ]
        })
        .insert(ChildOf(editor));

    let mut state = EcotopeEditor {
        preset,
        ecotope: Ecotope::default(),
        view: EcotopeView::default(),
        selected: None,
        add_kind: 0,
        list,
        detail,
        tree_editor: None,
        stale: Stale::default(),
        rng: Rng::new(std::process::id()),
    };
    state.show(preset.ecotope.clone());
    commands.entity(editor).insert(state);
    editor
}

/// Respawns the parts of each editor that an edit made stale.
fn rebuild(
    mut editors: Query<&mut EcotopeEditor>,
    children: Query<&Children>,
    presets: Res<Presets>,
    mut commands: Commands,
) {
    let clear = |commands: &mut Commands, node: Entity| {
        for child in children.get(node).into_iter().flatten() {
            commands.entity(*child).despawn();
        }
    };
    for mut editor in &mut editors {
        if !(editor.stale.list || editor.stale.detail) {
            continue;
        }
        let stale = std::mem::take(&mut editor.stale);
        if stale.list {
            clear(&mut commands, editor.list);
            for (key, scatter) in editor.ecotope.order() {
                let selected = editor.selected.as_ref() == Some(key);
                spawn_row(&mut commands, editor.list, key, scatter, selected);
            }
        }
        if stale.detail {
            clear(&mut commands, editor.detail);
            editor.tree_editor = spawn_detail(&mut commands, &editor, &presets);
        }
    }
}

/// Spawns a row for the Scatter `key` as the last child of `list`: a button
/// selecting it, its kind and priority, and a button removing it.
fn spawn_row(
    commands: &mut Commands,
    list: Entity,
    key: &ChildKey,
    scatter: &EcotopeScatter,
    selected: bool,
) {
    let name = key.to_string();
    let about = format!(
        "{} · {}",
        KINDS[kind_index(&scatter.scatter)],
        scatter.priority
    );
    let variant = ButtonVariant::selected(selected);
    commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                column_gap: px(6),
            }
            Children [
                @FeathersButton { @caption: bsn! { @caption(name) }, @variant: variant }
                Node { flex_grow: 1.0, min_width: px(0) }
                on(select_scatter)
                --
                Node { width: px(110), flex_shrink: 0.0, justify_content: JustifyContent::End }
                Children [ @label_small(about) ]
                --
                @FeathersButton {
                    @caption: bsn! { @icon(icons::X) },
                    @variant: ButtonVariant::Plain,
                }
                on(remove_scatter)
            ]
        })
        .insert((ScatterRow(key.clone()), ChildOf(list)));
}

/// Spawns the selected Scatter's settings in the editor's detail node.
/// Returns the tree Scatter editor, for a tree Scatter.
fn spawn_detail(
    commands: &mut Commands,
    editor: &EcotopeEditor,
    presets: &Presets,
) -> Option<Entity> {
    let key = editor.selected.as_ref()?;
    let scatter = editor.ecotope.scatters.get(key)?;
    let detail = editor.detail;

    // Its key, kind and place in the order.
    let kinds: Vec<String> = KINDS.iter().map(|kind| (*kind).to_owned()).collect();
    let kind = kind_index(&scatter.scatter);
    let key_field: Box<dyn Scene> = Box::new(bsn! {
        @FeathersTextInputContainer
        Node { flex_grow: 1.0 }
        Children [ @FeathersTextInput KeyField({Some(key.to_string())}) ]
    });
    let kind_picker: Box<dyn Scene> = Box::new(bsn! {
        @FeathersSelect { @options: list_rows_from_strings(kinds, Some(kind)) }
        Node { flex_grow: 1.0 }
        on(choose_kind)
    });
    commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0,
                row_gap: px(8),
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
                    @labelled_row("Key", key_field)
                    Node { flex_grow: 1.0 }
                    --
                    @FeathersButton { @caption: bsn! { @caption("Rename") } }
                    on(rename_selected)
                ]
                --
                @labelled_row("Kind", kind_picker)
            ]
        })
        .insert(ChildOf(detail));
    bound_panel(
        commands,
        detail,
        scatter.clone(),
        params::scatter_sections(),
        Part::Order,
        true,
    );

    // The settings of its kind.
    match &scatter.scatter {
        ScatterKind::Tree(trees) => {
            return Some(spawn_tree_scatter_editor(commands, detail, trees.clone()));
        }
        ScatterKind::Stone(options) => {
            for (i, class) in options.classes.iter().enumerate() {
                let header = format!("Class {}", class.key);
                commands
                    .spawn_scene(bsn! {
                        Node {
                            display: Display::Flex,
                            flex_direction: FlexDirection::Row,
                            flex_shrink: 0.0,
                            align_items: AlignItems::Center,
                            column_gap: px(6),
                            margin: UiRect::top(px(6)),
                        }
                        Children [
                            Node { flex_grow: 1.0 }
                            Children [ @label_small(header) ]
                            --
                            @FeathersButton {
                                @caption: bsn! { @icon(icons::X) },
                                @variant: ButtonVariant::Plain,
                            }
                            StoneClassIndex({i})
                            on(remove_stone_class)
                        ]
                    })
                    .insert(ChildOf(detail));
                spawn_asset_picker::<StonePreset>(
                    commands,
                    detail,
                    "Stone",
                    &class.stone,
                    i,
                    presets,
                );
                bound_panel(
                    commands,
                    detail,
                    class.clone(),
                    params::stone_class_sections(),
                    Part::Class(i),
                    true,
                );
                bound_panel(
                    commands,
                    detail,
                    class.stone.clone(),
                    crate::stones::params::sections(),
                    Part::ClassStone(i),
                    false,
                );
            }
            commands
                .spawn_scene(bsn! {
                    Node { flex_shrink: 0.0 }
                    Children [
                        @FeathersButton { @caption: bsn! { @caption("+ Add Class") } }
                        on(add_stone_class)
                    ]
                })
                .insert(ChildOf(detail));
        }
        ScatterKind::Deadwood(options) => {
            spawn_asset_picker::<DeadwoodPreset>(
                commands,
                detail,
                "Piece",
                &options.piece,
                0,
                presets,
            );
            bound_panel(
                commands,
                detail,
                options.clone(),
                params::deadwood_sections(),
                Part::Deadwood,
                true,
            );
            bound_panel(
                commands,
                detail,
                options.piece.clone(),
                crate::deadwood::params::sections(),
                Part::Piece,
                false,
            );
        }
        ScatterKind::Mushroom { mushroom, scatter } => {
            spawn_asset_picker::<MushroomPreset>(commands, detail, "Species", mushroom, 0, presets);
            bound_panel(
                commands,
                detail,
                scatter.clone(),
                params::mushroom_sections(),
                Part::MushroomScatter,
                true,
            );
            bound_panel(
                commands,
                detail,
                mushroom.clone(),
                crate::mushrooms::params::sections(),
                Part::Species,
                false,
            );
        }
        ScatterKind::Plant(options) => {
            bound_panel(
                commands,
                detail,
                options.clone(),
                params::plant_sections(),
                Part::Plants,
                true,
            );
        }
    }
    None
}

/// Marks a stone class's remove button with the class's place in the list.
#[derive(Component, Default, Clone, Copy)]
struct StoneClassIndex(usize);

/// Spawns a panel editing `value`, stored in the selected Scatter as `part`,
/// as the last child of `parent`, with its first section open if `open`.
fn bound_panel<T: Stored>(
    commands: &mut Commands,
    parent: Entity,
    value: T,
    sections: Vec<Section<T>>,
    part: Part,
    open: bool,
) {
    let open: &[usize] = if open { &[0] } else { &[] };
    let panel = spawn_param_panel(commands, parent, value, sections, open);
    commands
        .entity(panel)
        .insert(part)
        .observe(store_panel::<T>);
}

/// Spawns a picker of `P`'s presets, the project's and the built-in ones,
/// and Custom, showing which one `options` is, for the asset of the
/// selected Scatter that `P` names: stone class `class`'s stone, the dead
/// wood piece or the mushroom species.
fn spawn_asset_picker<P: Preset>(
    commands: &mut Commands,
    parent: Entity,
    label: &'static str,
    options: &P::Options,
    class: usize,
    presets: &Presets,
) where
    P::Options: PartialEq + Clone,
{
    let part = asset_part::<P>(class);
    let choices = preset_choices::<P>(presets);
    let chosen = choice_of(&choices, options);
    let names: Vec<String> = choices
        .into_iter()
        .map(|choice| choice.name)
        .chain([CUSTOM.to_owned()])
        .collect();
    let picker: Box<dyn Scene> = Box::new(bsn! {
        @FeathersSelect { @options: list_rows_from_strings(names, Some(chosen)) }
        Node { flex_grow: 1.0, min_width: px(0) }
        AssetPicker({part})
        on(choose_asset)
    });
    commands
        .spawn_scene(bsn! { @labelled_row(label, picker) })
        .insert(ChildOf(parent));
}

/// The part of a Scatter that a picker of `P`'s presets chooses.
fn asset_part<P: Preset>(class: usize) -> Part {
    match P::KIND {
        "stone" => Part::ClassStone(class),
        "deadwood" => Part::Piece,
        _ => Part::Species,
    }
}

/// The picker index of the preset of `P` whose options equal `options`, or
/// the Custom entry after them.
fn asset_choice<P: Preset>(options: &P::Options, presets: &Presets) -> usize
where
    P::Options: PartialEq + Clone,
{
    choice_of(&preset_choices::<P>(presets), options)
}

/// Where the asset a picker of `part` chooses sits in the settings of the
/// Scatter `key`, such as `scatters.boulders.classes.0.stone`.
fn asset_path(key: &ChildKey, part: Part) -> Option<SettingPath> {
    let scatter = SettingPath::parse("scatters").child(key.as_str());
    match part {
        Part::ClassStone(i) => Some(scatter.child("classes").child(i.to_string()).child("stone")),
        Part::Piece => Some(scatter.child("piece")),
        Part::Species => Some(scatter.child("mushroom")),
        _ => None,
    }
}

/// Stores a panel's edited value in the selected Scatter.
fn store_panel<T: Stored>(
    changed: On<ParamsChanged>,
    panels: Query<(&ParamPanel<T>, &Part)>,
    editor: Single<&mut EcotopeEditor, Without<Dormant>>,
    mut commands: Commands,
) {
    let mut editor = editor.into_inner();
    let Ok((panel, part)) = panels.get(changed.entity) else {
        return;
    };
    let Some(key) = editor.selected.clone() else {
        return;
    };
    let Some(scatter) = editor.ecotope.scatters.get_mut(&key) else {
        return;
    };
    panel.value().clone().store(*part, scatter);
    // The row shows the priority, and the list is in priority order.
    if *part == Part::Order {
        editor.stale.list = true;
    }
    commands.trigger(EcotopeEdited { reframe: false });
}

/// Shows in each editor's asset pickers the preset their asset now
/// matches, or Custom.
fn show_asset_choices(
    editors: Query<(Entity, &EcotopeEditor), Changed<EcotopeEditor>>,
    children: Query<&Children>,
    pickers: Query<&AssetPicker>,
    selects: SelectOptions,
    presets: Res<Presets>,
    mut commands: Commands,
) {
    for (editor_entity, editor) in &editors {
        let Some(scatter) = editor
            .selected
            .as_ref()
            .and_then(|key| editor.ecotope.scatters.get(key))
        else {
            continue;
        };
        for entity in children.iter_descendants(editor_entity) {
            if let Ok(picker) = pickers.get(entity) {
                show_asset_choice(entity, picker, scatter, &selects, &presets, &mut commands);
            }
        }
    }
}

/// Shows in the asset picker `entity` the preset its asset in `scatter`
/// now matches, or Custom.
fn show_asset_choice(
    entity: Entity,
    picker: &AssetPicker,
    scatter: &EcotopeScatter,
    selects: &SelectOptions,
    presets: &Presets,
    commands: &mut Commands,
) {
    let choice = match (picker.0, &scatter.scatter) {
        (Part::ClassStone(i), ScatterKind::Stone(options)) => options
            .classes
            .get(i)
            .map(|class| asset_choice::<StonePreset>(&class.stone, presets)),
        (Part::Piece, ScatterKind::Deadwood(options)) => {
            Some(asset_choice::<DeadwoodPreset>(&options.piece, presets))
        }
        (Part::Species, ScatterKind::Mushroom { mushroom, .. }) => {
            Some(asset_choice::<MushroomPreset>(mushroom, presets))
        }
        _ => None,
    };
    if let Some(choice) = choice {
        selects.select(commands, entity, choice);
    }
}

/// Stores the selected Ecotope's embedded tree Scatter editor's edits.
fn edit_tree_scatter(
    _: On<GroveEdited>,
    reader: GroveReader,
    editor: Option<Single<&mut EcotopeEditor, Without<Dormant>>>,
    mut commands: Commands,
) {
    let Some(mut editor) = editor.map(Single::into_inner) else {
        return;
    };
    let (Some(tree_editor), Some(key)) = (editor.tree_editor, editor.selected.clone()) else {
        return;
    };
    let Some(trees) = reader.grove_of(tree_editor) else {
        return;
    };
    if let Some(scatter) = editor.ecotope.scatters.get_mut(&key)
        && scatter.scatter != ScatterKind::Tree(trees.clone())
    {
        scatter.scatter = ScatterKind::Tree(trees);
        commands.trigger(EcotopeEdited { reframe: false });
    }
}

/// Uses a typed seed.
fn type_seed(
    change: On<ValueChange<i64>>,
    editor: Single<&mut EcotopeEditor, Without<Dormant>>,
    mut commands: Commands,
) {
    let mut editor = editor.into_inner();
    let Ok(seed) = u32::try_from(change.value) else {
        return;
    };
    // Ignore echoes of the seed already in use.
    if editor.ecotope.default_seed != seed {
        editor.ecotope.default_seed = seed;
        commands.trigger(EcotopeEdited { reframe: false });
    }
}

/// Picks a random seed.
fn random_seed(
    _: On<Activate>,
    editor: Single<&mut EcotopeEditor, Without<Dormant>>,
    mut commands: Commands,
) {
    let mut editor = editor.into_inner();
    editor.ecotope.default_seed = editor.rng.next_u32() % 65536;
    commands.trigger(EcotopeEdited { reframe: false });
}

/// Shows each Ecotope's seed in its own seed field whenever it changes.
fn show_seed(
    editors: Query<(Entity, &EcotopeEditor), Changed<EcotopeEditor>>,
    children: Query<&Children>,
    fields: Query<&NumberInputValue, With<EcotopeSeed>>,
    mut commands: Commands,
) {
    for (entity, editor) in &editors {
        let seed = NumberInputValue::I64(i64::from(editor.ecotope.default_seed));
        for field in children.iter_descendants(entity) {
            if fields.get(field).is_ok_and(|shown| *shown != seed) {
                commands.entity(field).insert(seed);
            }
        }
    }
}

/// Uses the edited ground and patch size.
fn edit_view(
    changed: On<ParamsChanged>,
    panels: Query<&ParamPanel<EcotopeView>>,
    editor: Single<&mut EcotopeEditor, Without<Dormant>>,
    mut commands: Commands,
) {
    if let Ok(panel) = panels.get(changed.entity) {
        let reframe = panel.value().size != editor.view.size;
        editor.into_inner().view = *panel.value();
        commands.trigger(EcotopeEdited { reframe });
    }
}

/// Shows the settings of the Scatter whose row was clicked.
fn select_scatter(
    activate: On<Activate>,
    parents: Query<&ChildOf>,
    rows: Query<&ScatterRow>,
    editor: Single<&mut EcotopeEditor, Without<Dormant>>,
) {
    let mut editor = editor.into_inner();
    let Some(row) = parents
        .iter_ancestors(activate.event_target())
        .find_map(|ancestor| rows.get(ancestor).ok())
    else {
        return;
    };
    if editor.selected.as_ref() != Some(&row.0) {
        editor.selected = Some(row.0.clone());
        editor.stale.list = true;
        editor.stale.detail = true;
    }
}

/// Removes the Scatter whose row's button was pressed.
fn remove_scatter(
    activate: On<Activate>,
    parents: Query<&ChildOf>,
    rows: Query<&ScatterRow>,
    editor: Single<&mut EcotopeEditor, Without<Dormant>>,
    mut commands: Commands,
) {
    let mut editor = editor.into_inner();
    let Some(row) = parents
        .iter_ancestors(activate.event_target())
        .find_map(|ancestor| rows.get(ancestor).ok())
    else {
        return;
    };
    editor.ecotope.scatters.remove(&row.0);
    if editor.selected.as_ref() == Some(&row.0) {
        editor.selected = editor
            .ecotope
            .order()
            .first()
            .map(|(key, _)| (*key).clone());
        editor.stale.detail = true;
    }
    editor.stale.list = true;
    commands.trigger(EcotopeEdited { reframe: false });
}

/// Remembers the kind `+ Add Scatter` adds.
fn choose_add_kind(
    change: On<ValueChange<Entity>>,
    rows: Query<&OptionIndex>,
    editor: Single<&mut EcotopeEditor, Without<Dormant>>,
) {
    if let Ok(index) = rows.get(change.value) {
        editor.into_inner().add_kind = index.0;
    }
}

/// Adds a Scatter of the chosen kind and shows its settings.
fn add_new_scatter(
    _: On<Activate>,
    editor: Single<&mut EcotopeEditor, Without<Dormant>>,
    mut commands: Commands,
) {
    let mut editor = editor.into_inner();
    let kind = editor.add_kind;
    let key = add_scatter(&mut editor.ecotope, kind);
    editor.selected = Some(key);
    editor.stale.list = true;
    editor.stale.detail = true;
    commands.trigger(EcotopeEdited { reframe: false });
}

/// Gives the selected Scatter the key typed in the selected Ecotope's key
/// field. Shows why if the key can't be used.
fn rename_selected(
    _: On<Activate>,
    fields: Query<&EditableText, With<KeyField>>,
    children: Query<&Children>,
    editor: Single<(Entity, &mut EcotopeEditor), Without<Dormant>>,
    mut commands: Commands,
) {
    let (entity, mut editor) = editor.into_inner();
    let field = children
        .iter_descendants(entity)
        .find_map(|child| fields.get(child).ok());
    let (Some(from), Some(field)) = (editor.selected.clone(), field) else {
        return;
    };
    let typed = field.value().to_string();
    match rename_scatter(&mut editor.ecotope, &from, &typed) {
        Ok(to) if to == from => {}
        Ok(to) => {
            editor.selected = Some(to);
            editor.stale.list = true;
            editor.stale.detail = true;
            commands.trigger(EcotopeEdited { reframe: false });
        }
        Err(message) => commands.trigger(ShowToast {
            kind: ToastKind::Error,
            message,
        }),
    }
}

/// Shows the selected Scatter's key in a newly spawned key field.
fn fill_key_field(mut fields: Query<(&mut EditableText, &mut KeyField)>) {
    for (mut text, mut field) in &mut fields {
        if let Some(key) = field.0.take() {
            text.queue_edit(TextEdit::SelectAll);
            text.queue_edit(TextEdit::Insert(key.into()));
        }
    }
}

/// Replaces the selected Scatter with a new one of the chosen kind, keeping
/// its key, priority and clearance.
fn choose_kind(
    change: On<ValueChange<Entity>>,
    rows: Query<&OptionIndex>,
    editor: Single<&mut EcotopeEditor, Without<Dormant>>,
    mut commands: Commands,
) {
    let mut editor = editor.into_inner();
    let (Ok(index), Some(key)) = (rows.get(change.value), editor.selected.clone()) else {
        return;
    };
    let Some(scatter) = editor.ecotope.scatters.get_mut(&key) else {
        return;
    };
    if kind_index(&scatter.scatter) == index.0 {
        return;
    }
    scatter.scatter = new_scatter(index.0);
    editor.stale.list = true;
    editor.stale.detail = true;
    commands.trigger(EcotopeEdited { reframe: false });
}

/// Makes the chosen preset the asset its picker chooses. Custom keeps the
/// settings shown.
fn choose_asset(
    change: On<ValueChange<Entity>>,
    rows: Query<&OptionIndex>,
    pickers: Query<&AssetPicker>,
    editor: Single<(Entity, &mut EcotopeEditor), Without<Dormant>>,
    presets: Res<Presets>,
    mut commands: Commands,
) {
    let (entity, mut editor) = editor.into_inner();
    let (Ok(index), Ok(picker), Some(key)) = (
        rows.get(change.value),
        pickers.get(change.source),
        editor.selected.clone(),
    ) else {
        return;
    };
    let Some(scatter) = editor.ecotope.scatters.get_mut(&key) else {
        return;
    };
    let picked = match (picker.0, &mut scatter.scatter) {
        (Part::ClassStone(i), ScatterKind::Stone(options)) => options
            .classes
            .get_mut(i)
            .and_then(|class| pick::<StonePreset>(index.0, &mut class.stone, &presets)),
        (Part::Piece, ScatterKind::Deadwood(options)) => {
            pick::<DeadwoodPreset>(index.0, &mut options.piece, &presets)
        }
        (Part::Species, ScatterKind::Mushroom { mushroom, .. }) => {
            pick::<MushroomPreset>(index.0, mushroom, &presets)
        }
        _ => None,
    };
    if let Some(reference) = picked {
        // The asset's settings panel shows the new settings, and the
        // document names the preset when it saves.
        editor.stale.detail = true;
        commands.trigger(EcotopeEdited { reframe: false });
        if let Some(path) = asset_path(&key, picker.0) {
            commands.trigger(PresetPicked {
                editor: entity,
                path,
                reference,
            });
        }
    }
}

/// Sets `options` to the settings of `P`'s preset at `index` in the
/// pickers' list, and returns the reference naming it when they changed.
/// Custom, past the presets, changes nothing.
fn pick<P: Preset>(index: usize, options: &mut P::Options, presets: &Presets) -> Option<String>
where
    P::Options: PartialEq + Clone,
{
    let choice = preset_choices::<P>(presets).into_iter().nth(index)?;
    if &choice.options == options {
        return None;
    }
    *options = choice.options;
    Some(choice.reference)
}

/// Adds a class of river stones to the selected stone Scatter.
fn add_stone_class(
    _: On<Activate>,
    editor: Single<&mut EcotopeEditor, Without<Dormant>>,
    mut commands: Commands,
) {
    let mut editor = editor.into_inner();
    let Some(key) = editor.selected.clone() else {
        return;
    };
    if let Some(ScatterKind::Stone(options)) = editor
        .ecotope
        .scatters
        .get_mut(&key)
        .map(|scatter| &mut scatter.scatter)
    {
        let class = (1..)
            .map(|n| format!("stones_{n}"))
            .filter_map(|name| ChildKey::new(name).ok())
            .find(|name| options.classes.iter().all(|class| class.key != *name))
            .expect("a free class key");
        options.classes.push(new_stone_class(class));
        editor.stale.detail = true;
        commands.trigger(EcotopeEdited { reframe: false });
    }
}

/// Removes a class from the selected stone Scatter.
fn remove_stone_class(
    activate: On<Activate>,
    classes: Query<&StoneClassIndex>,
    editor: Single<&mut EcotopeEditor, Without<Dormant>>,
    mut commands: Commands,
) {
    let mut editor = editor.into_inner();
    let (Ok(class), Some(key)) = (
        classes.get(activate.event_target()),
        editor.selected.clone(),
    ) else {
        return;
    };
    if let Some(ScatterKind::Stone(options)) = editor
        .ecotope
        .scatters
        .get_mut(&key)
        .map(|scatter| &mut scatter.scatter)
        && class.0 < options.classes.len()
    {
        options.classes.remove(class.0);
        editor.stale.detail = true;
        commands.trigger(EcotopeEdited { reframe: false });
    }
}

/// Saves the selected Ecotope as a `.ecotope.yml` preset file based on its
/// document's preset.
fn save_ecotope(
    _: On<Activate>,
    editor: Single<&EcotopeEditor, Without<Dormant>>,
    mut files: FileOps,
) {
    let ecotope = editor.ecotope.clone();
    let base = editor.preset;
    files.save(&format!("{}.ecotope.yml", base.key), move |path| {
        let key = file_key(path, EcotopePreset::KIND);
        write_file(path, file_text(&ecotope, &key, Some(base)).as_bytes())?;
        Ok(Done::new(format!("Saved {}", shown_name(path))))
    });
}

/// Loads a `.ecotope.yml` preset file into the selected Ecotope's editor.
fn load_ecotope_file(_: On<Activate>, mut files: FileOps) {
    files.open_file(|path, text, world| {
        let key = file_key(path, EcotopePreset::KIND);
        let preset = read_ecotope(&key, text)?;
        world
            .run_system_cached_with(load_ecotope, preset.ecotope)
            .map_err(|error| format!("Cannot show the Ecotope: {error}"))?;
        Ok(Done::new(format!("Loaded the Ecotope {key}")))
    });
}

/// Shows a loaded Ecotope in the selected Ecotope's editor, and fits the
/// camera to it. The preset, and so the suggested file name, stays as it
/// was. Run it with `World::run_system_cached_with`.
pub fn load_ecotope(
    In(ecotope): In<Ecotope>,
    editor: Single<&mut EcotopeEditor, Without<Dormant>>,
    mut commands: Commands,
) {
    editor.into_inner().show(ecotope);
    commands.trigger(EcotopeEdited { reframe: true });
}

#[cfg(test)]
mod tests {
    //! These start the whole studio on the GPU, so they are ignored by default.

    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;
    use crate::ecotope::{START_PRESET, start_preset};
    use crate::headless::{DEFAULT_SIZE, HeadlessStudio, lock};
    use crate::scene::{EcotopeGround, EcotopeSubject, ViewerCommand};

    /// Opens the start Ecotope preset from the Library.
    fn open_start(studio: &mut HeadlessStudio) {
        studio.click("Ecotope").unwrap();
        studio.click(&start_preset().name).unwrap();
    }

    /// The selected editor's Ecotope and view.
    fn edited(studio: &mut HeadlessStudio) -> (Ecotope, EcotopeView) {
        let world = studio.app_mut().world_mut();
        let mut editors = world.query_filtered::<&EcotopeEditor, Without<Dormant>>();
        let editor = editors.single(world).unwrap();
        (editor.ecotope.clone(), editor.view)
    }

    /// The Ecotope in the last Ecotope `Show` among `commands`.
    fn last_sent(commands: &[ViewerCommand]) -> Ecotope {
        commands
            .iter()
            .rev()
            .find_map(|command| match command {
                ViewerCommand::Show(show) => show.settings::<EcotopeSubject>(),
                _ => None,
            })
            .map(|settings| settings.ecotope.clone())
            .expect("an Ecotope was sent")
    }

    /// Waits up to a minute for the overlay to report a placed Ecotope, and
    /// returns its line.
    fn placed_line(studio: &mut HeadlessStudio) -> String {
        for _ in 0..120 {
            if let Some(shown) = studio
                .texts()
                .into_iter()
                .find(|shown| shown.text.contains(" copies in "))
            {
                return shown.text;
            }
            studio.wait(Duration::from_millis(500));
        }
        panic!("the Ecotope was never placed: {:?}", studio.texts());
    }

    /// An empty folder for `test`'s files.
    fn folder(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("procgena-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn key(key: &'static str) -> ChildKey {
        ChildKey::from_static(key)
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn an_ecotope_document_places_its_ecotope_and_edits_its_scatters() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        let sent = studio.record::<ViewerCommand>();
        let start = start_preset().ecotope.clone();
        open_start(&mut studio);

        // The opened Ecotope is sent and placed, with one row per Scatter.
        assert_eq!(last_sent(&lock(&sent)), start);
        let line = placed_line(&mut studio);
        assert!(line.contains("in 6 Scatters"), "{line}");
        for key in start.scatters.keys() {
            assert!(
                studio.texts().iter().any(|t| t.text == key.as_str()),
                "{key}"
            );
        }

        // Selecting a Scatter shows its settings.
        studio.click("logs").unwrap();
        assert!(studio.find("Class boulders").is_err());
        assert!(studio.find("Mossy log").is_ok());
        studio.click("boulders").unwrap();
        assert!(studio.find("Class boulders").is_ok());
        assert!(studio.find("Mossy log").is_err());

        // Adding a Scatter sends it and selects it.
        studio.click("+ Add Scatter").unwrap();
        let added = last_sent(&lock(&sent));
        assert_eq!(added.scatters.len(), start.scatters.len() + 1);
        assert!(added.scatters.contains_key(&key("trees")));
        assert_eq!(added, edited(&mut studio).0);
        // The tree Scatter editor shows one row per species, with its
        // trees per hectare.
        let rows = studio
            .texts()
            .into_iter()
            .filter(|shown| shown.text.ends_with("/ha"))
            .count();
        assert!(rows > 0, "the tree Scatter editor shows");

        // Renaming it keeps its settings under the new key.
        let world = studio.app_mut().world_mut();
        let mut fields = world.query_filtered::<&mut EditableText, With<KeyField>>();
        let mut field = fields.single_mut(world).unwrap();
        field.queue_edit(TextEdit::SelectAll);
        field.queue_edit(TextEdit::Insert("young_pines".into()));
        studio.settle();
        studio.click("Rename").unwrap();
        let renamed = last_sent(&lock(&sent));
        assert_eq!(
            renamed.scatters.get(&key("young_pines")),
            added.scatters.get(&key("trees"))
        );
        assert!(!renamed.scatters.contains_key(&key("trees")));

        // A key another Scatter has is refused.
        let world = studio.app_mut().world_mut();
        let mut fields = world.query_filtered::<&mut EditableText, With<KeyField>>();
        let mut field = fields.single_mut(world).unwrap();
        field.queue_edit(TextEdit::SelectAll);
        field.queue_edit(TextEdit::Insert("pines".into()));
        studio.settle();
        studio.click("Rename").unwrap();
        assert!(studio.shows_text_starting("Cannot use the key pines"));
        assert_eq!(edited(&mut studio).0, renamed);

        // Removing it leaves the starting Scatters, placed again.
        let rows = {
            let world = studio.app_mut().world_mut();
            let mut rows = world.query::<(Entity, &ScatterRow)>();
            rows.iter(world)
                .find(|(_, row)| row.0 == key("young_pines"))
                .map(|(entity, _)| entity)
                .unwrap()
        };
        let world = studio.app_mut().world_mut();
        let remove = world
            .entity(rows)
            .get::<Children>()
            .unwrap()
            .last()
            .copied()
            .unwrap();
        let at = studio.center_of(remove).unwrap();
        studio.click_at(at);
        assert_eq!(last_sent(&lock(&sent)), start);
        assert!(placed_line(&mut studio).contains("in 6 Scatters"));
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn an_edit_redraws_only_the_scatters_it_moves() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        open_start(&mut studio);
        placed_line(&mut studio);

        // Flat ground places everything again.
        edit(&mut studio, |editor| {
            editor.view.ground = EcotopeGround::Flat
        });
        assert!(placed_line_ending(&mut studio, "6 redrawn").ends_with("6 redrawn"));

        // Thinning the fly agarics redraws them and the undergrowth below.
        edit(&mut studio, |editor| {
            let scatter = editor
                .ecotope
                .scatters
                .get_mut(&key("fly_agarics"))
                .unwrap();
            let ScatterKind::Mushroom { scatter, .. } = &mut scatter.scatter else {
                panic!("fly agarics are mushrooms");
            };
            scatter.density *= 0.5;
        });
        let line = placed_line_ending(&mut studio, "2 redrawn");
        assert!(line.ends_with("2 redrawn"), "{line}");
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn a_plant_scatter_is_added_edited_and_removed() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        open_start(&mut studio);
        placed_line(&mut studio);

        // A new plant Scatter is placed and drawn as grass.
        let mut added = None;
        edit(&mut studio, |editor| {
            added = Some(super::super::add_scatter(&mut editor.ecotope, 4));
        });
        let added = added.unwrap();
        assert_eq!(added.as_str(), "plants");
        let line = placed_line_ending(&mut studio, "1 redrawn");
        assert!(line.contains("in 7 Scatters"), "{line}");
        let fields = |studio: &mut HeadlessStudio| {
            let world = studio.app_mut().world_mut();
            world
                .query::<&procgena_plant_bevy::PlantScatter>()
                .iter(world)
                .count()
        };
        assert_eq!(fields(&mut studio), 2, "the undergrowth and the new plants");

        // Thinning it redraws it alone: nothing lies below it.
        edit(&mut studio, |editor| {
            let ScatterKind::Plant(options) =
                &mut editor.ecotope.scatters.get_mut(&added).unwrap().scatter
            else {
                panic!("a plant Scatter");
            };
            options.density_scale *= 0.5;
        });
        let line = placed_line_ending(&mut studio, "1 redrawn");
        assert!(line.ends_with("1 redrawn"), "{line}");

        // Removing it leaves the Ecotope as it was.
        edit(&mut studio, |editor| {
            editor.ecotope.scatters.remove(&added);
        });
        let line = placed_line(&mut studio);
        assert!(line.contains("in 6 Scatters"), "{line}");
        assert_eq!(fields(&mut studio), 1, "the undergrowth alone");
    }

    /// Applies `change` to the selected editor as an edit would.
    fn edit(studio: &mut HeadlessStudio, change: impl FnOnce(&mut EcotopeEditor)) {
        let world = studio.app_mut().world_mut();
        let mut editors = world.query_filtered::<&mut EcotopeEditor, Without<Dormant>>();
        change(&mut editors.single_mut(world).unwrap());
        world.trigger(EcotopeEdited { reframe: false });
        studio.settle();
    }

    /// Waits up to half a minute for the overlay to report a placement whose
    /// line ends with `end`, and returns the last placed line.
    fn placed_line_ending(studio: &mut HeadlessStudio, end: &str) -> String {
        let mut line = placed_line(studio);
        for _ in 0..60 {
            if line.ends_with(end) {
                break;
            }
            studio.wait(Duration::from_millis(500));
            line = placed_line(studio);
        }
        line
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn ecotopes_save_and_load() {
        let dir = folder("ecotope");
        let path = dir.join("mossy.ecotope.yml");
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        open_start(&mut studio);
        studio.click("+ Add Scatter").unwrap();
        let saved = edited(&mut studio).0;

        // The file buttons sit below the settings, so scroll down to them.
        let panel = studio.find("Seed").unwrap();
        studio.scroll(panel, -100.0);

        // Saving writes the Ecotope based on the document's preset; loading
        // it undoes later edits and keeps the document's preset.
        studio.answer_dialog(Some(path.clone()));
        studio.click("Save Preset…").unwrap();
        assert!(studio.find("Saved mossy.ecotope.yml").is_ok());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("base: pine_forest_floor"), "{text}");
        edit(&mut studio, |editor| editor.ecotope.default_seed += 1);
        assert_ne!(edited(&mut studio).0, saved);
        studio.answer_dialog(Some(path.clone()));
        studio.click("Load Preset…").unwrap();
        assert!(studio.find("Loaded the Ecotope mossy").is_ok());
        assert_eq!(edited(&mut studio).0, saved);
        let preset = |studio: &mut HeadlessStudio| {
            let world = studio.app_mut().world_mut();
            let mut editors = world.query_filtered::<&EcotopeEditor, Without<Dormant>>();
            editors.single(world).unwrap().preset.key.to_string()
        };
        assert_eq!(preset(&mut studio), START_PRESET);

        // Loading the file again after it changed shows the change.
        let changed = text.replace("ecotope:\n", "ecotope:\n  default_seed: 5\n");
        std::fs::write(&path, changed).unwrap();
        // Let the last message hide, so it covers no button.
        studio.wait(Duration::from_secs(5));
        studio.answer_dialog(Some(path.clone()));
        studio.click("Load Preset…").unwrap();
        assert_eq!(edited(&mut studio).0.default_seed, 5);
        assert_eq!(preset(&mut studio), START_PRESET);

        // A file that is not an Ecotope changes nothing.
        std::fs::write(dir.join("bad.ecotope.yml"), "not a preset").unwrap();
        studio.wait(Duration::from_secs(5));
        studio.answer_dialog(Some(dir.join("bad.ecotope.yml")));
        studio.click("Load Preset…").unwrap();
        assert!(studio.shows_text_starting("Not Ecotope settings"));
        assert_eq!(edited(&mut studio).0.default_seed, 5);
    }
}
