//! Bevy plugin for [`procgena_mushroom`]: draws mushrooms, clusters and
//! scattered mushrooms, switching to simpler meshes as the camera moves
//! away.
//!
//! Add [`MushroomPlugin`], then spawn entities with one of these
//! components:
//!
//! - [`MushroomParams`]: one mushroom, or a cluster of one species when its
//!   options ask for more than one stem;
//! - [`MushroomScatterParams`]: mushrooms spread over the entity's parent,
//!   which must have a [`procgena_specimen::GeneratedSpecimen`]. Rocks from
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
//! [`MushroomPlugin::lod`], scaled by each mushroom's size and by the
//! [`LodQuality`] setting when the mushroom
//! is built, and the farthest level fades out at the cull distance. At the
//! two farthest default levels, the stem, and then the cap too, are flat
//! shapes that [`MushroomMaterial`] keeps turned toward the camera; levels
//! that don't cast shadows, such as the default farthest one, get
//! `NotShadowCaster`.
//!
//! The material draws each mushroom's warts, gill lines and cap pattern, so
//! they cost no triangles.
//!
//! To build your own entities, convert meshes with [`mushroom_mesh`] and
//! [`billboard_mesh`], and create materials with [`mushroom_material`] and
//! [`billboard_material`]. Keep the plugin installed, because it provides
//! the shaders.

mod material;
mod mesh;
mod spawn;

use bevy::asset::embedded_asset;
use bevy::pbr::MaterialPlugin;
use bevy::prelude::*;
use bevy::shader::load_shader_library;
use procgena_core_bevy::{LodPlugin, LodQuality};
use procgena_mushroom::{MushroomLod, TextureCompression, TextureSize, default_lod, validate_lod};

pub use material::{
    MushroomExtension, MushroomMaterial, MushroomSurface, TextureSet, billboard_material,
    mushroom_material,
};
pub use mesh::{
    ATTRIBUTE_BILLBOARD_AXIS, ATTRIBUTE_BILLBOARD_PIVOT, billboard_aabb, billboard_mesh,
    mushroom_mesh,
};
pub use spawn::{
    MushroomBuildError, MushroomFinished, MushroomGenerating, MushroomOptionsAsset, MushroomParams,
    MushroomPart, MushroomScatterParams,
};

use spawn::{MushroomCache, MushroomOptionsLoader, SpawnSettings, TextureSettings};

/// Adds mushroom building and drawing, and loading of `.mushroom.json`
/// files, to a Bevy app. It also adds [`LodPlugin`], which holds the
/// [`LodQuality`] setting and the shared dither shader.
///
/// # Panics
///
/// Adding the plugin panics if [`lod`](Self::lod) is invalid; see
/// [`validate_lod`].
#[derive(Debug, Clone)]
pub struct MushroomPlugin {
    /// Detail levels, written for a cap of their
    /// [`reference_size`](procgena_core::Lod::reference_size). Each mushroom
    /// scales the distances by its size relative to that, by its size within
    /// a cluster or scatter, and by the [`LodQuality`] when it is built.
    /// Every mushroom uses these levels; [`MushroomOptions::detail`] is not
    /// used by the plugin. Defaults to [`default_lod`].
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
            lod: default_lod(),
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

        if let Err(error) = validate_lod(&self.lod) {
            panic!("MushroomPlugin::lod is invalid: {error}");
        }
        app.add_plugins((LodPlugin, MaterialPlugin::<MushroomMaterial>::default()));
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
        .init_resource::<LodQuality>()
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
