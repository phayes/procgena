//! Bevy plugin for [`procgena_deadwood`]: draws fallen logs, branches,
//! stumps and scattered sticks, switching to simpler meshes as the camera
//! moves away.
//!
//! Add [`DeadwoodPlugin`], then spawn entities with one of these
//! components:
//!
//! - [`DeadwoodParams`]: one log, branch or stump;
//! - [`DeadwoodScatterParams`]: sticks and twigs spread over flat ground or
//!   over the entity's parent, which must have a
//!   [`procgena_specimen::GeneratedSpecimen`].
//!
//! Pieces are built on background threads, so they appear a moment after
//! spawning. A single piece then gets a [`procgena_specimen::GeneratedSpecimen`],
//! so moss or snow from `procgena_clump_bevy` can grow on it as a child.
//!
//! ```no_run
//! use bevy::prelude::*;
//! use procgena_deadwood::DeadwoodPreset;
//! use procgena_deadwood_bevy::{DeadwoodParams, DeadwoodPlugin};
//!
//! App::new()
//!     .add_plugins((DefaultPlugins, DeadwoodPlugin::default()))
//!     .add_systems(Startup, |mut commands: Commands| {
//!         commands.spawn(DeadwoodParams::preset(DeadwoodPreset::MossyLog));
//!     })
//!     .run();
//! ```
//!
//! Each piece is drawn by [`DeadwoodPart`] child entities, one or two per
//! detail level. Bevy's `VisibilityRange` shows each level only within its
//! distance band, blending between neighbouring levels. The bands come from
//! [`DeadwoodPlugin::lod`], scaled by each piece's size, and the farthest
//! level fades out at the cull distance. Thin twigs and branches become
//! flat strips that [`DeadwoodMaterial`] keeps turned toward the camera.
//!
//! To build your own entities, convert meshes with [`deadwood_mesh`] and
//! [`ribbon_mesh`] and create materials with [`deadwood_material`]. Keep
//! the plugin installed, because it provides the shaders.

mod material;
mod mesh;
mod spawn;

use bevy::asset::embedded_asset;
use bevy::pbr::MaterialPlugin;
use bevy::prelude::*;
use bevy::shader::load_shader_library;
use procgena_core_bevy::{LodPlugin, LodQuality};
use procgena_deadwood::{DeadwoodLod, TextureCompression, TextureSize, default_lod, validate_lod};

pub use material::{
    BarkTextures, DeadwoodExtension, DeadwoodMaterial, DeadwoodSurface, EndTextures, LeafTextures,
    deadwood_material, leaf_material,
};
pub use mesh::{
    ATTRIBUTE_BILLBOARD_AXIS, ATTRIBUTE_BILLBOARD_PIVOT, deadwood_mesh, ribbon_aabb, ribbon_mesh,
};
pub use spawn::{
    DeadwoodBuildError, DeadwoodFinished, DeadwoodGenerating, DeadwoodOptionsAsset, DeadwoodParams,
    DeadwoodPart, DeadwoodPartKind, DeadwoodPlacement, DeadwoodScatterParams, ScatterGround,
};

use spawn::{DeadwoodCache, DeadwoodOptionsLoader, SpawnSettings, TextureSettings};

/// Adds dead wood building and drawing, and loading of `.deadwood.json`
/// files, to a Bevy app. It also adds the shared [`LodPlugin`], whose
/// [`LodQuality`] resource scales every piece's switch distances.
///
/// # Panics
///
/// Adding the plugin panics if [`lod`](Self::lod) is invalid; see
/// [`validate_lod`].
#[derive(Debug, Clone)]
pub struct DeadwoodPlugin {
    /// Detail levels for a piece [`reference_size`](DeadwoodLod) across,
    /// 45 cm by default. Each piece multiplies the distances by its size
    /// relative to that (see [`procgena_deadwood::lod_size`]) and by the
    /// [`LodQuality`] in use when it is spawned. Every piece uses these
    /// levels; [`DeadwoodOptions::detail`] is not used by the plugin.
    ///
    /// [`DeadwoodOptions::detail`]: procgena_deadwood::DeadwoodOptions::detail
    pub lod: DeadwoodLod,
    /// Folder holding the `bark/` and `end/` texture folders, relative to
    /// Bevy's asset folder, for pieces whose options name a
    /// [`bark_texture`](procgena_deadwood::MaterialOptions::bark_texture) or
    /// [`end_texture`](procgena_deadwood::MaterialOptions::end_texture).
    /// Defaults to `procgena_deadwood/textures`: copy the repository's
    /// `deadwood/assets/textures` folder there after
    /// `uv run --project tools textures download deadwood`.
    ///
    /// A textured piece is not drawn until its textures load, and not at
    /// all if they fail to, so use `None` when the files aren't there:
    /// every piece then uses its vertex colours.
    pub texture_root: Option<String>,
    /// Size of the texture images to load. Defaults to 1k.
    pub texture_size: TextureSize,
    /// Which texture files to load. Defaults to BC7; UASTC needs Bevy's
    /// `basis-universal` feature.
    pub texture_compression: TextureCompression,
}

impl Default for DeadwoodPlugin {
    fn default() -> Self {
        Self {
            lod: default_lod(),
            texture_root: Some("procgena_deadwood/textures".to_owned()),
            texture_size: TextureSize::Size1k,
            texture_compression: TextureCompression::Bc7,
        }
    }
}

impl Plugin for DeadwoodPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(LodPlugin);
        load_shader_library!(app, "shaders/deadwood_billboard.wesl");
        embedded_asset!(app, "shaders/deadwood_vertex.wesl");
        embedded_asset!(app, "shaders/deadwood_prepass.wesl");
        embedded_asset!(app, "shaders/deadwood_fragment.wesl");

        if let Err(error) = validate_lod(&self.lod) {
            panic!("DeadwoodPlugin::lod is invalid: {error}");
        }
        app.add_plugins(MaterialPlugin::<DeadwoodMaterial>::default());
        add_systems(app, self.lod.clone());
        app.world_mut().resource_mut::<SpawnSettings>().textures =
            self.texture_root.clone().map(|root| TextureSettings {
                root,
                size: self.texture_size,
                compression: self.texture_compression,
            });
    }
}

/// Registers the options file loader, the shared cache and the build
/// systems, using detail levels `lod`, but nothing for drawing. The plugin
/// and the tests (which run without a renderer) both use it.
pub(crate) fn add_systems(app: &mut App, lod: DeadwoodLod) {
    app.init_asset::<DeadwoodOptionsAsset>()
        .register_asset_loader(DeadwoodOptionsLoader)
        .init_resource::<DeadwoodCache>()
        .init_resource::<LodQuality>()
        .insert_resource(SpawnSettings {
            lod,
            textures: None,
        })
        .add_systems(
            Update,
            (
                spawn::remove_deadwood,
                spawn::start_generation,
                spawn::finish_generation,
                spawn::prune_cache,
            )
                .chain(),
        );
}
