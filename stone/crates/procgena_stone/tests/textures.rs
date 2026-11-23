//! Verify the texture manifests and the downloaded texture files. KTX2 stores
//! compressed texture images; these checks read file metadata to confirm
//! encoding, colour handling, image size, and smaller images for distant
//! rendering.
//!
//! Texture files are not in the repository. Download them with
//! `uv run --project tools textures download stone`, then run the ignored test with
//! `cargo test -p procgena_stone --test textures -- --ignored`.

use std::collections::HashSet;
use std::path::PathBuf;

use procgena_stone::{
    ImageMap, TEXTURE_SETS, TextureCompression, TextureMap, TextureSize, image_path, texture_path,
};
use serde_json::Value;

const KTX2_IDENTIFIER: [u8; 12] = *b"\xabKTX 20\xbb\r\n\x1a\n";
const VK_FORMAT_UNDEFINED: u32 = 0;
const VK_FORMAT_BC7_UNORM_BLOCK: u32 = 145;
const VK_FORMAT_BC7_SRGB_BLOCK: u32 = 146;
const KHR_DF_MODEL_UASTC: u8 = 166;
const KHR_DF_TRANSFER_LINEAR: u8 = 1;
const KHR_DF_TRANSFER_SRGB: u8 = 2;
const SUPERCOMPRESSION_ZSTD: u32 = 2;

/// Texture metadata used to check the image encoding, colour space, size,
/// compression, and number of image sizes (mipmap levels).
struct Ktx2Header {
    vk_format: u32,
    width: u32,
    height: u32,
    level_count: u32,
    supercompression: u32,
    color_model: u8,
    transfer: u8,
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

/// The repository's `assets/textures/` folder.
fn texture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures")
}

/// Reads metadata for a path relative to `assets/textures/`.
/// Panics if the file cannot be read, lacks the KTX2 signature, or is too short
/// for the fields read here. This is a test helper, not a full file validator.
fn ktx2_header(rel: &str) -> Ktx2Header {
    let path = texture_dir().join(rel);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    assert_eq!(bytes[..12], KTX2_IDENTIFIER, "{rel} is not KTX2");

    // KTX 2.0 section 3 places the data format descriptor offset at byte 48.
    let dfd = read_u32(&bytes, 48) as usize;
    // Skip the descriptor's 4-byte size and 8-byte block header to reach
    // the colour model. The colour transfer function is two bytes later.
    Ktx2Header {
        vk_format: read_u32(&bytes, 12),
        width: read_u32(&bytes, 20),
        height: read_u32(&bytes, 24),
        level_count: read_u32(&bytes, 40),
        supercompression: read_u32(&bytes, 44),
        color_model: bytes[dfd + 12],
        transfer: bytes[dfd + 14],
    }
}

/// Checks the file against the requested encoding and colour space, and returns
/// its width and height. Set `srgb` for image colours; use linear data for normal
/// and roughness maps. Also requires a full set of smaller images and Zstandard
/// compression.
fn check_texture(compression: TextureCompression, rel: &str, srgb: bool) -> (u32, u32) {
    let header = ktx2_header(rel);
    let transfer = if srgb {
        KHR_DF_TRANSFER_SRGB
    } else {
        KHR_DF_TRANSFER_LINEAR
    };
    assert_eq!(header.transfer, transfer, "{rel} colour space");
    match compression {
        TextureCompression::Uastc => {
            assert_eq!(header.vk_format, VK_FORMAT_UNDEFINED, "{rel}");
            assert_eq!(header.color_model, KHR_DF_MODEL_UASTC, "{rel}");
        }
        TextureCompression::Bc7 => {
            let format = if srgb {
                VK_FORMAT_BC7_SRGB_BLOCK
            } else {
                VK_FORMAT_BC7_UNORM_BLOCK
            };
            assert_eq!(header.vk_format, format, "{rel}");
        }
    }
    // Mipmaps halve the image dimensions at each level down to 1×1, so a full
    // set has one level per halving of the longer side, plus the source image.
    let longest = header.width.max(header.height);
    assert!(longest > 0, "{rel} has no size");
    assert_eq!(header.level_count, longest.ilog2() + 1, "{rel} mip levels");
    assert_eq!(header.supercompression, SUPERCOMPRESSION_ZSTD, "{rel}");
    (header.width, header.height)
}

/// Reads a manifest at a path relative to `assets/textures/`.
fn manifest(rel: &str) -> Value {
    let path = texture_dir().join(rel);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    serde_json::from_str(&text).expect("manifest is valid JSON")
}

/// The texture ids in the manifest, in order.
fn manifest_ids(manifest: &Value) -> Vec<&str> {
    manifest["assets"]
        .as_array()
        .expect("manifest assets")
        .iter()
        .map(|asset| str_field(asset, "id"))
        .collect()
}

/// Every file entry in the manifest.
fn manifest_files(manifest: &Value) -> Vec<&Value> {
    manifest["assets"]
        .as_array()
        .expect("manifest assets")
        .iter()
        .flat_map(|asset| asset["files"].as_array().expect("asset files"))
        .collect()
}

