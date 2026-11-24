//! # procgena_stone_bevy
//!
//! Display procedural rocks in Bevy with background generation, projected
//! textures and distance-based mesh detail. Built on `procgena_stone`, this crate
//! targets Bevy 0.20.1.
//!
//! ## Display a rock
//!
//! Add `StonePlugin` after Bevy's default plugins, then spawn a `StoneParams`
//! component. This complete example includes a camera and light and runs
//! without texture files:
//!
//! ```no_run
//! use bevy::prelude::*;
//! use procgena_stone::StoneOptions;
//! use procgena_stone_bevy::{StoneParams, StonePlugin};
//!
//! fn main() {
//!     App::new()
//!         .add_plugins((
//!             DefaultPlugins,
//!             StonePlugin {
//!                 texture_root: None,
//!                 ..default()
//!             },
//!         ))
//!         .add_systems(Startup, setup)
//!         .run();
//! }
//!
//! fn setup(mut commands: Commands) {
//!     let mut options = StoneOptions::default();
//!     options.material.color = [0.35, 0.33, 0.3];
//!     commands.spawn(StoneParams::new(options));
//!     commands.spawn((
//!         Camera3d::default(),
//!         Transform::from_xyz(3.0, 2.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
//!     ));
//!     commands.spawn((
//!         DirectionalLight {
//!             illuminance: 12_000.0,
//!             shadow_maps_enabled: true,
//!             ..default()
//!         },
//!         Transform::from_xyz(3.0, 5.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
//!     ));
//! }
//! ```
//!
//! The rock appears when background generation finishes, centred on its
//! entity. Each detail level is spawned as a child entity. Rocks with
//! identical options share meshes and materials.
//!
//! With `texture_root: None`, rocks use [`MaterialOptions::color`],
//! [`MaterialOptions::roughness`] and [`MaterialOptions::metallic`] alone. The default colour is white, so set
//! a colour as above when you do not use textures.
//!
//! ## Add textures
//!
//! Rock textures are not stored in the repository. Download them first:
//!
//! ```sh
//! uv run --project tools textures download stone
//! ```
//!
//! This fetches the 512 and 1k sizes into `assets/textures/rock/`; add
//! `--sizes 512,1k,2k` to include 2k. Then copy the repository's
//! `assets/textures` folder to `assets/procgena_stone/textures` in your app and use
//! `StonePlugin::default()`.
//! The default texture root is `procgena_stone/textures`, relative to Bevy's asset
//! directory. Set `texture_root` to another path if you store textures
//! elsewhere. Missing textures cause asset-loading errors.
//!
//! Choose a texture set with [`MaterialOptions::texture`]; see
//! [`TEXTURE_SETS`](procgena_stone::TEXTURE_SETS) for the available names. Textures
//! are projected onto the rock along its three axes, so rocks need no texture
//! coordinates. They are fixed to the rock and move, turn and scale with it.
//! [`MaterialOptions::texture_size`] sets how much of the rock one repeat of
//! the texture covers.
//!
//! Every texture comes in two formats. The default, BC7, requires GPU support
//! for that compression format. For other GPUs, set `texture_compression` to
//! `TextureCompression::Uastc` and enable Bevy's `basis-universal` feature.
//!
//! ## Customize and position rocks
//!
//! Edit options and wrap them with `StoneParams::new`. Add a `Transform` to
//! position each rock. Register this function as a startup system in your app:
//!
//! ```no_run
//! use bevy::prelude::*;
//! use procgena_stone::StoneOptions;
//! use procgena_stone_bevy::StoneParams;
//!
//! fn place_rocks(mut commands: Commands) {
//!     for i in 0..5 {
//!         let mut options = StoneOptions::default();
//!         options.default_seed = i;
//!         options.stretch = [1.0, 0.6, 0.8];
//!         options.material.texture = "lichen_rock_02".to_owned();
//!
//!         commands.spawn((
//!             StoneParams::new(options),
//!             Transform::from_xyz(i as f32 * 4.0, 0.0, 0.0),
//!         ));
//!     }
//! }
//! ```
//!
//! The same options and seed produce the same shape. Changing a rock's
//! `StoneParams` component rebuilds its generated parts; removing the component
//! removes those children. Moving the parent entity moves the whole rock.
//!
//! ## Load a preset file
//!
//! Save a rock preset to `assets/rocks/boulder.stone.yml`. It names a
//! built-in preset as its `base` and gives only the settings that differ;
//! without a `base`, missing settings use their defaults. For example:
//!
//! ```yaml
//! base: boulder
//! stone:
//!   default_seed: 42
//!   scale: 4.0
//! ```
//!
//! Load the file through Bevy's asset server in a startup system:
//!
//! ```no_run
//! use bevy::prelude::*;
//! use procgena_stone_bevy::StoneParams;
//!
//! fn load_rock(mut commands: Commands, asset_server: Res<AssetServer>) {
//!     commands.spawn((
//!         StoneParams::Asset(asset_server.load("rocks/boulder.stone.yml")),
//!         Transform::from_xyz(5.0, 0.0, 0.0),
//!     ));
//! }
//! ```
//!
//! The filename must end in `.stone.yml`. The rock waits for the asset to
//! load, then generates in the background. It rebuilds when Bevy reports that
//! the asset has changed; detecting live file edits requires Bevy asset
//! watching to be enabled.
//!
//! ## Choose mesh detail
//!
//! Each rock's [`MeshOptions::detail`](procgena_stone::MeshOptions::detail) sets its
//! detail levels: the mesh resolution, the camera distance where each level
//! starts, and its texture size. Bevy fades between levels. Distances are for
//! the default rock size of 2.5 m
//! ([`StoneOptions::scale`](procgena_stone::StoneOptions::scale)); a rock twice that
//! size switches twice as far away.
//!
//! The default, [`StoneDetail::MEDIUM`](procgena_stone::StoneDetail::MEDIUM), builds
//! meshes at 96, 48 and 24 grid cells per axis, switching at 15 and 40 metres,
//! with 1k textures up close and 512 farther away. Pick another preset
//! (`LOW`, `HIGH`, `ULTRA`) or define your own levels. For a single 128-cell
//! mesh with 2k textures and no distance switching:
//!
//! ```rust
//! use procgena_stone::{StoneDetail, StoneOptions, TextureSize};
//! use procgena_stone_bevy::StoneParams;
//!
//! let mut options = StoneOptions::default();
//! options.mesh.detail = StoneDetail::single(128, TextureSize::Size2k);
//! let rock = StoneParams::new(options);
//! ```
//!
//! Generated children have a `StonePart` component identifying their detail level.
//!
//! ## Draw thousands of rocks from a GPU stone pool
//!
//! With the `gpu` feature, a `StonePool` generates a fixed set of rock shapes
//! on the GPU, and each `PooledStone` entity draws one of them at its
//! transform. The GPU chooses which rocks each camera and shadow map draws,
//! and at which detail level, so thousands of rocks cost little CPU time. The
//! meshes stay on the GPU; for a collider, build the same shape on the CPU from
//! `StonePool::variant_options`. Register this function as a startup system:
//!
//! ```no_run
//! # #[cfg(feature = "gpu")]
//! # mod example {
//! use bevy::prelude::*;
//! use procgena_stone::StoneOptions;
//! use procgena_stone_bevy::{PooledStone, StonePool};
//!
//! fn scatter_rocks(mut commands: Commands, mut pools: ResMut<Assets<StonePool>>) {
//!     let mut pool = StonePool::new(StoneOptions::default(), 16);
//!     pool.stretch_jitter = 0.3;
//!     let pool = pools.add(pool);
//!     for i in 0..1000u32 {
//!         let (x, z) = ((i % 40) as f32 * 6.0, (i / 40) as f32 * 6.0);
//!         commands.spawn((
//!             PooledStone {
//!                 pool: pool.clone(),
//!                 variant: i % 16,
//!             },
//!             Transform::from_xyz(x, 0.5, z).with_rotation(Quat::from_rotation_y(i as f32)),
//!         ));
//!     }
//! }
//! # }
//! ```
//!
//! Each variant reserves GPU memory for every detail level, about 2.6 MB with
//! the default levels. Pooled rocks should mostly stay still: moving any of
//! them uploads all of them again.
//!
//! ## Try the viewer
//!
//! From the repository, run:
//!
//! ```sh
//! cargo run -p procgena_stone_bevy --example viewer
//! ```
//!
//! The viewer uses the repository's textures; run `uv run --project tools textures download stone`
//! first. See the example's documentation for its controls.
//!
//! ## Build your own entities
//!
//! For direct control, convert a [`procgena_stone::StoneMesh`] with [`stone_mesh`],
//! create a material with [`stone_material`] and [`StoneTextures::load`], and
//! attach the mesh handle and `MeshMaterial3d<StoneMaterial>` to your entities.
//! Keep `StonePlugin` installed to register the material's shaders.
//!
//! [`MaterialOptions::color`]: procgena_stone::MaterialOptions::color
//! [`MaterialOptions::roughness`]: procgena_stone::MaterialOptions::roughness
//! [`MaterialOptions::metallic`]: procgena_stone::MaterialOptions::metallic
//! [`MaterialOptions::texture`]: procgena_stone::MaterialOptions::texture
//! [`MaterialOptions::texture_size`]: procgena_stone::MaterialOptions::texture_size

