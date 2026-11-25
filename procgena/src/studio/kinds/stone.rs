//! Stones: one rock, from a `.stone.yml` preset.

use bevy::prelude::*;
use procgena_stone::StonePreset;

use super::{Kind, RegisterKind, entries};
use crate::export::stone_panel::spawn_stone_export;
use crate::scene::{Show, StoneSubject};
use crate::stones::editor::{
    StoneEdited, StoneEditor, StoneEditorPlugin, StonePanel, spawn_stone_editor,
};
use crate::studio::documents::DocumentEdited;
use crate::studio::status;

/// Registers stones.
pub fn register(app: &mut App) {
    app.add_plugins(StoneEditorPlugin)
        .add_observer(|edited: On<StoneEdited>, mut commands: Commands| {
            commands.trigger(DocumentEdited {
                reframe: edited.reframe,
            });
        })
        .register_kind(Kind {
            id: "stone",
            label: "Stones",
            presets: || entries(StonePreset::all(), |p| &p.key, |p| &p.name, |p| &p.group),
            start: crate::stones::START_PRESET,
            spawn_editor: |commands, page, key| {
                let editor = spawn_stone_editor(commands, page, key);
                spawn_stone_export(commands, page);
                editor
            },
            show,
            detail_levels: &[],
            hidden_environment: crate::environment::SINGLE_OBJECT,
            status_line: |state| status::object_line(state, "rock"),
        });
}

/// The rock the editor `editor` holds.
fn show(world: &mut World, editor: Entity, document: u64) -> Option<Show> {
    let panel = world.get::<StoneEditor>(editor)?.panel();
    let options = world.get::<StonePanel>(panel)?.value().options().clone();
    Some(Show::new::<StoneSubject>(document, options))
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec2;
    use procgena_stone::StoneOptions;

    use super::*;
    use crate::headless::{DEFAULT_SIZE, HeadlessStudio, lock};
    use crate::scene::{OrbitCamera, ViewerCommand, ViewportCamera};

    /// The scene camera's view.
    fn camera(studio: &mut HeadlessStudio) -> OrbitCamera {
        let world = studio.app_mut().world_mut();
        let mut cameras = world.query_filtered::<&OrbitCamera, With<ViewportCamera>>();
        *cameras.single(world).unwrap()
    }

    /// Whether `command` shows a rock whose settings pass `test`.
    fn shows_rock(command: &ViewerCommand, test: impl Fn(&StoneOptions) -> bool) -> bool {
        matches!(command, ViewerCommand::Show(show)
            if show.settings::<StoneSubject>().is_some_and(&test))
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn rock_presets_rebuild_the_rock_and_refit_the_camera() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        studio.click("Stones").unwrap();
        studio.click("Boulder").unwrap();
        let sent = studio.record::<ViewerCommand>();

        // Turn the camera away, then open another preset.
        studio.drag(Vec2::new(500.0, 400.0), Vec2::new(800.0, 300.0));
        let turned = camera(&mut studio);
        studio.click("Ice Chunk").unwrap();
        let options: StoneOptions = studio.settings_value();
        assert_eq!(options, StonePreset::ice_chunk().stone);
        assert!(
            lock(&sent)
                .iter()
                .any(|command| shows_rock(command, |built| *built == options))
        );

        // The camera fits the new rock, as Reset Camera would.
        let refit = camera(&mut studio);
        assert_ne!(refit, turned);
        studio.click("Reset Camera").unwrap();
        assert_eq!(camera(&mut studio), refit);

        // Scrolling the Library's preset list leaves the settings beside it
        // in place.
        let seed_label = studio.find("Seed").unwrap();
        let list = studio.find("Ore").unwrap();
        studio.scroll(list, -5.0);
        assert_eq!(studio.find("Seed").unwrap(), seed_label);
        studio.click("Pebble").unwrap();
        let options: StoneOptions = studio.settings_value();
        assert_eq!(options, StonePreset::pebble().stone);
        assert_eq!(studio.find("Seed").unwrap(), seed_label);

        // Dragging across the seed field scrubs the seed.
        let field = studio.find(&options.default_seed.to_string()).unwrap();
        studio.drag(field, field + Vec2::new(120.0, 0.0));
        let scrubbed: StoneOptions = studio.settings_value();
        assert!(scrubbed.default_seed > options.default_seed);
        assert!(
            lock(&sent)
                .iter()
                .any(|command| shows_rock(command, |built| {
                    built.default_seed == scrubbed.default_seed
                }))
        );
    }
}
