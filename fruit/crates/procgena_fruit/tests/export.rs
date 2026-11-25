//! Check that a cluster exports to OBJ and glTF with every level.
#![cfg(all(feature = "export-obj", feature = "export-gltf"))]

use procgena_fruit::export::{gltf, obj};
use procgena_fruit::{Fruit, cluster_presets};

#[test]
fn every_preset_exports_each_level() {
    for (key, options) in cluster_presets() {
        let fruit = Fruit::new(options.clone(), options.default_seed, 0).unwrap();
        let chain = fruit.meshes();
        let meshes = vec![chain.game, chain.lower];

        let files = obj::to_obj(&meshes, fruit.options(), "fruit.mtl");
        let objects = files.obj.lines().filter(|l| l.starts_with("o ")).count();
        assert_eq!(objects, meshes.len(), "{key}");
        let faces = files.obj.lines().filter(|l| l.starts_with("f ")).count();
        assert_eq!(
            faces,
            meshes.iter().map(|m| m.triangle_count()).sum::<usize>()
        );
        assert!(files.mtl.contains("newmtl fruit"));

        let glb = gltf::to_glb(&meshes, fruit.options()).unwrap();
        let document = ::gltf::Gltf::from_slice(&glb).unwrap();
        assert_eq!(document.meshes().count(), meshes.len(), "{key}");
        assert!(document.materials().all(|m| m.double_sided()));
    }
}
