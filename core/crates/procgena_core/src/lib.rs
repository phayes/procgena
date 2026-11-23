//! The engine-free rules that every procgena level shares.
//!
//! - [`hash`]: repeatable pseudo-random integers and floats from integer
//!   inputs, matching the generators' GPU shaders.
//! - [`Rng`]: a small seeded stream of random numbers.
//! - [`Ground`]: the shape of the land things are scattered on, with its
//!   height and slope at any World position. [`FlatGround`], closures and
//!   [`HeightGrid`] are ready-made grounds.
//! - [`scatter`]: what every Scatter takes as input, and the shared rules
//!   for slope limits and resting rigid pieces on the ground.
//!
//! This crate depends on glam alone. Code about one generated specimen's
//! shape belongs in `procgena_specimen`, which builds on this crate.

mod ground;
pub mod hash;
mod rng;
pub mod scatter;

pub use ground::{FlatGround, GRADIENT_STEP, Ground, GroundSample, HeightGrid};
pub use rng::Rng;
