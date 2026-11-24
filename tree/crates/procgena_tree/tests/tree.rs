//! Verify that `Tree` produces the same layout and geometry as generating them
//! separately, and handles settings and detail levels.

use procgena_core::LodLevel;
use procgena_tree::{
    LodDetail, OptionsError, PresetFileError, Skeleton, Tree, TreeOptions, TreePreset, default_lod,
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
fn lod_meshes_are_sorted_by_distance_and_reuse_the_skeleton() {
    let tree = Tree::from_preset(TreePreset::aspen_small()).unwrap();

    // Supply levels in reverse distance order to check sorting. Give the extra
    // level at distance 10 different leaf detail so the two tied levels can be
    // distinguished when checking that their input order is preserved.
    let mut levels = default_lod().levels;
    levels.reverse();
    let mut tie = levels[2];
    assert_eq!(tie.distance, 10.0);
    tie.detail.leaf_stride = 3;
    levels.push(tie);

    let lods = tree.lod_meshes(&levels);
    let distances: Vec<f32> = lods.iter().map(|(level, _)| level.distance).collect();
    assert_eq!(distances, [0.0, 10.0, 10.0, 25.0, 50.0]);
    assert_eq!(lods[1].0, levels[2]);
    assert_eq!(lods[2].0, tie);

    for (level, mesh) in &lods {
        assert_eq!(mesh, &tree.mesh_with_detail(&level.detail));
    }
    assert_eq!(&lods[0].1, tree.mesh());
}

#[test]
fn lod_meshes_accepts_no_levels() {
    let tree = Tree::from_preset(TreePreset::bush_1()).unwrap();
    assert!(tree.lod_meshes(&[] as &[LodLevel<LodDetail>]).is_empty());
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
