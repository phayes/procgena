//! Clumps: a clump such as snow or moss, from a `.clump.yml` preset, grown
//! on a tree or rock subject.

use bevy::prelude::*;
use procgena_clump::{ClumpOptions, ClumpPreset};

use super::{Kind, RegisterKind, entries};
use crate::clump::editor::{
    ClumpEdited, ClumpEditor, ClumpEditorPlugin, ClumpPanel, spawn_clump_editor,
};
use crate::scene::{ClumpSubject, Show};
use crate::studio::documents::DocumentEdited;
use crate::studio::documents::panels::settings_panel;
use crate::studio::status;

/// Registers clumps.
pub fn register(app: &mut App) {
    app.add_plugins(ClumpEditorPlugin)
        .add_observer(|edited: On<ClumpEdited>, mut commands: Commands| {
            commands.trigger(DocumentEdited {
                reframe: edited.reframe,
            });
        })
        .register_kind(Kind {
            id: "clump",
            label: "Clumps",
            presets: || entries(ClumpPreset::all(), |p| &p.key, |p| &p.name, |p| &p.group),
            start: crate::clump::START_PRESET,
            spawn_editor: spawn_clump_editor,
            show,
            settings: Some(settings_panel::<ClumpOptions>()),
            detail_levels: &[],
            hidden_environment: crate::environment::STILL_AIR,
            status_line: |state| status::object_line(state, "clump"),
        });
}

/// The clump and subject the editor `editor` holds.
fn show(world: &mut World, editor: Entity, document: u64) -> Option<Show> {
    let editor = world.get::<ClumpEditor>(editor)?;
    let settings = editor.settings(|panel| world.get::<ClumpPanel>(panel))?;
    Some(Show::new::<ClumpSubject>(document, settings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clump::{Subject, subject_seed};
    use crate::headless::{DEFAULT_SIZE, HeadlessStudio, lock};
    use crate::scene::{ClumpSettings, ViewerCommand};
    use crate::studio::Dormant;

    /// The subject and clump of the selected clump document.
    fn edited(studio: &mut HeadlessStudio) -> ClumpSettings {
        let world = studio.app_mut().world_mut();
        let mut editors = world.query_filtered::<&ClumpEditor, Without<Dormant>>();
        let editor = editors.single(world).unwrap();
        editor
            .settings(|panel| world.get::<ClumpPanel>(panel))
            .unwrap()
    }

    /// The clump settings in the last `Show` of a clump among `commands`.
    fn last_clump(commands: &[ViewerCommand]) -> Option<ClumpSettings> {
        commands.iter().rev().find_map(|command| match command {
            ViewerCommand::Show(show) => show.settings::<ClumpSubject>().cloned(),
            _ => None,
        })
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn clump_tab_rerolls_the_subject_and_clump_seeds_separately() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        studio.click("Clumps").unwrap();
        studio.click("Fresh Snow Cap").unwrap();
        let sent = studio.record::<ViewerCommand>();
        let start = edited(&mut studio);

        // Reroll Clump changes only the clump's seed, and sends it.
        studio.click("Reroll Clump").unwrap();
        let reclumped = edited(&mut studio);
        assert_ne!(reclumped.clump.default_seed, start.clump.default_seed);
        assert_eq!(reclumped.subject, start.subject);
        assert!(
            studio
                .find(&reclumped.clump.default_seed.to_string())
                .is_ok()
        );

        // Reroll Subject changes only the subject's seed, and sends it.
        studio.click("Reroll Subject").unwrap();
        let resubjected = edited(&mut studio);
        assert_ne!(
            subject_seed(&resubjected.subject),
            subject_seed(&start.subject)
        );
        assert_eq!(resubjected.clump, reclumped.clump);
        assert_eq!(last_clump(&lock(&sent)), Some(resubjected.clone()));

        // Another clump preset opens on the start subject, and the first
        // keeps its edits.
        studio.click("Moss Patches").unwrap();
        let moss = edited(&mut studio);
        assert_eq!(moss.clump, ClumpPreset::moss_patches().clump);
        assert_eq!(moss.subject, Subject::start().settings());
        studio.click("Fresh Snow Cap").unwrap();
        assert_eq!(edited(&mut studio), resubjected);
    }
}
