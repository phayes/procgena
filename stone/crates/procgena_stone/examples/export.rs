//! Export sample rocks as OBJ and GLB files for viewing in a 3D tool.
//!
//! ```text
//! cargo run -p procgena_stone --example export --features export-obj,export-gltf -- [OUT_DIR] [IMAGE_DIR]
//! ```
//!
//! The output directory defaults to `target/procgena_stone_export` in this workspace.
//! Rocks use the default texture set's 1k JPEG images, read from
//! `IMAGE_DIR/rock/<set>-1k-<map>.jpg`. `IMAGE_DIR` defaults to the repository's
//! `assets/textures/images`; download its files with
//! `uv run --project tools textures download stone --images`.
//! Missing images, and images that cannot be copied or read, are omitted.
//!
//! The example generates each sample seed with both meshing methods:
//!
//! - `<name>.obj` and `<name>.mtl` contain the full-detail mesh and material.
//!   Images are copied into `OUT_DIR/textures/` unless a file already exists there.
//! - `<name>.glb` contains the default detail levels and embeds available images.
//!
//! All GLB detail levels are included as scene nodes. Select one in the viewer,
//! or configure switching yourself. OBJ tools may need manual normal-map setup
//! because support for the exported bump-map entry varies.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use procgena_stone::export::gltf::{EmbeddedImage, GltfOptions, GltfTextures, ImageFormat, to_glb};
use procgena_stone::export::obj::{ObjOptions, to_obj};
use procgena_stone::{ImageMap, Mesher, Stone, StoneOptions, TextureSize, image_path};

/// Linear grey for rocks exported without a colour image.
const UNTEXTURED_COLOR: [f32; 3] = [0.35, 0.35, 0.35];

const SEEDS: [u32; 3] = [880, 1, 2];

fn main() -> Result<(), Box<dyn Error>> {
    // Use the optional output and image folders, or the workspace's defaults.
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let mut args = std::env::args().skip(1);
    let out_dir = args.next().map_or_else(
        || workspace.join("target/procgena_stone_export"),
        PathBuf::from,
    );
    let image_dir = args
        .next()
        .map_or_else(|| workspace.join("assets/textures/images"), PathBuf::from);
    fs::create_dir_all(&out_dir)?;

    // Find the default texture set's 1k image for each map, if it exists.
    let set = StoneOptions::default().material.texture;
    let texture_path = |map: ImageMap| -> Option<PathBuf> {
        let path = image_dir.join(image_path(&set, TextureSize::Size1k, map));
        path.is_file().then_some(path)
    };
    match texture_path(ImageMap::Color) {
        Some(_) => println!("textures from {}", image_dir.display()),
        None => println!(
            "no {set} images in {}; exporting colours only",
            image_dir.display()
        ),
    }
    // Copy available images into the output folder, replacing older copies, and
    // reference them by relative path so the folder can be moved as a unit.
    let mut copied = Vec::new();
    for map in ImageMap::ALL {
        if let Some(source) = texture_path(map) {
            let name = source.file_name().ok_or("image path has no file name")?;
            let copy = out_dir.join("textures").join(name);
            fs::create_dir_all(out_dir.join("textures"))?;
            fs::copy(&source, &copy)?;
            copied.push((map, format!("textures/{}", name.to_string_lossy())));
        }
    }
    let obj_path = |map: ImageMap| {
        copied
            .iter()
            .find(|(m, _)| *m == map)
            .map(|(_, path)| path.clone())
    };
    let embed = |map: ImageMap| {
        Some(EmbeddedImage {
            bytes: fs::read(texture_path(map)?).ok()?,
            format: ImageFormat::Jpeg,
        })
    };

    // Keep the default white so a colour image is not tinted; use grey when none is found.
    let textured = texture_path(ImageMap::Color).is_some();

    for mesher in [Mesher::MarchingCubes, Mesher::DualContouring] {
        for seed in SEEDS {
            let mut options = StoneOptions {
                seed,
                ..StoneOptions::default()
            };
            options.mesh.mesher = mesher;
            if !textured {
                options.material.color = UNTEXTURED_COLOR;
            }
            let stone = Stone::new(options)?;
            let tag = match mesher {
                Mesher::MarchingCubes => "mc",
                Mesher::DualContouring => "dc",
            };
            let stem = format!("stone_{seed}_{tag}");

            // Write the full-detail OBJ and its MTL, referencing the local image copies.
            let mtl_name = format!("{stem}.mtl");
            let obj = to_obj(
                stone.mesh(),
                stone.options(),
                &ObjOptions {
                    material_library: &mtl_name,
                    textures: Some(&obj_path),
                },
            )?;
            fs::write(out_dir.join(format!("{stem}.obj")), obj.obj)?;
            fs::write(out_dir.join(&mtl_name), obj.mtl)?;

            // Package the rock's detail levels and readable image files into a GLB.
            let meshes: Vec<_> = stone.levels().map(|(_, mesh)| mesh.clone()).collect();
            let glb = to_glb(
                &meshes,
                stone.options(),
                &GltfOptions {
                    textures: GltfTextures::Embed(&embed),
                },
            )?;
            fs::write(out_dir.join(format!("{stem}.glb")), &glb)?;

            println!(
                "{stem:18} {:>7} triangles, {:>5.2} MB glb",
                stone.mesh().triangle_count(),
                glb.len() as f64 / 1e6
            );
        }
    }
    println!("wrote {}", out_dir.display());
    Ok(())
}
