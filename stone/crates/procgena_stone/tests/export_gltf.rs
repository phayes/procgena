//! Check glTF exports with an independent reader: geometry, materials, image
//! references, detail-level names, and generation metadata should survive export.
#![cfg(feature = "export-gltf")]

use gltf::Gltf;
use procgena_stone::export::gltf::{
    EmbeddedImage, GltfError, GltfOptions, GltfTextures, ImageFormat, to_glb, to_gltf,
};
use procgena_stone::{
    DetailLevel, ImageMap, MaterialOptions, Stone, StoneDetail, StoneMesh, StoneOptions,
    TextureSize,
};

/// Creates a small, repeatable rock for export checks, with two detail levels
/// at 24 and 12 cells per axis.
fn stone() -> Stone {
    let mut options = StoneOptions {
        default_seed: 5,
        ..StoneOptions::default()
    };
    options.mesh.detail = StoneDetail::new(vec![
        DetailLevel::new(24, 0.0, TextureSize::Size512),
        DetailLevel::new(12, 10.0, TextureSize::Size512),
    ]);
    Stone::new(options).unwrap()
}

/// Parses GLB contents, failing the test if the glTF reader rejects them.
fn read(glb: &[u8]) -> Gltf {
    Gltf::from_slice(glb).expect("the exported file is valid glTF")
}

#[test]
fn glb_holds_the_mesh_with_its_vertex_data() {
    let stone = stone();
    let mesh = stone.mesh();
    let glb = to_glb(
        std::slice::from_ref(mesh),
        stone.options(),
        &GltfOptions::default(),
    )
    .unwrap();
    assert_eq!(&glb[..4], b"glTF");
    assert_eq!(glb.len() % 4, 0);
    let gltf = read(&glb);
    let blob = gltf.blob.as_deref();

    let names: Vec<_> = gltf
        .meshes()
        .map(|m| m.name().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["stone"]);
    assert_eq!(gltf.default_scene().unwrap().nodes().count(), 1);

    let primitive = gltf.meshes().next().unwrap().primitives().next().unwrap();
    let reader = primitive.reader(|_| blob);
    let positions: Vec<[f32; 3]> = reader.read_positions().unwrap().collect();
    let normals: Vec<[f32; 3]> = reader.read_normals().unwrap().collect();
    let uvs: Vec<[f32; 2]> = reader.read_tex_coords(0).unwrap().into_f32().collect();
    let tangents: Vec<[f32; 4]> = reader.read_tangents().unwrap().collect();
    let indices: Vec<u32> = reader.read_indices().unwrap().into_u32().collect();
    assert!(
        positions.len() > mesh.vertex_count(),
        "seam vertices are split"
    );
    assert_eq!(normals.len(), positions.len());
    assert_eq!(uvs.len(), positions.len());
    assert_eq!(tangents.len(), positions.len());

    // Texture seams may split vertices, but must not move corners or change normals.
    assert_eq!(indices.len(), mesh.indices.len());
    for (&i, &original) in indices.iter().zip(&mesh.indices) {
        assert_eq!(positions[i as usize], mesh.positions[original as usize]);
        assert_eq!(normals[i as usize], mesh.normals[original as usize]);
    }

    // The exported position bounds should enclose the same sample rock.
    let bounds = primitive.bounding_box();
    let (min, max) = mesh.bounds().unwrap();
    assert_eq!(bounds.min, min);
    assert_eq!(bounds.max, max);

    let material = primitive.material();
    assert_eq!(material.name(), Some("stone"));
    assert_eq!(material.alpha_mode(), gltf::material::AlphaMode::Opaque);
    let pbr = material.pbr_metallic_roughness();
    assert_eq!(pbr.metallic_factor(), 0.0);
    assert_eq!(pbr.roughness_factor(), MaterialOptions::default().roughness);
}

#[test]
fn material_metalness_becomes_the_metallic_factor() {
    let stone = stone();
    let mut options = stone.options().clone();
    options.material.metallic = 0.75;
    let glb = to_glb(
        std::slice::from_ref(stone.mesh()),
        &options,
        &GltfOptions::default(),
    )
    .unwrap();
    let gltf = read(&glb);
    let material = gltf.materials().next().unwrap();
    assert_eq!(material.pbr_metallic_roughness().metallic_factor(), 0.75);
}

