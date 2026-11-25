//! Creates rock models for other 3D tools: a single GLB file or an OBJ with
//! separate material and texture files. The caller saves the returned contents.

use std::collections::HashMap;
use std::fmt;

use procgena_stone::export::InvalidTextureSize;
use procgena_stone::export::gltf::{
    EmbeddedImage, GltfError, GltfOptions, GltfTextures, ImageFormat,
};
use procgena_stone::export::obj::ObjOptions;
use procgena_stone::{
    ImageMap, OptionsError, StoneField, StoneMesh, StoneOptions, TextureSize, image_path,
    mesh_field,
};

/// Selects whether an exported rock includes texture images and simpler versions.
/// The rock's [`MeshOptions::detail`](procgena_stone::MeshOptions::detail) sets the
/// mesh resolutions and the texture image size (that of its most detailed level).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoneModelSettings {
    /// Includes texture images. When off, only the material tint and roughness
    /// are exported.
    pub textures: bool,
    /// For GLB, includes every detail level after the most detailed one. Empty
    /// levels are omitted. Configure distance-based switching in the receiving
    /// tool. All included meshes are visible on import until the receiving tool
    /// hides or switches them. OBJ exports ignore this setting.
    pub detail_levels: bool,
}

impl Default for StoneModelSettings {
    /// Includes texture images and simpler meshes in GLB exports.
    fn default() -> Self {
        Self {
            textures: true,
            detail_levels: true,
        }
    }
}

/// The texture image size for exports: that of the rock's game level.
fn texture_size(options: &StoneOptions) -> Option<TextureSize> {
    Some(options.mesh.game_level.texture_size)
}

/// The reason a rock export could not be created.
#[derive(Debug)]
pub enum StoneExportError {
    /// A rock setting is invalid or a mesh could not be generated.
    Stone(OptionsError),
    /// No triangles remain. Reduce carving or increase Detail to retain the rock.
    Empty,
    /// The material's texture size is invalid, so images cannot be positioned on the rock.
    TextureSize(InvalidTextureSize),
    /// Creating the GLB contents failed; the wrapped error gives the reason.
    Gltf(GltfError),
}

impl fmt::Display for StoneExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stone(error) => write!(f, "cannot build this rock: {error}"),
            Self::Empty => write!(
                f,
                "there is no rock to export: the cuts remove all of it, or Detail is too low"
            ),
            Self::TextureSize(error) => write!(f, "cannot texture this rock: {error}"),
            Self::Gltf(error) => write!(f, "cannot write the GLB: {error}"),
        }
    }
}

impl std::error::Error for StoneExportError {}

/// Returns relative paths for the selected rock's colour, surface-detail
/// (normal) and roughness images, at its most detailed level's texture size.
/// Returns an empty list when textures are disabled or the detail set is empty.
pub fn image_paths(options: &StoneOptions, settings: &StoneModelSettings) -> Vec<String> {
    let Some(size) = texture_size(options).filter(|_| settings.textures) else {
        return Vec::new();
    };
    ImageMap::ALL
        .iter()
        .map(|&map| image_path(&options.material.texture, size, map))
        .collect()
}

/// Builds the rock's game-level mesh, then its lower mesh when
/// `detail_levels` is set. An empty game-level mesh is an error; an empty
/// lower mesh is skipped.
fn meshes(options: &StoneOptions, detail_levels: bool) -> Result<Vec<StoneMesh>, StoneExportError> {
    let field = StoneField::new(options).map_err(StoneExportError::Stone)?;
    let lod = options.mesh.game_level.lod();
    let levels = [lod.game, lod.lower];
    let count = if detail_levels { levels.len() } else { 1 };

    // Sample the same rock shape at each resolution, omitting an empty lower mesh.
    let mut meshes = Vec::with_capacity(count);
    for resolution in levels.iter().take(count).map(|level| level.resolution) {
        let mut mesh =
            mesh_field(&field, options.mesh.mesher, resolution).map_err(StoneExportError::Stone)?;
        if mesh.triangle_count() == 0 {
            if meshes.is_empty() {
                return Err(StoneExportError::Empty);
            }
            continue;
        }
        mesh.scale(options.scale);
        meshes.push(mesh);
    }
    Ok(meshes)
}