fn str_field<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} in {value}"))
}

fn size_from(name: &str) -> TextureSize {
    *TextureSize::ALL
        .iter()
        .find(|s| s.as_str() == name)
        .unwrap_or_else(|| panic!("unknown size {name}"))
}

fn map_from(name: &str) -> TextureMap {
    *TextureMap::ALL
        .iter()
        .find(|m| m.as_str() == name)
        .unwrap_or_else(|| panic!("unknown map {name}"))
}

fn image_map_from(name: &str) -> ImageMap {
    *ImageMap::ALL
        .iter()
        .find(|m| m.as_str() == name)
        .unwrap_or_else(|| panic!("unknown image map {name}"))
}

fn compression_from(name: &str) -> TextureCompression {
    *TextureCompression::ALL
        .iter()
        .find(|c| c.as_str() == name)
        .unwrap_or_else(|| panic!("unknown format {name}"))
}

/// Returns the set id that starts `path` after `prefix`, by matching the known
/// sets, since set ids may contain the `-` that separates the other parts.
fn set_of(path: &str, prefix: &str) -> &'static str {
    let rest = path
        .strip_prefix(prefix)
        .unwrap_or_else(|| panic!("unexpected path {path}"));
    TEXTURE_SETS
        .iter()
        .copied()
        .filter(|set| rest.starts_with(&format!("{set}-")))
        .max_by_key(|set| set.len())
        .unwrap_or_else(|| panic!("unknown set in {path}"))
}

#[test]
fn texture_sets_match_the_manifests() {
    assert_eq!(
        manifest_ids(&manifest("assets_manifest.json")),
        TEXTURE_SETS
    );
    assert_eq!(
        manifest_ids(&manifest("images/images_manifest.json")),
        TEXTURE_SETS
    );
}

#[test]
fn manifest_lists_every_texture_file_once() {
    let manifest = manifest("assets_manifest.json");
    let files = manifest_files(&manifest);

    // Each entry's path must be the one built from its own size, map and format.
    for file in &files {
        let path = str_field(file, "path");
        let built = texture_path(
            set_of(path, "rock/"),
            size_from(str_field(file, "size")),
            map_from(str_field(file, "map")),
            compression_from(str_field(file, "format")),
        );
        assert_eq!(path, built);
    }

    // Every combination appears exactly once.
    let paths: HashSet<&str> = files.iter().map(|f| str_field(f, "path")).collect();
    assert_eq!(paths.len(), files.len(), "duplicate manifest paths");
    let mut expected = 0;
    for set in TEXTURE_SETS {
        for size in TextureSize::ALL {
            for map in TextureMap::ALL {
                for compression in TextureCompression::ALL {
                    let path = texture_path(set, size, map, compression);
                    assert!(
                        paths.contains(path.as_str()),
                        "{path} missing from manifest"
                    );
                    expected += 1;
                }
            }
        }
    }
    assert_eq!(files.len(), expected);
}

#[test]
fn image_manifest_lists_every_image_once() {
    let manifest = manifest("images/images_manifest.json");
    let files = manifest_files(&manifest);

    // Each entry's path must match its own size and map.
    for file in &files {
        let path = str_field(file, "path");
        let built = image_path(
            set_of(path, "rock/"),
            size_from(str_field(file, "size")),
            image_map_from(str_field(file, "map")),
        );
        assert_eq!(path, built);
    }

    // Every combination appears exactly once.
    let paths: HashSet<&str> = files.iter().map(|f| str_field(f, "path")).collect();
    assert_eq!(paths.len(), files.len(), "duplicate manifest paths");
    assert_eq!(
        files.len(),
        TEXTURE_SETS.len() * TextureSize::ALL.len() * ImageMap::ALL.len()
    );
}

/// Checks the downloaded texture files against the manifest. The 512 and 1k
/// sizes that `uv run --project tools textures download stone` fetches by default must be present;
/// 2k files are checked when present.
#[test]
#[ignore = "needs textures from uv run --project tools textures download stone"]
fn downloaded_textures_have_the_documented_formats() {
    let manifest = manifest("assets_manifest.json");
    let mut checked = 0;
    for file in manifest_files(&manifest) {
        let path = str_field(file, "path");
        let size = size_from(str_field(file, "size"));
        if size == TextureSize::Size2k && !texture_dir().join(path).exists() {
            continue;
        }
        let map = map_from(str_field(file, "map"));
        let (width, height) = check_texture(
            compression_from(str_field(file, "format")),
            path,
            map.is_srgb(),
        );
        assert_eq!(
            (Some(width as u64), Some(height as u64)),
            (file["width"].as_u64(), file["height"].as_u64()),
            "{path} size"
        );
        checked += 1;
    }
    assert!(
        checked >= TEXTURE_SETS.len() * 2 * 3 * 2,
        "too few files checked"
    );
}