#[test]
fn bitangents_point_towards_the_top_of_the_image() {
    // The normal map's positive green direction is image-up. The exported
    // bitangent, cross(normal, tangent) * w, should point towards decreasing v.
    let stone = stone();
    let glb = to_glb(
        std::slice::from_ref(stone.mesh()),
        stone.options(),
        &GltfOptions::default(),
    )
    .unwrap();
    let gltf = read(&glb);
    let primitive = gltf.meshes().next().unwrap().primitives().next().unwrap();
    let reader = primitive.reader(|_| gltf.blob.as_deref());
    let positions: Vec<glam::Vec3> = reader.read_positions().unwrap().map(Into::into).collect();
    let normals: Vec<glam::Vec3> = reader.read_normals().unwrap().map(Into::into).collect();
    let tangents: Vec<[f32; 4]> = reader.read_tangents().unwrap().collect();
    let uvs: Vec<glam::Vec2> = reader
        .read_tex_coords(0)
        .unwrap()
        .into_f32()
        .map(Into::into)
        .collect();
    let indices: Vec<u32> = reader.read_indices().unwrap().into_u32().collect();

    // Compare tangent directions with UV changes only where vertex normals
    // closely match the face normal. Smooth normals can tilt the lighting
    // frame away from the triangle's geometric texture directions.
    let (mut checked, mut wrong) = (0, 0);
    let triangles = indices.len() / 3;
    for t in indices.as_chunks::<3>().0 {
        let [a, b, c] = t.map(|i| i as usize);
        let (e1, e2) = (positions[b] - positions[a], positions[c] - positions[a]);
        let face = e1.cross(e2).normalize_or_zero();
        if [a, b, c].iter().any(|&i| normals[i].dot(face) < 0.9) {
            continue;
        }

        // Recover the world-space directions for increasing u and v from the triangle's UVs.
        let (d1, d2) = (uvs[b] - uvs[a], uvs[c] - uvs[a]);
        let det = d1.perp_dot(d2);
        if det.abs() < 1e-8 {
            continue;
        }
        let along_u = (e1 * d2.y - e2 * d1.y) / det;
        let along_v = (e2 * d1.x - e1 * d2.x) / det;
        let [x, y, z, w] = tangents[a];
        let tangent = glam::Vec3::new(x, y, z);
        let bitangent = normals[a].cross(tangent) * w;
        checked += 1;
        if tangent.dot(along_u) <= 0.0 || bitangent.dot(along_v) >= 0.0 {
            wrong += 1;
        }
    }
    assert!(checked * 2 > triangles, "{checked} of {triangles} checked");
    assert_eq!(wrong, 0, "{wrong} of {checked} triangles");
}

#[test]
fn settings_are_stored_in_the_file() {
    let stone = stone();
    let glb = to_glb(
        std::slice::from_ref(stone.mesh()),
        stone.options(),
        &GltfOptions::default(),
    )
    .unwrap();
    let gltf = read(&glb);
    let extras = gltf.document.as_json().asset.extras.as_ref().unwrap();
    let value: serde_json::Value = serde_json::from_str(extras.get()).unwrap();
    let stored: StoneOptions = serde_json::from_value(value["stoneOptions"].clone()).unwrap();
    assert_eq!(&stored, stone.options());
    assert!(
        gltf.document
            .as_json()
            .asset
            .generator
            .as_deref()
            .unwrap()
            .starts_with("procgena_stone ")
    );
}

#[test]
fn levels_of_detail_get_suffixed_names() {
    let stone = stone();
    let meshes: Vec<StoneMesh> = stone.levels().map(|(_, m)| m.clone()).collect();
    let gltf = read(&to_glb(&meshes, stone.options(), &GltfOptions::default()).unwrap());
    let names: Vec<_> = gltf
        .meshes()
        .map(|m| m.name().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["stone_LOD0", "stone_LOD1"]);
    let triangles: Vec<usize> = gltf
        .meshes()
        .map(|m| {
            let p = m.primitives().next().unwrap();
            p.reader(|_| gltf.blob.as_deref())
                .read_indices()
                .unwrap()
                .into_u32()
                .count()
                / 3
        })
        .collect();
    let expected: Vec<usize> = meshes.iter().map(StoneMesh::triangle_count).collect();
    assert_eq!(triangles, expected);
    // Detail levels should reuse one material rather than create copies.
    assert_eq!(gltf.materials().count(), 1);
}