/// Returns GLB file contents for the supplied rock and export settings.
/// Supply JPEG image bytes keyed by the paths from [`image_paths`]; available
/// images are embedded, and missing images are omitted.
/// Returns an error for invalid rock settings, an empty main mesh or GLB generation failure.
pub fn glb(
    options: &StoneOptions,
    settings: &StoneModelSettings,
    images: &HashMap<String, Vec<u8>>,
) -> Result<Vec<u8>, StoneExportError> {
    let meshes = meshes(options, settings.detail_levels)?;
    let size = texture_size(options).expect("meshes() validated the detail levels");
    let embed = |map: ImageMap| {
        Some(EmbeddedImage {
            bytes: images
                .get(&image_path(&options.material.texture, size, map))?
                .clone(),
            format: ImageFormat::Jpeg,
        })
    };
    let gltf = GltfOptions {
        textures: if settings.textures {
            GltfTextures::Embed(&embed)
        } else {
            GltfTextures::None
        },
    };
    procgena_stone::export::gltf::to_glb(&meshes, options, &gltf).map_err(StoneExportError::Gltf)
}

/// Files to save together for an OBJ export: model, materials and available textures.
#[derive(Debug)]
pub struct StoneObjExport {
    /// Each file's relative path and bytes. Preserve these paths when saving the export.
    pub files: Vec<(String, Vec<u8>)>,
}

/// Returns an OBJ model and its companion files at the rock's requested mesh resolution.
/// Pass a file name without an extension or directory as `stem`: `rock` produces
/// `rock.obj`, `rock.mtl` and, when images are available, `rock_textures/`.
/// Supply image bytes keyed by [`image_paths`]; missing images are omitted.
/// Extra detail levels are ignored. Returns an error for invalid rock settings,
/// an empty mesh or an invalid material texture size.
pub fn obj(
    options: &StoneOptions,
    settings: &StoneModelSettings,
    images: &HashMap<String, Vec<u8>>,
    stem: &str,
) -> Result<StoneObjExport, StoneExportError> {
    let mesh = meshes(options, false)?.swap_remove(0);
    let size = texture_size(options).expect("meshes() validated the detail levels");
    let folder = format!("{stem}_textures");

    // Keep texture references relative to the model so the files can be moved together.
    let texture_path = |map: ImageMap| {
        let path = image_path(&options.material.texture, size, map);
        images
            .contains_key(&path)
            .then(|| format!("{folder}/{}", file_name(&path)))
    };
    let texture_path: &dyn Fn(ImageMap) -> Option<String> = &texture_path;
    let material_library = format!("{stem}.mtl");
    let files = procgena_stone::export::obj::to_obj(
        &mesh,
        options,
        &ObjOptions {
            material_library: &material_library,
            textures: settings.textures.then_some(texture_path),
        },
    )
    .map_err(StoneExportError::TextureSize)?;

    let mut out = vec![
        (format!("{stem}.obj"), files.obj.into_bytes()),
        (material_library, files.mtl.into_bytes()),
    ];
    for path in image_paths(options, settings) {
        if let Some(bytes) = images.get(&path) {
            out.push((format!("{folder}/{}", file_name(&path)), bytes.clone()));
        }
    }
    Ok(StoneObjExport { files: out })
}

