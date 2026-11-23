//! The Deadwood tab's controls: preset, seed, and every dead wood setting.

use bevy::feathers::controls::{
    FeathersButton, FeathersNumberInput, FeathersSelect, HardLimit, NumberInputValue, OptionIndex,
    list_rows_from_strings,
};
use bevy::feathers::display::caption;
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui_widgets::{Activate, ValueChange};
use procgena_deadwood::{DeadwoodOptions, DeadwoodPreset};
use procgena_tree::Rng;

use super::{DeadwoodShow, START_PRESET, params, show_sections};
use crate::params::{ParamPanel, ParamPanelPlugin, ParamsChanged, spawn_param_panel};
use crate::scene::DeadwoodView;
use crate::ui::{LABEL_WIDTH, labelled_row};

/// Triggered after the dead wood settings change. Read them from the panel
/// named by [`DeadwoodEditor::panel`].
#[derive(Event, Debug)]
pub struct DeadwoodEdited {
    /// Whether the camera should reset to fit the new piece, as after
    /// choosing a preset or loading settings.
    pub reframe: bool,
}

/// The dead wood editor's state. There is one, on the Deadwood tab.
#[derive(Component)]
pub struct DeadwoodEditor {
    /// The preset last chosen.
    preset: DeadwoodPreset,
    /// The panel holding the edited options.
    panel: Entity,
    /// The panel holding what the tab shows: the view and cover.
    show_panel: Entity,
    /// Picks seeds for Random Seed.
    rng: Rng,
}

impl DeadwoodEditor {
    /// Suggests a file name without an extension from the chosen preset, in
    /// lowercase with underscores, such as `windthrow_log`.
    pub fn file_stem(&self) -> String {
        self.preset.name().to_lowercase().replace(' ', "_")
    }

    /// The panel entity holding the edited [`DeadwoodOptions`].
    pub fn panel(&self) -> Entity {
        self.panel
    }

    /// The panel entity holding the [`DeadwoodShow`] settings.
    pub fn show_panel(&self) -> Entity {
        self.show_panel
    }
}

/// Marks the seed field.
#[derive(Component, Default, Clone)]
struct DeadwoodSeed;

/// Makes the dead wood editor work. Needs the param panel plugins.
pub struct DeadwoodEditorPlugin;

impl Plugin for DeadwoodEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ParamPanelPlugin::<DeadwoodOptions>::default(),
            ParamPanelPlugin::<DeadwoodShow>::default(),
        ))
        .add_systems(PostUpdate, (show_seed, show_piece_settings));
    }
}

/// Spawns the dead wood editor, showing [`START_PRESET`], as the last child
/// of `parent`.
pub fn spawn_deadwood_editor(commands: &mut Commands, parent: Entity) {
    let names = DeadwoodPreset::ALL.map(DeadwoodPreset::name);
    let selected = DeadwoodPreset::ALL.iter().position(|p| *p == START_PRESET);
    let options = START_PRESET.options();
    let seed = i64::from(options.seed);

    // Preset, seed, and the seed button, above the panel.
    let preset: Box<dyn Scene> = Box::new(bsn! {
        @FeathersSelect { @options: list_rows_from_strings(names, selected) }
        Node { flex_grow: 1.0 }
        on(choose_preset)
    });
    let seed_field: Box<dyn Scene> = Box::new(bsn! {
        @FeathersNumberInput
        Node { flex_grow: 1.0 }
        NumberInputValue::I64({seed})
        HardLimit::i64(0..=i64::from(u32::MAX))
        DeadwoodSeed
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
                    padding: UiRect::left(px(LABEL_WIDTH + 8.0)),
                }
                Children [
                    @FeathersButton { @caption: bsn! { @caption("Random Seed") } }
                    on(random_seed)
                ]
            ]
        })
        .insert(ChildOf(parent))
        .id();

    // What the tab shows, open; then every piece setting, with Piece open.
    let show_panel = spawn_param_panel(
        commands,
        editor,
        DeadwoodShow::default(),
        show_sections(),
        &[0],
    );
    let panel = spawn_param_panel(commands, editor, options, params::sections(), &[0]);
    for edited in [show_panel, panel] {
        commands
            .entity(edited)
            .observe(|_: On<ParamsChanged>, mut commands: Commands| {
                commands.trigger(DeadwoodEdited { reframe: false });
            });
    }
    commands.entity(editor).insert(DeadwoodEditor {
        preset: START_PRESET,
        panel,
        show_panel,
        rng: Rng::new(std::process::id() as i32),
    });
}

