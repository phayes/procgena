//! # procgena_tree
//!
//! Generate procedural trees in Rust, from small bushes to branching oaks and pines.
//! Based on [ez-tree](https://github.com/dgreenheck/ez-tree), `procgena_tree` produces
//! branch and leaf meshes, repeatable shapes from a seed, and simpler meshes for
//! distant trees. Your application supplies the renderer. It uses ez-tree's
//! settings and growth rules but its own random numbers, so a seed grows a
//! different tree here than in ez-tree.
//!
//! ## Generate a tree
//!
//! Start with a built-in preset:
//!
//! ```rust
//! use procgena_tree::{Tree, TreePreset};
//!
//! let tree = Tree::from_preset(TreePreset::oak_small())?;
//! let mesh = tree.mesh();
//!
//! println!("{} triangles", tree.triangle_count());
//! println!("{} bark vertices", mesh.branches.positions.len());
//! println!("{} leaf vertices", mesh.leaves.positions.len());
//! # Ok::<(), procgena_tree::OptionsError>(())
//! ```
//!
//! `mesh.branches` and `mesh.leaves` contain positions, normals, texture coordinates,
//! and triangle indices. Upload these buffers to your renderer and give each mesh
//! its own material. Presets include ash, aspen, oak, pine, bushes, a tree trained
//! along a trellis, and flowering and fruiting trees such as apple and flowering
//! dogwood. `TreePreset::all()` lists them all.
//!
//! ## Customize the shape
//!
//! Edit a preset's options, then generate a new tree. The same options and seed
//! produce the same shape; change the seed for another variation.
//!
//! ```rust
//! use procgena_tree::{Tree, TreePreset};
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut options = TreePreset::oak_small().tree.clone();
//!     options.default_seed = 42;
//!     options.branch.length[0] = 1.2; // Trunk length, in metres.
//!     options.leaves.size = 0.15;
//!
//!     let tree = Tree::new(options)?;
//!     println!("{} triangles", tree.triangle_count());
//!     Ok(())
//! }
//! ```
//!
//! Branch settings use index `0` for the trunk and indices `1` through `3` for
//! successive branch levels. You can also start with `TreeOptions::default()`.
//! `Tree::new` checks branch depth and section counts; it does not validate every
//! numeric setting, so use sensible dimensions and counts.
//!
//! ## Load and save preset files
//!
//! A `.tree.yml` file names a built-in preset as its `base` and gives only the
//! settings that differ, using ez-tree's settings in snake case. Lengths are in
//! metres. Settings under the `tree` key merge into the base's, so nested
//! settings may be partial; lists replace the base's list whole.
//!
//! ```rust
//! use procgena_tree::{Tree, TreePreset};
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let preset = TreePreset::from_yaml(
//!         "tall_oak",
//!         "base: oak_small\n\
//!          name: Tall oak\n\
//!          tree:\n  \
//!            default_seed: 42\n  \
//!            branch:\n    \
//!              length: { \"0\": 1.2 }\n",
//!     )?;
//!     let tree = Tree::from_preset(&preset)?;
//!
//!     // Write it back, as only what differs from the base.
//!     let text = preset.to_yaml(Some(TreePreset::oak_small()));
//!     assert_eq!(TreePreset::from_yaml("tall_oak", &text)?, preset);
//!     assert_eq!(tree.options(), &preset.tree);
//!     Ok(())
//! }
//! ```
//!
//! The crate's `presets` folder holds every built-in preset as a file to copy
//! from. Deserializing `TreeOptions` directly with serde requires every field.
//!
//! ## Create simpler meshes
//!
//! Generate several levels of detail (LOD) from the same tree. Your renderer chooses
//! which mesh to show based on camera distance.
//!
//! ```rust
//! use procgena_tree::{Tree, TreePreset, default_lod};
//!
//! let tree = Tree::from_preset(TreePreset::ash_small())?;
//! for (level, mesh) in tree.lod_meshes(&default_lod().levels) {
//!     println!(
//!         "From distance {}: {} triangles",
//!         level.distance,
//!         mesh.triangle_count(),
//!     );
//! }
//! # Ok::<(), procgena_tree::OptionsError>(())
//! ```
//!
//! The four default levels use progressively simpler meshes at distances of 0, 10,
//! 25 and 50 metres. To choose your own mesh detail, pass a `LodDetail` to
//! `Tree::mesh_with_detail`.
//!
//! Only LOD0 and LOD1 are meant for gameplay. Their trunk and branches are closed,
//! outward-facing solids (`LodDetail::closed`), suitable for physics, collision and
//! volumes. LOD2 and LOD3 are for drawing distant trees only: their bark is open
//! and simplified, so don't build colliders or volumes from them.
//!
//! If you only need branch shapes and leaf placements, use `Skeleton::generate`
//! without building a mesh.
//!
//! ## Export a model
//!
//! Optional Cargo features enable file export:
//!
//! - `export-obj`: Wavefront OBJ geometry and MTL materials.
//! - `export-gltf`: glTF (`.gltf` and `.bin`) or a single binary `.glb` file.
//!
//! With `export-gltf` enabled, save a tree as GLB:
//!
//! ```no_run
//! # #[cfg(feature = "export-gltf")]
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use procgena_tree::{Tree, TreePreset};
//! use procgena_tree::export::gltf::{GltfOptions, to_glb};
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let tree = Tree::from_preset(TreePreset::pine_small())?;
//!     let bytes = to_glb(
//!         std::slice::from_ref(tree.mesh()),
//!         tree.options(),
//!         &GltfOptions::default(),
//!     )?;
//!     std::fs::write("pine.glb", bytes)?;
//!     Ok(())
//! }
//! # main()
//! # }
//! # #[cfg(not(feature = "export-gltf"))]
//! # fn main() {}
//! ```
//!
//! Exporters return file contents for you to save. By default, exports use material
//! colours without images; leaves appear as rectangles without a texture supplying
//! their outline. Use `GltfOptions::textures` to reference or embed PNG/JPEG images.
//! An optional rig prepares a model for bending animation, but does not add animation
//! clips. Exported LOD meshes require the receiving tool to choose which to display.
//!
//! To export every preset as OBJ and GLB from this repository:
//!
//! ```sh
//! cargo run -p procgena_tree --example export --features export-obj,export-gltf
//! ```
//!
//! Files are written to `target/procgena_tree_export` by default. The example accepts an
//! output directory and an optional source texture directory as positional arguments.
//!
//! ## Materials and animation
//!
//! [`BarkMaterial`] and [`LeafMaterial`] describe appearance settings for a renderer.
//! Texture images are not bundled with the crate. From the workspace root, run
//! `uv run --project tools textures download tree` to save ready-made KTX2
//! textures in `tree/assets/textures/`. The command downloads all leaf textures and the 512 and 1k bark sizes by default;
//! add `--sizes 512,1k,2k` to include 2k bark. Each image comes in two formats:
//!
//! - BC7 for GPUs that support it, including most desktop GPUs.
//! - UASTC for loaders that convert Basis Universal textures to a supported GPU format.
//!
//! Set `bark.kind` and `leaves.kind` to names from [`BARK_KINDS`] and [`LEAF_KINDS`].
//! [`bark_texture_path`] and [`leaf_texture_path`] give paths inside the texture folder,
//! such as `bark/sakura_bark-1k-color-bc7.ktx2` or `leaves/white_oak-normal-bc7.ktx2`.
//! Bark has three sizes; leaves have one size per kind. [`leaf_has_normal_map`] tells
//! you whether a leaf kind includes a normal map, which adds lighting detail without
//! extra geometry. `assets/textures/assets_manifest.json` lists the downloadable files.
//!
//! Load colour images as sRGB, and normal and roughness maps as linear data without
//! colour conversion. Normal maps use the OpenGL (+Y) convention. Leaf colour images
//! include straight alpha: transparency is stored separately from the RGB colours.
//! Keep the metallic factor at zero when using a roughness image as a combined
//! metallic-roughness texture.
//!
//! Normal maps add lighting detail without extra geometry. Supply the directions
//! in [`MeshBuffers::tangents`] from the corresponding bark or leaf mesh to orient
//! that detail. For bark, keep the default [`BarkUvMode::Continuous`].
//! [`BarkUvMode::Mirrored`] matches the JavaScript ez-tree generator's texture layout
//! but reverses normal-map lighting detail on every other branch section, so
//! Bevy and glTF export omit bark normal maps in that mode.
//!
//! For GPU wind, pass `MeshBuffers::wind` and `WindParams` to the `WIND_WESL` shader
//! module and add its `wind_offset` to vertex positions. For joint-based animation,
//! create joints with `Tree::rig` and vertex weights with `Rig::skin`.
//! `BranchGrouping::parts` and `TreeMesh::part` let you extract separate tree parts.
//! `TrellisOptions::wires` and `TrellisMaterial` describe the optional support wires.
//!
//! ## Scatter trees: groves and woodlands
//!
//! A [`TreeScatter`] mixes several species over a stretch of ground. Each
//! species is a [`ScatterTree`] with its own density in trees per hectare, the
//! spacing it keeps from its neighbours, and how much it gathers into stands.
//! [`TreeScatterPlacement::new`] lays the Scatter out over a [`ScatterInput`]
//! (ground, area, seed and an optional mask) and returns one [`TreeCopy`] per
//! tree:
//!
//! ```rust
//! use glam::{DVec2, Vec2};
//! use procgena_tree::{
//!     FlatGround, ScatterArea, ScatterInput, ScatterTree, TreePreset, TreeScatterPlacement,
//!     TreeScatterPreset,
//! };
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Start from a preset and add a few clumps of bushes.
//!     let mut scatter = TreeScatterPreset::oak_woodland().tree_scatter.clone();
//!     scatter.trees.insert(0, ScatterTree::new(TreePreset::bush_2()).scaled(0.05).clustered(0.7));
//!
//!     // A 40 m square around the World origin, placed with the Scatter's own seed.
//!     let area = ScatterArea::new(DVec2::ZERO, Vec2::splat(40.0));
//!     let input = ScatterInput::new(&FlatGround, area, scatter.default_seed);
//!     let placement = TreeScatterPlacement::new(&scatter, &input)?;
//!     for copy in placement.instances() {
//!         // Build each variant's mesh once and draw it at every copy.
//!         let _options = scatter.trees[copy.tree].variant(copy.variant);
//!         let _transform = copy.transform;
//!     }
//!     Ok(())
//! }
//! ```
//!
//! Each species is drawn from a few shape variants ([`ScatterTree::variants`]), so
//! a Scatter of hundreds of trees needs only a handful of meshes. Lowering its
//! density only removes trees, and raising it only adds them; the others stay where
//! they were. Ground is any [`Ground`]: [`FlatGround`], a closure taking a World
//! position (`DVec2`), or a [`HeightGrid`] sampled from another generator's
//! specimen.
//!
//! Scatters are saved as `.tree_scatter.yml` files with
//! [`TreeScatterPreset::to_yaml`] and read with [`TreeScatterPreset::from_yaml`].
//! A file may name a [`TreeScatterPreset`] as its `base` and list only what
//! differs. Its `trees` list replaces the base's whole:
//!
//! ```rust
//! use procgena_tree::TreeScatterPreset;
//!
//! let preset = TreeScatterPreset::from_yaml(
//!     "pine_and_cedar",
//!     "base: pine_stand\n\
//!      tree_scatter:\n  \
//!        density_scale: 0.5\n  \
//!        trees:\n    \
//!          - { tree: cedar, density_scale: 0.2, clustering: 0.5 }\n    \
//!          - { tree: { base: pine_medium, default_seed: 9 }, density_scale: 0.6 }\n",
//! )
//! .unwrap();
//! assert_eq!(preset.tree_scatter.trees.len(), 2);
//! ```
//!
//! ## Bevy integration
//!
//! The workspace also includes `procgena_tree_bevy`. Add its `TreePlugin` after Bevy's
//! default plugins and spawn a `TreeParams` component to generate and display trees.
//! It handles background generation, materials, wind, and mesh detail switching.
//! See that crate's documentation for setup and texture paths.

mod attached;
mod cluster;
mod compose;
mod content_hash;
#[cfg(any(feature = "export-obj", feature = "export-gltf"))]
pub mod export;
mod jiggle;
mod material;
mod mesh;
mod options;
mod part;
mod presets;
mod rig;
mod scatter;
mod skeleton;
mod specimen;
mod tree;
mod trellis;
mod units;
mod wind;

pub use attached::{AttachedFruit, ClusterPlacement, DEFAULT_FRUIT_SLOTS};
pub use cluster::{ClusterPart, SkeletonCluster};
pub use compose::{ClusterLink, ClusterRow, FLOWER_PART, FRUIT_PART};
pub use content_hash::{TREE_KIND, TreeCacheKey};
pub use material::{
    BARK_KINDS, BarkMap, BarkMaterial, LEAF_KINDS, LeafMap, LeafMaterial, TextureCompression,
    TextureSize, TrellisMaterial, bark_average_color, bark_texture_path, leaf_has_normal_map,
    leaf_texture_path,
};
pub use mesh::{
    BillboardCull, BillboardMode, BillboardVertex, BranchShape, BranchSpan, ClusterSpan,
    LOD_REFERENCE_SIZE, LeafSpan, LodDetail, MeshBuffers, TreeLod, TreeMesh, default_lod,
};
pub use options::{
    BarkOptions, BarkUvMode, Billboard, BranchOptions, GrowthForce, LeafOptions, MAX_LEVELS,
    OptionsError, PerLevel, TreeOptions, TreeType, TrellisForce, TrellisOptions,
};
pub use part::{BranchGrouping, MeshPart, PartMesh};
pub use presets::{TreePreset, TreeStand};
pub use procgena_core::preset::PresetFileError;
pub use procgena_core::scatter::{
    Footprints, Mask, Placement, Rest, Scatter, ScatterArea, ScatterInput,
};
pub use procgena_core::{AddressPrefix, ChildKey, CopyAddress, FlatGround, Ground, HeightGrid};
pub use procgena_fruit;
pub use rig::{Joint, MAX_RIG_JOINTS, MeshSkin, Rig, RigError, RigOptions, SkinWeights};
pub use scatter::{
    MAX_SCATTER_COPIES, MAX_SCATTER_SIZE, MAX_SCATTER_TREES, MAX_SCATTER_VARIANTS, ScatterTree,
    TreeCopy, TreeScatter, TreeScatterError, TreeScatterPlacement, TreeScatterPreset,
    TreeScatterVariation,
};
pub use skeleton::{Attachment, Section, Skeleton, SkeletonBranch, SkeletonLeaf};
pub use tree::Tree;
pub use trellis::{TrellisPull, TrellisWire};
pub use wind::{WIND_WESL, WindParams};
