//! The Trees tab's controls: preset, seed, and every tree setting.

use bevy::feathers::controls::{
    FeathersButton, FeathersNumberInput, FeathersSelect, HardLimit, NumberInputValue, OptionIndex,
    list_rows_from_strings,
};
use bevy::feathers::display::caption;
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui_widgets::{Activate, ValueChange};
use procgena_core::Rng;
use procgena_tree::{TreeOptions, TreePreset};

use super::{JIGGLE_AMOUNT, START_PRESET, params};
use crate::params::{ParamPanel, ParamPanelPlugin, ParamsChanged, spawn_param_panel};
use crate::ui::{LABEL_WIDTH, labelled_row};

/// Triggered after the tree settings change. Read them from the panel named by
/// [`TreeEditor::panel`].
#[derive(Event, Debug)]
pub struct TreeEdited;

/// The tree editor's state. There is one, on the Trees tab.
#[derive(Component)]
pub struct TreeEditor {
    /// The preset last chosen.
    preset: &'static TreePreset,
    /// The panel holding the edited options.
    panel: Entity,
    /// Picks seeds for Random Seed and Jiggle.
    rng: Rng,
}

impl TreeEditor {
    /// Suggests a file name without an extension from the chosen preset: its
    /// key, such as `oak_medium`.
    pub fn file_stem(&self) -> String {
        self.preset.key.to_string()
    }

    /// The preset last chosen, which saved files name as their base.
    pub fn preset(&self) -> &'static TreePreset {
        self.preset
    }

    /// The panel entity holding the edited [`TreeOptions`].
    pub fn panel(&self) -> Entity {
        self.panel
    }
}

/// Marks the seed field.
#[derive(Component, Default, Clone)]
struct TreeSeed;

/// Makes the tree editor work. Needs the param panel plugins.
pub struct TreeEditorPlugin;

impl Plugin for TreeEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ParamPanelPlugin::<TreeOptions>::default())
            .add_systems(PostUpdate, show_seed);
    }
}

/// Spawns the tree editor, showing [`START_PRESET`], as the last child of `parent`.
pub fn spawn_tree_editor(commands: &mut Commands, parent: Entity) {
    let names: Vec<String> = TreePreset::all().map(|p| p.name.to_string()).collect();
    let selected = TreePreset::all().position(|p| p.key == START_PRESET);
    let start = TreePreset::named(START_PRESET).expect("the start preset is built in");
    let options = start.tree.clone();
    let seed = options.default_seed;

    // Preset, seed, and the seed buttons, above the panel.
    let preset: Box<dyn Scene> = Box::new(bsn! {
        @FeathersSelect { @options: list_rows_from_strings(names, selected) }
        Node { flex_grow: 1.0 }
        on(choose_preset)
    });
    let seed_field: Box<dyn Scene> = Box::new(bsn! {
        @FeathersNumberInput
        Node { flex_grow: 1.0 }
        NumberInputValue::I32({seed})
        HardLimit::i32(i32::MIN..=i32::MAX)
        TreeSeed
        on(type_seed)
    });
    let editor = commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0,
                row_gap: px(8),
            }
            Children [
                @labelled_row("Preset", preset)
                --
                @labelled_row("Seed", seed_field)
                --
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    flex_shrink: 0.0,
                    column_gap: px(6),
                    padding: UiRect::left(px(LABEL_WIDTH + 8.0)),
                }
                Children [
                    @FeathersButton { @caption: bsn! { @caption("Random Seed") } }
                    on(random_seed)
                    --
                    @FeathersButton { @caption: bsn! { @caption("Jiggle") } }
                    on(jiggle)
                ]
            ]
        })
        .insert(ChildOf(parent))
        .id();

    // Every other setting; Bark starts open.
    let panel = spawn_param_panel(commands, editor, options, params::sections(), &[0]);
    commands
        .entity(panel)
        .observe(|_: On<ParamsChanged>, mut commands: Commands| {
            commands.trigger(TreeEdited);
        });
    commands.entity(editor).insert(TreeEditor {
        preset: start,
        panel,
        rng: Rng::new(std::process::id()),
    });
}