/// Removes slash-separated folders from an image path, leaving just the file name.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use procgena_stone::{StoneDetail, StonePreset};

    use super::*;

    /// Uses the selected preset with a small game level (32 cells, so a
    /// 16-cell lower mesh) to keep tests quick.
    fn quick(preset: &StonePreset) -> StoneOptions {
        let mut options = preset.stone.clone();
        options.mesh.game_level = StoneDetail::new(32, TextureSize::Size1k);
        options
    }

    #[test]
    fn image_paths_are_the_images_the_exporter_asks_for() {
        let settings = StoneModelSettings::default();
        for preset in StonePreset::all() {
            let options = quick(preset);
            let asked = RefCell::new(Vec::new());
            let record = |map: ImageMap| {
                asked.borrow_mut().push(image_path(
                    &options.material.texture,
                    TextureSize::Size1k,
                    map,
                ));
                None
            };
            let gltf = GltfOptions {
                textures: GltfTextures::Embed(&record),
            };
            let mesh = meshes(&options, false).unwrap();
            procgena_stone::export::gltf::to_glb(&mesh, &options, &gltf).unwrap();

            let mut expected = asked.into_inner();
            let mut paths = image_paths(&options, &settings);
            expected.sort();
            paths.sort();
            assert_eq!(paths, expected, "{}", preset.name);
        }
    }

    #[test]
    fn colours_only_needs_no_images() {
        let settings = StoneModelSettings {
            textures: false,
            ..StoneModelSettings::default()
        };
        assert!(image_paths(&quick(StonePreset::boulder()), &settings).is_empty());
    }

    #[test]
    fn images_use_the_game_levels_texture_size() {
        let mut options = quick(StonePreset::boulder());
        options.mesh.game_level = StoneDetail::HIGH;
        let paths = image_paths(&options, &StoneModelSettings::default());
        let expected = image_path(
            &options.material.texture,
            TextureSize::Size2k,
            ImageMap::ALL[0],
        );
        assert!(paths.contains(&expected), "{paths:?}");
    }

    /// Supplies recognisable test bytes for every requested image path; these are not real JPEGs.
    fn fake_images(
        options: &StoneOptions,
        settings: &StoneModelSettings,
    ) -> HashMap<String, Vec<u8>> {
        image_paths(options, settings)
            .into_iter()
            .map(|path| {
                let bytes = path.as_bytes().to_vec();
                (path, bytes)
            })
            .collect()
    }

    #[test]
    fn detail_levels_are_the_game_level_and_lower_mesh() {
        let options = StonePreset::boulder().stone.clone();
        let levels = meshes(&options, true).unwrap();
        assert_eq!(levels.len(), 2);
        assert!(
            levels
                .windows(2)
                .all(|pair| pair[0].triangle_count() > 2 * pair[1].triangle_count()),
            "each level has well under half the triangles of the one before"
        );
        assert_eq!(meshes(&options, false).unwrap().len(), 1);
    }

    #[test]
    fn glb_embeds_the_images_and_follows_the_settings() {
        let options = quick(StonePreset::pebble());
        let settings = StoneModelSettings::default();
        let images = fake_images(&options, &settings);
        let full = glb(&options, &settings, &images).unwrap();
        for bytes in images.values() {
            assert!(
                full.windows(bytes.len()).any(|w| w == bytes),
                "every image is embedded"
            );
        }

        // Removing textures and simpler meshes should reduce the file size.
        let plain = StoneModelSettings {
            textures: false,
            detail_levels: false,
        };
        let small = glb(&options, &plain, &images).unwrap();
        assert!(small.len() < full.len());
        assert!(!String::from_utf8_lossy(&small).contains("\"images\""));
    }

    #[test]
    fn a_rock_carved_away_is_an_error() {
        let mut options = quick(StonePreset::boulder());
        options.carve.steps = 72;
        options.carve.depth = 1.2;
        let settings = StoneModelSettings::default();
        assert!(matches!(
            glb(&options, &settings, &HashMap::new()),
            Err(StoneExportError::Empty)
        ));
        assert!(matches!(
            obj(&options, &settings, &HashMap::new(), "rock"),
            Err(StoneExportError::Empty)
        ));
    }

    /// Downloads textures and writes `boulder.glb`, `boulder.obj` and companion
    /// files to `target/stone_export` for manual inspection in another 3D tool.
    #[test]
    #[ignore = "needs the network"]
    fn exports_with_downloaded_images() {
        use crate::export::images::{ImageStore, STONE_IMAGES, fetch_images};

        let options = StonePreset::boulder().stone.clone();
        let settings = StoneModelSettings::default();
        let store = ImageStore::new(STONE_IMAGES, "stone").unwrap();
        let (found, warnings) = fetch_images(Some(&store), &image_paths(&options, &settings));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(found.len(), 3);

        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/stone_export");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(
            out.join("boulder.glb"),
            glb(&options, &settings, &found).unwrap(),
        )
        .unwrap();
        let export = obj(&options, &settings, &found, "boulder").unwrap();
        crate::export::write_files(&out.join("boulder.obj"), &export.files).unwrap();
    }

    #[test]
    fn obj_names_its_files_after_the_stem() {
        let options = quick(StonePreset::river_stone());
        let settings = StoneModelSettings::default();
        let images = fake_images(&options, &settings);
        let export = obj(&options, &settings, &images, "pebble").unwrap();
        let names: Vec<&str> = export.files.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names[..2], ["pebble.obj", "pebble.mtl"]);
        assert_eq!(names.len(), 2 + images.len());

        // Check that the model references its material file and the material references its textures.
        let mtl = String::from_utf8_lossy(&export.files[1].1);
        assert!(String::from_utf8_lossy(&export.files[0].1).contains("mtllib pebble.mtl"));
        for name in &names[2..] {
            assert!(name.starts_with("pebble_textures/"));
            assert!(mtl.contains(name), "{name} in {mtl}");
        }

        // Missing images should leave a usable model and material file without texture references.
        let export = obj(&options, &settings, &HashMap::new(), "pebble").unwrap();
        assert_eq!(export.files.len(), 2);
        assert!(!String::from_utf8_lossy(&export.files[1].1).contains("map_"));
    }
}
