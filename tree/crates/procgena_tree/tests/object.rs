//! Check the tagged object a tree hands to other generators.

use procgena_mesh::{Part, SurfaceMesh, TagSet, tags};
use procgena_tree::{MeshPart, Tree, TreePreset, default_lod_levels};

/// The parts of `parts` with exactly `wanted` tags.
fn find<'a>(parts: &'a [Part], wanted: &TagSet) -> Option<&'a Part> {
    parts.iter().find(|p| &p.tags == wanted)
}

/// Copies triangles `shell` of `part` into a mesh of their own.
fn shell_mesh(part: &Part, shell: std::ops::Range<u32>) -> SurfaceMesh {
    let indices = &part.mesh.indices[shell.start as usize * 3..shell.end as usize * 3];
    SurfaceMesh {
        positions: part.mesh.positions.clone(),
        normals: part.mesh.normals.clone(),
        indices: indices.to_vec(),
    }
}

#[test]
fn levels_follow_the_lod_meshes() {
    let tree = Tree::from_preset(TreePreset::OakSmall);
    let lods = tree.lod_meshes(&default_lod_levels());
    let object = tree.object(&lods);
    object.validate().unwrap();
    let starts: Vec<f32> = object.levels.iter().map(|l| l.start).collect();
    assert_eq!(starts, [0.0, 10.0, 25.0, 50.0]);
}

#[test]
fn bark_splits_into_closed_trunk_limb_and_branch_parts() {
    let tree = Tree::from_preset(TreePreset::OakSmall);
    let lods = tree.lod_meshes(&default_lod_levels());
    let (_, mesh) = &lods[0];
    let parts = &tree.object(&lods).levels.remove(0).parts;

    let trunk = find(parts, &[tags::BARK, tags::TRUNK].into()).unwrap();
    let limb = find(parts, &[tags::BARK, tags::LIMB].into()).unwrap();
    let branch = find(parts, &[tags::BARK, tags::BRANCH].into()).unwrap();

    // Together the three parts hold every bark triangle once.
    let bark_triangles = trunk.triangle_count() + limb.triangle_count() + branch.triangle_count();
    assert_eq!(bark_triangles, mesh.branches.indices.len() / 3);

    // The trunk includes the branches that carry on from its tip, so it has
    // more triangles than the level-0 branch alone.
    assert!(trunk.triangle_count() > mesh.part(MeshPart::Trunk).buffers.indices.len() / 3);

    // Every part, and every shell in it, is watertight on its own.
    for part in [trunk, limb, branch] {
        assert!(part.closed);
        assert!(part.surface().unwrap().is_closed(), "{:?}", part.tags);
        for shell in &part.shells {
            let piece = shell_mesh(part, shell.clone());
            assert!(
                piece.surface().unwrap().is_closed(),
                "{:?} {shell:?}",
                part.tags
            );
        }
    }

    // One trunk chain per stem, and many limbs.
    assert_eq!(trunk.shells.len(), tree.options().branch.stems as usize);
    assert!(limb.shells.len() > 1);
}

#[test]
fn leaves_are_open_cards() {
    let tree = Tree::from_preset(TreePreset::OakSmall);
    let lods = tree.lod_meshes(&default_lod_levels());
    let object = tree.object(&lods);
    let leaves = find(&object.levels[0].parts, &[tags::LEAF, tags::CARD].into()).unwrap();
    assert!(!leaves.closed);
    assert!(leaves.shells.is_empty());
    assert_eq!(leaves.triangle_count(), lods[0].1.leaves.indices.len() / 3);
}

#[test]
fn coarse_bark_is_open_and_ribbons_are_left_out() {
    let tree = Tree::from_preset(TreePreset::OakSmall);
    let lods = tree.lod_meshes(&default_lod_levels());
    let object = tree.object(&lods);

    // LOD2 has open tubes.
    let far = &object.levels[2].parts;
    let trunk = find(far, &[tags::BARK, tags::TRUNK].into()).unwrap();
    assert!(!trunk.closed);

    // LOD3 draws bark as ribbons without normals, so it has no bark parts.
    let farthest = &object.levels[3].parts;
    assert!(farthest.iter().all(|p| !p.tags.contains(&tags::BARK)));
    assert!(find(farthest, &[tags::LEAF, tags::CARD].into()).is_some());
}

#[test]
#[should_panic(expected = "at least one level")]
fn an_object_needs_a_level() {
    Tree::from_preset(TreePreset::OakSmall).object(&[]);
}
