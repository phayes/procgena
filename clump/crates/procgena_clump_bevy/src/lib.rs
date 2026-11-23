//! # procgena_clump_bevy
//!
//! Grow procedural clumps of snow, moss and other materials on rocks, trees
//! and your own meshes in Bevy, with background generation and projected
//! textures. Built on `procgena_clump`, this crate targets Bevy 0.20.1.
//!
//! ## Grow snow on a rock
//!
//! Add `ClumpPlugin` next to the plugin that makes the host, then spawn a
//! `ClumpParams` as a child of the host. This example uses a rock from
//! procgena_stone_bevy and runs without texture files:
//!
//! ```no_run
//! use bevy::prelude::*;
//! use procgena_clump::ClumpPreset;
//! use procgena_clump_bevy::{ClumpParams, ClumpPlugin};
//! use procgena_stone::StoneOptions;
//! use procgena_stone_bevy::{StoneParams, StonePlugin};
//!
//! fn main() {
//!     App::new()
//!         .add_plugins((
//!             DefaultPlugins,
//!             StonePlugin { texture_root: None, ..default() },
//!             ClumpPlugin { texture_root: None, ..default() },
//!         ))
//!         .add_systems(Startup, setup)
//!         .run();
//! }
//!
//! fn setup(mut commands: Commands) {
//!     let mut rock = StoneOptions::default();
//!     rock.material.color = [0.35, 0.33, 0.3];
//!     commands
//!         .spawn(StoneParams::new(rock))
//!         .with_child(ClumpParams::preset(ClumpPreset::FreshSnowCap));
//!     commands.spawn((
//!         Camera3d::default(),
//!         Transform::from_xyz(3.0, 2.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
//!     ));
//!     commands.spawn((
//!         DirectionalLight { illuminance: 12_000.0, shadow_maps_enabled: true, ..default() },
//!         Transform::from_xyz(3.0, 5.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
//!     ));
//! }
//! ```
//!
//! The snow appears once the rock has been generated and the snow has been
//! grown in the background. Several clumps can share a host: spawn one
//! `ClumpParams` child per layer, such as moss and snow.
//!
//! ## Choose what clumps grow on
//!
//! Generated rocks and trees describe their meshes with tags, such as `rock`,
//! or `bark` and `trunk` for a tree's trunk. Each clump's
//! [`ClumpOptions::grows_on`](procgena_clump::ClumpOptions::grows_on) picks
//! the parts to grow on; the presets already choose sensible ones, such as
//! snow on rock, bark and ground but never on leaves. See
//! [`procgena_mesh::tags`] for the tags in use.
//!
//! To grow clumps on your own mesh, add [`ObjectTags`] to its entity and
//! spawn the `ClumpParams` as its child.
//!
//! ## Host rules
//!
//! - The clump is built in its host's coordinates. Leave the `ClumpParams`
//!   entity's own transform at its default.
//! - Directions such as falling snow are in world space. Turning or
//!   scaling the host rebuilds its clumps; moving it doesn't. The host must
//!   be scaled the same along every axis.
//! - Clumps grow on the host's most detailed level and are drawn at every
//!   distance, even when the host switches to a simpler mesh.
//! - Rocks drawn from a GPU stone pool can't carry clumps: their meshes
//!   exist only on the GPU.
//! - Clumps on trees don't sway in the wind.
//!
//! ## Add textures
//!
//! Clump textures are not stored in the repository. Download them first:
//!
//! ```sh
//! uv run --project tools textures download clump
//! ```
//!
//! Then copy the repository's `clump/assets/textures` folder to
//! `assets/procgena_clump/textures` in your app and use
//! `ClumpPlugin::default()`. Set `texture_root` to another folder relative
//! to Bevy's asset directory if you store them elsewhere, or to `None` for
//! plain colours. Missing textures cause asset-loading errors.
//!
//! ## Build your own entities
//!
//! For direct control, grow a [`procgena_clump::Clump`] yourself, convert
//! its mesh with [`clump_mesh`], create a material with [`clump_material`]
//! and [`ClumpTextures::load`], and attach them to your entities. Keep
//! `ClumpPlugin` installed to register the material's shaders.

mod material;
mod mesh;
mod spawn;
mod tagged;

use bevy::asset::embedded_asset;
use bevy::pbr::MaterialPlugin;
use bevy::prelude::*;
use bevy::shader::load_shader_library;
use procgena_clump::{TextureCompression, TextureSize};

pub use material::{ClumpExtension, ClumpMaterial, ClumpSettings, ClumpTextures, clump_material};
pub use mesh::clump_mesh;
pub use spawn::{
    ClumpBuildError, ClumpBuilt, ClumpFinished, ClumpGenerating, ClumpParams, ClumpPart,
};
pub use tagged::ObjectTags;

use spawn::{ClumpMaterials, SpawnSettings};

/// Enables clump growing, rendering and [`ObjectTags`].
///
/// Add after Bevy's default plugins, then spawn [`ClumpParams`] entities as
/// children of their hosts.
#[derive(Debug, Clone)]
pub struct ClumpPlugin {
    /// Folder holding the clump texture folders (`snow/`, `moss/` and so on),
    /// relative to Bevy's asset directory. Defaults to
    /// `procgena_clump/textures`; copy the repository's
    /// `clump/assets/textures` folder there after downloading the textures.
    /// Use `None` for plain colours only.
    pub texture_root: Option<String>,
    /// Which texture files to load. Defaults to BC7; UASTC needs Bevy's
    /// `basis-universal` feature.
    pub texture_compression: TextureCompression,
    /// Texture image size. Defaults to 1k.
    pub texture_size: TextureSize,
}

impl Default for ClumpPlugin {
    fn default() -> Self {
        Self {
            texture_root: Some("procgena_clump/textures".to_owned()),
            texture_compression: TextureCompression::Bc7,
            texture_size: TextureSize::Size1k,
        }
    }
}

impl Plugin for ClumpPlugin {
    fn build(&self, app: &mut App) {
        load_shader_library!(app, "shaders/clump_triplanar.wesl");
        load_shader_library!(app, "shaders/clump_material.wesl");
        embedded_asset!(app, "shaders/clump.wesl");
        embedded_asset!(app, "shaders/clump_prepass.wesl");

        app.add_plugins(MaterialPlugin::<ClumpMaterial>::default())
            .init_resource::<ClumpMaterials>()
            .insert_resource(SpawnSettings {
                texture_root: self.texture_root.clone(),
                texture_compression: self.texture_compression,
                texture_size: self.texture_size,
            })
            .add_systems(Update, spawn_systems());
    }
}

/// The systems that describe tagged meshes and grow, show and remove clumps,
/// in order.
pub(crate) fn spawn_systems() -> impl IntoScheduleConfigs<bevy::ecs::system::ScheduleSystem, ()> {
    (
        tagged::describe_tagged_meshes,
        spawn::remove_clumps,
        spawn::start_generation,
        spawn::finish_generation,
        spawn::refresh_materials,
        spawn::prune_materials,
    )
        .chain()
}
