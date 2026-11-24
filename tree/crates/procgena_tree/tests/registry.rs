//! Tree and tree Scatter presets read through a preset registry, so a
//! project's Scatter can use the project's trees.

use procgena_core::preset::{Layer, PresetFileError, Registry};
use procgena_tree::{TreePreset, TreeScatterPreset};

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry
        .register::<TreePreset>()
        .register::<TreeScatterPreset>();
    registry
}

#[test]
fn a_built_in_scatter_reads_the_same_through_a_registry() {
    let registry = registry();
    for builtin in TreeScatterPreset::all() {
        let text = std::fs::read_to_string(format!(
            "{}/scatters/{}.tree_scatter.yml",
            env!("CARGO_MANIFEST_DIR"),
            builtin.key
        ))
        .unwrap();
        let read: TreeScatterPreset = registry.read(&builtin.key, &text).unwrap();
        assert_eq!(read.tree_scatter, builtin.tree_scatter, "{}", builtin.key);
    }
}

#[test]
fn a_project_scatter_uses_a_project_tree_and_its_root_s_stand() {
    let mut registry = registry();
    registry
        .add(
            "tree",
            "tall_pine",
            "base: pine_medium\ntree:\n  default_seed: 77\n",
            None,
        )
        .unwrap();
    registry
        .add("tree", "taller_pine", "base: tall_pine\n", None)
        .unwrap();
    registry
        .add(
            "tree_scatter",
            "pine_belt",
            "tree_scatter:\n  trees:\n    - { tree: taller_pine, density_scale: 0.5 }\n",
            None,
        )
        .unwrap();

    // The tree keeps its base's stand, two bases down.
    let taller = registry.resolve::<TreePreset>("taller_pine").unwrap();
    assert_eq!(taller.layer(), Layer::Project);
    assert_eq!(taller.stand, TreePreset::pine_medium().stand);
    assert_eq!(taller.tree.default_seed, 77);

    // The Scatter's species takes the tree's settings, key and stand.
    let belt = registry.resolve::<TreeScatterPreset>("pine_belt").unwrap();
    let species = &belt.tree_scatter.trees[0];
    let stand = TreePreset::pine_medium().stand.unwrap();
    assert_eq!(species.key.as_str(), "taller_pine");
    assert_eq!(species.tree, taller.tree);
    assert_eq!(species.density, stand.density);
    assert_eq!(species.spacing, stand.spacing);
    assert_eq!(species.max_slope, stand.max_slope);
    assert_eq!(species.density_scale, 0.5);

    // "Used by" finds the Scatter and the tree written over it.
    let uses = registry.used_by("tree", Layer::Project, "taller_pine");
    assert_eq!(uses.len(), 1);
    assert_eq!(
        (uses[0].kind, uses[0].path.as_deref()),
        ("tree_scatter", Some("trees.0.tree"))
    );
    let uses = registry.used_by("tree", Layer::Project, "tall_pine");
    assert_eq!(
        (uses[0].kind, uses[0].key.as_str()),
        ("tree", "taller_pine")
    );
}

#[test]
fn built_in_scatters_use_built_in_trees() {
    let registry = registry();
    let users: Vec<String> = registry
        .used_by("tree", Layer::BuiltIn, "pine_medium")
        .into_iter()
        .map(|used| format!("{}/{}", used.kind, used.key))
        .collect();
    assert!(
        users.contains(&"tree_scatter/pine_stand".to_owned()),
        "{users:?}"
    );
}

#[test]
fn a_species_key_never_includes_the_namespace() {
    // The project has its own pine_medium beside the built-in one.
    let mut registry = registry();
    registry.set_namespace("my_forest").unwrap();
    registry
        .add(
            "tree",
            "pine_medium",
            "base: builtin:pine_medium\ntree:\n  default_seed: 5\n",
            None,
        )
        .unwrap();
    registry
        .add(
            "tree_scatter",
            "mixed",
            "tree_scatter:\n  trees:\n    \
               - { tree: my_forest:pine_medium }\n    \
               - { key: old, tree: builtin:pine_medium }\n",
            None,
        )
        .unwrap();

    let mixed = registry.resolve::<TreeScatterPreset>("mixed").unwrap();
    let [mine, old] = &mixed.tree_scatter.trees[..] else {
        panic!("two species");
    };
    assert_eq!(mine.key.as_str(), "pine_medium");
    assert_eq!(mine.tree.default_seed, 5);
    assert_eq!(old.tree, TreePreset::pine_medium().tree);

    // A project file's bare key names the project's tree first.
    registry
        .add(
            "tree_scatter",
            "bare",
            "tree_scatter:\n  trees:\n    - { tree: pine_medium }\n",
            None,
        )
        .unwrap();
    let bare = registry.resolve::<TreeScatterPreset>("bare").unwrap();
    assert_eq!(bare.tree_scatter.trees[0].tree.default_seed, 5);

    // Built-in Scatters keep their built-in trees.
    registry
        .add("tree_scatter", "stand", "base: pine_stand\n", None)
        .unwrap();
    let stand = registry.resolve::<TreeScatterPreset>("stand").unwrap();
    assert_eq!(
        stand.tree_scatter.trees,
        TreeScatterPreset::pine_stand().tree_scatter.trees
    );
}

#[test]
fn an_unknown_tree_names_the_species() {
    let mut registry = registry();
    registry
        .add(
            "tree_scatter",
            "bad",
            "tree_scatter:\n  trees:\n    - { tree: nope }\n",
            None,
        )
        .unwrap();
    let error = registry.resolve::<TreeScatterPreset>("bad").unwrap_err();
    let PresetFileError::Setting { path, message } = error else {
        panic!("expected a setting error, got {error:?}");
    };
    assert_eq!(path, "tree_scatter.trees.0.tree");
    assert!(
        message.contains("unknown tree preset \"nope\""),
        "{message}"
    );
}
