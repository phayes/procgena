//! The Clumps tab's controls: subject preset and seed, clump preset and seed,
//! and every clump setting.

use bevy::feathers::controls::{
    FeathersButton, FeathersNumberInput, FeathersSelect, HardLimit, NumberInputValue, OptionIndex,
    list_rows_from_strings,
};
use bevy::feathers::display::caption;
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui_widgets::{Activate, ValueChange};
use procgena_clump::ClumpPreset;
use procgena_core::Rng;

use super::{
    START_PRESET, START_SUBJECT, Subject, params, preset_label, settings, start_settings,
    subject_seed, with_subject_seed,
};
use crate::params::{ParamPanel, ParamPanelPlugin, ParamsChanged, spawn_param_panel};
use crate::scene::ClumpSettings;
use crate::ui::{LABEL_WIDTH, labelled_row};

/// Largest seed the seed fields accept, and one more than the largest seed
/// the reroll buttons pick.
const MAX_SEED: i64 = i32::MAX as i64;
const REROLL_SEEDS: u32 = 65536;

/// Triggered after the subject or clump settings change. Read them from the
/// panel named by [`ClumpEditor::panel`].
#[derive(Event, Debug)]
pub struct ClumpEdited {
    /// Whether the camera should reset to fit the subject, as after choosing
    /// another subject.
    pub reframe: bool,
}

/// The clump editor's state. There is one, on the Clumps tab.
#[derive(Component)]
pub struct ClumpEditor {
    /// The panel holding the edited settings.
    panel: Entity,
    /// Picks seeds for the reroll buttons.
    rng: Rng,
}

impl ClumpEditor {
    /// The panel entity holding the edited [`ClumpSettings`].
    pub fn panel(&self) -> Entity {
        self.panel
    }
}

/// Marks the subject seed field.
#[derive(Component, Default, Clone)]
struct SubjectSeed;

/// Marks the clump seed field.
#[derive(Component, Default, Clone)]
struct ClumpSeed;

/// Makes the clump editor work. Needs the param panel plugins.
pub struct ClumpEditorPlugin;

impl Plugin for ClumpEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ParamPanelPlugin::<ClumpSettings>::default())
            .add_systems(PostUpdate, show_seeds);
    }
}

/// Spawns the clump editor, showing [`start_settings`], as the last child of `parent`.
pub fn spawn_clump_editor(commands: &mut Commands, parent: Entity) {
    let start = start_settings();

    // Subject and clump presets, each with its seed and reroll button.
    let subjects: Vec<String> = Subject::all().into_iter().map(Subject::label).collect();
    let subject_index = Subject::all().iter().position(|s| *s == START_SUBJECT);
    let subject_picker: Box<dyn Scene> = Box::new(bsn! {
        @FeathersSelect { @options: list_rows_from_strings(subjects, subject_index) }
        Node { flex_grow: 1.0 }
        on(choose_subject)
    });
    let seed = subject_seed(&start.subject);
    let subject_seed_field: Box<dyn Scene> = Box::new(bsn! {
        @FeathersNumberInput
        Node { flex_grow: 1.0 }
        NumberInputValue::I64({seed})
        HardLimit::i64(0..=MAX_SEED)
        SubjectSeed
        on(type_subject_seed)
    });
    let presets: Vec<String> = ClumpPreset::ALL.into_iter().map(preset_label).collect();
    let preset_index = ClumpPreset::ALL.iter().position(|p| *p == START_PRESET);
    let preset_picker: Box<dyn Scene> = Box::new(bsn! {
        @FeathersSelect { @options: list_rows_from_strings(presets, preset_index) }
        Node { flex_grow: 1.0 }
        on(choose_preset)
    });
    let seed = i64::from(start.clump.seed);
    let clump_seed_field: Box<dyn Scene> = Box::new(bsn! {
        @FeathersNumberInput
        Node { flex_grow: 1.0 }
        NumberInputValue::I64({seed})
        HardLimit::i64(0..=MAX_SEED)
        ClumpSeed
        on(type_clump_seed)
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
                @labelled_row("Subject", subject_picker)
                --
                @labelled_row("Subject Seed", subject_seed_field)
                --
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    flex_shrink: 0.0,
                    padding: UiRect::left(px(LABEL_WIDTH + 8.0)),
                }
                Children [
                    @FeathersButton { @caption: bsn! { @caption("Reroll Subject") } }
                    on(reroll_subject)
                ]
                --
                @labelled_row("Clump", preset_picker)
                --
                @labelled_row("Clump Seed", clump_seed_field)
                --
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    flex_shrink: 0.0,
                    padding: UiRect::left(px(LABEL_WIDTH + 8.0)),
                }
                Children [
                    @FeathersButton { @caption: bsn! { @caption("Reroll Clump") } }
                    on(reroll_clump)
                ]
            ]
        })
        .insert(ChildOf(parent))
        .id();

    // Every other setting; Shape starts open.
    let panel = spawn_param_panel(commands, editor, start, params::sections(), &[0]);
    commands
        .entity(panel)
        .observe(|_: On<ParamsChanged>, mut commands: Commands| {
            commands.trigger(ClumpEdited { reframe: false });
        });
    commands.entity(editor).insert(ClumpEditor {
        panel,
        rng: Rng::new(std::process::id()),
    });
}

