//! Procedural mushrooms: single mushrooms, clusters, and mushrooms scattered
//! over other generators' surfaces, each at several levels of detail.
//!
//! Describe a mushroom with [`MushroomOptions`], or start from a
//! [`MushroomPreset`] such as a fly agaric, then create a [`Mushroom`]. Its
//! cap and stem are smooth side-view outlines spun around a curved stem, so
//! it can be meshed at any [`MushroomDetail`]: from closed, smooth meshes for
//! close-ups down to two triangles that turn toward the camera, for drawing
//! thousands at once.
//!
//! Distant objects don't need fine detail, so renderers swap in simpler
//! meshes as the camera moves away; each of those is a "level of detail".
//! [`MushroomLod`] says which detail to use at which distance, and
//! [`MushroomLod::for_size`] scales the distances to suit the mushroom's
//! size.
//!
//! ```
//! use procgena_mushroom::{Mushroom, MushroomLod, MushroomPreset};
//!
//! let mushroom = Mushroom::from_preset(MushroomPreset::Porcini);
//! let lod = MushroomLod::for_size(mushroom.options().size);
//! for (level, mesh) in mushroom.lod_meshes(&lod).unwrap() {
//!     println!("from {} m: {} triangles", level.distance, mesh.triangle_count());
//! }
//! ```
//!
//! A mushroom can also be a group: with [`ClusterOptions::stems`] above 1,
//! several mushrooms of one species grow together in a tuft, a row or a
//! fairy ring. [`MushroomScatter`] spreads mushrooms and clusters over
//! another generator's [`procgena_mesh::Object`], such as a rock. Both give
//! a list of placed copies of a few different [`MushroomBody`] meshes, so a
//! renderer can mesh each one once and draw it many times.
//!
//! With the `export-obj` and `export-gltf` features, [`export`] writes
//! meshes as files for 3D tools.
//!
//! All lengths are in metres, Y is up, and a mushroom stands on the ground
//! at the origin. A closed stem bottom hangs a little below it.
//!
//! Cap outline families follow Infinigen's mushroom generator (BSD-3-Clause,
//! Princeton University).

mod cluster;
mod colour;
mod detail;
#[cfg(any(feature = "export-obj", feature = "export-gltf"))]
pub mod export;
mod mesh;
mod mushroom;
mod object;
mod options;
mod presets;
mod profile;
mod rng;
mod scatter;
mod shape;
mod texture;

pub use cluster::{
    ClusterLayout, ClusterOptions, FairyRingOptions, MAX_STEMS, MAX_VARIANTS, MushroomInstance,
};
pub use colour::{linear_from_hex, srgb_to_linear};
pub use detail::{BodyShape, DEFAULT_LOD_SIZE, LodLevel, MushroomDetail, MushroomLod};
pub use mesh::{
    BillboardMesh, BillboardVertex, MeshBuffers, MushroomMesh, PartKind, PartRange, Zone,
};
pub use mushroom::{Mushroom, MushroomBody, MushroomError};
pub use object::{object_from_meshes, tags};
pub use options::{
    AnnulusOptions, BulbOptions, CapOptions, MAX_GILL_LINES, MAX_LOBES, MAX_SEGMENTS, MAX_WARTS,
    MaterialOptions, MushroomOptions, OptionsError, Pattern, StemOptions, UndersideOptions,
    WartOptions,
};
pub use presets::MushroomPreset;
pub use profile::CapFamily;
pub use scatter::{MAX_SITES, MushroomScatter, ScatterError, ScatterOptions};
pub use texture::{
    CAP_TEXTURE_SETS, STEM_TEXTURE_SETS, TextureCompression, TextureMap, TexturePart, TextureSize,
    stem_texture_average, texture_path,
};
