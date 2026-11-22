//! Where the viewer finds its asset files.

use bevy::asset::io::AssetSourceBuilder;
use bevy::prelude::*;

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

/// The workspace's tree asset folder, fixed at build time.
const TREE_ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../tree/assets");
/// The workspace's stone asset folder, fixed at build time.
const STONE_ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../stone/assets");
/// The workspace's clump asset folder, fixed at build time.
const CLUMP_ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../clump/assets");

/// Makes tree, rock and clump textures available through `procgena_tree://`,
/// `procgena_stone://` and `procgena_clump://`.
///
/// Call before adding `AssetPlugin` or the default plugins. The workspace's
/// `tree/assets`, `stone/assets` and `clump/assets` folders must remain at
/// their build-time locations; copying the executable alone does not
/// include them.
pub fn add_asset_sources(app: &mut App) {
    app.register_asset_source(
        "procgena_tree",
        AssetSourceBuilder::platform_default(TREE_ASSETS, None),
    )
    .register_asset_source(
        "procgena_stone",
        AssetSourceBuilder::platform_default(STONE_ASSETS, None),
    )
    .register_asset_source(
        "procgena_clump",
        AssetSourceBuilder::platform_default(CLUMP_ASSETS, None),
    );
}
