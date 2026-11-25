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
//! - [`chain`]: how every asset kind is drawn: its game-level mesh, one
//!   coarser mesh, then an impostor, switched by size on screen and scaled
//!   by the [`Quality`] setting.
//! - [`Lod`]: the older table of levels of detail by distance in metres,
//!   still used by the assets that have not moved to [`chain`] yet.
//! - [`scatter`]: the [`Scatter`](scatter::Scatter) trait, what every
//!   Scatter takes as input, the shared integer placement rules on a
//!   lattice fixed to the World, footprints and masks, and the shared rules
//!   for slope limits and resting rigid pieces on the ground.
//! - [`CopyAddress`]: names one placed copy anywhere in a World, and
//!   [`Address`] a part of one, through pinned [`PartStep`]s.
//! - [`choose_slot`]: which specimen of a pool a placed copy draws.
//! - [`preset`]: named, ready-made options in catalogues, and `.<kind>.yml`
//!   preset files that name a base preset and give only what differs.
//! - [`meta`]: each kind's names, parts and settings metadata, read from
//!   `<kind>.meta.yml` files, for tools that edit presets.
//! - [`texture`]: the tiling texture sets each crate lists in its
//!   `textures.yml`, their sizes and file paths, and a catalogue of
//!   several lists.
//!
//! This crate uses no game engine. Code about one generated specimen's
//! shape belongs in `procgena_specimen`, which builds on this crate.

mod address;
pub mod chain;
mod content_hash;
mod ground;
pub mod hash;
mod lod;
pub mod meta;
pub mod preset;
mod quality;
mod rng;
pub mod scatter;
mod seed;
mod slot;
mod specimen_id;
pub mod texture;

pub use address::{
    Address, AddressPrefix, CellId, CopyAddress, MAX_PART_STEPS, PartStep, RegionId,
};
pub use content_hash::{ContentHash, ContentHasher, OptionsHash, content_hash};
pub use ground::{FlatGround, GRADIENT_STEP, Ground, GroundSample, HeightGrid};
pub use lod::{FarForm, LevelForm, Lod, LodError, LodLevel, LodRange};
pub use quality::Quality;
pub use rng::Rng;
pub use seed::{CHILD_KEY_MAX_LEN, ChildKey, KeyError, child_seed, region_seed};
pub use slot::{MAX_SLOTS, choose_slot};
pub use specimen_id::{GeneratorKind, PoolId, SpecimenId};
