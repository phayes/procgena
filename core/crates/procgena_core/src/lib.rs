//! The engine-free rules that every procgena level shares.
//!
//! - [`hash`]: repeatable pseudo-random integers and floats from integer
//!   inputs, matching the generators' GPU shaders.
//! - [`Rng`]: a small seeded stream of random numbers.
//! - [`Ground`]: the shape of the land things are scattered on, with its
//!   height and slope at any World position. [`FlatGround`], closures and
//!   [`HeightGrid`] are ready-made grounds.
//! - [`Lod`]: an asset's levels of detail, scaled by each copy's size and
//!   by the [`Quality`] setting.
//!
//! This crate depends on glam alone, plus serde with the `serde` feature,
//! which lets the level-of-detail and quality types be saved and loaded. Code about one generated specimen's
//! shape belongs in `procgena_specimen`, which builds on this crate.

mod ground;
pub mod hash;
mod lod;
mod quality;
mod rng;

pub use ground::{FlatGround, GRADIENT_STEP, Ground, GroundSample, HeightGrid};
pub use lod::{FarForm, LevelForm, Lod, LodError, LodLevel, LodRange};
pub use quality::Quality;
pub use rng::Rng;
