//! A tree document's editor: seed, and every tree setting.

use bevy::feathers::controls::{FeathersButton, FeathersNumberInput, HardLimit, NumberInputValue};
use bevy::feathers::display::caption;
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui_widgets::{Activate, ValueChange};
use procgena_core::Rng;
use procgena_tree::{TreeOptions, TreePreset};

use super::JIGGLE_AMOUNT;
use crate::params::{ParamPanel, ParamsChanged};
use crate::studio::Dormant;
use crate::studio::inspector::{
    InspectorPlugin, Settings, edit_options, edit_setting, spawn_meta_panel,
};
use crate::ui::{LABEL_WIDTH, labelled_row};

/// The panel holding a tree editor's settings.
pub type TreePanel = ParamPanel<Settings<TreeOptions>>;

/// Triggered after the selected tree's settings change. Read them from the
/// panel named by [`TreeEditor::panel`].
#[derive(Event, Debug)]
pub struct TreeEdited;

/// A tree editor's state, one per open tree.
#[derive(Component)]
pub struct TreeEditor {
    /// The preset edited.
    preset: &'static TreePreset,
    /// The panel holding the edited options.
    panel: Entity,
    /// Picks seeds for Random Seed and Jiggle.
    rng: Rng,
}

impl TreeEditor {
    /// Suggests a file name without an extension from the preset: its
    /// key, such as `oak_medium`.
    pub fn file_stem(&self) -> String {
        self.preset.key.to_string()
    }

    /// The preset edited, which saved files name as their base.
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

/// Makes tree editors work. Needs the param panel plugins.
pub struct TreeEditorPlugin;

impl Plugin for TreeEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(InspectorPlugin::<TreeOptions>::default())
            .add_systems(PostUpdate, show_seed);
    }
}

/// Spawns a tree editor for the built-in preset `key` as the last child of
/// `parent`, and returns the editor's entity.
///
/// # Panics
///
/// Panics if `key` names no built-in tree preset.
pub fn spawn_tree_editor(commands: &mut Commands, parent: Entity, key: &str) -> Entity {
    let preset = TreePreset::named(key).expect("the tree preset is built in");
    let options = preset.tree.clone();
    let seed = options.default_seed;

    // Seed, and the seed buttons, above the panel.
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
    let panel = spawn_meta_panel(
        commands,
        editor,
        &super::META,
        Settings::new(options),
        &[0],
        &super::custom,
    );
    commands
        .entity(panel)
        .observe(|_: On<ParamsChanged>, mut commands: Commands| {
            commands.trigger(TreeEdited);
        });
    commands.entity(editor).insert(TreeEditor {
        preset,
        panel,
        rng: Rng::new(std::process::id()),
    });
    editor
}

/// Shows loaded tree settings in the selected tree's editor and rebuilds
/// the tree. The preset, and so the suggested file name, stays as it was.
/// Run it with `World::run_system_cached_with`.
pub fn load_tree(
    In(options): In<TreeOptions>,
    editor: Single<&TreeEditor, Without<Dormant>>,
    mut panels: Query<&mut TreePanel>,
    mut commands: Commands,
) {
    if let Ok(mut panel) = panels.get_mut(editor.panel) {
        panel.set_value(Settings::new(options));
        commands.trigger(TreeEdited);
    }
}

/// Uses a typed seed.
fn type_seed(
    change: On<ValueChange<i32>>,
    editor: Single<&TreeEditor, Without<Dormant>>,
    mut panels: Query<&mut TreePanel>,
    mut commands: Commands,
) {
    let seed = change.value.into();
    edit_setting(
        &mut commands,
        &mut panels,
        editor.panel,
        "default_seed",
        seed,
    );
}

/// Picks a random seed.
fn random_seed(
    _: On<Activate>,
    mut editor: Single<&mut TreeEditor, Without<Dormant>>,
    mut panels: Query<&mut TreePanel>,
    mut commands: Commands,
) {
    let seed = (editor.rng.next_u32() % 65536).into();
    edit_setting(
        &mut commands,
        &mut panels,
        editor.panel,
        "default_seed",
        seed,
    );
}

/// Varies the shape slightly, with a new seed.
fn jiggle(
    _: On<Activate>,
    mut editor: Single<&mut TreeEditor, Without<Dormant>>,
    mut panels: Query<&mut TreePanel>,
    mut commands: Commands,
) {
    let Ok(panel) = panels.get(editor.panel) else {
        return;
    };
    let mut options = panel.value().options().clone();
    options.jiggle(JIGGLE_AMOUNT, &mut editor.rng);
    edit_options(&mut commands, &mut panels, editor.panel, &options);
}

/// Shows each tree's seed in its own seed field whenever it changes.
fn show_seed(
    editors: Query<(Entity, &TreeEditor)>,
    panels: Query<&TreePanel, Changed<TreePanel>>,
    children: Query<&Children>,
    fields: Query<&NumberInputValue, With<TreeSeed>>,
    mut commands: Commands,
) {
    for (entity, editor) in &editors {
        let Ok(panel) = panels.get(editor.panel) else {
            continue;
        };
        let seed = NumberInputValue::I32(panel.value().options().default_seed);
        for field in children.iter_descendants(entity) {
            if fields.get(field).is_ok_and(|shown| *shown != seed) {
                commands.entity(field).insert(seed);
            }
        }
    }
}
