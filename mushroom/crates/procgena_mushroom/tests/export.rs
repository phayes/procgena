//! OBJ and glTF exports.
#![cfg(all(feature = "export-obj", feature = "export-gltf"))]

use procgena_mushroom::export::{gltf, obj};
use procgena_mushroom::{Mushroom, MushroomMesh, MushroomPreset};

fn meshes() -> (Mushroom, Vec<MushroomMesh>) {
    let mushroom = Mushroom::from_preset(MushroomPreset::fly_agaric()).unwrap();
    let meshes = mushroom.lod_meshes().map(|(_, m)| m).to_vec();
    (mushroom, meshes)
}

#[test]
fn obj_has_one_object_per_level() {
    let (mushroom, meshes) = meshes();
    let files = obj::to_obj(&meshes, mushroom.options(), "mushroom.mtl");
    assert!(files.obj.starts_with("# procgena_mushroom"));
    assert!(files.obj.contains("mtllib mushroom.mtl"));
    for lod in 0..meshes.len() {
        assert!(files.obj.contains(&format!("o mushroom_LOD{lod}\n")));
    }
    assert!(files.mtl.contains("newmtl mushroom_open"));
    let vertices = files.obj.lines().filter(|l| l.starts_with("v ")).count();
    let faces = files.obj.lines().filter(|l| l.starts_with("f ")).count();
    assert!(vertices > 0 && faces > 0);
}

#[test]
fn glb_loads_with_every_level_and_the_options() {
    let (mushroom, meshes) = meshes();
    let bytes = gltf::to_glb(&meshes, mushroom.options()).unwrap();
    let document = ::gltf::Gltf::from_slice(&bytes).unwrap();
    let names: Vec<_> = document
        .nodes()
        .filter_map(|n| n.name().map(str::to_owned))
        .collect();
    assert_eq!(names.len(), meshes.len());

    // The lower mesh is open, so it is drawn two-sided.
    let last = document.meshes().last().unwrap();
    let primitive = last.primitives().next().unwrap();
    assert!(primitive.material().double_sided());
    let reader = primitive.reader(|_| document.blob.as_deref());
    assert!(reader.read_colors(0).is_some());
    assert!(reader.read_positions().unwrap().count() > 0);

    let extras = document.as_json().asset.extras.as_ref().unwrap().get();
    assert!(extras.contains("mushroomOptions"));
}

#[test]
fn gltf_needs_meshes() {
    let mushroom = Mushroom::from_preset(MushroomPreset::button()).unwrap();
    assert!(matches!(
        gltf::to_gltf(&[], mushroom.options(), "m.bin"),
        Err(gltf::GltfError::NoMeshes)
    ));
}
