//! # procgena_tree_bevy
//!
//! Display procedural trees in Bevy with background generation, textured materials,
//! wind animation, and distance-based mesh detail. Built on `procgena_tree`, this crate
//! targets Bevy 0.20.1.
//!
//! ## Display a tree
//!
//! Add `TreePlugin` after Bevy's default plugins, then spawn a `TreeParams` component.
//! This complete example includes a camera and light and runs without texture files:
//!
//! ```no_run
//! use bevy::prelude::*;
//! use procgena_tree::TreePreset;
//! use procgena_tree_bevy::{TreeParams, TreePlugin};
//!
//! fn main() {
//!     App::new()
//!         .add_plugins((
//!             DefaultPlugins,
//!             TreePlugin {
//!                 texture_root: None,
//!                 ..default()
//!             },
//!         ))
//!         .add_systems(Startup, setup)
//!         .run();
//! }
//!
//! fn setup(mut commands: Commands) {
//!     commands.spawn(TreeParams::preset(TreePreset::oak_small()));
//!     commands.spawn((
//!         Camera3d::default(),
//!         Transform::from_xyz(3.5, 2.5, 5.5)
//!             .looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
//!     ));
//!     commands.spawn((
//!         DirectionalLight {
//!             illuminance: 12_000.0,
//!             shadow_maps_enabled: true,
//!             ..default()
//!         },
//!         Transform::from_xyz(30.0, 50.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y),
//!     ));
//! }
//! ```
//!
//! The tree appears when background generation finishes. Bark, leaves, and any
//! visible trellis support wires are spawned as child entities. Trees with identical
//! options share meshes and materials.
//!
//! With `texture_root: None`, trees use material colours and leaves are solid
//! rectangles. Add textures to give leaves their outline and bark its surface detail.
//!
//! ## Add textures
//!
//! Bark textures are not stored in the repository. Download them first:
//!
//! ```sh
//! uv run --project tools textures download tree
//! ```
//!
//! This fetches the 512 and 1k sizes into `assets/textures/bark/`; add
//! `--sizes 512,1k,2k` to include 2k. Then copy the repository's `assets/textures`
//! directory to `assets/procgena_tree/textures` in your app and use
//! `TreePlugin::default()`. The default texture root is `procgena_tree/textures`,
//! relative to Bevy's asset directory. Set `texture_root` to another path if you
//! store textures elsewhere. Missing textures cause asset-loading errors.
//!
//! Every texture comes in two formats. The default, BC7, requires GPU support for
//! that compression format. For other GPUs, set `texture_compression` to
//! `TextureCompression::Uastc` and enable Bevy's `basis-universal` feature.
//!
//! Bark normal maps add lighting detail without extra geometry. Keep the default
//! `BarkUvMode::Continuous` to use them. With `BarkUvMode::Mirrored`, Bevy omits
//! the bark normal map to avoid reversed lighting detail on every other branch
//! section.
//!
//! ## Customize and position trees
//!
//! Edit a preset's options and wrap them with `TreeParams::new`. Add a `Transform` to
//! position each tree. Register this function as a startup system in your app:
//!
//! ```no_run
//! use bevy::prelude::*;
//! use procgena_tree::TreePreset;
//! use procgena_tree_bevy::TreeParams;
//!
//! fn plant_trees(mut commands: Commands) {
//!     for i in 0..5 {
//!         let mut options = TreePreset::oak_small().tree.clone();
//!         options.default_seed = i;
//!         options.branch.length[0] = 12.0; // Trunk length.
//!
//!         commands.spawn((
//!             TreeParams::new(options),
//!             Transform::from_xyz(i as f32 * 20.0, 0.0, 0.0),
//!         ));
//!     }
//! }
//! ```
//!
//! The same options and seed produce the same shape. Changing a tree's `TreeParams`
//! component rebuilds its generated parts; removing the component removes those
//! children. Moving the parent entity moves the whole tree.
//!
//! ## Plant a grove
//!
//! Spawn a `TreeScatterParams` to spread a mix of trees over the ground around an
//! entity. Start from a `procgena_tree::TreeScatterPreset`, or build a
//! `procgena_tree::TreeScatter` of your own:
//!
//! ```no_run
//! use bevy::prelude::*;
//! use procgena_tree::TreeScatterPreset;
//! use procgena_tree_bevy::TreeScatterParams;
//!
//! fn plant_grove(mut commands: Commands) {
//!     commands.spawn((
//!         TreeScatterParams::preset(TreeScatterPreset::oak_woodland(), Vec2::new(60.0, 40.0)),
//!         Transform::from_xyz(0.0, 0.0, -30.0),
//!     ));
//! }
//! ```
//!
//! The grove draws all its trees itself, without an entity per tree, so one grove
//! can hold thousands of trees. Trees of the same species and shape variant share
//! meshes and materials, so even a large grove generates only a handful of meshes.
//! `TreeScatterInstances` on the grove lists every tree.
//!
//! Change the component to change the grove: only the trees that appear, disappear
//! or change are affected, so lowering the density leaves the remaining trees exactly
//! as they were. `TreeScatterGenerating` is present until every tree is ready, and
//! `TreeScatterFinished` is triggered on the grove when it is.
//!
//! To give single trees an entity of their own, for a collider or to interact with
//! them, spawn a `PromotedTree` naming the grove and the tree. `TreeScatterTrees`
//! finds trees near a point or under the mouse:
//!
//! ```no_run
//! use bevy::prelude::*;
//! use procgena_tree_bevy::{PromotedTree, TreeScatterTrees};
//!
//! /// A game's own component.
//! #[derive(Component)]
//! struct Collider;
//!
//! fn promote_nearby(
//!     mut commands: Commands,
//!     trees: TreeScatterTrees,
//!     player: Single<&GlobalTransform, With<Camera3d>>,
//!     promoted: Query<&PromotedTree>,
//! ) {
//!     for (grove, tree) in trees.within(player.translation(), 10.0) {
//!         let promotion = PromotedTree::new(grove, tree.address);
//!         if !promoted.iter().any(|p| *p == promotion) {
//!             commands.spawn((promotion, Collider));
//!         }
//!     }
//! }
//! ```
//!
//! To stand the trees on terrain or another generated surface, spawn the grove as a
//! child of an entity with a `GeneratedSpecimen` and use `on_host`:
//!
//! ```no_run
//! use bevy::prelude::*;
//! use procgena_specimen::{TagFilter, tags};
//! use procgena_tree::TreeScatterPreset;
//! use procgena_tree_bevy::TreeScatterParams;
//!
//! fn plant_on(terrain: Entity, mut commands: Commands) {
//!     commands.spawn((
//!         TreeScatterParams::preset(TreeScatterPreset::pine_stand(), Vec2::splat(50.0))
//!             .on_host(TagFilter::any_of([tags::GROUND]), 0.5),
//!         ChildOf(terrain),
//!     ));
//! }
//! ```
//!
//! Groves also load from `.tree_scatter.yml` files through
//! `TreeScatterSource::Asset`, and update when the file changes if Bevy is watching
//! asset files. Try it with `cargo run -p procgena_tree_bevy --example scatter`.
//!
//! Far away, trees are drawn as baked impostors; see `default_tree_impostor`. To generate a
//! grove's trees and bake their impostors before the grove appears, for example
//! behind a loading screen, spawn a `PrepareTreeScatter` first.
//!
//! ## Load a preset file
//!
//! Save a tree preset to `assets/trees/oak.tree.yml`. It names a built-in preset as
//! its `base` and gives only the settings that differ. For example:
//!
//! ```yaml
//! base: oak_small
//! tree:
//!   default_seed: 42
//!   branch:
//!     length: { "0": 12.0 }
//! ```
//!
//! Load the file through Bevy's asset server in a startup system:
//!
//! ```no_run
//! use bevy::prelude::*;
//! use procgena_tree_bevy::TreeParams;
//!
//! fn load_tree(mut commands: Commands, asset_server: Res<AssetServer>) {
//!     commands.spawn((
//!         TreeParams::Asset(asset_server.load("trees/oak.tree.yml")),
//!         Transform::from_xyz(20.0, 0.0, 0.0),
//!     ));
//! }
//! ```
//!
//! The filename must end in `.tree.yml`. The tree waits for the asset to load,
//! then generates in the background. It rebuilds when Bevy reports that the asset
//! has changed; detecting live file edits requires Bevy asset watching to be enabled.
//!
//! ## Control wind
//!
//! By default, trunks bend, branches sway, and leaves flutter. Change the global
//! `Wind` resource to update all tree materials. For example, register this startup
//! system to keep trunks and branches still while leaves flutter:
//!
//! ```no_run
//! use bevy::prelude::*;
//! use procgena_tree::WindParams;
//! use procgena_tree_bevy::Wind;
//!
//! fn set_wind(mut wind: ResMut<Wind>) {
//!     wind.0 = WindParams::LEAVES_ONLY;
//! }
//! ```
//!
//! Wind animation runs on the GPU and is included in shadows and motion vectors.
//! Each tree's position varies the timing so copies do not all move together.
//! The plugin synchronizes every `TreeMaterial` with the global resource, including
//! materials you create yourself.
//!
//! ## Choose mesh detail
//!
//! The plugin generates four mesh detail levels. Bevy fades to simpler meshes at
//! camera distances of 10, 25 and 50 metres. With a texture folder configured,
//! the nearest level uses 1k bark textures and the next two use 512 textures.
//! At the farthest level (LOD3), thin branches are omitted and the remaining branches
//! use flat strips. With textures configured, those strips use a single colour
//! approximating the bark texture, and fewer, larger leaf cards show clumps of leaves.
//! Fruit above the size threshold become flat hexagons without stalks. These flat
//! shapes, called billboards, turn toward the camera to help preserve the tree’s
//! outline as the view changes. For shadow rendering, they turn toward the light.
//! To see shadows at this distance, extend the directional light’s shadow cascades
//! far enough to cover the trees.
//! Set `TreePlugin::lod` when adding the plugin to choose mesh detail,
//! switching distances, bark appearance and leaf images. For a single full-detail
//! mesh with 2k bark textures, no distance switching and no impostor:
//!
//! ```rust
//! use procgena_core::{Lod, LodLevel};
//! use procgena_tree::{LOD_REFERENCE_SIZE, LodDetail, TextureSize};
//! use procgena_tree_bevy::{BarkTexture, LeafTexture, TreeLevel, TreePlugin};
//!
//! let level = TreeLevel {
//!     detail: LodDetail::default(),
//!     bark: BarkTexture::Textured(TextureSize::Size2k),
//!     leaves: LeafTexture::Leaf,
//! };
//! let plugin = TreePlugin {
//!     lod: Lod::new(LOD_REFERENCE_SIZE, vec![LodLevel::new(0.0, 0.0, level)]),
//!     impostor: None,
//!     ..Default::default()
//! };
//! ```
//!
//! The 2k bark textures are downloaded only with `--sizes 512,1k,2k`.
//!
//! No levels at all produce no bark or leaf meshes. Generated children have a
//! `TreePart` component identifying their detail level and whether they draw bark,
//! leaves, or a trellis wire.
//!
//! ## Try the viewer
//!
//! From the repository, run:
//!
//! ```sh
//! cargo run -p procgena_tree_bevy --example viewer
//! ```
//!
//! The viewer uses the repository's textures; run `uv run --project tools textures download tree`
//! first to fetch the bark textures. Press Left/Right to switch presets,
//! F to toggle a forest, Space to toggle trunk and branch movement, and N to toggle
//! bark normal maps. Hold the left mouse button to look around and use WASD and Q/E
//! to fly. Press J to toggle shape variation and R to generate new variations.
//!
//! ## Build your own entities
//!
//! For direct control, convert a `procgena_tree::TreeMesh` with `bark_mesh` and `leaf_mesh`,
//! create materials with `bark_material` and `leaf_material`, and attach the mesh
//! handles and `MeshMaterial3d<TreeMaterial>` to your entities. Keep `TreePlugin`
//! installed to register shaders and synchronize wind settings.
//!
//! ## Load exported glTF trees
//!
//! Enable this crate's `gltf` feature to animate billboards in trees saved with
//! `procgena_tree::export::gltf`. Billboards are flat shapes used for distant tree parts;
//! this integration lets them turn toward the camera and move with the wind.
//! Export with `GltfOptions::wind` enabled (the default).
//!
//! Register the tree's custom vertex attributes with Bevy's `GltfPlugin`, then add
//! `TreePlugin` after Bevy's default plugins:
//!
//! ```rust,ignore
//! use bevy::gltf::GltfPlugin;
//! use bevy::prelude::*;
//! use procgena_tree_bevy::{TreePlugin, add_tree_gltf_attributes};
//!
//! App::new().add_plugins((
//!     DefaultPlugins.set(add_tree_gltf_attributes(GltfPlugin::default())),
//!     TreePlugin::default(),
//! ));
//! ```
//!
//! Load and spawn the file's scene with Bevy's glTF loader. Eligible meshes under
//! nodes tagged as billboards use `TreeMaterial` for wind and camera-facing rotation.
//! Their crossed fallback shapes are removed once all meshes under the billboard
//! node are converted. Bounds are updated to keep the rotating shapes visible;
//! these bounds do not include wind movement. Other meshes keep their original
//! materials, and converted billboard materials do not retain normal maps.
//!
//! Meshes keep their original shapes and materials if wind or billboard attributes
//! are missing or unregistered, or if they use animation joints. Their fallback
//! partners remain in the scene. Missing or unregistered attributes produce a
//! warning; check both the export's wind setting and the plugin setup above.
//!
//! Try the example, which exports and loads a distant Apple tree by default:
//!
//! ```sh
//! cargo run -p procgena_tree_bevy --example gltf_tree --features gltf
//! ```
//!
//! To load your own file, append `-- path/to/tree.glb`. Hold the left mouse button
//! to look around, and use WASD and Q/E to fly.

