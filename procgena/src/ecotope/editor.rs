//! The Ecotope tab's controls: preset, seed, ground, one row per Scatter,
//! the selected Scatter's settings, and saving and loading.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

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
use procgena_stone::{StoneClass, StoneOptions, StonePreset};

use super::{
    EcotopeView, KINDS, START_PRESET, add_scatter, file_text, kind_index, new_scatter,
    new_stone_class, params, read_ecotope, rename_scatter, start_preset,
};
use crate::export::{Done, FileOps, file_key, shown_name, write_file};
use crate::grove::editor::{GroveEdited, GroveReader, spawn_tree_scatter_editor};
use crate::params::{ParamPanel, ParamPanelPlugin, ParamsChanged, Section, spawn_param_panel};
use crate::ui::toast::{ShowToast, ToastKind};
use crate::ui::{LABEL_WIDTH, SelectOptions, labelled_row};

/// How often loaded files are checked for changes on disk.
const WATCH_INTERVAL: Duration = Duration::from_secs(1);

/// Picker label for an asset whose settings match no built-in preset.
const CUSTOM: &str = "Custom";

/// Triggered after the Ecotope or its view changes. Read them with
/// [`EcotopeEditor::ecotope`] and [`EcotopeEditor::view`].
#[derive(Event, Debug)]
pub struct EcotopeEdited {
    /// Whether the camera should reset to fit the new patch, as after
    /// choosing a preset or loading a file.
    pub reframe: bool,
}

/// One entry of the preset list: a built-in preset, or a loaded file.
#[derive(Debug, Clone)]
struct Entry {
    /// The name shown in the list.
    name: String,
    /// The Ecotope it holds.
    ecotope: Ecotope,
    /// The built-in preset that saved files name as their base.
    base: Option<&'static EcotopePreset>,
    /// The file it was read from, for a loaded file.
    file: Option<WatchedFile>,
}

/// A loaded file, and when it last changed on disk.
#[derive(Debug, Clone)]
struct WatchedFile {
    path: PathBuf,
    modified: Option<SystemTime>,
}

/// The Ecotope editor's state. There is one, on the Ecotope tab.
#[derive(Component)]
pub struct EcotopeEditor {
    /// The built-in presets, then each loaded file.
    entries: Vec<Entry>,
    /// The entry last chosen or loaded.
    chosen: usize,
    /// The Ecotope being edited.
    ecotope: Ecotope,
    /// The ground it is shown on.
    view: EcotopeView,
    /// The Scatter whose settings are shown, if any.
    selected: Option<ChildKey>,
    /// The kind `+ Add Scatter` adds, an index in [`KINDS`].
    add_kind: usize,
    /// The node holding the preset picker.
    preset_slot: Entity,
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

    /// The built-in preset that saved files name as their base, if any.
    fn base(&self) -> Option<&'static EcotopePreset> {
        self.entries.get(self.chosen).and_then(|entry| entry.base)
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
    presets: bool,
    list: bool,
    detail: bool,
}

