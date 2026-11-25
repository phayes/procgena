//! Procedural dead wood: fallen logs, fallen branches, sticks and stumps,
//! resting on the ground, meshed at a chosen detail.
//!
//! Describe a piece with [`DeadwoodOptions`], or start from a
//! [`DeadwoodPreset`] such as a log blown down by wind, then create a
//! [`Piece`]. Each stem is a smooth outline swept along a curved centre
//! line, so it can be meshed at any [`DeadwoodDetail`], from close-ups with
//! splintered breaks down to a few dozen triangles. Its ends can be sawn,
//! cut with a chainsaw or an axe, snapped, or a natural tapering tip.
//!
//! One setting, [`DeadwoodOptions::decay`], ages the wood through the five
//! decay classes foresters use to describe how far fallen wood has rotted.
//! It runs from fresh wood resting on its stubs, through peeling bark and
//! lost twigs, to a soft, flattened mound sinking into the ground. Most
//! effects of decay also have their own setting that overrides it.
//!
//! ```
//! use procgena_deadwood::{DeadwoodPreset, Piece};
//!
//! let log = Piece::from_preset(DeadwoodPreset::windthrow_log())?;
//! for (step, mesh) in log.lod_meshes() {
//!     println!("{}: {} triangles", step.name(), mesh.triangle_count());
//! }
//! # Ok::<(), procgena_deadwood::OptionsError>(())
//! ```
//!
//! A `.deadwood.yml` file names a preset as its `base` and gives only the
//! settings it changes:
//!
//! ```
//! use procgena_deadwood::{DeadwoodPreset, Piece};
//!
//! let preset = DeadwoodPreset::from_yaml(
//!     "short_mossy_log",
//!     "base: mossy_log\n\
//!      deadwood:\n  \
//!        default_seed: 7\n  \
//!        length: 2.0\n  \
//!        ends:\n    \
//!          tip: { kind: axe, angle: 45 }\n",
//! )?;
//! let log = Piece::from_preset(&preset)?;
//! assert!(log.mesh().triangle_count() > 0);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! [`LeafOptions`] grows leafy shoots, ferns or shelf fungi out of the main
//! stem, as see-through picture cards that a renderer textures.
//!
//! A piece's [`game_level`](DeadwoodOptions::game_level) is its real
//! shape, drawn near the camera. Small on screen, renderers draw a coarser
//! mesh derived from it, then an impostor (a baked picture), following the
//! shared [`procgena_core::chain::DistanceChain`]. [`DeadwoodDetail::lod`]
//! gives both meshes' details, and [`Piece::lod_meshes`] builds them.
//!
//! Pieces rest on flat ground at `y = 0`, with Y up. Logs and branches turn
//! onto a stable resting side, as if dropped; see [`RestingPose`]. Stumps
//! stand upright with their roots and base below the ground. A piece's
//! [`Piece::specimen`] lets other generators, such as moss from
//! `procgena_clump`, grow on it.
//!
//! [`StickScatter`] spreads sticks and twigs over an area of ground, flat
//! or shaped by a [`Ground`]. It returns a few stick meshes and a list of
//! placed copies of them, so a renderer can build each mesh once and draw
//! it many times.
//!
//! With the `export-obj` and `export-gltf` features, [`export`] writes
//! meshes as files for 3D tools.
//!
//! All lengths are in metres.
//!
//! Snapped breaks and hollows follow ideas from Infinigen's deformed trees
//! (BSD-3-Clause, Princeton University). The search for a resting side
//! follows `procgena_stone`'s placement.

mod build;
mod colour;
mod content_hash;
mod curve;
mod decay;
mod detail;
#[cfg(any(feature = "export-obj", feature = "export-gltf"))]
pub mod export;
mod leaves;
mod limb;
mod mesh;
mod meta;
mod options;
mod piece;
mod presets;
mod rng;
mod scatter;
mod seed_keys;
mod settle;
mod specimen;
mod texture;

pub use colour::{linear_from_hex, srgb_to_linear};
pub use content_hash::DEADWOOD_KIND;
pub use detail::{DETAIL_REFERENCE_SIZE, DeadwoodDetail, DeadwoodLod, detail_size};
pub use limb::SizeClass;
pub use mesh::{BillboardVertex, DeadwoodMesh, MeshBuffers, PartRange, Ribbons, Zone};
pub use meta::{DEADWOOD_META, meta_choices};
pub use options::{
    BarkOptions, BranchingOptions, DeadwoodOptions, EndKind, EndsOptions, HollowOptions,
    LeafOptions, MAX_CHILDREN, MAX_DECAY, MAX_END_SHARE, MAX_LEVELS, MAX_LOBES, MAX_ROOTS,
    MAX_SEGMENTS, MAX_SPLINTERS, MAX_SPRAY_CARDS, MAX_SPRAYS_PER_METRE, MAX_STUBS_PER_METRE,
    MaterialOptions, OptionsError, PieceKind, RootOptions, SettleOptions, ShapeOptions,
    StubOptions,
};
pub use piece::Piece;
pub use presets::DeadwoodPreset;
pub use procgena_core::preset::PresetFileError;
pub use scatter::{
    FlatGround, Ground, HeightGrid, Instance, MAX_INSTANCES, MAX_VARIANTS, ScatterError,
    ScatterOptions, StickScatter,
};
pub use settle::RestingPose;
pub use specimen::{lower_start, specimen_from_meshes};
pub use texture::{
    BARK_TEXTURE_SETS, END_TEXTURE_SETS, LEAF_TEXTURE_SETS, LeafMap, TextureCompression,
    TextureMap, TextureSize, bark_texture_path, end_texture_path, leaf_texture_path,
};
