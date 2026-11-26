//! Canonical ground: a triangle mesh of each finest-level chunk that
//! follows the terrain's ground within a tolerance, and that every build
//! makes the same, whatever order chunks are built in.
//!
//! Positions and heights are whole centimetres (see [`Cm`] and
//! [`CmHeight`]). The ground is sampled at the
//! [`Fine`](crate::FeatureTier::Fine) tier through a
//! [`ChunkSource`](crate::ChunkSource). Neighbouring chunks share the
//! vertices along their common edge (see [`EdgeContract`]), so they meet
//! exactly. Placement, masks and labels keep reading the terrain's own
//! ground; this mesh is for collision and for drawing near ground.
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
