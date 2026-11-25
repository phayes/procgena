//! Mushrooms: one cluster of mushrooms, from a `.mushroom.yml` preset.

use bevy::prelude::*;
use procgena_mushroom::MushroomPreset;

use super::{Kind, RegisterKind, entries};
use crate::export::mushroom::spawn_mushroom_export;
use crate::mushrooms::editor::{
    MushroomEdited, MushroomEditor, MushroomEditorPlugin, MushroomPanel, spawn_mushroom_editor,
};
use crate::scene::{MushroomSubject, Show};
use crate::studio::documents::DocumentEdited;
use crate::studio::status;

/// The mushroom detail levels, most detailed first.
pub const LEVELS: &[&str] = &[
    "0 · Full",
    "1 · High",
    "2 · Medium",
    "3 · Flat",
    "4 · Billboard",
];

/// Registers mushrooms.
pub fn register(app: &mut App) {
    app.add_plugins(MushroomEditorPlugin)
        .add_observer(|edited: On<MushroomEdited>, mut commands: Commands| {
            commands.trigger(DocumentEdited {
                reframe: edited.reframe,
            });
        })
        .register_kind(Kind {
            id: "mushroom",
            label: "Mushrooms",
            presets: || entries(MushroomPreset::all(), |p| &p.key, |p| &p.name, |p| &p.group),
            start: crate::mushrooms::START_PRESET,
            spawn_editor: |commands, page, key| {
                let editor = spawn_mushroom_editor(commands, page, key);
                spawn_mushroom_export(commands, page);
                editor
            },
            show,
            detail_levels: LEVELS,
            hidden_environment: crate::environment::SINGLE_OBJECT,
            status_line: |state| status::leveled_line(state, "mushroom"),
        });
}

/// The mushroom the editor `editor` holds.
fn show(world: &mut World, editor: Entity, document: u64) -> Option<Show> {
    let panel = world.get::<MushroomEditor>(editor)?.panel();
    let options = world.get::<MushroomPanel>(panel)?.value().options().clone();
    Some(Show::new::<MushroomSubject>(document, options))
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec2;
    use procgena_mushroom::MushroomOptions;

    use super::*;
    use crate::headless::{DEFAULT_SIZE, HeadlessStudio, lock};
    use crate::scene::{OrbitCamera, ViewerCommand, ViewportCamera};

    /// The scene camera's view.
    fn camera(studio: &mut HeadlessStudio) -> OrbitCamera {
        let world = studio.app_mut().world_mut();
        let mut cameras = world.query_filtered::<&OrbitCamera, With<ViewportCamera>>();
        *cameras.single(world).unwrap()
    }

    /// Whether `command` shows exactly the mushroom `options`.
    fn shows_mushroom(command: &ViewerCommand, options: &MushroomOptions) -> bool {
        matches!(command, ViewerCommand::Show(show)
            if show.settings::<MushroomSubject>() == Some(options))
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn mushroom_presets_rebuild_the_mushroom_and_refit_the_camera() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        studio.click("Mushrooms").unwrap();
        studio.click("Fly agaric").unwrap();
        let sent = studio.record::<ViewerCommand>();
        let texts = studio.texts();
        assert!(
            texts.iter().any(|t| t.text.ends_with(" ms")),
            "no mushroom size shown: {texts:?}"
        );

        // Turn the camera away, then open another preset.
        studio.drag(Vec2::new(500.0, 400.0), Vec2::new(800.0, 300.0));
        let turned = camera(&mut studio);
        studio.click("Enoki").unwrap();
        let options: MushroomOptions = studio.settings_value();
        assert_eq!(options, MushroomPreset::enoki().mushroom);
        assert!(
            lock(&sent)
                .iter()
                .any(|command| shows_mushroom(command, &options))
        );

        // The camera fits the new mushroom, as Reset Camera would.
        let refit = camera(&mut studio);
        assert_ne!(refit, turned);
        studio.click("Reset Camera").unwrap();
        assert_eq!(camera(&mut studio), refit);

        // Random Seed changes only the seed, and sends it.
        studio.click("Random Seed").unwrap();
        let reseeded: MushroomOptions = studio.settings_value();
        assert_ne!(reseeded.default_seed, options.default_seed);
        assert_eq!(
            MushroomOptions {
                default_seed: options.default_seed,
                ..reseeded.clone()
            },
            options
        );
        assert!(studio.find(&reseeded.default_seed.to_string()).is_ok());
        assert!(
            lock(&sent)
                .last()
                .is_some_and(|command| shows_mushroom(command, &reseeded))
        );
    }
}