#[test]
fn embedded_and_referenced_textures() {
    let stone = stone();
    let mesh = std::slice::from_ref(stone.mesh());
    let embed = |map: ImageMap| {
        let format = if map == ImageMap::Normal {
            ImageFormat::Png
        } else {
            ImageFormat::Jpeg
        };
        Some(EmbeddedImage {
            bytes: format!("{map:?}").into_bytes(),
            format,
        })
    };
    let options = GltfOptions {
        textures: GltfTextures::Embed(&embed),
    };
    let gltf = read(&to_glb(mesh, stone.options(), &options).unwrap());
    let blob = gltf.blob.as_deref().unwrap();

    // These payloads are test labels, not real image files. Check that bytes and
    // format labels survive export; this test does not decode or validate images.
    let images: Vec<(String, &str)> = gltf
        .images()
        .map(|image| match image.source() {
            gltf::image::Source::View { view, mime_type } => {
                let bytes = &blob[view.offset()..view.offset() + view.length()];
                (String::from_utf8(bytes.to_vec()).unwrap(), mime_type)
            }
            gltf::image::Source::Uri { .. } => panic!("expected an embedded image"),
        })
        .collect();
    assert_eq!(
        images,
        [
            ("Color".to_owned(), "image/jpeg"),
            ("Orm".to_owned(), "image/jpeg"),
            ("Normal".to_owned(), "image/png"),
        ]
    );

    // Material inputs should reference the intended images with repeat wrapping.
    let material = gltf.materials().next().unwrap();
    let pbr = material.pbr_metallic_roughness();
    let color = pbr.base_color_texture().unwrap().texture();
    assert_eq!(color.name(), Some("stone_color"));
    assert_eq!(
        color.sampler().wrap_s(),
        gltf::texture::WrappingMode::Repeat
    );
    // Both material properties must reference the same combined ORM image.
    let orm = pbr.metallic_roughness_texture().unwrap().texture();
    assert_eq!(orm.name(), Some("stone_orm"));
    let occlusion = material.occlusion_texture().unwrap();
    assert_eq!(occlusion.texture().index(), orm.index());
    assert_eq!(occlusion.strength(), 1.0);
    assert_eq!(
        material.normal_texture().unwrap().texture().name(),
        Some("stone_normal")
    );

    // Keep supplied image URIs unchanged, and leave out maps the callback omits.
    let uri = |map: ImageMap| (map == ImageMap::Color).then(|| "textures/c.jpg".to_owned());
    let options = GltfOptions {
        textures: GltfTextures::Uri(&uri),
    };
    let gltf = read(&to_glb(mesh, stone.options(), &options).unwrap());
    let uris: Vec<String> = gltf
        .images()
        .map(|image| match image.source() {
            gltf::image::Source::Uri { uri, .. } => uri.to_owned(),
            gltf::image::Source::View { .. } => panic!("expected a referenced image"),
        })
        .collect();
    assert_eq!(uris, ["textures/c.jpg"]);
    let material = gltf.materials().next().unwrap();
    assert!(material.normal_texture().is_none());
}

#[test]
fn gltf_with_separate_binary_file() {
    let stone = stone();
    let files = to_gltf(
        std::slice::from_ref(stone.mesh()),
        stone.options(),
        &GltfOptions::default(),
        "rock.bin",
    )
    .unwrap();
    let gltf = Gltf::from_slice(files.json.as_bytes()).unwrap();
    let buffer = gltf.buffers().next().unwrap();
    assert_eq!(buffer.length(), files.bin.len());
    assert!(matches!(
        buffer.source(),
        gltf::buffer::Source::Uri("rock.bin")
    ));

    // Use the separate binary payload to read the geometry described by the JSON.
    let primitive = gltf.meshes().next().unwrap().primitives().next().unwrap();
    let reader = primitive.reader(|_| Some(&files.bin));
    let indices: Vec<u32> = reader.read_indices().unwrap().into_u32().collect();
    assert_eq!(indices.len(), stone.mesh().indices.len());
}

#[test]
fn missing_or_empty_meshes_are_errors() {
    let options = StoneOptions::default();
    assert!(matches!(
        to_glb(&[], &options, &GltfOptions::default()),
        Err(GltfError::NoMeshes)
    ));
    let meshes = [stone().mesh().clone(), StoneMesh::default()];
    assert!(matches!(
        to_glb(&meshes, &options, &GltfOptions::default()),
        Err(GltfError::EmptyMesh(1))
    ));
}

#[test]
fn invalid_texture_sizes_are_errors() {
    let meshes = [stone().mesh().clone()];
    let gltf = GltfOptions::default();
    for texture_size in [0.0, -2.0, f32::NAN, f32::NEG_INFINITY] {
        let mut options = StoneOptions::default();
        options.material.texture_size = texture_size;
        assert!(matches!(
            to_glb(&meshes, &options, &gltf),
            Err(GltfError::InvalidTextureSize(_))
        ));
        assert!(matches!(
            to_gltf(&meshes, &options, &gltf, "stone.bin"),
            Err(GltfError::InvalidTextureSize(_))
        ));
    }
}
