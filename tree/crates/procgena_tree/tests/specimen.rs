//! Check the tagged specimen a tree hands to other generators.

use procgena_specimen::{Part, SurfaceMesh, TagSet, tags};
use procgena_tree::{MeshPart, Tree, TreePreset};

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
fn the_specimen_holds_the_game_level_then_the_lower_mesh() {
    let tree = Tree::from_preset(TreePreset::oak_small()).unwrap();
    let specimen = tree.specimen();
    specimen.validate().unwrap();
    assert_eq!(specimen.levels.len(), 2);

    // The lower mesh is the coarser one.
    let triangles = |level: usize| -> usize {
        specimen.levels[level]
            .parts
            .iter()
            .map(Part::triangle_count)
            .sum()
    };
    assert!(triangles(1) < triangles(0));

    // The size the chain switches by covers the whole tree.
    let diameter = tree.mesh().bounding_diameter();
    assert!(diameter > tree.options().height_bound() * 0.5);
}

#[test]
fn bark_splits_into_closed_trunk_limb_and_branch_parts() {
    let tree = Tree::from_preset(TreePreset::oak_small()).unwrap();
    let mesh = tree.mesh();
    let parts = &tree.specimen().levels.remove(0).parts;

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
    let tree = Tree::from_preset(TreePreset::oak_small()).unwrap();
    let specimen = tree.specimen();
    let leaves = find(&specimen.levels[0].parts, &[tags::LEAF, tags::CARD].into()).unwrap();
    assert!(!leaves.closed);
    assert!(leaves.shells.is_empty());
    assert_eq!(
        leaves.triangle_count(),
        tree.mesh().leaves.indices.len() / 3
    );
}

#[test]
fn the_lower_level_has_open_bark_and_leaf_cards() {
    let tree = Tree::from_preset(TreePreset::oak_small()).unwrap();
    let specimen = tree.specimen();
    let lower = &specimen.levels[1].parts;
    let trunk = find(lower, &[tags::BARK, tags::TRUNK].into()).unwrap();
    assert!(!trunk.closed);
    assert!(find(lower, &[tags::LEAF, tags::CARD].into()).is_some());
}