#[cfg(feature = "gltf")]
mod gltf;
mod impostor;
mod material;
mod material_key;
mod mesh;
mod scatter;
mod spawn;
mod wind;

use bevy::asset::embedded_asset;
use bevy::pbr::MaterialPlugin;
use bevy::prelude::*;
use bevy::shader::{Shader, load_shader_library};
use procgena_tree::{TextureCompression, WIND_WESL};

pub use impostor::default_tree_impostor;
pub use material::{
    BarkTextures, CARD_BAKE, LeafTextures, TreeMaterial, WindExtension, WindKey, WindUniform,
    average_bark_material, bake_copy, bark_material, card_material, flower_material,
    fruit_material, leaf_material, trellis_material,
};
pub use mesh::{
    ATTRIBUTE_BILLBOARD_AXIS, ATTRIBUTE_BILLBOARD_NORMAL, ATTRIBUTE_BILLBOARD_PIVOT,
    ATTRIBUTE_WIND, bark_mesh, billboard_aabb, cluster_mesh, leaf_mesh,
};
pub use scatter::{
    PlacedTree, PrepareTreeScatter, PromotedTree, TreeScatterAsset, TreeScatterAssets,
    TreeScatterBuildError, TreeScatterFinished, TreeScatterGenerating, TreeScatterGround,
    TreeScatterHit, TreeScatterInstances, TreeScatterLevelSize, TreeScatterParams, TreeScatterPart,
    TreeScatterPreparing, TreeScatterReady, TreeScatterSource, TreeScatterStats, TreeScatterTrees,
    TreeScatterVariant, tree_scatter_variants,
};
pub use spawn::{
    BarkTexture, LeafTexture, TreeFinished, TreeGenerating, TreeLevel, TreeParams, TreePart,
    TreePartKind, TreePresetAsset, default_tree_lod,
};
pub use wind::Wind;

