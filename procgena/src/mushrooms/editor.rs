//! The Mushrooms tab's controls: preset, seed, and every mushroom setting.

use bevy::feathers::controls::{
    FeathersButton, FeathersNumberInput, FeathersSelect, HardLimit, NumberInputValue, OptionIndex,
    list_rows_from_strings,
};
use bevy::feathers::display::caption;
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui_widgets::{Activate, ValueChange};
use procgena_mushroom::{MushroomOptions, MushroomPreset};
use procgena_tree::Rng;

use super::{START_PRESET, params};
use crate::params::{ParamPanel, ParamPanelPlugin, ParamsChanged, spawn_param_panel};
use crate::ui::{LABEL_WIDTH, labelled_row};

/// Triggered after the mushroom settings change. Read them from the panel
/// named by [`MushroomEditor::panel`].
#[derive(Event, Debug)]
pub struct MushroomEdited {
    /// Whether the camera should reset to fit the new mushroom, as after
    /// choosing a preset or loading settings.
    pub reframe: bool,
}

/// The mushroom editor's state. There is one, on the Mushrooms tab.
#[derive(Component)]
pub struct MushroomEditor {
    /// The preset last chosen.
    preset: MushroomPreset,
    /// The panel holding the edited options.
    panel: Entity,
    /// Picks seeds for Random Seed.
    rng: Rng,
}

impl MushroomEditor {
    /// Suggests a file name without an extension from the chosen preset, in
    /// lowercase with underscores, such as `fly_agaric`.
    pub fn file_stem(&self) -> String {
        self.preset.name().to_lowercase().replace(' ', "_")
    }

    /// The panel entity holding the edited [`MushroomOptions`].
    pub fn panel(&self) -> Entity {
        self.panel
    }
}

/// Marks the seed field.
#[derive(Component, Default, Clone)]
struct MushroomSeed;

/// Makes the mushroom editor work. Needs the param panel plugins.
pub struct MushroomEditorPlugin;

impl Plugin for MushroomEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ParamPanelPlugin::<MushroomOptions>::default())
            .add_systems(PostUpdate, show_seed);
    }
}

/// Spawns the mushroom editor, showing [`START_PRESET`], as the last child of
/// `parent`.
pub fn spawn_mushroom_editor(commands: &mut Commands, parent: Entity) {
    let names = MushroomPreset::ALL.map(MushroomPreset::name);
    let selected = MushroomPreset::ALL.iter().position(|p| *p == START_PRESET);
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
        MushroomSeed
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

    // Every other setting; Cap starts open.
    let panel = spawn_param_panel(commands, editor, options, params::sections(), &[0]);
    commands
        .entity(panel)
        .observe(|_: On<ParamsChanged>, mut commands: Commands| {
            commands.trigger(MushroomEdited { reframe: false });
        });
    commands.entity(editor).insert(MushroomEditor {
        preset: START_PRESET,
        panel,
        rng: Rng::new(std::process::id() as i32),
    });
}

/// Shows loaded mushroom settings in the editor, rebuilds the mushroom and
/// fits the camera to it. The chosen preset, and so the suggested file name,
/// stays as it was. Run it with `World::run_system_cached_with`.
pub fn load_mushroom(
    In(options): In<MushroomOptions>,
    editor: Single<&MushroomEditor>,
    mut panels: Query<&mut ParamPanel<MushroomOptions>>,
    mut commands: Commands,
) {
    replace(options, true, &editor, &mut panels, &mut commands);
}

/// Shows `options` in the editor's controls and triggers [`MushroomEdited`].
fn replace(
    options: MushroomOptions,
    reframe: bool,
    editor: &MushroomEditor,
    panels: &mut Query<&mut ParamPanel<MushroomOptions>>,
    commands: &mut Commands,
) {
    if let Ok(mut panel) = panels.get_mut(editor.panel) {
        panel.set_value(options);
        commands.trigger(MushroomEdited { reframe });
    }
}

/// Starts over from the chosen preset, and fits the camera to it.
fn choose_preset(
    change: On<ValueChange<Entity>>,
    rows: Query<&OptionIndex>,
    mut editor: Single<&mut MushroomEditor>,
    mut panels: Query<&mut ParamPanel<MushroomOptions>>,
    mut commands: Commands,
) {
    let Some(preset) = rows
        .get(change.value)
        .ok()
        .and_then(|index| MushroomPreset::ALL.get(index.0))
    else {
        return;
    };
    editor.preset = *preset;
    replace(preset.options(), true, &editor, &mut panels, &mut commands);
}

/// Uses a typed seed.
fn type_seed(
    change: On<ValueChange<i64>>,
    editor: Single<&MushroomEditor>,
    mut panels: Query<&mut ParamPanel<MushroomOptions>>,
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
    mut editor: Single<&mut MushroomEditor>,
    mut panels: Query<&mut ParamPanel<MushroomOptions>>,
    mut commands: Commands,
) {
    let Ok(panel) = panels.get(editor.panel) else {
        return;
    };
    let mut options = panel.value().clone();
    options.seed = editor.rng.next_u32() % 65536;
    replace(options, false, &editor, &mut panels, &mut commands);
}

/// Shows the options' seed in the seed field whenever it changes.
fn show_seed(
    editor: Single<&MushroomEditor>,
    panels: Query<&ParamPanel<MushroomOptions>, Changed<ParamPanel<MushroomOptions>>>,
    fields: Query<(Entity, &NumberInputValue), With<MushroomSeed>>,
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