/// Shows loaded dead wood settings in the editor, rebuilds the piece and
/// fits the camera to it. The chosen preset, and so the suggested file
/// name, stays as it was. Run it with `World::run_system_cached_with`.
pub fn load_deadwood(
    In(options): In<DeadwoodOptions>,
    editor: Single<&DeadwoodEditor>,
    mut panels: Query<&mut ParamPanel<DeadwoodOptions>>,
    mut commands: Commands,
) {
    replace(options, true, &editor, &mut panels, &mut commands);
}

/// Shows `options` in the editor's controls and triggers [`DeadwoodEdited`].
fn replace(
    options: DeadwoodOptions,
    reframe: bool,
    editor: &DeadwoodEditor,
    panels: &mut Query<&mut ParamPanel<DeadwoodOptions>>,
    commands: &mut Commands,
) {
    if let Ok(mut panel) = panels.get_mut(editor.panel) {
        panel.set_value(options);
        commands.trigger(DeadwoodEdited { reframe });
    }
}

/// Starts over from the chosen preset, and fits the camera to it.
fn choose_preset(
    change: On<ValueChange<Entity>>,
    rows: Query<&OptionIndex>,
    mut editor: Single<&mut DeadwoodEditor>,
    mut panels: Query<&mut ParamPanel<DeadwoodOptions>>,
    mut commands: Commands,
) {
    let Some(preset) = rows
        .get(change.value)
        .ok()
        .and_then(|index| DeadwoodPreset::ALL.get(index.0))
    else {
        return;
    };
    editor.preset = *preset;
    replace(preset.options(), true, &editor, &mut panels, &mut commands);
}

/// Uses a typed seed.
fn type_seed(
    change: On<ValueChange<i64>>,
    editor: Single<&DeadwoodEditor>,
    mut panels: Query<&mut ParamPanel<DeadwoodOptions>>,
    mut commands: Commands,
) {
    let (Ok(panel), Ok(seed)) = (panels.get(editor.panel), u32::try_from(change.value)) else {
        return;
    };
    // Ignore echoes of the seed already in use.
    if panel.value().seed == seed {
        return;
    }
    let mut options = panel.value().clone();
    options.seed = seed;
    replace(options, false, &editor, &mut panels, &mut commands);
}

/// Picks a random seed.
fn random_seed(
    _: On<Activate>,
    mut editor: Single<&mut DeadwoodEditor>,
    mut panels: Query<&mut ParamPanel<DeadwoodOptions>>,
    mut commands: Commands,
) {
    let Ok(panel) = panels.get(editor.panel) else {
        return;
    };
    let mut options = panel.value().clone();
    options.seed = editor.rng.next_u32() % 65536;
    replace(options, false, &editor, &mut panels, &mut commands);
}

/// Shows the piece's settings only while a single piece is shown; the
/// scatters don't use them.
fn show_piece_settings(
    editor: Single<&DeadwoodEditor>,
    shows: Query<&ParamPanel<DeadwoodShow>, Changed<ParamPanel<DeadwoodShow>>>,
    mut nodes: Query<&mut Node>,
) {
    let Ok(show) = shows.get(editor.show_panel) else {
        return;
    };
    let wanted = if show.value().view == DeadwoodView::Piece {
        Display::Flex
    } else {
        Display::None
    };
    if let Ok(mut node) = nodes.get_mut(editor.panel) {
        node.display = wanted;
    }
}

/// Shows the options' seed in the seed field whenever it changes.
fn show_seed(
    editor: Single<&DeadwoodEditor>,
    panels: Query<&ParamPanel<DeadwoodOptions>, Changed<ParamPanel<DeadwoodOptions>>>,
    fields: Query<(Entity, &NumberInputValue), With<DeadwoodSeed>>,
    mut commands: Commands,
) {
    let Ok(panel) = panels.get(editor.panel) else {
        return;
    };
    let seed = NumberInputValue::I64(i64::from(panel.value().seed));
    for (field, shown) in &fields {
        if *shown != seed {
            commands.entity(field).insert(seed);
        }
    }
}
