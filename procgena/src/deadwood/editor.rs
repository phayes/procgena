//! A dead wood document's editor: seed, what the view shows, and every
//! dead wood setting.

use bevy::feathers::controls::{FeathersButton, FeathersNumberInput, HardLimit, NumberInputValue};
use bevy::feathers::display::caption;
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui_widgets::{Activate, ValueChange};
use procgena_core::Rng;
use procgena_deadwood::{DeadwoodOptions, DeadwoodPreset};

use super::{DeadwoodShow, show_sections};
use crate::params::{ParamPanel, ParamPanelPlugin, ParamsChanged, spawn_param_panel};
use crate::scene::DeadwoodView;
use crate::studio::Dormant;
use crate::studio::inspector::{InspectorPlugin, Settings, edit_setting, spawn_meta_panel};
use crate::ui::{LABEL_WIDTH, labelled_row};

/// The panel holding a dead wood editor's piece settings.
pub type DeadwoodPanel = ParamPanel<Settings<DeadwoodOptions>>;

/// Triggered after the selected dead wood's settings change. Read them from
/// the panels named by [`DeadwoodEditor::panel`] and
/// [`DeadwoodEditor::show_panel`].
#[derive(Event, Debug)]
pub struct DeadwoodEdited {
    /// Whether the camera should reset to fit the new piece, as after
    /// loading settings or choosing another view.
    pub reframe: bool,
}

/// A dead wood editor's state, one per open piece of dead wood.
#[derive(Component)]
pub struct DeadwoodEditor {
    /// The preset edited.
    preset: &'static DeadwoodPreset,
    /// The panel holding the edited options.
    panel: Entity,
    /// The panel holding what the view shows: the view and cover.
    show_panel: Entity,
    /// The view last chosen in the show panel.
    view: DeadwoodView,
    /// Picks seeds for Random Seed.
    rng: Rng,
}

impl DeadwoodEditor {
    /// Suggests a file name without an extension from the preset: its key,
    /// such as `windthrow_log`.
    pub fn file_stem(&self) -> String {
        self.preset.key.to_string()
    }

    /// The preset edited, which saved files name as their base.
    pub fn preset(&self) -> &'static DeadwoodPreset {
        self.preset
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

/// Makes dead wood editors work. Needs the param panel plugins.
pub struct DeadwoodEditorPlugin;

impl Plugin for DeadwoodEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            InspectorPlugin::<DeadwoodOptions>::default(),
            ParamPanelPlugin::<DeadwoodShow>::default(),
        ))
        .add_systems(PostUpdate, (show_seed, show_piece_settings));
    }
}

/// Spawns a dead wood editor for the built-in preset `key` as the last
/// child of `parent`, and returns the editor's entity. It starts showing a
/// single bare piece.
///
/// # Panics
///
/// Panics if `key` names no built-in dead wood preset.
pub fn spawn_deadwood_editor(commands: &mut Commands, parent: Entity, key: &str) -> Entity {
    let preset = DeadwoodPreset::named(key).expect("the dead wood preset is built in");
    let options = preset.deadwood.clone();
    let seed = i64::from(options.default_seed);

    // Seed, and the seed button, above the panels.
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

    // What the view shows, open; then every piece setting, with Piece open.
    let show_panel = spawn_param_panel(
        commands,
        editor,
        DeadwoodShow::default(),
        show_sections(),
        &[0],
    );
    let panel = spawn_meta_panel(
        commands,
        editor,
        &super::META,
        Settings::new(options),
        &[0],
        &|_, _| Vec::new(),
    );
    commands.entity(show_panel).observe(change_show);
    commands
        .entity(panel)
        .observe(|_: On<ParamsChanged>, mut commands: Commands| {
            commands.trigger(DeadwoodEdited { reframe: false });
        });
    commands.entity(editor).insert(DeadwoodEditor {
        preset,
        panel,
        show_panel,
        view: DeadwoodShow::default().view,
        rng: Rng::new(std::process::id()),
    });
    editor
}

/// Shows loaded dead wood settings in the selected dead wood's editor,
/// rebuilds the piece and fits the camera to it. The preset, and so the
/// suggested file name, stays as it was. Run it with
/// `World::run_system_cached_with`.
pub fn load_deadwood(
    In(options): In<DeadwoodOptions>,
    editor: Single<&DeadwoodEditor, Without<Dormant>>,
    mut panels: Query<&mut DeadwoodPanel>,
    mut commands: Commands,
) {
    if let Ok(mut panel) = panels.get_mut(editor.panel) {
        panel.set_value(Settings::new(options));
        commands.trigger(DeadwoodEdited { reframe: true });
    }
}

/// Triggers [`DeadwoodEdited`] after the view or cover changes, fitting the
/// camera when the view is a new one.
fn change_show(
    changed: On<ParamsChanged>,
    mut editors: Query<&mut DeadwoodEditor>,
    shows: Query<&ParamPanel<DeadwoodShow>>,
    mut commands: Commands,
) {
    let Ok(show) = shows.get(changed.entity) else {
        return;
    };
    let view = show.value().view;

    // Note the new view on the editor that owns this panel.
    let mut reframe = false;
    for mut editor in &mut editors {
        if editor.show_panel == changed.entity {
            reframe = editor.view != view;
            editor.view = view;
        }
    }
    commands.trigger(DeadwoodEdited { reframe });
}

/// Uses a typed seed.
fn type_seed(
    change: On<ValueChange<i64>>,
    editor: Single<&DeadwoodEditor, Without<Dormant>>,
    mut panels: Query<&mut DeadwoodPanel>,
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

/// Picks a random seed.
fn random_seed(
    _: On<Activate>,
    mut editor: Single<&mut DeadwoodEditor, Without<Dormant>>,
    mut panels: Query<&mut DeadwoodPanel>,
    mut commands: Commands,
) {
    let seed = editor.rng.next_u32() % 65536;
    edit_setting(
        &mut commands,
        &mut panels,
        editor.panel,
        "default_seed",
        seed.into(),
    );
}

/// Shows each editor's piece settings only while it shows a single piece;
/// the scatters don't use them.
fn show_piece_settings(
    editors: Query<&DeadwoodEditor>,
    shows: Query<&ParamPanel<DeadwoodShow>, Changed<ParamPanel<DeadwoodShow>>>,
    mut nodes: Query<&mut Node>,
) {
    for editor in &editors {
        let Ok(show) = shows.get(editor.show_panel) else {
            continue;
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
}

/// Shows each piece's seed in its own seed field whenever it changes.
fn show_seed(
    editors: Query<(Entity, &DeadwoodEditor)>,
    panels: Query<&DeadwoodPanel, Changed<DeadwoodPanel>>,
    children: Query<&Children>,
    fields: Query<&NumberInputValue, With<DeadwoodSeed>>,
    mut commands: Commands,
) {
    for (entity, editor) in &editors {
        let Ok(panel) = panels.get(editor.panel) else {
            continue;
        };
        let seed = NumberInputValue::I64(i64::from(panel.value().options().default_seed));
        for field in children.iter_descendants(entity) {
            if fields.get(field).is_ok_and(|shown| *shown != seed) {
                commands.entity(field).insert(seed);
            }
        }
    }
}
