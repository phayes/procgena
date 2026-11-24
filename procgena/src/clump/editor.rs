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
use procgena_clump::{ClumpOptions, ClumpPreset};
use procgena_core::Rng;

use super::{Subject, subject_seed, with_subject_seed};
use crate::params::{ParamPanel, ParamsChanged};
use crate::scene::{ClumpHost, ClumpSettings};
use crate::studio::Dormant;
use crate::studio::inspector::{InspectorPlugin, Settings, edit_setting, spawn_meta_panel};
use crate::ui::{LABEL_WIDTH, labelled_row};

/// The panel holding a clump editor's clump settings.
pub type ClumpPanel = ParamPanel<Settings<ClumpOptions>>;

/// Largest seed the seed fields accept, and one more than the largest seed
/// the reroll buttons pick.
const MAX_SEED: i64 = i32::MAX as i64;
const REROLL_SEEDS: u32 = 65536;

/// Triggered after the selected clump's subject or clump settings change.
/// Read them with [`ClumpEditor::settings`].
#[derive(Event, Debug)]
pub struct ClumpEdited {
    /// Whether the camera should reset to fit the subject, as after choosing
    /// another subject.
    pub reframe: bool,
}

/// A clump editor's state, one per open clump.
#[derive(Component)]
pub struct ClumpEditor {
    /// The panel holding the edited clump settings.
    panel: Entity,
    /// The tree or rock the clump grows on.
    subject: ClumpHost,
    /// Picks seeds for the reroll buttons.
    rng: Rng,
}

impl ClumpEditor {
    /// The edited subject and clump, reading the clump from `panels`, or
    /// `None` before the panel is spawned.
    pub fn settings<'a>(
        &self,
        panels: impl FnOnce(Entity) -> Option<&'a ClumpPanel>,
    ) -> Option<ClumpSettings> {
        Some(ClumpSettings {
            subject: self.subject.clone(),
            clump: panels(self.panel)?.value().options().clone(),
        })
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
        app.add_plugins(InspectorPlugin::<ClumpOptions>::default())
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
    let start = super::settings(Subject::start().settings(), preset);

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
    let panel = spawn_meta_panel(
        commands,
        editor,
        &super::META,
        Settings::new(start.clump),
        &[0],
        &|_, _| Vec::new(),
    );
    commands
        .entity(panel)
        .observe(|_: On<ParamsChanged>, mut commands: Commands| {
            commands.trigger(ClumpEdited { reframe: false });
        });
    commands.entity(editor).insert(ClumpEditor {
        panel,
        subject: start.subject,
        rng: Rng::new(std::process::id()),
    });
    editor
}

/// Grows the clump on `subject`, unless it grows there already, and
/// triggers [`ClumpEdited`].
fn change_subject(
    editor: &mut ClumpEditor,
    subject: ClumpHost,
    reframe: bool,
    commands: &mut Commands,
) {
    if editor.subject != subject {
        editor.subject = subject;
        commands.trigger(ClumpEdited { reframe });
    }
}

/// Shows the chosen subject preset with its own seed, keeping the clump
/// settings, and fits the camera to it.
fn choose_subject(
    change: On<ValueChange<Entity>>,
    rows: Query<&OptionIndex>,
    mut editor: Single<&mut ClumpEditor, Without<Dormant>>,
    mut commands: Commands,
) {
    let Some(subject) = rows
        .get(change.value)
        .ok()
        .and_then(|index| Subject::all().get(index.0).copied())
    else {
        return;
    };
    change_subject(&mut editor, subject.settings(), true, &mut commands);
}

/// Uses a typed subject seed.
fn type_subject_seed(
    change: On<ValueChange<i64>>,
    mut editor: Single<&mut ClumpEditor, Without<Dormant>>,
    mut commands: Commands,
) {
    if let Some(subject) = with_subject_seed(&editor.subject, change.value) {
        change_subject(&mut editor, subject, false, &mut commands);
    }
}

/// Uses a typed clump seed.
fn type_clump_seed(
    change: On<ValueChange<i64>>,
    editor: Single<&ClumpEditor, Without<Dormant>>,
    mut panels: Query<&mut ClumpPanel>,
    mut commands: Commands,
) {
    if let Ok(seed) = u32::try_from(change.value) {
        edit_setting(
            &mut commands,
            &mut panels,
            editor.panel,
            "default_seed",
            seed.into(),
        );
    }
}

/// Picks a random subject seed, keeping the clump's.
fn reroll_subject(
    _: On<Activate>,
    mut editor: Single<&mut ClumpEditor, Without<Dormant>>,
    mut commands: Commands,
) {
    let seed = i64::from(editor.rng.next_u32() % REROLL_SEEDS);
    if let Some(subject) = with_subject_seed(&editor.subject, seed) {
        change_subject(&mut editor, subject, false, &mut commands);
    }
}

/// Picks a random clump seed, keeping the subject's.
fn reroll_clump(
    _: On<Activate>,
    mut editor: Single<&mut ClumpEditor, Without<Dormant>>,
    mut panels: Query<&mut ClumpPanel>,
    mut commands: Commands,
) {
    let seed = editor.rng.next_u32() % REROLL_SEEDS;
    edit_setting(
        &mut commands,
        &mut panels,
        editor.panel,
        "default_seed",
        seed.into(),
    );
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
    editors: Query<(Entity, Ref<ClumpEditor>)>,
    panels: Query<Ref<ClumpPanel>>,
    children: Query<&Children>,
    fields: SeedFields,
    mut commands: Commands,
) {
    for (entity, editor) in &editors {
        let Ok(panel) = panels.get(editor.panel) else {
            continue;
        };
        if !editor.is_changed() && !panel.is_changed() {
            continue;
        }
        let subject = NumberInputValue::I64(subject_seed(&editor.subject));
        let clump = NumberInputValue::I64(i64::from(panel.value().options().default_seed));

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
