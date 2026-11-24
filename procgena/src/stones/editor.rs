//! The Stones tab's controls: preset, seed, and every rock setting.

use bevy::feathers::controls::{
    FeathersButton, FeathersNumberInput, FeathersSelect, HardLimit, NumberInputValue, OptionIndex,
    list_rows_from_strings,
};
use bevy::feathers::display::caption;
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui_widgets::{Activate, ValueChange};
use procgena_core::Rng;
use procgena_stone::{StoneOptions, StonePreset};

use super::{START_PRESET, params};
use crate::params::{ParamPanel, ParamPanelPlugin, ParamsChanged, spawn_param_panel};
use crate::ui::{LABEL_WIDTH, labelled_row};

/// Triggered after the rock settings change. Read them from the panel named by
/// [`StoneEditor::panel`].
#[derive(Event, Debug)]
pub struct StoneEdited {
    /// Whether the camera should reset to fit the new rock, as after choosing
    /// a preset or loading settings.
    pub reframe: bool,
}

/// The rock editor's state. There is one, on the Stones tab.
#[derive(Component)]
pub struct StoneEditor {
    /// The preset last chosen.
    preset: &'static StonePreset,
    /// The panel holding the edited options.
    panel: Entity,
    /// Picks seeds for Random Seed.
    rng: Rng,
}

impl StoneEditor {
    /// Suggests a file name without an extension from the chosen preset: its
    /// key, such as `river_stone`.
    pub fn file_stem(&self) -> String {
        self.preset.key.to_string()
    }

    /// The preset last chosen, which saved files name as their base.
    pub fn preset(&self) -> &'static StonePreset {
        self.preset
    }

    /// The panel entity holding the edited [`StoneOptions`].
    pub fn panel(&self) -> Entity {
        self.panel
    }
}

/// Marks the seed field.
#[derive(Component, Default, Clone)]
struct StoneSeed;

/// Makes the rock editor work. Needs the param panel plugins.
pub struct StoneEditorPlugin;

impl Plugin for StoneEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ParamPanelPlugin::<StoneOptions>::default())
            .add_systems(PostUpdate, show_seed);
    }
}

/// Spawns the rock editor, showing [`START_PRESET`], as the last child of `parent`.
pub fn spawn_stone_editor(commands: &mut Commands, parent: Entity) {
    let names: Vec<String> = StonePreset::all().map(|p| p.name.to_string()).collect();
    let selected = StonePreset::all().position(|p| p.key == START_PRESET);
    let start = StonePreset::named(START_PRESET).expect("the start preset is built in");
    let options = start.stone.clone();
    let seed = i64::from(options.default_seed);

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
        StoneSeed
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

    // Every other setting; Shape starts open.
    let panel = spawn_param_panel(commands, editor, options, params::sections(), &[0]);
    commands
        .entity(panel)
        .observe(|_: On<ParamsChanged>, mut commands: Commands| {
            commands.trigger(StoneEdited { reframe: false });
        });
    commands.entity(editor).insert(StoneEditor {
        preset: start,
        panel,
        rng: Rng::new(std::process::id()),
    });
}

/// Shows loaded rock settings in the editor, rebuilds the rock and fits the
/// camera to it. The chosen preset, and so the suggested file name, stays as
/// it was. Run it with `World::run_system_cached_with`.
pub fn load_stone(
    In(options): In<StoneOptions>,
    editor: Single<&StoneEditor>,
    mut panels: Query<&mut ParamPanel<StoneOptions>>,
    mut commands: Commands,
) {
    replace(options, true, &editor, &mut panels, &mut commands);
}

/// Shows `options` in the editor's controls and triggers [`StoneEdited`].
fn replace(
    options: StoneOptions,
    reframe: bool,
    editor: &StoneEditor,
    panels: &mut Query<&mut ParamPanel<StoneOptions>>,
    commands: &mut Commands,
) {
    if let Ok(mut panel) = panels.get_mut(editor.panel) {
        panel.set_value(options);
        commands.trigger(StoneEdited { reframe });
    }
}

/// Starts over from the chosen preset, and fits the camera to it.
fn choose_preset(
    change: On<ValueChange<Entity>>,
    rows: Query<&OptionIndex>,
    mut editor: Single<&mut StoneEditor>,
    mut panels: Query<&mut ParamPanel<StoneOptions>>,
    mut commands: Commands,
) {
    let Some(preset) = rows
        .get(change.value)
        .ok()
        .and_then(|index| StonePreset::all().nth(index.0))
    else {
        return;
    };
    editor.preset = preset;
    replace(
        preset.stone.clone(),
        true,
        &editor,
        &mut panels,
        &mut commands,
    );
}

/// Uses a typed seed.
fn type_seed(
    change: On<ValueChange<i64>>,
    editor: Single<&StoneEditor>,
    mut panels: Query<&mut ParamPanel<StoneOptions>>,
    mut commands: Commands,
) {
    let (Ok(panel), Ok(seed)) = (panels.get(editor.panel), u32::try_from(change.value)) else {
        return;
    };
    // Ignore echoes of the seed already in use.
    if panel.value().default_seed == seed {
        return;
    }
    let mut options = panel.value().clone();
    options.default_seed = seed;
    replace(options, false, &editor, &mut panels, &mut commands);
}

/// Picks a random seed.
fn random_seed(
    _: On<Activate>,
    mut editor: Single<&mut StoneEditor>,
    mut panels: Query<&mut ParamPanel<StoneOptions>>,
    mut commands: Commands,
) {
    let Ok(panel) = panels.get(editor.panel) else {
        return;
    };
    let mut options = panel.value().clone();
    options.default_seed = editor.rng.next_u32() % 65536;
    replace(options, false, &editor, &mut panels, &mut commands);
}

/// Shows the options' seed in the seed field whenever it changes.
fn show_seed(
    editor: Single<&StoneEditor>,
    panels: Query<&ParamPanel<StoneOptions>, Changed<ParamPanel<StoneOptions>>>,
    fields: Query<(Entity, &NumberInputValue), With<StoneSeed>>,
    mut commands: Commands,
) {
    let Ok(panel) = panels.get(editor.panel) else {
        return;
    };
    let seed = NumberInputValue::I64(i64::from(panel.value().default_seed));
    for (field, shown) in &fields {
        if *shown != seed {
            commands.entity(field).insert(seed);
        }
    }
}
