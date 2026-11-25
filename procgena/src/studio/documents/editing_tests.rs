//! Headless tests of editing, undo and saving documents into a project.

use std::path::PathBuf;

use procgena_tree::TreeOptions;

use crate::headless::{DEFAULT_SIZE, HeadlessStudio};

/// A new, missing project folder for one test.
fn project_folder(name: &str) -> PathBuf {
    let folder =
        std::env::temp_dir().join(format!("procgena_documents_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    folder
}

/// Whether the Inspector title says the document has unsaved changes.
fn shows_unsaved(studio: &mut HeadlessStudio) -> bool {
    studio
        .texts()
        .iter()
        .any(|shown| shown.text.ends_with("· unsaved"))
}

/// The text of the project preset file `kind/key` in `folder`.
fn preset_file(folder: &std::path::Path, kind: &str, key: &str) -> String {
    let path = folder.join(format!("presets/{kind}/{key}.{kind}.yml"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[test]
#[ignore = "needs a GPU"]
fn edits_undo_redo_and_show_unsaved_changes() {
    let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
    let start: TreeOptions = studio.settings_value();
    assert!(!shows_unsaved(&mut studio));

    // A new seed is one change, shown as unsaved.
    studio.click("Random Seed").unwrap();
    let seeded: TreeOptions = studio.settings_value();
    assert_ne!(seeded.default_seed, start.default_seed);
    assert!(shows_unsaved(&mut studio));

    // Undo takes it back, and the tree is as opened again.
    studio.command("undo").unwrap();
    let undone: TreeOptions = studio.settings_value();
    assert_eq!(undone, start);
    assert!(!shows_unsaved(&mut studio));

    // Redo makes it again; undoing past the start does nothing.
    studio.command("redo").unwrap();
    let redone: TreeOptions = studio.settings_value();
    assert_eq!(redone, seeded);
    studio.command("undo").unwrap();
    studio.command("undo").unwrap();
    let still: TreeOptions = studio.settings_value();
    assert_eq!(still, start);
}

#[test]
#[ignore = "needs a GPU"]
fn a_built_in_saved_as_new_updates_and_reopens() {
    let folder = project_folder("tree");
    let mut studio = HeadlessStudio::with_project(DEFAULT_SIZE, Some(folder.clone()));

    // Saving the unedited built-in start tree asks for a key, proposes a
    // free one, and writes only its base.
    studio.command("save").unwrap();
    studio.click("Save preset").unwrap();
    let key = format!("{}_2", crate::tree::START_PRESET);
    assert_eq!(
        preset_file(&folder, "tree", &key),
        format!("base: {}\n", crate::tree::START_PRESET)
    );
    assert!(studio.shows_text_starting(&format!("{key} ({key}) · project")));

    // An edit and Save update the file in place.
    studio.click("Random Seed").unwrap();
    assert!(shows_unsaved(&mut studio));
    studio.command("save").unwrap();
    assert!(!shows_unsaved(&mut studio));
    let saved: TreeOptions = studio.settings_value();
    let text = preset_file(&folder, "tree", &key);
    assert!(
        text.contains(&format!("default_seed: {}", saved.default_seed)),
        "{text}"
    );

    // Undo still reaches back across the save.
    studio.command("undo").unwrap();
    assert!(shows_unsaved(&mut studio));
    drop(studio);

    // Reopened, the project preset shows its saved settings.
    let mut studio = HeadlessStudio::with_project(DEFAULT_SIZE, Some(folder.clone()));
    studio.select(&format!("tree/{key}")).unwrap();
    let reopened: TreeOptions = studio.settings_value();
    assert_eq!(reopened, saved);
    assert!(!shows_unsaved(&mut studio));
    std::fs::remove_dir_all(&folder).unwrap();
}

#[test]
#[ignore = "needs a GPU"]
fn every_plain_kind_saves_an_unedited_built_in_as_its_base_alone() {
    let folder = project_folder("kinds");
    let mut studio = HeadlessStudio::with_project(DEFAULT_SIZE, Some(folder.clone()));
    for (kind, start) in [
        ("stone", crate::stones::START_PRESET),
        ("mushroom", crate::mushrooms::START_PRESET),
        ("fruit", crate::fruit::START_FRUIT),
        ("flower", crate::fruit::START_FLOWER),
        ("deadwood", crate::deadwood::START_PRESET),
        ("clump", crate::clump::START_PRESET),
        ("terrain", crate::terrain::START_PRESET),
    ] {
        studio.select(&format!("{kind}/{start}")).unwrap();
        studio.command("save-as-new").unwrap();
        studio.click("Save preset").unwrap();
        assert_eq!(
            preset_file(&folder, kind, &format!("{start}_2")),
            format!("base: {start}\n"),
            "{kind}"
        );
    }
    std::fs::remove_dir_all(&folder).unwrap();
}
