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

/// Writes the project preset file `kind/key` in `folder`.
fn write_preset(folder: &std::path::Path, kind: &str, key: &str, text: &str) {
    let path = folder.join(format!("presets/{kind}/{key}.{kind}.yml"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Changes the selected document's default seed to `seed` through its
/// kind's settings hook, as an edit in its editor would.
fn set_seed(studio: &mut HeadlessStudio, seed: u32) {
    let world = studio.app_mut().world_mut();
    let document = world.resource::<super::Selection>().document.unwrap();
    let found = world.get::<super::Document>(document).unwrap();
    let (kind, editor) = (found.kind, found.editor);
    let settings = world
        .resource::<crate::studio::kinds::Kinds>()
        .get(kind)
        .unwrap()
        .settings
        .unwrap();
    let mut value = (settings.value)(world, editor).unwrap();
    value["default_seed"] = seed.into();
    super::history::queue_load(world, document, value);
    studio.settle();
}

#[test]
#[ignore = "needs a GPU"]
fn scatters_and_ecotopes_keep_the_references_their_files_wrote() {
    // A project tree, and a project tree Scatter and Ecotope that use it.
    let folder = project_folder("references");
    crate::studio::project::Project::create(&folder, "forest", "Forest").unwrap();
    write_preset(
        &folder,
        "tree",
        "tall_pine",
        "base: pine_medium\ntree:\n  default_seed: 77\n",
    );
    write_preset(
        &folder,
        "tree_scatter",
        "pine_belt",
        "# By hand.\ntree_scatter:\n  trees:\n    - { tree: tall_pine, density_scale: 0.5 }\n",
    );
    write_preset(
        &folder,
        "ecotope",
        "dark_forest",
        "base: pine_forest_floor\necotope:\n  scatters:\n    pines:\n      trees:\n        \
         - { tree: tall_pine }\n",
    );
    let mut studio = HeadlessStudio::with_project(DEFAULT_SIZE, Some(folder.clone()));

    // Unedited built-ins save as their base alone.
    for (kind, start) in [
        ("tree_scatter", crate::grove::START_PRESET),
        ("ecotope", crate::ecotope::START_PRESET),
    ] {
        studio.select(&format!("{kind}/{start}")).unwrap();
        studio.command("save-as-new").unwrap();
        if let Err(error) = studio.click("Save preset") {
            let shown: Vec<String> = studio.texts().into_iter().map(|shown| shown.text).collect();
            panic!("{kind}: {error}; shown: {shown:?}");
        }
        assert_eq!(
            preset_file(&folder, kind, &format!("{start}_2")),
            format!("base: {start}\n"),
            "{kind}"
        );
    }

    // Edited and saved, the project Scatter and Ecotope still name the
    // project tree by its key.
    for (kind, key) in [("tree_scatter", "pine_belt"), ("ecotope", "dark_forest")] {
        studio.select(&format!("{kind}/{key}")).unwrap();
        set_seed(&mut studio, 4242);
        assert!(shows_unsaved(&mut studio), "{kind}");
        studio.command("save").unwrap();
        let text = preset_file(&folder, kind, key);
        assert!(text.contains("default_seed: 4242"), "{kind}: {text}");
        assert!(text.contains("tree: tall_pine"), "{kind}: {text}");
        assert!(!shows_unsaved(&mut studio), "{kind}");
    }
    std::fs::remove_dir_all(&folder).unwrap();
}

/// The selected Ecotope document's stone class `class` of the Scatter
/// `scatter`.
fn ecotope_stone(
    studio: &mut HeadlessStudio,
    scatter: &'static str,
    class: usize,
) -> procgena_stone::StoneOptions {
    let world = studio.app_mut().world_mut();
    let document = world.resource::<super::Selection>().document.unwrap();
    let editor = world.get::<super::Document>(document).unwrap().editor;
    let ecotope = world
        .get::<crate::ecotope::editor::EcotopeEditor>(editor)
        .unwrap()
        .ecotope();
    let scatter = &ecotope.scatters[&procgena_core::ChildKey::from_static(scatter)];
    let procgena_biome::ScatterKind::Stone(stones) = &scatter.scatter else {
        panic!("{scatter:?} is not a stone Scatter");
    };
    stones.classes[class].stone.clone()
}

#[test]
#[ignore = "needs a GPU"]
fn a_saved_stone_is_picked_in_an_ecotope_and_still_resolves_after_reopening() {
    let folder = project_folder("picked");
    let mut studio = HeadlessStudio::with_project(DEFAULT_SIZE, Some(folder.clone()));

    // A stone with its own seed, saved as new.
    studio.select("stone/boulder").unwrap();
    studio.click("Random Seed").unwrap();
    studio.command("save-as-new").unwrap();
    studio.click("Save preset").unwrap();
    let stone: procgena_stone::StoneOptions = studio.settings_value();
    assert!(preset_file(&folder, "stone", "boulder_2").starts_with("base: boulder\n"));

    // Picked for the first class of the oak and maple forest's boulders.
    studio.select("ecotope/oak_maple_forest").unwrap();
    studio.click("boulders").unwrap();
    studio.click("Lichen Boulder").unwrap();
    studio.click("boulder_2 · project").unwrap();
    assert_eq!(ecotope_stone(&mut studio, "boulders", 0), stone);
    assert!(shows_unsaved(&mut studio));

    // Saved as a new Ecotope, which names the stone by its key.
    studio.command("save").unwrap();
    studio.click("Save preset").unwrap();
    let text = preset_file(&folder, "ecotope", "oak_maple_forest_2");
    assert!(text.starts_with("base: oak_maple_forest\n"), "{text}");
    assert!(text.contains("stone: boulder_2"), "{text}");
    drop(studio);

    // Reopened, the Ecotope still grows the saved stone.
    let mut studio = HeadlessStudio::with_project(DEFAULT_SIZE, Some(folder.clone()));
    studio.select("ecotope/oak_maple_forest_2").unwrap();
    assert_eq!(ecotope_stone(&mut studio, "boulders", 0), stone);
    assert!(!shows_unsaved(&mut studio));
    std::fs::remove_dir_all(&folder).unwrap();
}
