//! Bevy plugin for [`procgena_mushroom`]: draws mushrooms, clusters and
//! scattered mushrooms, switching to a cheaper mesh and then to an impostor
//! as they get smaller on screen.
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
//!         commands.spawn(MushroomParams::preset(MushroomPreset::fly_agaric()));
//!     })
//!     .run();
//! ```
//!
//! ## Load a preset file
//!
//! Save a mushroom preset to `assets/mushrooms/pale_porcini.mushroom.yml`.
//! It names a built-in preset as its `base` and gives only the settings
//! that differ; without a `base`, missing settings use their defaults.
//! Colours start with `#`, so quote them:
//!
//! ```yaml
//! base: porcini
//! mushroom:
//!   default_seed: 42
//!   material:
//!     cap_centre: "#c09060"
//! ```
//!
//! Then load it through Bevy's asset server:
//!
//! ```no_run
//! use bevy::prelude::*;
//! use procgena_mushroom_bevy::MushroomParams;
//!
//! fn load_mushroom(mut commands: Commands, asset_server: Res<AssetServer>) {
//!     commands.spawn(MushroomParams::Asset(
//!         asset_server.load("mushrooms/pale_porcini.mushroom.yml"),
//!     ));
//! }
//! ```
//!
//! The file name must end in `.mushroom.yml`. The mushroom waits for the
//! file to load, and rebuilds when it changes if Bevy is watching asset
//! files.
//!
//! ## What is drawn when
//!
//! Every mushroom is drawn along the shared drawing chain of
//! [`procgena_core::chain`]: its game-level mesh
//! ([`MushroomOptions::game_level`]) up close, a cheaper lower mesh further
//! out, and an impostor when it is small on screen, until it is culled.
//! Which one shows depends on how large the mushroom looks on screen, the
//! same for every kind of object, and on the [`LodQuality`] setting when the
//! mushroom is built. A single mushroom or cluster switches as a whole, by
//! the size of the whole cluster.
//!
//! A single mushroom or cluster is drawn by [`MushroomPart`] child entities:
//! one per member and mesh, and one impostor of the whole mushroom or
//! cluster. Bevy's `VisibilityRange` blends between neighbouring steps. A
//! scatter's mushrooms are drawn instead by the shared instanced renderer of
//! `procgena_core_bevy`, which fades them the same way, with no entity per
//! mushroom, and gives each different body its own impostor. Impostors are
//! baked in the background the first time a mushroom is built (see
//! [`default_mushroom_impostor`]); until then, and with
//! [`MushroomPlugin::impostor`] set to `None`, the lower mesh is drawn until
//! the mushroom is culled. Impostors cast no shadows.
//!
//! The material draws each mushroom's warts, gill lines and cap pattern, so
//! they cost no triangles.
//!
//! To build your own entities, convert meshes with [`mushroom_mesh`] and
//! create materials with [`mushroom_material`]. Keep the plugin installed,
//! because it provides the shaders.
//!
//! [`MushroomOptions::game_level`]: procgena_mushroom::MushroomOptions::game_level

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
use procgena_impostor::{BakeSettings, ImpostorMaterial, ImpostorPipelineKey, ImpostorPlugin};
use procgena_mushroom::{TextureCompression, TextureSize};

pub use impostor::default_mushroom_impostor;
pub use material::{
    MushroomExtension, MushroomKey, MushroomMaterial, MushroomSurface, TextureSet, bake_copy,
    mushroom_material,
};

use material::MushroomMaterialKey;
pub use mesh::mushroom_mesh;
pub use spawn::{
    MushroomBuildError, MushroomFinished, MushroomGenerating, MushroomGroundScatterParams,
    MushroomParams, MushroomPart, MushroomPresetAsset, MushroomScatterParams,
};

use spawn::{MushroomCache, MushroomPresetLoader, SpawnSettings, TextureSettings};

/// The key Bevy specializes impostor materials by, which the shared
/// instanced renderer marks for drawn copies.
type ImpostorMaterialKey = CombinedBindGroupData<StandardMaterialKey, ImpostorPipelineKey>;

/// Adds mushroom building and drawing, and loading of `.mushroom.yml`
/// preset files, to a Bevy app. It also adds [`LodPlugin`], which holds the
/// [`LodQuality`] setting and the shared dither shader, and, while
/// [`impostor`](Self::impostor) is set, [`ImpostorPlugin`] unless the app
/// already has it.
#[derive(Debug, Clone)]
pub struct MushroomPlugin {
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
    /// How mushrooms' impostors are baked; mushrooms are drawn as their
    /// impostor when small on screen, after their game-level and lower
    /// meshes. Defaults to [`default_mushroom_impostor`]. `None` draws each
    /// mushroom's lower mesh until it is culled.
    pub impostor: Option<BakeSettings>,
}

impl Default for MushroomPlugin {
    fn default() -> Self {
        Self {
            texture_root: Some("procgena_mushroom/textures".to_owned()),
            texture_size: TextureSize::Size1k,
            texture_compression: TextureCompression::Bc7,
            impostor: Some(default_mushroom_impostor()),
        }
    }
}

impl Plugin for MushroomPlugin {
    fn build(&self, app: &mut App) {
        load_shader_library!(app, "shaders/mushroom_triplanar.wesl");
        embedded_asset!(app, "shaders/mushroom_vertex.wesl");
        embedded_asset!(app, "shaders/mushroom_prepass.wesl");
        embedded_asset!(app, "shaders/mushroom_fragment.wesl");

        // Impostors, unless the app added the plugin itself with settings of
        // its own.
        if self.impostor.is_some() && !app.is_plugin_added::<ImpostorPlugin>() {
            app.add_plugins(ImpostorPlugin::default());
        }

        app.add_plugins((
            LodPlugin,
            MaterialPlugin::<MushroomMaterial>::default(),
            InstancedMaterialPlugin::<MushroomMaterial, MushroomMaterialKey>::new(|key| {
                key.extension.instanced = true;
            }),
        ));
        if self.impostor.is_some() {
            // Scattered mushrooms draw their baked impostors as their last
            // level.
            app.add_plugins(InstancedMaterialPlugin::<
                ImpostorMaterial,
                ImpostorMaterialKey,
            >::new(|key| {
                key.extension.instanced = true;
            }));
        }
        add_systems(app, self.impostor);
        app.world_mut().resource_mut::<SpawnSettings>().textures =
            self.texture_root.clone().map(|root| TextureSettings {
                root,
                size: self.texture_size,
                compression: self.texture_compression,
            });
    }
}

/// Registers the preset file loader, the shared cache and the build
/// systems, baking impostors with `impostor` when set, but nothing for
/// drawing. The plugin and the tests (which run without a renderer) both
/// use it. Impostors wait until the impostor plugin's resources exist.
pub(crate) fn add_systems(app: &mut App, impostor: Option<BakeSettings>) {
    app.init_asset::<MushroomPresetAsset>()
        .register_asset_loader(MushroomPresetLoader)
        .init_resource::<MushroomCache>()
        .init_resource::<SpecimenCache>()
        .init_resource::<LodQuality>()
        .insert_resource(SpawnSettings {
            impostor,
            textures: None,
        })
        .add_systems(
            Update,
            (
                spawn::remove_mushrooms,
                spawn::start_generation,
                spawn::finish_generation,
                spawn::prune_cache,
                impostor::show_single_impostors,
                impostor::show_scatter_impostors,
            )
                .chain(),
        );
}
