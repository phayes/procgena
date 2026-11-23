//! Where the viewer finds its asset files.

#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use bevy::asset::io::AssetSourceBuilder;
use bevy::prelude::*;

use super::texture_reader::{TextureDownloads, TextureFiles, TextureReader};

/// Asset path prefix of procgena_tree's bark and leaf textures.
///
/// Pass it as `TreePlugin::texture_root`. It only resolves after
/// [`add_asset_sources`] has run.
pub const TREE_TEXTURE_ROOT: &str = "procgena_tree://textures";

/// Path prefix for loading the rock textures supplied with procgena_stone.
///
/// Use as `StonePlugin::texture_root` after registering the texture
/// locations with [`add_asset_sources`].
pub const STONE_TEXTURE_ROOT: &str = "procgena_stone://textures";

/// Path prefix for loading the snow, moss and leaf-litter textures supplied
/// with procgena_clump.
///
/// Use as `ClumpPlugin::texture_root` after registering the texture
/// locations with [`add_asset_sources`].
pub const CLUMP_TEXTURE_ROOT: &str = "procgena_clump://textures";

/// Path prefix for loading the mushroom cap textures supplied with
/// procgena_mushroom.
///
/// Use as `MushroomPlugin::texture_root` after registering the texture
/// locations with [`add_asset_sources`].
pub const MUSHROOM_TEXTURE_ROOT: &str = "procgena_mushroom://textures";

/// Path prefix for loading the bark and end-grain textures supplied with
/// procgena_deadwood.
///
/// Use as `DeadwoodPlugin::texture_root` after registering the texture
/// locations with [`add_asset_sources`].
pub const DEADWOOD_TEXTURE_ROOT: &str = "procgena_deadwood://textures";

/// A generator's asset folder, registered as an asset source.
struct GeneratorAssets {
    /// The asset source name, as in `procgena_tree://`.
    source: &'static str,
    /// The cache folder name for its downloaded textures.
    cache: &'static str,
    /// The workspace's asset folder, fixed at build time.
    folder: &'static str,
    /// The published texture list, `textures/assets_manifest.json`.
    manifest: &'static str,
}

/// Declares a generator's assets from its workspace folder name.
macro_rules! generator_assets {
    ($source:literal, $folder:literal) => {
        GeneratorAssets {
            source: $source,
            cache: $folder,
            folder: concat!(env!("CARGO_MANIFEST_DIR"), "/../", $folder, "/assets"),
            manifest: include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../",
                $folder,
                "/assets/textures/assets_manifest.json"
            )),
        }
    };
}

/// Every generator whose assets the studio loads.
const GENERATORS: [GeneratorAssets; 5] = [
    generator_assets!("procgena_tree", "tree"),
    generator_assets!("procgena_stone", "stone"),
    generator_assets!("procgena_clump", "clump"),
    generator_assets!("procgena_mushroom", "mushroom"),
    generator_assets!("procgena_deadwood", "deadwood"),
];

/// Makes tree, rock, clump, mushroom and dead wood textures available
/// through `procgena_tree://`, `procgena_stone://`, `procgena_clump://`,
/// `procgena_mushroom://` and `procgena_deadwood://`.
///
/// Files are read from the workspace's `tree/assets`, `stone/assets`,
/// `clump/assets`, `mushroom/assets` and `deadwood/assets` folders, at their build-time
/// locations. Published textures missing there are downloaded when first
/// loaded and kept in the user's cache folder (such as
/// `~/Library/Caches/procgena/textures/` on macOS), so the textures need no
/// separate download step and a copied executable still finds them. Without
/// a cache folder, downloads are kept only while the studio runs. Adds a
/// [`TextureDownloads`] resource counting the downloads in progress.
///
/// Call before adding `AssetPlugin` or the default plugins.
pub fn add_asset_sources(app: &mut App) {
    let cache = dirs::cache_dir().map(|dir| dir.join("procgena").join("textures"));
    add_asset_sources_from(app, |generator| PathBuf::from(generator.folder), cache);
}

/// Like [`add_asset_sources`], but reads each generator's assets from
/// `folder` and keeps downloads under `cache`, so tests can start without
/// any textures on disk.
#[cfg(test)]
pub fn add_asset_sources_in(app: &mut App, folder: &Path, cache: &Path) {
    add_asset_sources_from(
        app,
        |generator| folder.join(generator.cache),
        Some(cache.to_owned()),
    );
}

/// Registers each generator's asset source, reading files from
/// `folder(generator)` and keeping downloaded textures under `cache`.
fn add_asset_sources_from(
    app: &mut App,
    folder: impl Fn(&GeneratorAssets) -> PathBuf,
    cache: Option<PathBuf>,
) {
    let downloads = TextureDownloads::default();
    app.insert_resource(downloads.clone());
    for generator in &GENERATORS {
        let cache = cache.as_ref().map(|cache| cache.join(generator.cache));
        let manifest = generator.manifest;
        let files = match TextureFiles::from_manifest(manifest, cache, downloads.clone()) {
            Ok(files) => Arc::new(files),
            Err(error) => {
                error!(
                    "{}: {error}; its textures won't be downloaded",
                    generator.source
                );
                Arc::new(TextureFiles::none())
            }
        };
        let folder = folder(generator);
        let source = AssetSourceBuilder::platform_default(&folder.to_string_lossy(), None)
            .with_reader(move || Box::new(TextureReader::new(&folder, files.clone())));
        app.register_asset_source(generator.source, source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_generator_publishes_its_textures() {
        // One texture each generator's plugin loads in the studio.
        let samples = [
            ("procgena_tree", "textures/bark/"),
            ("procgena_stone", "textures/"),
            ("procgena_clump", "textures/snow/"),
            (
                "procgena_mushroom",
                "textures/cap/mushroom_top_001-1k-color-bc7.ktx2",
            ),
            (
                "procgena_deadwood",
                "textures/end/TreeEnd005-1k-color-bc7.ktx2",
            ),
        ];
        for (generator, (source, sample)) in GENERATORS.iter().zip(samples) {
            assert_eq!(generator.source, source);
            let downloads = TextureDownloads::default();
            let files = TextureFiles::from_manifest(generator.manifest, None, downloads).unwrap();
            assert!(
                files.publishes_under(sample),
                "{source} publishes nothing at {sample}"
            );
        }
    }
}