use procgena_core::Lod;
use procgena_core_bevy::LodPlugin;
use procgena_impostor::{ImpostorLevel, ImpostorPlugin};

#[cfg(feature = "gltf")]
pub use gltf::add_tree_gltf_attributes;

use scatter::TreeScatterLoader;
use spawn::{SpawnSettings, TreeCache, TreePresetLoader};

/// Enables tree generation, rendering, wind, groves, and `.tree.yml` and
/// `.tree_scatter.yml` loading.
///
/// Add after Bevy's default plugins, then spawn entities with [`TreeParams`]
/// or [`TreeScatterParams`].
#[derive(Debug, Clone)]
pub struct TreePlugin {
    /// Folder holding the `bark/` and `leaves/` texture folders, relative to Bevy's
    /// asset directory. Defaults to `procgena_tree/textures`; copy the repository's
    /// `assets/textures` folder there after downloading the bark textures.
    /// Use `None` for tint colours only.
    pub texture_root: Option<String>,
    /// Which texture files to load. Defaults to BC7; UASTC needs Bevy's
    /// `basis-universal` feature.
    pub texture_compression: TextureCompression,
    /// The mesh and far-form levels, nearest first: each one's mesh detail,
    /// switching distance in metres, and bark and leaf appearance. Defaults
    /// to [`default_tree_lod`]. Every tree uses the distances as written, scaled
    /// for the [`LodQuality`](procgena_core_bevy::LodQuality) in use when it is
    /// generated: trees are not yet
    /// scaled by their own size. Supply one level to disable distance
    /// switching, or no levels to produce no tree meshes.
    ///
    /// The levels must pass [`Lod::validate`], or adding the plugin panics.
    pub lod: Lod<TreeLevel>,
    /// Draws trees far away as baked impostors, as the last level after
    /// [`lod`](Self::lod)'s. Defaults to [`default_tree_impostor`]. `None`
    /// draws the farthest mesh level at every distance beyond.
    pub impostor: Option<ImpostorLevel>,
}