mod impostor;
mod material;
mod mesh;
#[cfg(feature = "gpu")]
mod pool;
mod spawn;

use bevy::asset::embedded_asset;
use bevy::pbr::MaterialPlugin;
use bevy::prelude::*;
use bevy::shader::load_shader_library;
use procgena_core_bevy::LodPlugin;
use procgena_core_bevy::instanced::InstancedPlugin;
use procgena_impostor::{ImpostorLevel, ImpostorPlugin};
use procgena_stone::TextureCompression;

pub use impostor::default_stone_impostor;
pub use material::{
    StoneMaterial, StoneTextures, TriplanarExtension, TriplanarKey, TriplanarSettings, bake_copy,
    stone_material,
};
pub use mesh::stone_mesh;
#[cfg(feature = "gpu")]
pub use pool::{PooledStone, StonePool, StonePoolError};
pub use spawn::{
    StoneEmbedDepth, StoneFinished, StoneGenerating, StoneParams, StonePart, StonePresetAsset,
};

use spawn::{SpawnSettings, StoneCache, StonePresetLoader};

/// Enables rock generation, rendering and `.stone.yml` loading.
///
/// Add after Bevy's default plugins, then spawn entities with [`StoneParams`].
#[derive(Debug, Clone)]
pub struct StonePlugin {
    /// Folder holding the `rock/` texture folder, relative to Bevy's asset
    /// directory. Defaults to `procgena_stone/textures`; copy the repository's
    /// `assets/textures` folder there after downloading the textures.
    /// Use `None` for material colours only.
    pub texture_root: Option<String>,
    /// Which texture files to load. Defaults to BC7; UASTC needs Bevy's
    /// `basis-universal` feature.
    pub texture_compression: TextureCompression,
    /// Draws [`StoneParams`] rocks far away as baked impostors, as the last
    /// level after each rock's own detail levels. Defaults to
    /// [`default_stone_impostor`]. `None` draws each rock's farthest level
    /// at every distance beyond. Rocks in a GPU stone pool have no
    /// impostors yet.
    pub impostor: Option<ImpostorLevel>,
}

