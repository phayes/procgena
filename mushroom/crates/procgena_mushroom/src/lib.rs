//! Procedural mushrooms: single mushrooms, clusters, and mushrooms scattered
//! over the ground or over other generators' surfaces.
//!
//! Describe a mushroom with [`MushroomOptions`], or start from a
//! [`MushroomPreset`] such as a fly agaric, then create a [`Mushroom`]. Its
//! cap and stem are smooth side-view outlines spun around a curved stem, so
//! it can be meshed at any [`MushroomDetail`], from closed, smooth meshes
//! for close-ups to coarse open ones.
//!
//! Each mushroom's [`game_level`](MushroomOptions::game_level) is its real
//! shape. Renderers draw a coarser mesh derived from it when the mushroom is
//! small on screen, then an impostor baked from it, as every procgena asset
//! does (see [`procgena_core::chain`]). [`Mushroom::lod_meshes`] builds the
//! two meshes:
//!
//! ```
//! use procgena_core::Quality;
//! use procgena_core::chain::{DistanceChain, ViewScale};
//! use procgena_mushroom::{Mushroom, MushroomPreset};
//!
//! let mushroom = Mushroom::from_preset(MushroomPreset::porcini()).unwrap();
//! let chain = DistanceChain::DEFAULT;
//! let factor = DistanceChain::factor(mushroom.bounding_diameter(), ViewScale::DEFAULT);
//! let ranges = chain.ranges(factor, Quality::High);
//! for (step, mesh) in mushroom.lod_meshes() {
//!     let until = ranges[step.index()].fade_out.end;
//!     println!("{} until {until} m: {} triangles", step.name(), mesh.triangle_count());
//! }
//! ```
//!
//! A preset can also come from a `.mushroom.yml` file, which names a
//! built-in preset as its `base` and gives only the settings it changes.
//! Colours start with `#`, so quote them in the file:
//!
//! ```
//! use procgena_mushroom::{Mushroom, MushroomPreset};
//!
//! let preset = MushroomPreset::from_yaml(
//!     "pale_porcini",
//!     "base: porcini\n\
//!      mushroom:\n  \
//!        default_seed: 7\n  \
//!        material:\n    \
//!          cap_centre: \"#c09060\"\n",
//! )?;
//! let mushroom = Mushroom::from_preset(&preset)?;
//! assert_eq!(mushroom.options().size, MushroomPreset::porcini().mushroom.size);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! A mushroom can also be a group: with [`ClusterOptions::stems`] above 1,
//! several mushrooms of one species grow together in a tuft, a row or a
//! fairy ring. [`MushroomScatter::new`] spreads whole mushrooms and clusters
//! over a [`procgena_core::Ground`], setting each member on the ground, and
//! [`MushroomScatter::on`] grows them over another generator's
//! [`procgena_specimen::Specimen`], such as a rock. Each gives a list of
//! placed copies of a few different [`MushroomBody`] meshes, so a renderer
//! can mesh each one once and draw it many times.
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
mod ground;
mod mesh;
mod meta;
mod mushroom;
mod options;
mod presets;
mod profile;
mod rng;
mod scatter;
mod shape;
mod specimen;
mod specimen_id;
mod texture;

pub use cluster::{
    ClusterLayout, ClusterOptions, FairyRingOptions, MAX_STEMS, MAX_VARIANTS, MushroomInstance,
};
pub use colour::{linear_from_hex, srgb_to_linear};
pub use detail::{BodyShape, MushroomDetail};
pub use ground::MAX_CELLS;
pub use mesh::{MeshBuffers, MushroomMesh, PartKind, PartRange, Zone};
pub use meta::{MUSHROOM_META, meta_choices};
pub use mushroom::{BodyId, Mushroom, MushroomBody};
pub use options::{
    AnnulusOptions, BulbOptions, CapOptions, MAX_GILL_LINES, MAX_LOBES, MAX_SEGMENTS, MAX_WARTS,
    MaterialOptions, MushroomOptions, OptionsError, Pattern, StemOptions, UndersideOptions,
    WartOptions,
};
pub use presets::MushroomPreset;
pub use procgena_core::preset::PresetFileError;
pub use profile::CapFamily;
pub use scatter::{
    MAX_SITES, MAX_SPECIMENS, MushroomCopy, MushroomScatter, ScatterError, ScatterOptions,
};
pub use specimen::specimen_from_meshes;
pub use specimen_id::GENERATOR_KIND;
pub use texture::{
    CAP_TEXTURE_SETS, STEM_TEXTURE_SETS, TextureCompression, TextureMap, TexturePart, TextureSize,
    stem_texture_average, texture_path,
};