impl Default for TreePlugin {
    fn default() -> Self {
        Self {
            texture_root: Some("procgena_tree/textures".to_owned()),
            texture_compression: TextureCompression::Bc7,
            lod: default_tree_lod(),
            impostor: Some(default_tree_impostor()),
        }
    }
}

/// Retains the core wind shader so tree shaders can continue importing it.
#[derive(Resource)]
struct WindShader(#[allow(dead_code)] Handle<Shader>);

impl Plugin for TreePlugin {
    fn build(&self, app: &mut App) {
        // Register the shared wind module under the path imported by the tree shaders.
        let wind_shader = app
            .world_mut()
            .resource_mut::<Assets<Shader>>()
            .add(Shader::from_wesl(
                WIND_WESL,
                "embedded://procgena_tree/wind.wesl",
            ));
        app.insert_resource(WindShader(wind_shader));
        load_shader_library!(app, "shaders/tree_wind.wesl");
        load_shader_library!(app, "shaders/tree_billboard.wesl");
        embedded_asset!(app, "shaders/tree.wesl");
        embedded_asset!(app, "shaders/tree_prepass.wesl");

        // The mesh levels must be valid. The impostor, as the last level,
        // must fit after them, or impostors are turned off.
        if !self.lod.levels.is_empty()
            && let Err(error) = self.lod.validate()
        {
            panic!("procgena_tree_bevy: invalid tree levels of detail: {error}");
        }
        let mut settings = SpawnSettings {
            texture_root: self.texture_root.clone(),
            texture_compression: self.texture_compression,
            lod: self.lod.clone(),
            impostor: self.impostor,
        };
        if settings.impostor.is_some()
            && let Err(error) = settings.draw_lod().validate()
        {
            error!(
                "procgena_tree_bevy: tree impostors don't fit after the levels ({error}); impostors are off"
            );
            settings.impostor = None;
        }
        let impostor = settings.impostor;

        // Impostors and cluster card pictures, unless the app added the
        // plugin itself with settings of its own. The tree shaders always
        // import its layout functions.
        if !app.is_plugin_added::<ImpostorPlugin>() {
            app.add_plugins(ImpostorPlugin::default());
        }

        app.add_plugins((
            LodPlugin,
            MaterialPlugin::<TreeMaterial>::default(),
            scatter::GroveRenderPlugin,
        ))
        .init_asset::<TreePresetAsset>()
        .register_asset_loader(TreePresetLoader)
        .init_asset::<TreeScatterAsset>()
        .register_asset_loader(TreeScatterLoader)
        .init_resource::<Wind>()
        .init_resource::<TreeCache>()
        .insert_resource(settings)
        .add_systems(
            Update,
            (
                // Grove variants requested here are generated by the tree
                // systems below in the same frame.
                scatter::remove_groves,
                scatter::finish_placing,
                scatter::start_groves,
                scatter::stop_preparing,
                scatter::start_preparing,
                // Promoted trees' parts are generated below in the
                // same frame.
                scatter::sync_promoted_trees,
                scatter::demote_trees,
                spawn::reload_modified_options,
                spawn::reset_changed_trees,
                spawn::start_generation,
                spawn::finish_generation,
                spawn::bake_cards,
                spawn::spawn_parts,
                scatter::refresh_grove_assets,
                spawn::prune_cache,
                scatter::finish_groves,
                wind::sync_wind,
            )
                .chain(),
        )
        .add_systems(Update, scatter::track_preparing.after(spawn::prune_cache));
        // Impostors that became ready are kept before groves count them.
        if impostor.is_some() {
            app.add_systems(
                Update,
                (impostor::request_impostors, impostor::show_impostors)
                    .chain()
                    .after(spawn::prune_cache)
                    .before(scatter::track_preparing),
            );
        }
        #[cfg(feature = "gltf")]
        gltf::register(app);
    }
}
