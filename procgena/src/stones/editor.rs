//! A rock document's editor: seed, and every rock setting.

use bevy::feathers::controls::{FeathersButton, FeathersNumberInput, HardLimit, NumberInputValue};
use bevy::feathers::display::caption;
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui_widgets::{Activate, ValueChange};
use procgena_core::Rng;
use procgena_stone::{StoneOptions, StonePreset};

use crate::params::{ParamPanel, ParamsChanged};
use crate::studio::Dormant;
use crate::studio::inspector::{InspectorPlugin, Settings, edit_setting, spawn_meta_panel};
use crate::ui::{LABEL_WIDTH, labelled_row};

/// The panel holding a rock editor's settings.
pub type StonePanel = ParamPanel<Settings<StoneOptions>>;

/// Triggered after the selected rock's settings change. Read them from the
/// panel named by [`StoneEditor::panel`].
#[derive(Event, Debug)]
pub struct StoneEdited {
    /// Whether the camera should reset to fit the new rock, as after loading
    /// settings.
    pub reframe: bool,
}

/// A rock editor's state, one per open rock.
#[derive(Component)]
pub struct StoneEditor {
    /// The preset edited.
    preset: &'static StonePreset,
    /// The panel holding the edited options.
    panel: Entity,
    /// Picks seeds for Random Seed.
    rng: Rng,
}

impl StoneEditor {
    /// Suggests a file name without an extension from the preset: its key,
    /// such as `river_stone`.
    pub fn file_stem(&self) -> String {
        self.preset.key.to_string()
    }

    /// The preset edited, which saved files name as their base.
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

/// Makes rock editors work. Needs the param panel plugins.
pub struct StoneEditorPlugin;

impl Plugin for StoneEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(InspectorPlugin::<StoneOptions>::default())
            .add_systems(PostUpdate, show_seed);
    }
}

/// Spawns a rock editor for the built-in preset `key` as the last child of
/// `parent`, and returns the editor's entity.
///
/// # Panics
///
/// Panics if `key` names no built-in rock preset.
pub fn spawn_stone_editor(commands: &mut Commands, parent: Entity, key: &str) -> Entity {
    let preset = StonePreset::named(key).expect("the stone preset is built in");
    let options = preset.stone.clone();
    let seed = i64::from(options.default_seed);

    // Seed, and the seed button, above the panel.
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
    let panel = spawn_meta_panel(
        commands,
        editor,
        &super::META,
        Settings::new(options),
        &[0],
        &|_, _| Vec::new(),
    );
    commands
        .entity(panel)
        .observe(|_: On<ParamsChanged>, mut commands: Commands| {
            commands.trigger(StoneEdited { reframe: false });
        });
    commands.entity(editor).insert(StoneEditor {
        preset,
        panel,
        rng: Rng::new(std::process::id()),
    });
    editor
}

/// Shows loaded rock settings in the selected rock's editor, rebuilds the
/// rock and fits the camera to it. The preset, and so the suggested file
/// name, stays as it was. Run it with `World::run_system_cached_with`.
pub fn load_stone(
    In(options): In<StoneOptions>,
    editor: Single<&StoneEditor, Without<Dormant>>,
    mut panels: Query<&mut StonePanel>,
    mut commands: Commands,
) {
    if let Ok(mut panel) = panels.get_mut(editor.panel) {
        panel.set_value(Settings::new(options));
        commands.trigger(StoneEdited { reframe: true });
    }
}

/// Uses a typed seed.
fn type_seed(
    change: On<ValueChange<i64>>,
    editor: Single<&StoneEditor, Without<Dormant>>,
    mut panels: Query<&mut StonePanel>,
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
    mut editor: Single<&mut StoneEditor, Without<Dormant>>,
    mut panels: Query<&mut StonePanel>,
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

/// Shows each rock's seed in its own seed field whenever it changes.
fn show_seed(
    editors: Query<(Entity, &StoneEditor)>,
    panels: Query<&StonePanel, Changed<StonePanel>>,
    children: Query<&Children>,
    fields: Query<&NumberInputValue, With<StoneSeed>>,
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
