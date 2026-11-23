//! The engine-free rules that every procgena level shares.
//!
//! - [`hash`]: repeatable pseudo-random integers and floats from integer
//!   inputs, matching the generators' GPU shaders, and seeds derived from
//!   named seed keys.
//! - [`Rng`]: a small seeded stream of random numbers.
//! - [`ContentHash`]: a stable hash of an options type, the same in every
//!   run and on every platform. [`OptionsHash`] holds one asset's.
//! - [`SpecimenId`]: names one generated specimen, from its World seed, its
//!   [`PoolId`] and its slot, and gives the seed it is built from.
//! - [`child_seed`]: the seed of a child, named by a [`ChildKey`], from its
//!   parent's seed.
//! - [`Ground`]: the shape of the land things are scattered on, with its
//!   height and slope at any World position. [`FlatGround`], closures and
//!   [`HeightGrid`] are ready-made grounds.
//!
//! This crate depends on glam alone. Code about one generated specimen's
//! shape belongs in `procgena_specimen`, which builds on this crate.

mod content_hash;
mod ground;
pub mod hash;
mod rng;
mod seed;
mod specimen_id;

pub use content_hash::{ContentHash, ContentHasher, OptionsHash, content_hash};
pub use ground::{FlatGround, GRADIENT_STEP, Ground, GroundSample, HeightGrid};
pub use rng::Rng;
pub use seed::{CHILD_KEY_MAX_LEN, ChildKey, KeyError, child_seed};
pub use specimen_id::{GeneratorKind, PoolId, SpecimenId};