/// Shows loaded tree settings in the editor and rebuilds the tree. The
/// chosen preset, and so the suggested file name, stays as it was.
/// Run it with `World::run_system_cached_with`.
pub fn load_tree(
    In(options): In<TreeOptions>,
    editor: Single<&TreeEditor>,
    mut panels: Query<&mut ParamPanel<TreeOptions>>,
    mut commands: Commands,
) {
    replace(options, &editor, &mut panels, &mut commands);
}

/// Shows `options` in the editor's controls and triggers [`TreeEdited`].
fn replace(
    options: TreeOptions,
    editor: &TreeEditor,
    panels: &mut Query<&mut ParamPanel<TreeOptions>>,
    commands: &mut Commands,
) {
    if let Ok(mut panel) = panels.get_mut(editor.panel) {
        panel.set_value(options);
        commands.trigger(TreeEdited);
    }
}

/// Starts over from the chosen preset.
fn choose_preset(
    change: On<ValueChange<Entity>>,
    rows: Query<&OptionIndex>,
    mut editor: Single<&mut TreeEditor>,
    mut panels: Query<&mut ParamPanel<TreeOptions>>,
    mut commands: Commands,
) {
    let Some(preset) = rows
        .get(change.value)
        .ok()
        .and_then(|index| TreePreset::all().nth(index.0))
    else {
        return;
    };
    editor.preset = preset;
    replace(preset.tree.clone(), &editor, &mut panels, &mut commands);
}

/// Uses a typed seed.
fn type_seed(
    change: On<ValueChange<i32>>,
    editor: Single<&TreeEditor>,
    mut panels: Query<&mut ParamPanel<TreeOptions>>,
    mut commands: Commands,
) {
    let Ok(panel) = panels.get(editor.panel) else {
        return;
    };
    // Ignore echoes of the seed already in use.
    if panel.value().default_seed == change.value {
        return;
    }
    let mut options = panel.value().clone();
    options.default_seed = change.value;
    replace(options, &editor, &mut panels, &mut commands);
}

/// Picks a random seed.
fn random_seed(
    _: On<Activate>,
    mut editor: Single<&mut TreeEditor>,
    mut panels: Query<&mut ParamPanel<TreeOptions>>,
    mut commands: Commands,
) {
    let Ok(panel) = panels.get(editor.panel) else {
        return;
    };
    let mut options = panel.value().clone();
    options.default_seed = (editor.rng.next_u32() % 65536) as i32;
    replace(options, &editor, &mut panels, &mut commands);
}

/// Varies the shape slightly, with a new seed.
fn jiggle(
    _: On<Activate>,
    mut editor: Single<&mut TreeEditor>,
    mut panels: Query<&mut ParamPanel<TreeOptions>>,
    mut commands: Commands,
) {
    let Ok(panel) = panels.get(editor.panel) else {
        return;
    };
    let mut options = panel.value().clone();
    options.jiggle(JIGGLE_AMOUNT, &mut editor.rng);
    replace(options, &editor, &mut panels, &mut commands);
}

/// Shows the options' seed in the seed field whenever it changes.
fn show_seed(
    editor: Single<&TreeEditor>,
    panels: Query<&ParamPanel<TreeOptions>, Changed<ParamPanel<TreeOptions>>>,
    fields: Query<(Entity, &NumberInputValue), With<TreeSeed>>,
    mut commands: Commands,
) {
    let Ok(panel) = panels.get(editor.panel) else {
        return;
    };
    let seed = NumberInputValue::I32(panel.value().default_seed);
    for (field, shown) in &fields {
        if *shown != seed {
            commands.entity(field).insert(seed);
        }
    }
}
