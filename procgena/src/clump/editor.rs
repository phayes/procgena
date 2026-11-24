//! A clump document's editor: subject preset and seed, clump seed, and every
//! clump setting.

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

use super::{Subject, params, settings, subject_seed, with_subject_seed};
use crate::params::{ParamPanel, ParamPanelPlugin, ParamsChanged, spawn_param_panel};
use crate::scene::ClumpSettings;
use crate::studio::Dormant;
use crate::ui::{LABEL_WIDTH, labelled_row};

/// Largest seed the seed fields accept, and one more than the largest seed
/// the reroll buttons pick.
const MAX_SEED: i64 = i32::MAX as i64;
const REROLL_SEEDS: u32 = 65536;

/// Triggered after the selected clump's subject or clump settings change.
/// Read them from the panel named by [`ClumpEditor::panel`].
#[derive(Event, Debug)]
pub struct ClumpEdited {
    /// Whether the camera should reset to fit the subject, as after choosing
    /// another subject.
    pub reframe: bool,
}

/// A clump editor's state, one per open clump.
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

/// Makes clump editors work. Needs the param panel plugins.
pub struct ClumpEditorPlugin;

impl Plugin for ClumpEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ParamPanelPlugin::<ClumpSettings>::default())
            .add_systems(PostUpdate, show_seeds);
    }
}

/// Spawns a clump editor for the built-in clump preset `key`, grown on
/// [`Subject::start`], as the last child of `parent`, and returns the
/// editor's entity.
///
/// # Panics
///
/// Panics if `key` names no built-in clump preset.
pub fn spawn_clump_editor(commands: &mut Commands, parent: Entity, key: &str) -> Entity {
    let preset = ClumpPreset::named(key).expect("the clump preset is built in");
    let start = settings(Subject::start().settings(), preset);

    // The subject preset, and each seed with its reroll button.
    let subjects: Vec<String> = Subject::all().into_iter().map(Subject::label).collect();
    let subject_index = Subject::all().iter().position(|s| *s == Subject::start());
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
    let seed = i64::from(start.clump.default_seed);
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
    editor
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
    editor: Single<&ClumpEditor, Without<Dormant>>,
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

/// Uses a typed subject seed.
fn type_subject_seed(
    change: On<ValueChange<i64>>,
    editor: Single<&ClumpEditor, Without<Dormant>>,
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
    editor: Single<&ClumpEditor, Without<Dormant>>,
    mut panels: Query<&mut ParamPanel<ClumpSettings>>,
    mut commands: Commands,
) {
    let (Some(mut settings), Ok(seed)) = (current(&editor, &panels), u32::try_from(change.value))
    else {
        return;
    };
    // Ignore echoes of the seed already in use.
    if settings.clump.default_seed == seed {
        return;
    }
    settings.clump.default_seed = seed;
    replace(settings, false, &editor, &mut panels, &mut commands);
}

/// Picks a random subject seed, keeping the clump's.
fn reroll_subject(
    _: On<Activate>,
    mut editor: Single<&mut ClumpEditor, Without<Dormant>>,
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
    mut editor: Single<&mut ClumpEditor, Without<Dormant>>,
    mut panels: Query<&mut ParamPanel<ClumpSettings>>,
    mut commands: Commands,
) {
    let Some(mut settings) = current(&editor, &panels) else {
        return;
    };
    settings.clump.default_seed = editor.rng.next_u32() % REROLL_SEEDS;
    replace(settings, false, &editor, &mut panels, &mut commands);
}

/// The subject and clump seed fields, with whether each is the subject's.
type SeedFields<'w, 's> = Query<
    'w,
    's,
    (&'static NumberInputValue, Has<SubjectSeed>),
    Or<(With<SubjectSeed>, With<ClumpSeed>)>,
>;

/// Shows each clump's subject and clump seeds in its own seed fields
/// whenever they change.
fn show_seeds(
    editors: Query<(Entity, &ClumpEditor)>,
    panels: Query<&ParamPanel<ClumpSettings>, Changed<ParamPanel<ClumpSettings>>>,
    children: Query<&Children>,
    fields: SeedFields,
    mut commands: Commands,
) {
    for (entity, editor) in &editors {
        let Ok(panel) = panels.get(editor.panel) else {
            continue;
        };
        let subject = NumberInputValue::I64(subject_seed(&panel.value().subject));
        let clump = NumberInputValue::I64(i64::from(panel.value().clump.default_seed));

        // Update only this editor's seed fields that show another seed.
        for field in children.iter_descendants(entity) {
            let Ok((shown, is_subject)) = fields.get(field) else {
                continue;
            };
            let seed = if is_subject { subject } else { clump };
            if *shown != seed {
                commands.entity(field).insert(seed);
            }
        }
    }
}
