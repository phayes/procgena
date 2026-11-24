//! Check exported OBJ geometry and MTL materials by reading their text records.
#![cfg(feature = "export-obj")]

use procgena_stone::export::obj::{ObjOptions, to_obj};
use procgena_stone::{
    ImageMap, MaterialOptions, Stone, StoneDetail, StoneMesh, StoneOptions, TextureSize,
};

/// Creates a small, repeatable rock for export checks.
fn stone() -> Stone {
    let mut options = StoneOptions {
        default_seed: 3,
        ..StoneOptions::default()
    };
    options.mesh.detail = StoneDetail::single(24, TextureSize::Size512);
    Stone::new(options).unwrap()
}

/// Parses numeric values from lines beginning with `prefix`, such as `v ` or `vt `.
/// Malformed numbers fail the test.
fn records(text: &str, prefix: &str) -> Vec<Vec<f32>> {
    text.lines()
        .filter_map(|line| line.strip_prefix(prefix))
        .map(|rest| rest.split(' ').map(|n| n.parse().unwrap()).collect())
        .collect()
}

#[test]
fn obj_contains_every_triangle_with_shared_indices() {
    let stone = stone();
    let mesh = stone.mesh();
    let files = to_obj(mesh, stone.options(), &ObjOptions::default()).unwrap();
    let obj = &files.obj;
    assert!(obj.contains("mtllib stone.mtl\n"));
    assert!(obj.contains("o stone\n"));
    assert!(obj.contains("usemtl stone\n"));

    let positions = records(obj, "v ");
    let uvs = records(obj, "vt ");
    let normals = records(obj, "vn ");
    assert!(
        positions.len() > mesh.vertex_count(),
        "seam vertices are split"
    );
    assert_eq!(uvs.len(), positions.len());
    assert_eq!(normals.len(), positions.len());

    // Each face corner should use matching position, UV, and normal indices,
    // and should retain the original mesh position.
    let faces: Vec<&str> = obj.lines().filter(|l| l.starts_with("f ")).collect();
    assert_eq!(faces.len(), mesh.triangle_count());
    for (face, t) in faces.iter().zip(mesh.indices.as_chunks::<3>().0) {
        let corners: Vec<usize> = face[2..]
            .split(' ')
            .map(|corner| {
                let parts: Vec<&str> = corner.split('/').collect();
                assert_eq!(parts.len(), 3);
                assert!(parts.iter().all(|p| *p == parts[0]));
                parts[0].parse().unwrap()
            })
            .collect();
        for (&c, &original) in corners.iter().zip(t) {
            let expected = mesh.positions[original as usize].to_vec();
            assert_eq!(positions[c - 1], expected);
        }
    }
}

#[test]
fn mtl_lists_material_and_textures() {
    let options = StoneOptions {
        material: MaterialOptions {
            color: [0.25, 0.5, 0.75],
            roughness: 0.6,
            metallic: 0.8,
            texture_size: 2.0,
            ..MaterialOptions::default()
        },
        ..StoneOptions::default()
    };
    let path = |map: ImageMap| match map {
        ImageMap::Color => Some("textures/c.jpg".to_owned()),
        ImageMap::Normal => Some("textures/n.png".to_owned()),
        ImageMap::Orm => None,
    };
    let files = to_obj(
        stone().mesh(),
        &options,
        &ObjOptions {
            material_library: "rock.mtl",
            textures: Some(&path),
        },
    )
    .unwrap();
    assert!(files.obj.contains("mtllib rock.mtl\n"));
    let mtl = &files.mtl;
    assert!(mtl.contains("newmtl stone\n"));
    assert!(mtl.contains("Kd 0.25 0.5 0.75\n"));
    assert!(mtl.contains("Pr 0.6\n"));
    assert!(mtl.contains("Pm 0.8\n"));
    assert!(mtl.contains("map_Kd textures/c.jpg\n"));
    assert!(mtl.contains("map_Bump textures/n.png\n"));
    assert!(!mtl.contains("map_Pr"));

    // The material must request the green channel, which stores roughness.
    let orm = |map: ImageMap| (map == ImageMap::Orm).then(|| "textures/orm.jpg".to_owned());
    let with_orm = to_obj(
        stone().mesh(),
        &options,
        &ObjOptions {
            textures: Some(&orm),
            ..ObjOptions::default()
        },
    )
    .unwrap()
    .mtl;
    assert!(with_orm.contains("map_Pr -imfchan g textures/orm.jpg\n"));

    // The default material should contain no texture-image references.
    let plain = to_obj(stone().mesh(), &options, &ObjOptions::default())
        .unwrap()
        .mtl;
    assert!(!plain.contains("map_"));
}

/// Default settings with the given material texture size.
fn with_texture_size(texture_size: f32) -> StoneOptions {
    let mut options = StoneOptions::default();
    options.material.texture_size = texture_size;
    options
}

#[test]
fn texture_size_scales_coordinates() {
    let stone = stone();
    let uvs = |texture_size| {
        let options = with_texture_size(texture_size);
        records(
            &to_obj(stone.mesh(), &options, &ObjOptions::default())
                .unwrap()
                .obj,
            "vt ",
        )
    };
    for (a, b) in uvs(1.0).iter().zip(uvs(2.0)) {
        assert!((a[0] - 2.0 * b[0]).abs() < 1e-5 && (a[1] - 2.0 * b[1]).abs() < 1e-5);
    }
}

#[test]
fn invalid_texture_sizes_are_rejected() {
    let stone = stone();
    for texture_size in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        let options = with_texture_size(texture_size);
        let err = to_obj(stone.mesh(), &options, &ObjOptions::default()).unwrap_err();
        assert!(err.0 == texture_size || (err.0.is_nan() && texture_size.is_nan()));
    }
}

#[test]
fn empty_mesh_has_no_faces() {
    let obj = to_obj(
        &StoneMesh::default(),
        &StoneOptions::default(),
        &ObjOptions::default(),
    )
    .unwrap()
    .obj;
    assert!(obj.contains("o stone\n"));
    assert!(
        !obj.lines()
            .any(|l| l.starts_with("v ") || l.starts_with("f "))
    );
}
