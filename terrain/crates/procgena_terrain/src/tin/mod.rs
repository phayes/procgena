//! Canonical ground: a triangle mesh of each chunk, at any chunk level,
//! that follows the terrain's ground within a tolerance, and that every
//! build makes the same, whatever order chunks are built in. A chunk at
//! level `L` is 64 m × 2^L wide, samples a lattice 2^L metres apart, and
//! follows the ground within the world's tolerance scaled to its level
//! ([`TinTolerance::at_level`]).
//!
//! Positions and heights are whole centimetres (see [`Cm`] and
//! [`CmHeight`]). The ground is sampled with every feature
//! ([`FeatureLevel::FINEST`](crate::FeatureLevel::FINEST)) through a
//! [`ChunkSource`](crate::ChunkSource). Neighbouring chunks of one level
//! share the vertices along their common edge (see [`EdgeContract`]), so
//! they meet exactly; chunks of different levels share nothing.
//!
//! [`build_chunk`] builds one chunk's mesh.

mod cache;
mod cdt;
mod contract;
mod hash;
mod mesh;
mod numeric;
mod probe;
mod product;
mod refine;
mod resolve;
mod sampling;
mod surface;
mod tolerance;
mod water;

pub use cache::ContractCache;
pub use cdt::triangulate;
pub use contract::{ChunkEdgeKey, EdgeAxis, EdgeContract, EdgeCrossing};
pub use hash::probes_hash;
pub use mesh::{
    ConstraintKind, DroppedConstraint, TinChunk, TinConstraint, TinTriangle, TinVertex,
    UnmetTolerance,
};
pub use numeric::{CM_PER_M, Cm, CmHeight, MAX_CM, NumericProfile};
pub use probe::{ProbeAddr, TinError, sample_heights};
pub use product::TinCollider;
pub use refine::{build_chunk, build_chunk_with};
pub use surface::{GroundSample, GroundSurface, ResidentTins, TinIndex};
pub use tolerance::{DomainTolerance, TinSampling, TinTolerance};
