//! Bevy plugin for [`procgena_mushroom`]: draws mushrooms, clusters and
//! scattered mushrooms, switching to simpler meshes as the camera moves
//! away.
//!
//! Add [`MushroomPlugin`], then spawn entities with one of these
//! components:
//!
//! - [`MushroomParams`]: one mushroom;
//! - [`MushroomClusterParams`]: a cluster of one species;
//! - [`MushroomScatterParams`]: mushrooms spread over the entity's parent,
//!   which must have a [`procgena_mesh::GeneratedObject`]. Rocks from
//!   `procgena_stone_bevy` and trees from `procgena_tree_bevy` get one
//!   automatically.
//!
//! Mushrooms are built on background threads, so they appear a moment
//! after spawning.
//!
//! ```no_run
//! use bevy::prelude::*;
//! use procgena_mushroom::MushroomPreset;
//! use procgena_mushroom_bevy::{MushroomParams, MushroomPlugin};
//!
//! App::new()
//!     .add_plugins((DefaultPlugins, MushroomPlugin::default()))
//!     .add_systems(Startup, |mut commands: Commands| {
//!         commands.spawn(MushroomParams::preset(MushroomPreset::FlyAgaric));
//!     })
//!     .run();
//! ```
//!
//! Each mushroom is drawn by [`MushroomPart`] child entities, one or two per
//! detail level. Bevy's `VisibilityRange` shows each level only within its
//! distance band, blending between neighbouring levels. The bands come from
//! [`MushroomPlugin::lod`], scaled by each mushroom's size, and the farthest
//! level fades out at the cull distance. At that farthest default level the
//! stem is a flat strip that [`MushroomMaterial`] keeps turned toward the
//! camera.
//!
//! To build your own entities, convert meshes with [`mushroom_mesh`] and
//! [`ribbon_mesh`] and create materials with [`mushroom_material`]. Keep
//! the plugin installed, because it provides the shaders.

mod material;
mod mesh;
mod spawn;

use bevy::asset::embedded_asset;
use bevy::pbr::MaterialPlugin;
use bevy::prelude::*;
use bevy::shader::load_shader_library;
use procgena_mushroom::{MushroomLod, TextureCompression, TextureSize};

pub use material::{
    MushroomExtension, MushroomMaterial, MushroomSurface, TextureSet, mushroom_material,
};
pub use mesh::{
    ATTRIBUTE_BILLBOARD_AXIS, ATTRIBUTE_BILLBOARD_PIVOT, mushroom_mesh, ribbon_aabb, ribbon_mesh,
};
pub use spawn::{
    MushroomBuildError, MushroomClusterParams, MushroomFinished, MushroomGenerating,
    MushroomOptionsAsset, MushroomParams, MushroomPart, MushroomScatterParams,
};

use spawn::{MushroomCache, MushroomOptionsLoader, SpawnSettings, TextureSettings};

/// Adds mushroom building and drawing, and loading of `.mushroom.json`
/// files, to a Bevy app.
///
/// # Panics
///
/// Adding the plugin panics if [`lod`](Self::lod) is invalid; see
/// [`MushroomLod::validate`].
#[derive(Debug, Clone)]
pub struct MushroomPlugin {
    /// Detail levels for a mushroom with an 8 cm cap
    /// ([`DEFAULT_LOD_SIZE`](procgena_mushroom::DEFAULT_LOD_SIZE)). Each
    /// mushroom multiplies the distances by its size relative to that, and
    /// by its size within a cluster or scatter. Every mushroom uses these
    /// levels; [`MushroomOptions::detail`] is not used by the plugin.
    ///
    /// [`MushroomOptions::detail`]: procgena_mushroom::MushroomOptions::detail
    pub lod: MushroomLod,
    /// Folder holding the `cap/` and `stem/` texture folders, relative to
    /// Bevy's asset folder, for mushrooms whose options name a
    /// [`cap_texture`](procgena_mushroom::MaterialOptions::cap_texture) or
    /// [`stem_texture`](procgena_mushroom::MaterialOptions::stem_texture),
    /// such as the fly agaric and porcini presets. Defaults to
    /// `procgena_mushroom/textures`: copy the repository's
    /// `mushroom/assets/textures` folder there after
    /// `uv run --project tools textures download mushroom`.
    ///
    /// A textured mushroom is not drawn until its textures load, so use
    /// `None` when the files aren't there: every mushroom then uses its
    /// vertex colours.
    pub texture_root: Option<String>,
    /// Size of the texture images to load. Defaults to 1k.
    pub texture_size: TextureSize,
    /// Which texture files to load. Defaults to BC7; UASTC needs Bevy's
    /// `basis-universal` feature.
    pub texture_compression: TextureCompression,
}

impl Default for MushroomPlugin {
    fn default() -> Self {
        Self {
            lod: MushroomLod::default(),
            texture_root: Some("procgena_mushroom/textures".to_owned()),
            texture_size: TextureSize::Size1k,
            texture_compression: TextureCompression::Bc7,
        }
    }
}

impl Plugin for MushroomPlugin {
    fn build(&self, app: &mut App) {
        load_shader_library!(app, "shaders/mushroom_billboard.wesl");
        load_shader_library!(app, "shaders/mushroom_triplanar.wesl");
        embedded_asset!(app, "shaders/mushroom_vertex.wesl");
        embedded_asset!(app, "shaders/mushroom_prepass.wesl");
        embedded_asset!(app, "shaders/mushroom_fragment.wesl");

        if let Err(error) = self.lod.validate() {
            panic!("MushroomPlugin::lod is invalid: {error}");
        }
        app.add_plugins(MaterialPlugin::<MushroomMaterial>::default());
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
pub(crate) fn add_systems(app: &mut App, lod: MushroomLod) {
    app.init_asset::<MushroomOptionsAsset>()
        .register_asset_loader(MushroomOptionsLoader)
        .init_resource::<MushroomCache>()
        .insert_resource(SpawnSettings {
            lod,
            textures: None,
        })
        .add_systems(
            Update,
            (
                spawn::remove_mushrooms,
                spawn::start_generation,
                spawn::finish_generation,
                spawn::prune_cache,
            )
                .chain(),
        );
}
