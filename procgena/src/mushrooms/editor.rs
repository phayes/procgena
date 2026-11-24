//! A mushroom document's editor: seed, and every mushroom setting.

use bevy::feathers::controls::{FeathersButton, FeathersNumberInput, HardLimit, NumberInputValue};
use bevy::feathers::display::caption;
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui_widgets::{Activate, ValueChange};
use procgena_core::Rng;
use procgena_mushroom::{MushroomOptions, MushroomPreset};

use super::params;
use crate::params::{ParamPanel, ParamPanelPlugin, ParamsChanged, spawn_param_panel};
use crate::studio::Dormant;
use crate::ui::{LABEL_WIDTH, labelled_row};

/// Triggered after the selected mushroom's settings change. Read them from
/// the panel named by [`MushroomEditor::panel`].
#[derive(Event, Debug)]
pub struct MushroomEdited {
    /// Whether the camera should reset to fit the new mushroom, as after
    /// loading settings.
    pub reframe: bool,
}

/// A mushroom editor's state, one per open mushroom.
#[derive(Component)]
pub struct MushroomEditor {
    /// The preset edited.
    preset: &'static MushroomPreset,
    /// The panel holding the edited options.
    panel: Entity,
    /// Picks seeds for Random Seed.
    rng: Rng,
}

impl MushroomEditor {
    /// Suggests a file name without an extension from the preset: its key,
    /// such as `fly_agaric`.
    pub fn file_stem(&self) -> String {
        self.preset.key.to_string()
    }

    /// The preset edited, which saved files name as their base.
    pub fn preset(&self) -> &'static MushroomPreset {
        self.preset
    }

    /// The panel entity holding the edited [`MushroomOptions`].
    pub fn panel(&self) -> Entity {
        self.panel
    }
}

/// Marks the seed field.
#[derive(Component, Default, Clone)]
struct MushroomSeed;

/// Makes mushroom editors work. Needs the param panel plugins.
pub struct MushroomEditorPlugin;

impl Plugin for MushroomEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ParamPanelPlugin::<MushroomOptions>::default())
            .add_systems(PostUpdate, show_seed);
    }
}

/// Spawns a mushroom editor for the built-in preset `key` as the last child
/// of `parent`, and returns the editor's entity.
///
/// # Panics
///
/// Panics if `key` names no built-in mushroom preset.
pub fn spawn_mushroom_editor(commands: &mut Commands, parent: Entity, key: &str) -> Entity {
    let preset = MushroomPreset::named(key).expect("the mushroom preset is built in");
    let options = preset.mushroom.clone();
    let seed = i64::from(options.default_seed);

    // Seed, and the seed button, above the panel.
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
        preset,
        panel,
        rng: Rng::new(std::process::id()),
    });
    editor
}

/// Shows loaded mushroom settings in the selected mushroom's editor,
/// rebuilds the mushroom and fits the camera to it. The preset, and so the
/// suggested file name, stays as it was. Run it with
/// `World::run_system_cached_with`.
pub fn load_mushroom(
    In(options): In<MushroomOptions>,
    editor: Single<&MushroomEditor, Without<Dormant>>,
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

/// Uses a typed seed.
fn type_seed(
    change: On<ValueChange<i64>>,
    editor: Single<&MushroomEditor, Without<Dormant>>,
    mut panels: Query<&mut ParamPanel<MushroomOptions>>,
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
    mut editor: Single<&mut MushroomEditor, Without<Dormant>>,
    mut panels: Query<&mut ParamPanel<MushroomOptions>>,
    mut commands: Commands,
) {
    let Ok(panel) = panels.get(editor.panel) else {
        return;
    };
    let mut options = panel.value().clone();
    options.default_seed = editor.rng.next_u32() % 65536;
    replace(options, false, &editor, &mut panels, &mut commands);
}

/// Shows each mushroom's seed in its own seed field whenever it changes.
fn show_seed(
    editors: Query<(Entity, &MushroomEditor)>,
    panels: Query<&ParamPanel<MushroomOptions>, Changed<ParamPanel<MushroomOptions>>>,
    children: Query<&Children>,
    fields: Query<&NumberInputValue, With<MushroomSeed>>,
    mut commands: Commands,
) {
    for (entity, editor) in &editors {
        let Ok(panel) = panels.get(editor.panel) else {
            continue;
        };
        let seed = NumberInputValue::I64(i64::from(panel.value().default_seed));
        for field in children.iter_descendants(entity) {
            if fields.get(field).is_ok_and(|shown| *shown != seed) {
                commands.entity(field).insert(seed);
            }
        }
    }
}
