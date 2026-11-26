//! The canonical mesh of one chunk: vertices, triangles and the edges they
//! must keep.

use crate::chunk::ChunkKey;
use crate::feature::FeatureId;

use super::numeric::{Mm, MmHeight, NumericProfile};

/// A vertex of a canonical mesh: a world position and the ground height
/// there, in whole millimetres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TinVertex {
    pub x: Mm,
    pub z: Mm,
    pub h: MmHeight,
}

impl TinVertex {
    /// The vertex's position across the ground, x and z.
    pub fn xz(self) -> (Mm, Mm) {
        (self.x, self.z)
    }
}

/// A triangle: three indices into its mesh's vertices, wound so that it
/// faces up (counter-clockwise seen from above, like
/// [`ChunkMesh`](crate::ChunkMesh)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TinTriangle(pub [u32; 3]);

/// Why an edge must be in the mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ConstraintKind {
    /// A piece of the chunk's own border, shared with the neighbour across
    /// it.
    ChunkBoundary,
}

impl ConstraintKind {
    /// A stable number for the kind, for hashing.
    pub fn code(self) -> u32 {
        match self {
            Self::ChunkBoundary => 1,
        }
    }
}

/// An edge the mesh must keep: two indices into its vertices, why, and the
/// feature it follows, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TinConstraint {
    pub v: [u32; 2],
    pub kind: ConstraintKind,
    pub feature: Option<FeatureId>,
}

/// A place where the mesh is further from the ground than its tolerance
/// allows, because the chunk ran out of vertices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnmetTolerance {
    /// Where the error is largest, x and z.
    pub at: (Mm, Mm),
    /// How far the mesh is from the ground there.
    pub error: MmHeight,
}

/// The canonical mesh of one finest-level chunk: a triangulation of the
/// chunk's square that keeps every edge in `constraints`, with every
/// border edge shared exactly with the neighbour across it.
///
/// Two meshes of the same chunk from the same ground under the same
/// [`NumericProfile`] are equal and hash the same (see
/// [`content_hash`](Self::content_hash)), whatever order they were built
/// in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TinChunk {
    /// The chunk, at level 0.
    pub key: ChunkKey,
    /// The numeric rules it was built under.
    pub profile: NumericProfile,
    /// Vertices, sorted by z, then x.
    pub vertices: Vec<TinVertex>,
    /// Triangles, each starting at its smallest index, sorted.
    pub triangles: Vec<TinTriangle>,
    /// The edges the mesh keeps, each from its smaller index, sorted.
    pub constraints: Vec<TinConstraint>,
    /// Where the mesh misses its tolerance, if anywhere.
    pub unmet: Vec<UnmetTolerance>,
}
