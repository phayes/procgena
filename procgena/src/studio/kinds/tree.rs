//! Trees: one tree, from a `.tree.yml` preset.

use bevy::prelude::*;
use procgena_tree::TreePreset;

use super::{Kind, RegisterKind, entries};
use crate::export::tree_panel::spawn_tree_export;
use crate::scene::{Show, TreeSubject};
use crate::studio::documents::DocumentEdited;
use crate::studio::documents::panels::settings_panel;
use crate::studio::status;
use crate::tree::editor::{TreeEdited, TreeEditor, TreeEditorPlugin, TreePanel, spawn_tree_editor};

/// The tree detail levels, most detailed first.
pub const LEVELS: &[&str] = &["0 · Full", "1 · Medium", "2 · Low", "3 · Lowest"];

/// Registers trees.
pub fn register(app: &mut App) {
    app.add_plugins(TreeEditorPlugin)
        .add_observer(|_: On<TreeEdited>, mut commands: Commands| {
            commands.trigger(DocumentEdited::default());
        })
        .register_kind(Kind {
            id: "tree",
            label: "Trees",
            presets: || entries(TreePreset::all(), |p| &p.key, |p| &p.name, |p| &p.group),
            start: crate::tree::START_PRESET,
            spawn_editor: |commands, page, key| {
                let editor = spawn_tree_editor(commands, page, key);
                spawn_tree_export(commands, page);
                editor
            },
            show,
            settings: Some(settings_panel::<TreePreset>()),
            detail_levels: LEVELS,
            hidden_environment: crate::environment::SINGLE_OBJECT,
            status_line: |state| status::leveled_line(state, "tree"),
        });
}

/// The tree the editor `editor` holds.
fn show(world: &mut World, editor: Entity, document: u64) -> Option<Show> {
    let panel = world.get::<TreeEditor>(editor)?.panel();
    let options = world.get::<TreePanel>(panel)?.value().options().clone();
    Some(Show::new::<TreeSubject>(document, options))
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec2;
    use procgena_tree::TreeOptions;

    use super::*;
    use crate::headless::{DEFAULT_SIZE, HeadlessStudio, lock};
    use crate::scene::ViewerCommand;

    /// The tree in the last tree `Show` among `commands`.
    fn last_tree(commands: &[ViewerCommand]) -> Option<TreeOptions> {
        commands.iter().rev().find_map(|command| match command {
            ViewerCommand::Show(show) => show.settings::<TreeSubject>().cloned(),
            _ => None,
        })
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn the_leaves_chip_shows_only_leaf_settings_with_their_tooltips() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);

        // The part chips sit above the sections, so the topmost "Leaves" is
        // the chip rather than the section of the same name.
        let chip = studio
            .texts()
            .into_iter()
            .filter(|shown| shown.text == "Leaves")
            .min_by(|a, b| a.center.y.total_cmp(&b.center.y))
            .expect("a Leaves chip")
            .center;
        studio.click_at(chip);

        // Leaf settings show, open; other parts' settings don't.
        assert!(studio.find("Alpha Test").is_ok());
        // Choosing a part opens every section, so each of these (one per
        // other part) would show if its part weren't hidden.
        for other in ["Flat Shading", "Levels", "Tree Type", "Enabled"] {
            assert!(studio.find(other).is_err(), "{other} still shows");
        }

        // Hovering a setting shows its description.
        let description = "How see-through a pixel of the leaf picture may be before it is \
                           cut away, which gives the card a leaf-shaped outline.";
        assert!(studio.find(description).is_err());
        let setting = studio.find("Alpha Test").unwrap();
        studio.hover(setting);
        assert!(studio.find(description).is_ok());

        // "All" shows every part's settings again.
        studio.click("All").unwrap();
        assert!(studio.find("Tree Type").is_ok());
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn tree_presets_and_seed_buttons_rebuild_the_tree() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        let sent = studio.record::<ViewerCommand>();

        // Opening a preset from the Library shows and builds it.
        studio.click("Aspen Small").unwrap();
        let options: TreeOptions = studio.settings_value();
        assert_eq!(options, TreePreset::aspen_small().tree);
        assert!(studio.find(&options.default_seed.to_string()).is_ok());
        assert_eq!(last_tree(&lock(&sent)), Some(options.clone()));

        // Random Seed changes only the seed; Jiggle changes the shape too.
        studio.click("Random Seed").unwrap();
        let reseeded: TreeOptions = studio.settings_value();
        assert_ne!(reseeded.default_seed, options.default_seed);
        assert!(studio.find(&reseeded.default_seed.to_string()).is_ok());
        assert_eq!(
            TreeOptions {
                default_seed: options.default_seed,
                ..reseeded.clone()
            },
            options
        );
        studio.click("Jiggle").unwrap();
        let jiggled: TreeOptions = studio.settings_value();
        assert_ne!(
            TreeOptions {
                default_seed: reseeded.default_seed,
                ..jiggled.clone()
            },
            reseeded
        );

        // The game default draws no finer than level 1 near the camera.
        assert!(
            studio.shows_text_starting("LOD 1 ·"),
            "{:?}",
            studio.texts()
        );

        // Dragging across the seed field scrubs the seed.
        let field = studio.find(&jiggled.default_seed.to_string()).unwrap();
        studio.drag(field, field + Vec2::new(120.0, 0.0));
        let scrubbed: TreeOptions = studio.settings_value();
        assert!(scrubbed.default_seed > jiggled.default_seed);
        assert!(studio.find(&scrubbed.default_seed.to_string()).is_ok());
        assert_eq!(
            last_tree(&lock(&sent)).map(|tree| tree.default_seed),
            Some(scrubbed.default_seed)
        );
    }
}
