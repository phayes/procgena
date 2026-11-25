//! Bevy plugin for [`procgena_deadwood`]: draws fallen logs, branches,
//! stumps and scattered sticks, switching to a simpler mesh and then an
//! impostor as they get small on screen.
//!
//! Add [`DeadwoodPlugin`], then spawn entities with one of these
//! components:
//!
//! - [`DeadwoodParams`]: one log, branch or stump;
//! - [`DeadwoodScatterParams`]: sticks and twigs spread over flat ground or
//!   over the entity's parent, which must have a
//!   [`procgena_specimen::GeneratedSpecimen`];
//! - [`DeadwoodPlacedParams`]: dead wood already placed by a
//!   [`StickScatter`](procgena_deadwood::StickScatter), such as an Ecotope's
//!   logs.
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
//!         commands.spawn(DeadwoodParams::preset(DeadwoodPreset::mossy_log()));
//!     })
//!     .run();
//! ```
//!
//! A piece can also come from a `.deadwood.yml` preset file in Bevy's
//! asset folder, such as `assets/wood/short_log.deadwood.yml`. The file
//! names a built-in preset as its `base` and gives only the settings that
//! differ; without a `base`, missing settings use their defaults:
//!
//! ```yaml
//! base: mossy_log
//! deadwood:
//!   default_seed: 42
//!   length: 2.0
//! ```
//!
//! Load it with [`DeadwoodParams::Asset`]:
//!
//! ```no_run
//! use bevy::prelude::*;
//! use procgena_deadwood_bevy::DeadwoodParams;
//!
//! fn load_log(mut commands: Commands, asset_server: Res<AssetServer>) {
//!     commands.spawn(DeadwoodParams::Asset(
//!         asset_server.load("wood/short_log.deadwood.yml"),
//!     ));
//! }
//! ```
//!
//! Every piece is drawn through the shared drawing chain
//! ([`DistanceChain`](procgena_core::chain::DistanceChain)): its game-level
//! mesh ([`DeadwoodOptions::game_level`](procgena_deadwood::DeadwoodOptions::game_level))
//! near the camera, a coarser mesh once it is small on screen, then its
//! impostor, a baked picture drawn as one quad (see
//! [`DeadwoodPlugin::impostor`]). Each step's distance band follows the
//! piece's size on screen, its bounding-sphere diameter, and the
//! [`LodQuality`] in use when it is built. A single piece is drawn by
//! [`DeadwoodPart`] child entities, whose `VisibilityRange` blends
//! neighbouring steps. A scatter's sticks are drawn instead by the shared
//! instanced renderer of `procgena_core_bevy`, which fades them the same
//! way, with no entity per stick. Thin twigs and branches become flat
//! strips that [`DeadwoodMaterial`] keeps turned toward the camera.
//!
//! To build your own entities, convert meshes with [`deadwood_mesh`] and
//! [`ribbon_mesh`] and create materials with [`deadwood_material`]. Keep
//! the plugin installed, because it provides the shaders.

mod impostor;
mod material;
mod mesh;
mod spawn;

use bevy::asset::embedded_asset;
use bevy::pbr::{MaterialPlugin, StandardMaterialKey};
use bevy::prelude::*;
use bevy::render::combined_bind_group::CombinedBindGroupData;
use bevy::shader::load_shader_library;
use procgena_core_bevy::instanced::{InstancedMaterialPlugin, SpecimenCache};
use procgena_core_bevy::{LodPlugin, LodQuality};
use procgena_deadwood::{TextureCompression, TextureSize};
use procgena_impostor::{BakeSettings, ImpostorMaterial, ImpostorPipelineKey, ImpostorPlugin};

/// The pipeline key of [`ImpostorMaterial`].
type ImpostorMaterialKey = CombinedBindGroupData<StandardMaterialKey, ImpostorPipelineKey>;

pub use impostor::default_deadwood_impostor;
pub use material::{
    BarkTextures, DeadwoodExtension, DeadwoodKey, DeadwoodMaterial, DeadwoodSurface, EndTextures,
    LeafTextures, bake_copy, deadwood_material, leaf_material,
};

use material::DeadwoodMaterialKey;
pub use mesh::{
    ATTRIBUTE_BILLBOARD_AXIS, ATTRIBUTE_BILLBOARD_PIVOT, deadwood_mesh, ribbon_aabb, ribbon_mesh,
};
pub use spawn::{
    DeadwoodBuildError, DeadwoodFinished, DeadwoodGenerating, DeadwoodParams, DeadwoodPart,
    DeadwoodPartKind, DeadwoodPlacedParams, DeadwoodPose, DeadwoodPresetAsset,
    DeadwoodScatterParams, ScatterGround,
};

use spawn::{DeadwoodCache, DeadwoodPresetLoader, SpawnSettings, TextureSettings};

/// Adds dead wood building and drawing, and loading of `.deadwood.yml`
/// preset files, to a Bevy app. It also adds the shared [`LodPlugin`], whose
/// [`LodQuality`] resource scales every piece's switch distances, and
/// [`ImpostorPlugin`] when impostors are on and the app has not added it.
#[derive(Debug, Clone)]
pub struct DeadwoodPlugin {
    /// How pieces' impostors are baked; pieces are drawn as their impostor
    /// when small on screen. Defaults to [`default_deadwood_impostor`].
    /// `None` draws each piece's lower mesh until it is culled instead.
    pub impostor: Option<BakeSettings>,
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
            impostor: Some(default_deadwood_impostor()),
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

        app.add_plugins((
            MaterialPlugin::<DeadwoodMaterial>::default(),
            InstancedMaterialPlugin::<DeadwoodMaterial, DeadwoodMaterialKey>::new(|key| {
                key.extension.instanced = true;
            }),
        ));
        // Impostors, unless the app added the plugin itself with settings of
        // its own.
        if self.impostor.is_some() {
            if !app.is_plugin_added::<ImpostorPlugin>() {
                app.add_plugins(ImpostorPlugin::default());
            }
            // A scatter's sticks draw their impostors through the shared
            // instanced renderer.
            app.add_plugins(InstancedMaterialPlugin::<
                ImpostorMaterial,
                ImpostorMaterialKey,
            >::new(|key| {
                key.extension.instanced = true;
            }));
        }
        add_systems(app);
        let textures = self.texture_root.clone().map(|root| TextureSettings {
            root,
            size: self.texture_size,
            compression: self.texture_compression,
        });
        app.insert_resource(SpawnSettings {
            textures,
            impostor: self.impostor,
        });
        if self.impostor.is_some() {
            app.add_systems(
                Update,
                (
                    impostor::request_impostors,
                    impostor::show_impostors,
                    impostor::show_scatter_impostors,
                )
                    .chain()
                    .after(spawn::finish_generation)
                    .before(spawn::prune_cache),
            );
        }
    }
}

/// Registers the preset file loader, the shared cache and the build
/// systems, without textures or impostors, and nothing for drawing. The
/// plugin and the tests (which run without a renderer) both use it.
pub(crate) fn add_systems(app: &mut App) {
    app.init_asset::<DeadwoodPresetAsset>()
        .register_asset_loader(DeadwoodPresetLoader)
        .init_resource::<DeadwoodCache>()
        .init_resource::<SpecimenCache>()
        .init_resource::<LodQuality>()
        .init_resource::<SpawnSettings>()
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
