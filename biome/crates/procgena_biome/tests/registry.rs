//! Ecotope presets read through a preset registry, so a project's Ecotope
//! can use the project's trees, stones, dead wood and mushrooms.

use procgena_biome::{EcotopePreset, ScatterKind};
use procgena_core::ChildKey;
use procgena_core::preset::{PresetFileError, Registry};
use procgena_deadwood::DeadwoodPreset;
use procgena_mushroom::MushroomPreset;
use procgena_stone::StonePreset;
use procgena_tree::{TreePreset, TreeScatterPreset};

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry
        .register::<TreePreset>()
        .register::<TreeScatterPreset>()
        .register::<StonePreset>()
        .register::<DeadwoodPreset>()
        .register::<MushroomPreset>()
        .register::<EcotopePreset>();
    registry
}

#[test]
fn a_built_in_ecotope_reads_the_same_through_a_registry() {
    let registry = registry();
    for builtin in EcotopePreset::all() {
        let text = std::fs::read_to_string(format!(
            "{}/ecotopes/{}.ecotope.yml",
            env!("CARGO_MANIFEST_DIR"),
            builtin.key
        ))
        .unwrap();
        let read: EcotopePreset = registry.read(&builtin.key, &text).unwrap();
        assert_eq!(read.ecotope, builtin.ecotope, "{}", builtin.key);
    }
}

#[test]
fn a_project_ecotope_uses_project_assets() {
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
        .add("stone", "my_boulder", "base: lichen_boulder\n", None)
        .unwrap();
    registry
        .add(
            "ecotope",
            "dark_forest",
            "base: pine_forest_floor\n\
             ecotope:\n  \
               scatters:\n    \
                 pines:\n      \
                   trees:\n        \
                     - { tree: taller_pine, density_scale: 0.5 }\n    \
                 boulders:\n      \
                   classes:\n        \
                     - { key: boulders, stone: my_boulder, density: 0.006, spacing: 5.0 }\n",
            None,
        )
        .unwrap();

    let forest = registry.resolve::<EcotopePreset>("dark_forest").unwrap();
    let taller = registry.resolve::<TreePreset>("taller_pine").unwrap();

    // The pines are the project's tree, keyed by its key.
    let pines = &forest.ecotope.scatters[&ChildKey::from_static("pines")];
    let ScatterKind::Tree(scatter) = &pines.scatter else {
        panic!("pines is a tree Scatter");
    };
    assert_eq!(scatter.trees.len(), 1);
    assert_eq!(scatter.trees[0].key.as_str(), "taller_pine");
    assert_eq!(scatter.trees[0].tree, taller.tree);

    // The boulders are the project's stone.
    let boulders = &forest.ecotope.scatters[&ChildKey::from_static("boulders")];
    let ScatterKind::Stone(stones) = &boulders.scatter else {
        panic!("boulders is a stone Scatter");
    };
    assert_eq!(stones.classes.len(), 1);
    assert_eq!(stones.classes[0].stone, StonePreset::lichen_boulder().stone);

    // Everything else comes from the base.
    let base = EcotopePreset::pine_forest_floor();
    assert_eq!(forest.ecotope.scatters.len(), base.ecotope.scatters.len());

    // "Used by" finds the Ecotope from each asset.
    let uses = registry.used_by("tree", "taller_pine");
    let paths: Vec<_> = uses
        .iter()
        .map(|used| (used.key.as_str(), used.path.as_deref()))
        .collect();
    assert_eq!(
        paths,
        [("dark_forest", Some("scatters.pines.trees.0.tree"))]
    );
    let uses = registry.used_by("stone", "my_boulder");
    assert_eq!(
        uses[0].path.as_deref(),
        Some("scatters.boulders.classes.0.stone")
    );
}

#[test]
fn an_unknown_asset_names_its_setting() {
    let mut registry = registry();
    registry
        .add(
            "ecotope",
            "bad",
            "base: pine_forest_floor\necotope:\n  scatters:\n    logs:\n      piece: nope\n",
            None,
        )
        .unwrap();
    let error = registry.resolve::<EcotopePreset>("bad").unwrap_err();
    let PresetFileError::Setting { path, message } = error else {
        panic!("expected a setting error, got {error:?}");
    };
    assert_eq!(path, "ecotope.scatters.logs.piece");
    assert!(
        message.contains("unknown deadwood preset \"nope\""),
        "{message}"
    );
}
