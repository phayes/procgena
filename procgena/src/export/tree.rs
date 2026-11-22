//! Export trees for other 3D tools as GLB files or OBJ files with companion materials.

use std::collections::HashMap;
use std::fmt;

use procgena_tree::export::gltf::{
    EmbeddedImage, GltfError, GltfOptions, GltfTextures, ImageFormat,
};
use procgena_tree::export::obj::ObjOptions;
use procgena_tree::export::{TextureMap, TextureRef};
use procgena_tree::{
    BarkMaterial, OptionsError, RigError, RigOptions, TextureSize, Tree, TreeOptions,
    default_lod_levels, leaf_has_normal_map,
};

/// Controls the images, detail levels and animation joints included in an export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSettings {
    /// Bark image resolution, or `None` to export colours without texture images.
    /// Leaf images use the fixed resolution available for each leaf type.
    pub textures: Option<TextureSize>,
    /// GLB only: include two simpler versions alongside the full-detail tree.
    /// These can be used at a distance; the receiving tool must configure switching.
    pub detail_levels: bool,
    /// GLB only: include a rig (joints used to animate bark and leaves).
    /// No animation clips are included; fruit and flowers remain fixed.
    pub rig: bool,
}

impl Default for ModelSettings {
    /// Uses 1k bark images and includes all detail levels and animation joints.
    fn default() -> Self {
        Self {
            textures: Some(TextureSize::Size1k),
            detail_levels: true,
            rig: true,
        }
    }
}

/// A problem that prevented the tree model from being exported.
#[derive(Debug)]
pub enum ExportError {
    /// The supplied settings could not produce a tree.
    Tree(OptionsError),
    /// Animation joints could not be created for this tree.
    Rig(RigError),
    /// The GLB file contents could not be generated.
    Gltf(GltfError),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tree(error) => write!(f, "cannot build this tree: {error}"),
            Self::Rig(error) => write!(f, "cannot rig this tree: {error}"),
            Self::Gltf(error) => write!(f, "cannot write the GLB: {error}"),
        }
    }
}

impl std::error::Error for ExportError {}

/// Lists the bark and leaf images needed for the selected export settings.
/// Returns no paths for a colours-only export.
pub fn image_paths(options: &TreeOptions, settings: &ModelSettings) -> Vec<String> {
    let Some(size) = settings.textures else {
        return Vec::new();
    };
    let mut wanted = Vec::new();
    if BarkMaterial::new(&options.bark).textured {
        for map in [
            TextureMap::BarkColor,
            TextureMap::BarkNormal,
            TextureMap::BarkRoughness,
        ] {
            wanted.push(TextureRef {
                kind: &options.bark.kind,
                map,
            });
        }
    }
    wanted.push(TextureRef {
        kind: &options.leaves.kind,
        map: TextureMap::LeafColor,
    });
    if leaf_has_normal_map(&options.leaves.kind) {
        wanted.push(TextureRef {
            kind: &options.leaves.kind,
            map: TextureMap::LeafNormal,
        });
    }
    wanted
        .iter()
        .map(|texture| texture.image_path(size))
        .collect()
}

/// Creates GLB file contents from the tree settings. Available images are
/// embedded in the file; supply them under the paths returned by [`image_paths`].
pub fn glb(
    options: &TreeOptions,
    settings: &ModelSettings,
    images: &HashMap<String, Vec<u8>>,
) -> Result<Vec<u8>, ExportError> {
    let tree = Tree::new(options.clone()).map_err(ExportError::Tree)?;

    // Include the full-detail tree first, followed by simpler versions if requested.
    let meshes = if settings.detail_levels {
        tree.lod_meshes(&default_lod_levels())
            .into_iter()
            .map(|(_, mesh)| mesh)
            .collect()
    } else {
        vec![tree.mesh().clone()]
    };
    let rig = settings
        .rig
        .then(|| tree.rig(&RigOptions::default()))
        .transpose()
        .map_err(ExportError::Rig)?;

    // Look up each image by its export path and infer PNG or JPEG from its file name.
    let size = settings.textures.unwrap_or(TextureSize::Size1k);
    let embed = |texture: TextureRef<'_>| {
        let path = texture.image_path(size);
        let format = if path.ends_with(".png") {
            ImageFormat::Png
        } else {
            ImageFormat::Jpeg
        };
        Some(EmbeddedImage {
            bytes: images.get(&path)?.clone(),
            format,
        })
    };
    let gltf = GltfOptions {
        rig: rig.as_ref(),
        textures: if settings.textures.is_some() {
            GltfTextures::Embed(&embed)
        } else {
            GltfTextures::None
        },
        ..GltfOptions::default()
    };
    procgena_tree::export::gltf::to_glb(&meshes, tree.options(), &gltf).map_err(ExportError::Gltf)
}

/// An OBJ model, its material description and any available texture images.
#[derive(Debug)]
pub struct ObjExport {
    /// File names relative to the export folder, paired with the contents to save.
    pub files: Vec<(String, Vec<u8>)>,
}

