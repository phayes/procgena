//! Canonical ground: a triangle mesh of each finest-level chunk that
//! follows the terrain's ground within a tolerance, and that every build
//! makes the same, whatever order chunks are built in.
//!
//! Positions and heights are whole millimetres (see [`Mm`] and
//! [`MmHeight`]). The ground is sampled at the
//! [`Fine`](crate::FeatureTier::Fine) tier through a
//! [`ChunkSource`](crate::ChunkSource). Neighbouring chunks share the
//! vertices along their common edge (see [`EdgeContract`]), so they meet
//! exactly. Placement, masks and labels keep reading the terrain's own
//! ground; this mesh is for collision and for drawing near ground.
//!
//! [`build_chunk`] builds one chunk's mesh.

mod cdt;
mod contract;
mod hash;
mod mesh;
mod numeric;
mod probe;
mod refine;
mod resolve;
mod tolerance;
mod water;

pub use cdt::triangulate;
pub use contract::{ChunkEdgeKey, EdgeAxis, EdgeContract, EdgeCrossing};
pub use hash::probes_hash;
pub use mesh::{
    ConstraintKind, DroppedConstraint, TinChunk, TinConstraint, TinTriangle, TinVertex,
    UnmetTolerance,
};
pub use numeric::{MAX_MM, MM_PER_M, Mm, MmHeight, NumericProfile};
pub use probe::{ProbeAddr, TinError, sample_heights};
pub use refine::build_chunk;
pub use tolerance::{DomainTolerance, TinTolerance};
