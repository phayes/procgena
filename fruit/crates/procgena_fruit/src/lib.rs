//! # procgena_fruit
//!
//! Procedural fruit and flower clusters: an apple on its stalk, a dogwood
//! bloom, a spray of willow catkins. A cluster is an *attached asset*: it
//! has no Scatter of its own, and a host asset, such as a tree from
//! `procgena_tree`, grows copies of it on itself. The host decides where
//! each copy grows; this crate decides what a cluster is.
//!
//! Describe a cluster with [`FruitOptions`], or start from a
//! [`FruitPreset`], then grow a [`Fruit`]. Each cluster is built in its own
//! frame, starting at the origin where it attaches and facing local +Y, so
//! one cluster can be drawn at many places on its host.
//!
//! ```
//! use procgena_fruit::{Fruit, FruitPreset, default_lod};
//!
//! let catkins = Fruit::from_preset(FruitPreset::pussy_willow_catkin())?;
//! let lod = default_lod();
//! let factor = lod.factor(catkins.size(), procgena_core::Quality::High);
//! for (level, mesh) in catkins.lod_meshes(&lod) {
//!     println!("from {} m: {} triangles", level.distance * factor, mesh.triangle_count());
//! }
//! # Ok::<(), procgena_fruit::OptionsError>(())
//! ```
//!
//! ## Pools and slots
//!
//! Every cluster grown from one set of settings belongs to one pool
//! ([`FruitOptions::pool_id`]). A pool holds a small number of distinct
//! specimens, its slots, and every copy draws one of them, chosen with
//! [`procgena_core::choose_slot`]. [`Variation`] makes the specimens of a pool differ in
//! ripeness, colour and shape, so even single-fruit clusters vary.
//!
//! ## Preset files
//!
//! A `.fruit.yml` file names a built-in preset as its `base` and gives only
//! the settings that differ:
//!
//! ```
//! use procgena_fruit::FruitPreset;
//!
//! let preset = FruitPreset::from_yaml(
//!     "green_apple",
//!     "base: apple\nfruit:\n  item:\n    kind: fruit\n    color: 0x8fc040\n",
//! )?;
//! assert_eq!(preset.fruit.size, FruitPreset::apple().fruit.size);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Export
//!
//! With the `export-obj` and `export-gltf` features, [`export`] writes one
//! cluster as an OBJ or glTF file.

mod content_hash;
#[cfg(any(feature = "export-obj", feature = "export-gltf"))]
pub mod export;
mod fruit;
mod lod;
mod material;
mod mesh;
mod options;
mod presets;
mod skeleton;
mod specimen;

pub use content_hash::FRUIT_KIND;
pub use fruit::{Fruit, FruitPart, ITEM_PART, STALK_PART};
pub use lod::{FRUIT_LOD_REFERENCE_SIZE, FruitLod, default_lod};
pub use material::FruitMaterial;
pub use mesh::{ClusterDetail, FruitMesh, ItemDetail};
pub use options::{
    ClusterKind, ClusterShape, Facing, FlowerShape, FruitOptions, FruitShape, ItemShape,
    OptionsError, Variation,
};
pub use presets::{FlowerPreset, FruitPreset, cluster_presets};
pub use procgena_core::preset::PresetFileError;
pub use skeleton::{ClusterItem, ClusterStalk, FruitSkeleton, ItemVariation};
