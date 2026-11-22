//! Verify that `Tree` produces the same layout and geometry as generating them
//! separately, and handles settings and detail levels.

use procgena_tree::{
    LodDetail, LodLevel, OptionsError, Skeleton, Tree, TreeError, TreeOptions, TreePreset,
    default_lod_levels,
};

#[test]
fn new_combines_skeleton_and_full_detail_mesh() {
    let options = TreePreset::OakSmall.options();
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
    let tree = Tree::from_preset(TreePreset::AspenSmall);

    // Supply levels in reverse distance order to check sorting. Give the extra
    // level at distance 10 different leaf detail so the two tied levels can be
    // distinguished when checking that their input order is preserved.
    let mut levels = default_lod_levels().to_vec();
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
    let tree = Tree::from_preset(TreePreset::Bush1);
    assert!(tree.lod_meshes(&[] as &[LodLevel]).is_empty());
}

#[test]
fn every_preset_builds() {
    for preset in TreePreset::ALL {
        let tree = Tree::from_preset(preset);
        assert!(tree.triangle_count() > 0, "{}", preset.name());
    }
}

#[test]
fn from_json_merges_onto_defaults() {
    let tree = Tree::from_json(r#"{ "seed": 7, "branch": { "levels": 1 } }"#).unwrap();
    let mut expected = TreeOptions {
        seed: 7,
        ..TreeOptions::default()
    };
    expected.branch.levels = 1;
    assert_eq!(tree, Tree::new(expected).unwrap());
}

#[test]
fn from_json_reports_each_kind_of_error() {
    let err = Tree::from_json("not json").unwrap_err();
    assert!(matches!(err, TreeError::Json(_)), "{err:?}");

    let err = Tree::from_json(r#"{ "branch": { "levels": 4 } }"#).unwrap_err();
    assert!(
        matches!(
            err,
            TreeError::Options(OptionsError::TooManyLevels { levels: 4 })
        ),
        "{err:?}"
    );
    assert!(std::error::Error::source(&err).is_some());
}
