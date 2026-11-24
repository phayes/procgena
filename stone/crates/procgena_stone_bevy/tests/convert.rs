//! Verifies that mesh and material conversions preserve the data Bevy needs to draw rocks.

use bevy::mesh::{Indices, Mesh, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::Face;
use procgena_stone::{MaterialOptions, Stone, StoneDetail, StoneOptions, TextureSize};
use procgena_stone_bevy::{StoneTextures, stone_material, stone_mesh};

fn float3(mesh: &Mesh, id: impl Into<bevy::mesh::MeshVertexAttributeId>) -> Vec<[f32; 3]> {
    match mesh.attribute(id) {
        Some(VertexAttributeValues::Float32x3(values)) => values.clone(),
        other => panic!("unexpected attribute {other:?}"),
    }
}

#[test]
fn stone_mesh_keeps_vertices_and_triangles() {
    let mut options = StoneOptions {
        default_seed: 4,
        ..StoneOptions::default()
    };
    options.mesh.detail = StoneDetail::single(24, TextureSize::Size512);
    let stone = Stone::new(options).unwrap();
    let source = stone.mesh();
    let mesh = stone_mesh(source);

    assert_eq!(float3(&mesh, Mesh::ATTRIBUTE_POSITION), source.positions);
    assert_eq!(float3(&mesh, Mesh::ATTRIBUTE_NORMAL), source.normals);
    match mesh.indices() {
        Some(Indices::U32(indices)) => assert_eq!(indices, &source.indices),
        other => panic!("unexpected indices {other:?}"),
    }
    // Textures are projected by the material, so the mesh carries no texture coordinates.
    assert!(!mesh.contains_attribute(Mesh::ATTRIBUTE_UV_0));
}

#[test]
fn material_uses_colour_roughness_metalness_and_texture_size() {
    let options = MaterialOptions {
        color: [0.2, 0.4, 0.6],
        roughness: 0.7,
        metallic: 0.4,
        texture_size: 3.0,
        ..MaterialOptions::default()
    };

    let plain = stone_material(&options, None);
    assert_eq!(plain.base.base_color, Color::linear_rgb(0.2, 0.4, 0.6));
    assert_eq!(plain.base.perceptual_roughness, 0.7);
    assert_eq!(plain.base.metallic, 0.4);
    assert_eq!(plain.base.cull_mode, Some(Face::Back));
    assert_eq!(plain.extension.settings.texture_size, 3.0);
    assert_eq!(plain.extension.settings.textured, 0);
    assert!(plain.extension.color.is_none());

    let textures = StoneTextures {
        color: Handle::default(),
        normal: Handle::default(),
        orh: Handle::default(),
    };
    let textured = stone_material(&options, Some(&textures));
    assert_eq!(textured.extension.settings.textured, 1);
    assert_eq!(textured.extension.color, Some(textures.color.clone()));
    assert_eq!(textured.extension.normal, Some(textures.normal.clone()));
    assert_eq!(textured.extension.orh, Some(textures.orh.clone()));
    // The base material's own texture slots stay empty; the extension samples the images.
    assert!(textured.base.base_color_texture.is_none());
    assert!(textured.base.normal_map_texture.is_none());
}

#[test]
fn shader_settings_match_the_rust_layout() {
    // The WESL struct must list the same fields, in the same order, as `TriplanarSettings`.
    let shader = include_str!("../src/shaders/stone_material.wesl");
    let start = shader.find("struct TriplanarSettings {").unwrap();
    let end = start + shader[start..].find('}').unwrap();
    let fields: Vec<&str> = shader[start..end]
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//") && !line.starts_with("struct"))
        .collect();
    assert_eq!(fields, ["texture_size: f32,", "textured: u32,"]);
}