impl Default for StonePlugin {
    fn default() -> Self {
        Self {
            texture_root: Some("procgena_stone/textures".to_owned()),
            texture_compression: TextureCompression::Bc7,
            impostor: Some(default_stone_impostor()),
        }
    }
}

impl Plugin for StonePlugin {
    fn build(&self, app: &mut App) {
        load_shader_library!(app, "shaders/stone_triplanar.wesl");
        load_shader_library!(app, "shaders/stone_material.wesl");
        embedded_asset!(app, "shaders/stone.wesl");
        embedded_asset!(app, "shaders/stone_prepass.wesl");

        // Impostors, unless the app added the plugin itself with settings of
        // its own.
        if self.impostor.is_some() && !app.is_plugin_added::<ImpostorPlugin>() {
            app.add_plugins(ImpostorPlugin::default());
        }

        app.add_plugins((
            LodPlugin,
            InstancedPlugin,
            MaterialPlugin::<StoneMaterial>::default(),
        ))
        .init_asset::<StonePresetAsset>()
        .register_asset_loader(StonePresetLoader)
        .init_resource::<StoneCache>()
        .insert_resource(SpawnSettings {
            texture_root: self.texture_root.clone(),
            texture_compression: self.texture_compression,
            impostor: self.impostor,
        })
        .add_systems(
            Update,
            (
                spawn::reload_modified_options,
                spawn::reset_changed_stones,
                spawn::start_generation,
                spawn::finish_generation,
                spawn::spawn_parts,
                spawn::prune_cache,
            )
                .chain(),
        );
        if self.impostor.is_some() {
            app.add_systems(
                Update,
                (impostor::request_impostors, impostor::show_impostors)
                    .chain()
                    .after(spawn::prune_cache),
            );
        }

        #[cfg(feature = "gpu")]
        app.add_plugins(pool::StonePoolPlugin);
    }
}
