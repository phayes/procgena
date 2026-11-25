//! OBJ and glTF export.

#![cfg(all(feature = "export-obj", feature = "export-gltf"))]

use procgena_deadwood::export::{gltf::to_glb, obj::to_obj};
use procgena_deadwood::{DeadwoodPreset, Piece};

/// Builds `preset` and its game-level and lower meshes.
fn meshes(preset: &DeadwoodPreset) -> (Piece, Vec<procgena_deadwood::DeadwoodMesh>) {
    let piece = Piece::from_preset(preset).unwrap();
    let meshes = piece.lod_meshes().into_iter().map(|(_, m)| m).collect();
    (piece, meshes)
}

#[test]
fn glb_has_one_node_per_level_and_wood_values() {
    let (piece, meshes) = meshes(DeadwoodPreset::fallen_limb());
    let glb = to_glb(&meshes, piece.options()).unwrap();
    let doc = gltf::Gltf::from_slice(&glb).unwrap();
    let names: Vec<_> = doc
        .nodes()
        .filter_map(|n| n.name().map(str::to_owned))
        .collect();
    assert_eq!(names.len(), meshes.len());
    assert_eq!(names[0], "deadwood_LOD0");
    let first = doc.meshes().next().unwrap();
    let primitive = first.primitives().next().unwrap();
    assert!(
        primitive
            .attributes()
            .any(|(s, _)| s == gltf::Semantic::Extras("WOOD".to_owned()))
    );
    let extras = doc.as_json().asset.extras.as_ref().unwrap().get();
    assert!(extras.contains("deadwoodOptions"));
}

#[test]
fn strips_get_a_two_sided_material() {
    let (piece, meshes) = meshes(DeadwoodPreset::fallen_limb());
    assert!(meshes.iter().any(|m| m.ribbons.is_some()));
    let glb = to_glb(&meshes, piece.options()).unwrap();
    let doc = gltf::Gltf::from_slice(&glb).unwrap();
    let strip = doc
        .materials()
        .find(|m| m.name() == Some("deadwood_strip"))
        .unwrap();
    assert!(strip.double_sided());
}

#[test]
fn obj_has_one_object_per_level() {
    let (piece, meshes) = meshes(DeadwoodPreset::hollow_log());
    let files = to_obj(&meshes, piece.options(), "log.mtl");
    let objects = files.obj.lines().filter(|l| l.starts_with("o ")).count();
    assert_eq!(objects, meshes.len());
    assert!(files.obj.contains("mtllib log.mtl"));
    assert!(files.mtl.contains("newmtl deadwood"));
}