/// Creates a full-detail OBJ model and its companion files. The name `stem`
/// produces `stem.obj`, `stem.mtl` and a `stem_textures/` folder for available
/// images. OBJ exports do not include animation joints or extra detail levels.
pub fn obj(
    options: &TreeOptions,
    settings: &ModelSettings,
    images: &HashMap<String, Vec<u8>>,
    stem: &str,
) -> Result<ObjExport, ExportError> {
    let tree = Tree::new(options.clone()).map_err(ExportError::Tree)?;
    let size = settings.textures.unwrap_or(TextureSize::Size1k);
    let folder = format!("{stem}_textures");

    // Use relative image paths so the export folder can be moved as a whole.
    let texture_path = |texture: TextureRef<'_>| {
        let path = texture.image_path(size);
        images
            .contains_key(&path)
            .then(|| format!("{folder}/{}", file_name(&path)))
    };
    let texture_path: &dyn Fn(TextureRef<'_>) -> Option<String> = &texture_path;
    let material_library = format!("{stem}.mtl");
    let files = procgena_tree::export::obj::to_obj(
        tree.mesh(),
        tree.options(),
        &ObjOptions {
            material_library: &material_library,
            textures: settings.textures.is_some().then_some(texture_path),
            ..ObjOptions::default()
        },
    );

    // Collect the model and material files, then the available texture images.
    let mut out = vec![
        (format!("{stem}.obj"), files.obj.into_bytes()),
        (material_library, files.mtl.into_bytes()),
    ];
    for path in image_paths(options, settings) {
        if let Some(bytes) = images.get(&path) {
            out.push((format!("{folder}/{}", file_name(&path)), bytes.clone()));
        }
    }
    Ok(ObjExport { files: out })
}

/// Returns the image file name without its folders, for example `a-1k-color.jpg`.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use procgena_tree::TreePreset;

    use super::*;

    /// Records which image paths the GLB exporter requests, preserving request order.
    fn requested(options: &TreeOptions, size: TextureSize) -> Vec<String> {
        let asked = RefCell::new(Vec::new());
        let record = |texture: TextureRef<'_>| {
            asked.borrow_mut().push(texture.image_path(size));
            None
        };
        let tree = Tree::new(options.clone()).unwrap();
        let gltf = GltfOptions {
            textures: GltfTextures::Embed(&record),
            ..GltfOptions::default()
        };
        procgena_tree::export::gltf::to_glb(
            std::slice::from_ref(tree.mesh()),
            tree.options(),
            &gltf,
        )
        .unwrap();
        asked.into_inner()
    }

    #[test]
    fn image_paths_are_the_images_the_exporter_asks_for() {
        let settings = ModelSettings::default();
        for preset in TreePreset::ALL {
            let mut options = preset.options();
            let mut expected = requested(&options, TextureSize::Size1k);
            let mut paths = image_paths(&options, &settings);
            expected.sort();
            paths.sort();
            assert_eq!(paths, expected, "{}", preset.name());

            // Disabling bark textures leaves only the leaf image requests.
            options.bark.textured = false;
            assert_eq!(
                image_paths(&options, &settings),
                requested(&options, TextureSize::Size1k),
                "{}",
                preset.name()
            );
        }
    }

    #[test]
    fn colours_only_needs_no_images() {
        let settings = ModelSettings {
            textures: None,
            ..ModelSettings::default()
        };
        assert!(image_paths(&TreePreset::OakMedium.options(), &settings).is_empty());
    }

    /// Provides placeholder image contents for each path required by the preset.
    fn fake_images(options: &TreeOptions, settings: &ModelSettings) -> HashMap<String, Vec<u8>> {
        image_paths(options, settings)
            .into_iter()
            .map(|path| {
                let bytes = path.as_bytes().to_vec();
                (path, bytes)
            })
            .collect()
    }

    #[test]
    fn glb_embeds_the_images_and_follows_the_settings() {
        let options = TreePreset::OakSmall.options();
        let settings = ModelSettings::default();
        let images = fake_images(&options, &settings);
        let full = glb(&options, &settings, &images).unwrap();
        for bytes in images.values() {
            assert!(
                full.windows(bytes.len()).any(|w| w == bytes),
                "every image is embedded"
            );
        }

        // Exporting only the full-detail geometry and colours should produce a smaller file.
        let plain = ModelSettings {
            textures: None,
            detail_levels: false,
            rig: false,
        };
        let small = glb(&options, &plain, &images).unwrap();
        assert!(small.len() < full.len());
        let text = String::from_utf8_lossy(&small);
        assert!(!text.contains("\"skins\""));
        assert!(!text.contains("_LOD1"));
        assert!(!text.contains("\"images\""));
    }

    #[test]
    fn obj_names_its_files_after_the_stem() {
        let options = TreePreset::OakSmall.options();
        let settings = ModelSettings::default();
        let images = fake_images(&options, &settings);
        let export = obj(&options, &settings, &images, "oak").unwrap();
        let names: Vec<&str> = export.files.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names[..2], ["oak.obj", "oak.mtl"]);
        assert_eq!(names.len(), 2 + images.len());

        // Check that the material file points to the copied images within the export folder.
        let mtl = String::from_utf8_lossy(&export.files[1].1);
        assert!(String::from_utf8_lossy(&export.files[0].1).contains("mtllib oak.mtl"));
        for name in &names[2..] {
            assert!(name.starts_with("oak_textures/"));
            assert!(mtl.contains(name), "{name} in {mtl}");
        }

        // When no images are available, export only the model and material colours.
        let export = obj(&options, &settings, &HashMap::new(), "oak").unwrap();
        assert_eq!(export.files.len(), 2);
        assert!(!String::from_utf8_lossy(&export.files[1].1).contains("map_"));
    }
}
