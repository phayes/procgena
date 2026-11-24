use std::path::PathBuf;

use procgena_biome::{EcotopePreset, ScatterKind};
use procgena_core::ChildKey;
use procgena_core::preset::{Document, Layer, Problem};
use procgena_tree::TreePreset;
use serde_json::{Value, json};

use super::*;

/// A new, missing folder for one test.
fn folder(name: &str) -> PathBuf {
    let folder =
        std::env::temp_dir().join(format!("procgena_project_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    folder
}

/// A document over `base` holding its full settings with `edit` applied.
fn edited(project: &Project, kind: &str, base: &str, edit: impl FnOnce(&mut Value)) -> Document {
    let mut settings = project.registry().settings(kind, base).unwrap();
    edit(&mut settings);
    Document {
        base: Some(base.to_owned()),
        settings: Some(settings),
        ..Document::default()
    }
}

#[test]
fn a_new_project_has_its_file_folders_and_gitignore() {
    let root = folder("new");
    let project = Project::create(&root, "my_forest", "My forest").unwrap();
    assert_eq!(project.manifest().name, "My forest");
    assert_eq!(
        project.registry().namespace(Layer::Project),
        Some("my_forest")
    );
    assert!(project.presets().is_dir());
    assert_eq!(
        std::fs::read_to_string(root.join(MANIFEST)).unwrap(),
        "format: 1\nid: my_forest\nname: My forest\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join(".gitignore")).unwrap(),
        ".cache/\n"
    );

    // It opens again, and a second project can't be made over it.
    let again = Project::open_or_create(&root, "other", "Other").unwrap();
    assert_eq!(again.manifest().name, "My forest");
    assert!(matches!(
        Project::create(&root, "other", "Other"),
        Err(ProjectError::NotEmpty(_))
    ));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn an_unknown_format_is_refused() {
    let root = folder("format");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(MANIFEST), "format: 2\nid: later\nname: Later\n").unwrap();
    let error = Project::open(&root).unwrap_err();
    assert!(error.to_string().contains("format 2"), "{error}");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_bundle_manifest_opens_as_a_project() {
    // The manifest saved-file-format.md gives a bundle.
    let root = folder("bundle");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join(MANIFEST),
        "format: 1\nid: alpine_pack\nname: Alpine pack\nversion: 0.3.0\n\
         description: Pines and edelweiss.\nlicence: CC0-1.0\nauthors: [Jo Example]\n\
         made_with: 0.1.0\nroots: [biome/alpine]\n",
    )
    .unwrap();
    let project = Project::open(&root).unwrap();
    let manifest = project.manifest();
    assert_eq!(manifest.licence.as_deref(), Some("CC0-1.0"));
    assert_eq!(manifest.authors, ["Jo Example"]);
    assert_eq!(manifest.roots, ["biome/alpine"]);
    assert_eq!(
        project.registry().namespace(Layer::Project),
        Some("alpine_pack")
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_shared_key_makes_a_built_in_base_s_bare_references_ambiguous() {
    // Today's behaviour, pinned while the coordinator decides it: a
    // project tree named pine_small makes the bare `tree: pine_small` that
    // a project Ecotope inherits from the built-in pine forest floor
    // ambiguous, though the reference was written in a built-in file.
    let root = folder("inherited");
    let mut project = Project::create(&root, "my_forest", "My forest").unwrap();
    let forest = edited(&project, "ecotope", "pine_forest_floor", |_| {});
    project.save_new("ecotope", "my_floor", &forest).unwrap();
    assert!(
        project
            .registry()
            .resolve::<EcotopePreset>("my_floor")
            .is_ok()
    );

    let pine = edited(&project, "tree", "pine_small", |_| {});
    let pine = Document {
        base: Some("builtin:pine_small".to_owned()),
        ..pine
    };
    project.save_new("tree", "pine_small", &pine).unwrap();
    let error = project
        .registry()
        .resolve::<EcotopePreset>("my_floor")
        .unwrap_err();
    assert!(error.to_string().contains("builtin:pine_small"), "{error}");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_project_can_t_be_named_builtin() {
    // Not when it is made...
    let root = folder("builtin");
    let error = Project::create(&root, "builtin", "Built in").unwrap_err();
    assert!(error.to_string().contains("reserved"), "{error}");
    assert!(!root.exists());

    // ...nor when its file says so.
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join(MANIFEST),
        "format: 1\nid: builtin\nname: Sneaky\n",
    )
    .unwrap();
    let error = Project::open(&root).unwrap_err();
    assert!(
        matches!(&error, ProjectError::Manifest { message } if message.contains("reserved")),
        "{error}"
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_project_preset_may_share_a_built_in_key() {
    let root = folder("shared");
    let mut project = Project::create(&root, "my_forest", "My forest").unwrap();
    let mine = edited(&project, "tree", "pine_medium", |tree| {
        tree["default_seed"] = json!(5);
    });
    let mine = Document {
        base: Some("builtin:pine_medium".to_owned()),
        ..mine
    };
    project.save_new("tree", "pine_medium", &mine).unwrap();

    // Both are reached by namespace, and the shared key is a problem.
    let tree = |reference| {
        project
            .registry()
            .resolve::<TreePreset>(reference)
            .map(|tree| tree.tree.default_seed)
    };
    assert_eq!(tree("my_forest:pine_medium").unwrap(), 5);
    assert_eq!(
        tree("builtin:pine_medium").unwrap(),
        TreePreset::pine_medium().tree.default_seed
    );
    assert!(tree("pine_medium").is_err());
    assert!(matches!(
        project.problems().as_slice(),
        [Problem::SharedKey { choices, .. }]
            if choices == &["my_forest:pine_medium", "builtin:pine_medium"]
    ));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn saved_presets_build_on_each_other_and_reopen() {
    let root = folder("pines");
    let mut project = Project::create(&root, "pines", "Pines").unwrap();

    // A built-in pine saved as new, and another over it.
    let tall = edited(&project, "tree", "pine_medium", |tree| {
        tree["default_seed"] = json!(77);
    });
    let path = project.save_new("tree", "tall_pine", &tall).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "base: pine_medium\ntree:\n  default_seed: 77\n"
    );
    let taller = edited(&project, "tree", "tall_pine", |_| {});
    project.save_new("tree", "taller_pine", &taller).unwrap();

    // A project Ecotope with it in place of the built-in pines.
    let forest = edited(&project, "ecotope", "pine_forest_floor", |ecotope| {
        ecotope["scatters"]["pines"]["trees"] = json!([{ "tree": "taller_pine" }]);
    });
    project.save_new("ecotope", "dark_forest", &forest).unwrap();

    // A saved preset's key is taken.
    assert!(matches!(
        project.save_new("tree", "tall_pine", &tall),
        Err(ProjectError::KeyTaken { .. })
    ));
    assert!(matches!(
        project.save_new("tree", "Tall Pine", &tall),
        Err(ProjectError::BadKey(_))
    ));

    // Reopened, the Ecotope grows the project's pine.
    let project = Project::open(&root).unwrap();
    assert!(project.problems().is_empty(), "{:?}", project.problems());
    let ecotope = project
        .registry()
        .resolve::<EcotopePreset>("dark_forest")
        .unwrap();
    let pines = &ecotope.ecotope.scatters[&ChildKey::from_static("pines")];
    let ScatterKind::Tree(scatter) = &pines.scatter else {
        panic!("pines is a tree Scatter");
    };
    assert_eq!(scatter.trees[0].key.as_str(), "taller_pine");
    let tree = project
        .registry()
        .resolve::<TreePreset>("taller_pine")
        .unwrap();
    assert_eq!(tree.layer(), Layer::Project);
    assert_eq!(tree.tree.default_seed, 77);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn update_rewrites_the_file_three_ways() {
    let root = folder("update");
    let mut project = Project::create(&root, "update", "Update").unwrap();
    let tall = edited(&project, "tree", "pine_medium", |tree| {
        tree["default_seed"] = json!(77);
    });
    let path = project.save_new("tree", "tall_pine", &tall).unwrap();

    // A changed value, a newly differing nested setting, and a setting set
    // back to its base's value.
    let update = |project: &mut Project, edit: &dyn Fn(&mut Value)| {
        let document = edited(project, "tree", "tall_pine", |_| {});
        let mut settings = document.settings.unwrap();
        edit(&mut settings);
        let document = Document {
            base: Some("pine_medium".to_owned()),
            settings: Some(settings),
            ..Document::default()
        };
        project.update("tree", "tall_pine", &document).unwrap();
        std::fs::read_to_string(&path).unwrap()
    };
    let base_bark =
        project.registry().settings("tree", "pine_medium").unwrap()["bark"]["tint"].clone();
    let text = update(&mut project, &|tree| tree["default_seed"] = json!(78));
    assert_eq!(text, "base: pine_medium\ntree:\n  default_seed: 78\n");
    let text = update(&mut project, &|tree| tree["bark"]["tint"] = json!(255));
    assert!(text.contains("bark:\n    tint: 255\n"), "{text}");
    let text = update(&mut project, &|tree| {
        tree["bark"]["tint"] = base_bark.clone()
    });
    assert_eq!(text, "base: pine_medium\ntree:\n  default_seed: 78\n");

    // Only project presets update.
    assert!(matches!(
        project.update("tree", "pine_small", &tall),
        Err(ProjectError::NotInProject { .. })
    ));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn free_keys_are_numbered_and_fit() {
    let root = folder("keys");
    let mut project = Project::create(&root, "keys", "Keys").unwrap();
    assert_eq!(project.free_key("tree", "mine"), "mine");
    assert_eq!(project.free_key("tree", "pine_small"), "pine_small_2");
    let pine = edited(&project, "tree", "pine_small", |_| {});
    project.save_new("tree", "pine_small_2", &pine).unwrap();
    assert_eq!(project.free_key("tree", "pine_small"), "pine_small_3");
    assert_eq!(project.free_key("tree", "Not A Key"), "preset");

    // A long stem is shortened to leave room for the number.
    let long = "a".repeat(CHILD_KEY_MAX_LEN);
    project.save_new("tree", &long, &pine).unwrap();
    let key = project.free_key("tree", &long);
    assert_eq!(key.len(), CHILD_KEY_MAX_LEN);
    assert!(key.ends_with("_2"), "{key}");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn outside_edits_are_noticed_and_reloaded() {
    let root = folder("outside");
    let mut project = Project::create(&root, "outside", "Outside").unwrap();
    assert!(!project.changed_on_disk());

    // A file added by hand, as `git pull` might.
    let path = root.join("presets/tree/hand_made.tree.yml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "# Written by hand.\nbase: oak_small\n").unwrap();
    assert!(project.changed_on_disk());
    project.reload().unwrap();
    assert!(!project.changed_on_disk());
    assert!(
        project
            .registry()
            .project_preset("tree", "hand_made")
            .is_some()
    );

    // A file the studio can't read is a problem, not a failure.
    std::fs::write(root.join("presets/tree/broken.tree.yml"), "tree: [").unwrap();
    project.reload().unwrap();
    assert!(matches!(
        project.problems().as_slice(),
        [Problem::Unreadable { key, .. }] if key == "broken"
    ));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_plugin_opens_the_project_folder_given() {
    let root = folder("plugin");
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, ProjectPlugin))
        .insert_resource(ProjectFolder(root.clone()));
    app.update();
    let project = app.world().resource::<OpenProject>();
    assert_eq!(project.0.root(), root);
    assert!(root.join(MANIFEST).is_file());

    // Without a folder, no project opens.
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, ProjectPlugin));
    app.update();
    assert!(app.world().get_resource::<OpenProject>().is_none());
    std::fs::remove_dir_all(&root).unwrap();
}