/// Shows `settings` in the editor's controls and triggers [`ClumpEdited`].
fn replace(
    settings: ClumpSettings,
    reframe: bool,
    editor: &ClumpEditor,
    panels: &mut Query<&mut ParamPanel<ClumpSettings>>,
    commands: &mut Commands,
) {
    if let Ok(mut panel) = panels.get_mut(editor.panel) {
        panel.set_value(settings);
        commands.trigger(ClumpEdited { reframe });
    }
}

/// The edited settings, or `None` before the panel is spawned.
fn current(
    editor: &ClumpEditor,
    panels: &Query<&mut ParamPanel<ClumpSettings>>,
) -> Option<ClumpSettings> {
    panels
        .get(editor.panel)
        .ok()
        .map(|panel| panel.value().clone())
}

/// Shows the chosen subject preset with its own seed, keeping the clump
/// settings, and fits the camera to it.
fn choose_subject(
    change: On<ValueChange<Entity>>,
    rows: Query<&OptionIndex>,
    editor: Single<&ClumpEditor>,
    mut panels: Query<&mut ParamPanel<ClumpSettings>>,
    mut commands: Commands,
) {
    let Some(subject) = rows
        .get(change.value)
        .ok()
        .and_then(|index| Subject::all().get(index.0).copied())
    else {
        return;
    };
    let Some(mut settings) = current(&editor, &panels) else {
        return;
    };
    settings.subject = subject.settings();
    replace(settings, true, &editor, &mut panels, &mut commands);
}

/// Starts the clump over from the chosen preset, keeping the subject.
fn choose_preset(
    change: On<ValueChange<Entity>>,
    rows: Query<&OptionIndex>,
    editor: Single<&ClumpEditor>,
    mut panels: Query<&mut ParamPanel<ClumpSettings>>,
    mut commands: Commands,
) {
    let Some(preset) = rows
        .get(change.value)
        .ok()
        .and_then(|index| ClumpPreset::ALL.get(index.0).copied())
    else {
        return;
    };
    let Some(old) = current(&editor, &panels) else {
        return;
    };
    replace(
        settings(old.subject, preset),
        false,
        &editor,
        &mut panels,
        &mut commands,
    );
}

/// Uses a typed subject seed.
fn type_subject_seed(
    change: On<ValueChange<i64>>,
    editor: Single<&ClumpEditor>,
    mut panels: Query<&mut ParamPanel<ClumpSettings>>,
    mut commands: Commands,
) {
    let Some(mut settings) = current(&editor, &panels) else {
        return;
    };
    // Ignore echoes of the seed already in use.
    if subject_seed(&settings.subject) == change.value {
        return;
    }
    let Some(subject) = with_subject_seed(&settings.subject, change.value) else {
        return;
    };
    settings.subject = subject;
    replace(settings, false, &editor, &mut panels, &mut commands);
}

/// Uses a typed clump seed.
fn type_clump_seed(
    change: On<ValueChange<i64>>,
    editor: Single<&ClumpEditor>,
    mut panels: Query<&mut ParamPanel<ClumpSettings>>,
    mut commands: Commands,
) {
    let (Some(mut settings), Ok(seed)) = (current(&editor, &panels), u32::try_from(change.value))
    else {
        return;
    };
    // Ignore echoes of the seed already in use.
    if settings.clump.seed == seed {
        return;
    }
    settings.clump.seed = seed;
    replace(settings, false, &editor, &mut panels, &mut commands);
}

/// Picks a random subject seed, keeping the clump's.
fn reroll_subject(
    _: On<Activate>,
    mut editor: Single<&mut ClumpEditor>,
    mut panels: Query<&mut ParamPanel<ClumpSettings>>,
    mut commands: Commands,
) {
    let Some(mut settings) = current(&editor, &panels) else {
        return;
    };
    let seed = i64::from(editor.rng.next_u32() % REROLL_SEEDS);
    if let Some(subject) = with_subject_seed(&settings.subject, seed) {
        settings.subject = subject;
        replace(settings, false, &editor, &mut panels, &mut commands);
    }
}

/// Picks a random clump seed, keeping the subject's.
fn reroll_clump(
    _: On<Activate>,
    mut editor: Single<&mut ClumpEditor>,
    mut panels: Query<&mut ParamPanel<ClumpSettings>>,
    mut commands: Commands,
) {
    let Some(mut settings) = current(&editor, &panels) else {
        return;
    };
    settings.clump.seed = editor.rng.next_u32() % REROLL_SEEDS;
    replace(settings, false, &editor, &mut panels, &mut commands);
}

/// Shows the subject and clump seeds in their fields whenever they change.
fn show_seeds(
    editor: Single<&ClumpEditor>,
    panels: Query<&ParamPanel<ClumpSettings>, Changed<ParamPanel<ClumpSettings>>>,
    subject_fields: Query<(Entity, &NumberInputValue), With<SubjectSeed>>,
    clump_fields: Query<(Entity, &NumberInputValue), With<ClumpSeed>>,
    mut commands: Commands,
) {
    let Ok(panel) = panels.get(editor.panel) else {
        return;
    };
    let subject = NumberInputValue::I64(subject_seed(&panel.value().subject));
    let clump = NumberInputValue::I64(i64::from(panel.value().clump.seed));
    let fields = subject_fields
        .iter()
        .map(|field| (field, &subject))
        .chain(clump_fields.iter().map(|field| (field, &clump)));
    for ((field, shown), seed) in fields {
        if shown != seed {
            commands.entity(field).insert(*seed);
        }
    }
}
