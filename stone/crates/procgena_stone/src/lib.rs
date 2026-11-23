//! # procgena_stone
//!
//! Generate repeatable rock shapes and triangle meshes from a seed and settings.
//! This crate is a Rust port of
//! [Unity-Procedural-Rock-Generation](https://github.com/przemyslawzaworski/Unity-Procedural-Rock-Generation).
//!
//! Create a [`Stone`] for a mesh in metres, ready to use in your application.
//! Choose a [`Mesher`] and a [`StoneDetail`] (a preset or your own levels) to
//! balance surface detail and generation cost.
//! For direct shape queries, use [`StoneField`]: its values are negative inside
//! the rock, zero on the surface, and positive outside. Field coordinates use a
//! unit-width cube, before the world scale is applied.
//!
//! Supply only the settings you want to change; omitted fields use defaults:
//!
//! ```
//! use procgena_stone::Stone;
//!
//! let stone = Stone::from_json(
//!     r#"{"seed": 7, "mesh": {"detail": [
//!         {"resolution": 32, "distance": 0.0, "hysteresis": 0.0, "texture_size": "1k"}
//!     ]}}"#,
//! )?;
//! let mesh = stone.mesh();
//! assert!(mesh.triangle_count() > 0);
//! assert_eq!(mesh.positions.len(), mesh.normals.len());
//! # Ok::<(), procgena_stone::StoneError>(())
//! ```
//!
//! The default `rayon` feature enables generation across multiple CPU threads.
//! Disable default features for generation on the calling thread with the same
//! mesh output.
//!
//! Enable an optional Cargo feature to turn meshes into 3D file contents:
//!
//! - `export-obj`: OBJ geometry and a companion MTL material description.
//! - `export-gltf`: a glTF scene (`.gltf` plus `.bin`) or binary GLB (`.glb`).
//!
//! Export functions return strings or bytes for your application to save. Images
//! are optional and must be supplied separately; glTF can also embed them.
//!
//! With `export-gltf` enabled, create GLB bytes containing a rock at several
//! levels of detail (simpler meshes for distant viewing):
//!
//! ```
//! # #[cfg(feature = "export-gltf")]
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use procgena_stone::export::gltf::{GltfOptions, to_glb};
//! use procgena_stone::{Stone, StoneDetail, StoneOptions};
//!
//! let mut options = StoneOptions::default();
//! options.mesh.detail = StoneDetail::LOW;
//! let stone = Stone::new(options)?;
//! let meshes: Vec<_> = stone.levels().map(|(_, mesh)| mesh.clone()).collect();
//! let glb = to_glb(&meshes, stone.options(), &GltfOptions::default())?;
//! assert_eq!(&glb[..4], b"glTF");
//! # Ok(())
//! # }
//! # #[cfg(not(feature = "export-gltf"))]
//! # fn main() {}
//! ```
//!
//! The example above creates bytes in memory. Each detail level is included as
//! a scene node; the receiving tool must select which one to show.
//!
//! To write sample OBJ, MTL, and GLB files to `target/procgena_stone_export` in this
//! workspace, run:
//!
//! ```text
//! cargo run -p procgena_stone --example export --features export-obj,export-gltf
//! ```

mod detail;
#[cfg(any(feature = "export-obj", feature = "export-gltf"))]
pub mod export;
mod field;
#[cfg(feature = "gpu")]
pub mod gpu;
mod grid;
mod material;
mod mesh;
mod noise;
mod options;
mod parallel;
mod placement;
mod presets;
pub mod shaders;
mod stone;

pub use detail::{DetailLevel, StoneDetail};
pub use field::{CarveSet, CarveSphere, StoneField};
pub use material::{
    ImageMap, TEXTURE_SETS, TextureCompression, TextureMap, TextureSize, image_path, texture_path,
};
pub use mesh::{StoneMesh, mesh_field};
pub use options::{
    CarveOptions, MAX_CARVE_STEPS, MAX_DETAIL_LEVELS, MAX_OCTAVES, MAX_RESOLUTION, MIN_RESOLUTION,
    MaterialOptions, MeshOptions, Mesher, OptionsError, PlacementOptions, StoneOptions,
    SurfaceOptions,
};
pub use placement::StonePlacement;
pub use presets::StonePreset;
pub use stone::{Stone, StoneError};
