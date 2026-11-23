//! The engine-free rules that every procgena level shares.
//!
//! - [`hash`]: repeatable pseudo-random integers and floats from integer
//!   inputs, matching the generators' GPU shaders.
//! - [`Rng`]: a small seeded stream of random numbers.
//!
//! This crate depends on glam alone. Code about one generated specimen's
//! shape belongs in `procgena_specimen`, which builds on this crate.

pub mod hash;
mod rng;

pub use rng::Rng;