impl Stale {
    fn all() -> Self {
        Self {
            presets: true,
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

/// Makes the Ecotope editor work. Needs the param panel plugins, and the
/// tree Scatter, stone, dead wood and mushroom editor plugins, which
/// register the panels it embeds.
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
        .add_systems(Update, (watch_files, rebuild, fill_key_field).chain())
        .add_systems(PostUpdate, (show_seed, show_asset_choices))
        .add_observer(edit_tree_scatter);
    }
}

/// Spawns the Ecotope editor, showing [`START_PRESET`], as the last child of
/// `parent`.
pub fn spawn_ecotope_editor(commands: &mut Commands, parent: Entity) {
    let start = start_preset();
    let seed = i64::from(start.ecotope.default_seed);
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

    // Preset, seed and the seed button, then the ground.
    let editor = commands.spawn((column.clone(), ChildOf(parent))).id();
    let preset_slot = commands.spawn((column.clone(), ChildOf(editor))).id();
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

    let entries = EcotopePreset::all()
        .map(|preset| Entry {
            name: preset.name.to_string(),
            ecotope: preset.ecotope.clone(),
            base: Some(preset),
            file: None,
        })
        .collect::<Vec<_>>();
    let chosen = entries
        .iter()
        .position(|entry| entry.base.is_some_and(|base| base.key == START_PRESET))
        .unwrap_or(0);
    let mut state = EcotopeEditor {
        entries,
        chosen,
        ecotope: Ecotope::default(),
        view: EcotopeView::default(),
        selected: None,
        add_kind: 0,
        preset_slot,
        list,
        detail,
        tree_editor: None,
        stale: Stale::default(),
        rng: Rng::new(std::process::id()),
    };
    state.show(start.ecotope.clone());
    commands.entity(editor).insert(state);
}

/// Respawns the parts of the editor that an edit made stale.
fn rebuild(editor: Single<&mut EcotopeEditor>, children: Query<&Children>, mut commands: Commands) {
    let mut editor = editor.into_inner();
    let stale = std::mem::take(&mut editor.stale);
    let clear = |commands: &mut Commands, node: Entity| {
        for child in children.get(node).into_iter().flatten() {
            commands.entity(*child).despawn();
        }
    };
    if stale.presets {
        clear(&mut commands, editor.preset_slot);
        spawn_preset_picker(&mut commands, &editor);
    }
    if stale.list {
        clear(&mut commands, editor.list);
        for (key, scatter) in editor.ecotope.order() {
            let selected = editor.selected.as_ref() == Some(key);
            spawn_row(&mut commands, editor.list, key, scatter, selected);
        }
    }
    if stale.detail {
        clear(&mut commands, editor.detail);
        editor.tree_editor = spawn_detail(&mut commands, &editor);
    }
}

/// Spawns the preset picker, listing every entry, in the editor's preset
/// slot.
fn spawn_preset_picker(commands: &mut Commands, editor: &EcotopeEditor) {
    let names: Vec<String> = editor
        .entries
        .iter()
        .map(|entry| entry.name.clone())
        .collect();
    let preset: Box<dyn Scene> = Box::new(bsn! {
        @FeathersSelect { @options: list_rows_from_strings(names, Some(editor.chosen)) }
        Node { flex_grow: 1.0 }
        on(choose_preset)
    });
    commands
        .spawn_scene(bsn! { @labelled_row("Preset", preset) })
        .insert(ChildOf(editor.preset_slot));
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
fn spawn_detail(commands: &mut Commands, editor: &EcotopeEditor) -> Option<Entity> {
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
                spawn_asset_picker::<StonePreset>(commands, detail, "Stone", &class.stone, i);
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
            spawn_asset_picker::<DeadwoodPreset>(commands, detail, "Piece", &options.piece, 0);
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
            spawn_asset_picker::<MushroomPreset>(commands, detail, "Species", mushroom, 0);
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

/// Spawns a picker of `P`'s built-in presets, and Custom, showing which
/// one `options` is, for the asset of the selected Scatter that `P` names:
/// stone class `class`'s stone, the dead wood piece or the mushroom
/// species.
fn spawn_asset_picker<P: Preset>(
    commands: &mut Commands,
    parent: Entity,
    label: &'static str,
    options: &P::Options,
    class: usize,
) where
    P::Options: PartialEq,
{
    let part = asset_part::<P>(class);
    let names: Vec<String> = P::all()
        .map(|preset| preset.name().to_owned())
        .chain([CUSTOM.to_owned()])
        .collect();
    let chosen = asset_choice::<P>(options);
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
fn asset_choice<P: Preset>(options: &P::Options) -> usize
where
    P::Options: PartialEq,
{
    P::all()
        .position(|preset| preset.options() == options)
        .unwrap_or_else(|| P::all().count())
}

/// Stores a panel's edited value in the selected Scatter.
fn store_panel<T: Stored>(
    changed: On<ParamsChanged>,
    panels: Query<(&ParamPanel<T>, &Part)>,
    editor: Single<&mut EcotopeEditor>,
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

/// Shows in each asset picker the preset its asset now matches, or Custom.
fn show_asset_choices(
    editor: Single<&EcotopeEditor, Changed<EcotopeEditor>>,
    pickers: Query<(Entity, &AssetPicker)>,
    selects: SelectOptions,
    mut commands: Commands,
) {
    let Some(scatter) = editor
        .selected
        .as_ref()
        .and_then(|key| editor.ecotope.scatters.get(key))
    else {
        return;
    };
    for (entity, picker) in &pickers {
        let choice = match (picker.0, &scatter.scatter) {
            (Part::ClassStone(i), ScatterKind::Stone(options)) => options
                .classes
                .get(i)
                .map(|class| asset_choice::<StonePreset>(&class.stone)),
            (Part::Piece, ScatterKind::Deadwood(options)) => {
                Some(asset_choice::<DeadwoodPreset>(&options.piece))
            }
            (Part::Species, ScatterKind::Mushroom { mushroom, .. }) => {
                Some(asset_choice::<MushroomPreset>(mushroom))
            }
            _ => None,
        };
        if let Some(choice) = choice {
            selects.select(&mut commands, entity, choice);
        }
    }
}

/// Stores the embedded tree Scatter editor's edits.
fn edit_tree_scatter(
    _: On<GroveEdited>,
    reader: GroveReader,
    editor: Option<Single<&mut EcotopeEditor>>,
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

/// Starts over from the chosen preset or file.
fn choose_preset(
    change: On<ValueChange<Entity>>,
    rows: Query<&OptionIndex>,
    editor: Single<&mut EcotopeEditor>,
    mut commands: Commands,
) {
    let mut editor = editor.into_inner();
    let Some(index) = rows.get(change.value).ok().map(|index| index.0) else {
        return;
    };
    let Some(entry) = editor.entries.get(index).cloned() else {
        return;
    };
    if index == editor.chosen && entry.ecotope == editor.ecotope {
        return;
    }
    editor.chosen = index;
    editor.show(entry.ecotope);
    commands.trigger(EcotopeEdited { reframe: true });
}

/// Uses a typed seed.
fn type_seed(
    change: On<ValueChange<i64>>,
    editor: Single<&mut EcotopeEditor>,
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
fn random_seed(_: On<Activate>, editor: Single<&mut EcotopeEditor>, mut commands: Commands) {
    let mut editor = editor.into_inner();
    editor.ecotope.default_seed = editor.rng.next_u32() % 65536;
    commands.trigger(EcotopeEdited { reframe: false });
}

/// Shows the Ecotope's seed in the seed field whenever it changes.
fn show_seed(
    editor: Single<&EcotopeEditor, Changed<EcotopeEditor>>,
    fields: Query<(Entity, &NumberInputValue), With<EcotopeSeed>>,
    mut commands: Commands,
) {
    let seed = NumberInputValue::I64(i64::from(editor.ecotope.default_seed));
    for (field, shown) in &fields {
        if *shown != seed {
            commands.entity(field).insert(seed);
        }
    }
}

/// Uses the edited ground and patch size.
fn edit_view(
    changed: On<ParamsChanged>,
    panels: Query<&ParamPanel<EcotopeView>>,
    editor: Single<&mut EcotopeEditor>,
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
    editor: Single<&mut EcotopeEditor>,
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
    editor: Single<&mut EcotopeEditor>,
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
    editor: Single<&mut EcotopeEditor>,
) {
    if let Ok(index) = rows.get(change.value) {
        editor.into_inner().add_kind = index.0;
    }
}

/// Adds a Scatter of the chosen kind and shows its settings.
fn add_new_scatter(_: On<Activate>, editor: Single<&mut EcotopeEditor>, mut commands: Commands) {
    let mut editor = editor.into_inner();
    let kind = editor.add_kind;
    let key = add_scatter(&mut editor.ecotope, kind);
    editor.selected = Some(key);
    editor.stale.list = true;
    editor.stale.detail = true;
    commands.trigger(EcotopeEdited { reframe: false });
}

/// Gives the selected Scatter the key typed in the key field. Shows why if
/// the key can't be used.
fn rename_selected(
    _: On<Activate>,
    fields: Query<&EditableText, With<KeyField>>,
    editor: Single<&mut EcotopeEditor>,
    mut commands: Commands,
) {
    let mut editor = editor.into_inner();
    let (Some(from), Ok(field)) = (editor.selected.clone(), fields.single()) else {
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
    editor: Single<&mut EcotopeEditor>,
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
    editor: Single<&mut EcotopeEditor>,
    mut commands: Commands,
) {
    let mut editor = editor.into_inner();
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
    let changed = match (picker.0, &mut scatter.scatter) {
        (Part::ClassStone(i), ScatterKind::Stone(options)) => options
            .classes
            .get_mut(i)
            .is_some_and(|class| pick::<StonePreset>(index.0, &mut class.stone)),
        (Part::Piece, ScatterKind::Deadwood(options)) => {
            pick::<DeadwoodPreset>(index.0, &mut options.piece)
        }
        (Part::Species, ScatterKind::Mushroom { mushroom, .. }) => {
            pick::<MushroomPreset>(index.0, mushroom)
        }
        _ => false,
    };
    if changed {
        // The asset's settings panel shows the new settings.
        editor.stale.detail = true;
        commands.trigger(EcotopeEdited { reframe: false });
    }
}

/// Sets `options` to the settings of `P`'s preset at `index`. Returns
/// whether they changed; Custom, past the presets, changes nothing.
fn pick<P: Preset>(index: usize, options: &mut P::Options) -> bool
where
    P::Options: PartialEq + Clone,
{
    match P::all().nth(index) {
        Some(preset) if preset.options() != options => {
            *options = preset.options().clone();
            true
        }
        _ => false,
    }
}

/// Adds a class of river stones to the selected stone Scatter.
fn add_stone_class(_: On<Activate>, editor: Single<&mut EcotopeEditor>, mut commands: Commands) {
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
    editor: Single<&mut EcotopeEditor>,
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

/// Saves the Ecotope as a `.ecotope.yml` preset file based on the chosen
/// preset.
fn save_ecotope(_: On<Activate>, editor: Single<&EcotopeEditor>, mut files: FileOps) {
    let ecotope = editor.ecotope.clone();
    let base = editor.base();
    let stem = base.map_or("ecotope", |base| base.key.as_ref()).to_owned();
    files.save(&format!("{stem}.ecotope.yml"), move |path| {
        let key = file_key(path, EcotopePreset::KIND);
        write_file(path, file_text(&ecotope, &key, base).as_bytes())?;
        Ok(Done::new(format!("Saved {}", shown_name(path))))
    });
}

/// Loads a `.ecotope.yml` preset file, adds it to the preset list and shows
/// it.
fn load_ecotope_file(_: On<Activate>, mut files: FileOps) {
    files.open_file(|path, text, world| {
        let key = file_key(path, EcotopePreset::KIND);
        let preset = read_ecotope(&key, text)?;
        let path = path.to_owned();
        world
            .run_system_cached_with(add_file, (path, preset))
            .map_err(|error| format!("Cannot show the Ecotope: {error}"))?;
        Ok(Done::new(format!("Loaded the Ecotope {key}")))
    });
}

/// Adds a loaded file to the preset list, or replaces the entry it already
/// has, and shows it. Run it with `World::run_system_cached_with`.
fn add_file(
    In((path, preset)): In<(PathBuf, EcotopePreset)>,
    editor: Single<&mut EcotopeEditor>,
    mut commands: Commands,
) {
    let mut editor = editor.into_inner();
    let entry = Entry {
        name: format!("File: {}", preset.name),
        base: EcotopePreset::all().find(|base| file_base(&path, base)),
        ecotope: preset.ecotope,
        file: Some(WatchedFile {
            modified: modified(&path),
            path,
        }),
    };
    let index = match editor
        .entries
        .iter()
        .position(|e| e.file.as_ref().map(|f| &f.path) == entry.file.as_ref().map(|f| &f.path))
    {
        Some(index) => {
            editor.entries[index] = entry;
            index
        }
        None => {
            editor.entries.push(entry);
            editor.entries.len() - 1
        }
    };
    editor.chosen = index;
    let ecotope = editor.entries[index].ecotope.clone();
    editor.show(ecotope);
    commands.trigger(EcotopeEdited { reframe: true });
}

/// Whether the file at `path` names `base` as its base.
fn file_base(path: &Path, base: &EcotopePreset) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| procgena_core::preset::Document::parse(EcotopePreset::KIND, &text).ok())
        .and_then(|document| document.base)
        .is_some_and(|key| key == base.key)
}

/// When the file at `path` last changed, if that can be read.
fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Reads loaded files again when they change on disk. The file shown
/// replaces the Ecotope being edited; a file that no longer reads keeps its
/// last settings and reports why.
fn watch_files(
    time: Res<Time<Real>>,
    mut last: Local<Duration>,
    editor: Option<Single<&mut EcotopeEditor>>,
    mut commands: Commands,
) {
    if time.elapsed().saturating_sub(*last) < WATCH_INTERVAL {
        return;
    }
    *last = time.elapsed();
    let Some(mut editor) = editor.map(Single::into_inner) else {
        return;
    };
    for index in 0..editor.entries.len() {
        // Only files whose change time moved.
        let Some(file) = &editor.entries[index].file else {
            continue;
        };
        let now = modified(&file.path);
        if now.is_none() || now == file.modified {
            continue;
        }
        let path = file.path.clone();
        if let Some(file) = &mut editor.entries[index].file {
            file.modified = now;
        }

        // Read it again, and show it if it is the one shown.
        let key = file_key(&path, EcotopePreset::KIND);
        let read = std::fs::read_to_string(&path)
            .map_err(|error| format!("Cannot read {}: {error}", path.display()))
            .and_then(|text| read_ecotope(&key, &text));
        match read {
            Ok(preset) => {
                editor.entries[index].name = format!("File: {}", preset.name);
                editor.entries[index].ecotope = preset.ecotope.clone();
                editor.stale.presets = true;
                if index == editor.chosen {
                    let selected = editor.selected.clone();
                    editor.show(preset.ecotope);
                    // Keep the same Scatter selected if it is still there.
                    if let Some(key) =
                        selected.filter(|key| editor.ecotope.scatters.contains_key(key))
                    {
                        editor.selected = Some(key);
                    }
                    commands.trigger(EcotopeEdited { reframe: false });
                }
                commands.trigger(ShowToast {
                    kind: ToastKind::Success,
                    message: format!("Reloaded {}", shown_name(&path)),
                });
            }
            Err(message) => commands.trigger(ShowToast {
                kind: ToastKind::Error,
                message,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    //! These start the whole studio on the GPU, so they are ignored by default.

    use std::time::Duration;

    use super::*;
    use crate::headless::{DEFAULT_SIZE, HeadlessStudio, lock};
    use crate::scene::{EcotopeGround, ViewerCommand};

    /// The editor's Ecotope and view.
    fn edited(studio: &mut HeadlessStudio) -> (Ecotope, EcotopeView) {
        let world = studio.app_mut().world_mut();
        let mut editors = world.query::<&EcotopeEditor>();
        let editor = editors.single(world).unwrap();
        (editor.ecotope.clone(), editor.view)
    }

    /// The Ecotope in the last `SetEcotope` among `commands`.
    fn last_sent(commands: &[ViewerCommand]) -> Ecotope {
        commands
            .iter()
            .rev()
            .find_map(|command| match command {
                ViewerCommand::SetEcotope(settings) => Some(settings.ecotope.clone()),
                _ => None,
            })
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
    fn the_ecotope_tab_places_its_ecotope_and_edits_its_scatters() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        studio.click("Ecotope").unwrap();
        let sent = studio.record::<ViewerCommand>();
        let start = start_preset().ecotope.clone();

        // The starting Ecotope is placed, with one row per Scatter.
        let line = placed_line(&mut studio);
        assert!(line.contains("in 5 Scatters"), "{line}");
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
        assert!(placed_line(&mut studio).contains("in 5 Scatters"));
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn an_edit_redraws_only_the_scatters_it_moves() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        studio.click("Ecotope").unwrap();
        placed_line(&mut studio);

        // Flat ground places everything again.
        edit(&mut studio, |editor| {
            editor.view.ground = EcotopeGround::Flat
        });
        assert!(placed_line_ending(&mut studio, "5 redrawn").ends_with("5 redrawn"));

        // Thinning the lowest Scatter redraws it alone.
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
        let line = placed_line_ending(&mut studio, "1 redrawn");
        assert!(line.ends_with("1 redrawn"), "{line}");
    }

    /// Applies `change` to the editor as an edit would.
    fn edit(studio: &mut HeadlessStudio, change: impl FnOnce(&mut EcotopeEditor)) {
        let world = studio.app_mut().world_mut();
        let mut editors = world.query::<&mut EcotopeEditor>();
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
    fn ecotopes_save_load_and_reload_when_their_file_changes() {
        let dir = folder("ecotope");
        let path = dir.join("mossy.ecotope.yml");
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        studio.click("Ecotope").unwrap();
        studio.click("+ Add Scatter").unwrap();
        let saved = edited(&mut studio).0;

        // The file buttons sit below the settings, so scroll down to them.
        let panel = studio.find("Preset").unwrap();
        studio.scroll(panel, -100.0);

        // Saving writes the Ecotope; loading it adds it to the list.
        studio.answer_dialog(Some(path.clone()));
        studio.click("Save Preset…").unwrap();
        assert!(studio.find("Saved mossy.ecotope.yml").is_ok());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("base: pine_forest_floor"), "{text}");
        studio.answer_dialog(Some(path.clone()));
        studio.click("Load Preset…").unwrap();
        assert!(studio.find("Loaded the Ecotope mossy").is_ok());
        assert_eq!(edited(&mut studio).0, saved);
        {
            let world = studio.app_mut().world_mut();
            let mut editors = world.query::<&EcotopeEditor>();
            let editor = editors.single(world).unwrap();
            assert_eq!(editor.entries.len(), EcotopePreset::all().count() + 1);
            assert_eq!(editor.chosen, editor.entries.len() - 1);
        }

        // Changing the file on disk shows the change.
        let changed = format!("{text}\n# edited\n").replace("default_seed: 11", "default_seed: 5");
        let changed = if changed.contains("default_seed: 5") {
            changed
        } else {
            changed.replace("ecotope:\n", "ecotope:\n  default_seed: 5\n")
        };
        std::thread::sleep(Duration::from_millis(1100));
        std::fs::write(&path, changed).unwrap();
        for _ in 0..20 {
            studio.wait(Duration::from_millis(500));
            if edited(&mut studio).0.default_seed == 5 {
                break;
            }
        }
        assert_eq!(edited(&mut studio).0.default_seed, 5);
        assert!(studio.find("Reloaded mossy.ecotope.yml").is_ok());

        // A file that is not an Ecotope changes nothing.
        std::fs::write(dir.join("bad.ecotope.yml"), "not a preset").unwrap();
        studio.answer_dialog(Some(dir.join("bad.ecotope.yml")));
        studio.click("Load Preset…").unwrap();
        assert!(studio.shows_text_starting("Not Ecotope settings"));
        assert_eq!(edited(&mut studio).0.default_seed, 5);
    }
}
