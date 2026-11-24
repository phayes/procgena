//! Procedural dead wood: fallen logs, fallen branches, sticks and stumps,
//! resting on the ground at several levels of detail.
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
//! use procgena_deadwood::{DeadwoodPreset, Piece, default_lod};
//!
//! let log = Piece::from_preset(DeadwoodPreset::WindthrowLog);
//! for (level, mesh) in log.lod_meshes(&default_lod()).unwrap() {
//!     println!("from {} m: {} triangles", level.distance, mesh.triangle_count());
//! }
//! ```
//!
//! [`LeafOptions`] grows leafy shoots, ferns or shelf fungi out of the main
//! stem, as see-through picture cards that a renderer textures.
//!
//! Distant objects don't need fine detail, so renderers swap in simpler
//! meshes as the camera moves away; each of those is a "level of detail"
//! (LOD). [`DeadwoodLod`], the shared [`procgena_core::Lod`] type, says
//! which detail to use at which distance; [`default_lod`] gives the
//! standard levels.
//!
//! Pieces rest on flat ground at `y = 0`, with Y up. Logs and branches turn
//! onto a stable resting side, as if dropped; see [`Placement`]. Stumps
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
pub use detail::{
    DEFAULT_LOD_SIZE, DeadwoodDetail, DeadwoodLod, LodLevel, default_lod, lod_size, size_factor,
    validate_lod,
};
pub use limb::SizeClass;
pub use mesh::{BillboardVertex, DeadwoodMesh, MeshBuffers, PartRange, Ribbons, Zone};
pub use options::{
    BarkOptions, BranchingOptions, DeadwoodOptions, EndKind, EndsOptions, HollowOptions,
    LeafOptions, MAX_CHILDREN, MAX_DECAY, MAX_END_SHARE, MAX_LEVELS, MAX_LOBES, MAX_ROOTS,
    MAX_SEGMENTS, MAX_SPLINTERS, MAX_SPRAY_CARDS, MAX_SPRAYS_PER_METRE, MAX_STUBS_PER_METRE,
    MaterialOptions, OptionsError, PieceKind, RootOptions, SettleOptions, ShapeOptions,
    StubOptions,
};
pub use piece::{DeadwoodError, Piece};
pub use presets::DeadwoodPreset;
pub use scatter::{
    FlatGround, Ground, HeightGrid, Instance, MAX_INSTANCES, MAX_VARIANTS, ScatterError,
    ScatterOptions, StickScatter,
};
pub use settle::Placement;
pub use specimen::specimen_from_meshes;
pub use texture::{
    BARK_TEXTURE_SETS, END_TEXTURE_SETS, LEAF_TEXTURE_SETS, LeafMap, TextureCompression,
    TextureMap, TextureSize, bark_texture_path, end_texture_path, leaf_texture_path,
};
