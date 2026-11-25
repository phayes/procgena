//! Verify that `Tree` produces the same layout and geometry as generating them
//! separately, and handles settings and detail levels.

use procgena_core::chain::LowerDetail;
use procgena_tree::{
    LodDetail, OptionsError, PresetFileError, Skeleton, Tree, TreeOptions, TreePreset,
};

#[test]
fn new_combines_skeleton_and_full_detail_mesh() {
    let options = TreePreset::oak_small().tree.clone();
    let tree = Tree::new(options.clone()).unwrap();

    let skeleton = Skeleton::generate(&options).unwrap();
    let mesh = skeleton.mesh(&options, &LodDetail::default());
    assert_eq!(tree.options(), &options);
    assert_eq!(tree.skeleton(), &skeleton);
    assert_eq!(tree.mesh(), &mesh);
    assert_eq!(tree.vertex_count(), mesh.vertex_count());
    assert_eq!(tree.triangle_count(), mesh.triangle_count());

    let (parts_options, parts_skeleton, parts_mesh) = tree.into_parts();
    assert_eq!(
        (parts_options, parts_skeleton, parts_mesh),
        (options, skeleton, mesh)
    );
}

#[test]
fn the_lower_mesh_is_built_from_the_lower_detail() {
    let tree = Tree::from_preset(TreePreset::aspen_small()).unwrap();
    let lod = tree.lod();
    assert_eq!(lod.game, tree.options().game_level);
    assert_eq!(lod.lower, tree.options().game_level.lower());

    // The lower mesh reuses the skeleton and is coarser than the game level.
    let lower = tree.lower_mesh();
    assert_eq!(lower, tree.mesh_with_detail(&lod.lower));
    assert!(lower.triangle_count() < tree.triangle_count());
}

#[test]
fn the_game_level_sets_the_stored_mesh() {
    let mut options = TreePreset::aspen_small().tree.clone();
    options.game_level.leaf_stride = 2;
    let tree = Tree::new(options.clone()).unwrap();
    assert_eq!(tree.mesh(), &tree.mesh_with_detail(&options.game_level));
}

#[test]
fn every_preset_builds() {
    for preset in TreePreset::all() {
        let tree = Tree::from_preset(preset).unwrap();
        assert!(tree.triangle_count() > 0, "{}", preset.name);
    }
}

#[test]
fn a_preset_file_without_a_base_merges_onto_defaults() {
    let preset = TreePreset::from_yaml(
        "test",
        "tree:\n  default_seed: 7\n  branch: { levels: 1 }\n",
    )
    .unwrap();
    let tree = Tree::from_preset(&preset).unwrap();
    let mut expected = TreeOptions {
        default_seed: 7,
        ..TreeOptions::default()
    };
    expected.branch.levels = 1;
    assert_eq!(tree, Tree::new(expected).unwrap());
}

#[test]
fn preset_files_and_presets_report_each_kind_of_error() {
    let err = TreePreset::from_yaml("test", "tree: {").unwrap_err();
    assert!(matches!(err, PresetFileError::Yaml { .. }), "{err:?}");

    let err = TreePreset::from_yaml("test", "tree:\n  branch: { levels: 4 }\n").unwrap_err();
    assert!(
        matches!(&err, PresetFileError::Invalid { message } if message.contains("branch.levels is 4")),
        "{err:?}"
    );

    let mut preset = TreePreset::oak_small().clone();
    preset.tree.branch.levels = 4;
    assert_eq!(
        Tree::from_preset(&preset),
        Err(OptionsError::TooManyLevels { levels: 4 })
    );
}
