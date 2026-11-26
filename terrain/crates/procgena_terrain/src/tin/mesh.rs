//! The canonical mesh of one chunk: vertices, triangles and the edges they
//! must keep.

use crate::chunk::ChunkKey;
use crate::feature::FeatureId;

use super::numeric::{Cm, CmHeight, NumericProfile};

/// A vertex of a canonical mesh: a world position and the ground height
/// there, in whole centimetres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TinVertex {
    pub x: Cm,
    pub z: Cm,
    pub h: CmHeight,
}

impl TinVertex {
    /// The vertex's position across the ground, x and z.
    pub fn xz(self) -> (Cm, Cm) {
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
    /// A river's water's edge, where its bed meets its banks.
    RiverEdge,
    /// A pond's shore, where its bed meets its banks.
    PondShore,
    /// The lowest line along a river's bed.
    RiverThalweg,
}

impl ConstraintKind {
    /// A stable number for the kind, for hashing.
    pub fn code(self) -> u32 {
        match self {
            Self::ChunkBoundary => 1,
            Self::RiverEdge => 2,
            Self::PondShore => 3,
            Self::RiverThalweg => 4,
        }
    }

    /// Whether the ground creases along edges of this kind, so a drawn
    /// mesh gives them hard shading normals: true for a river's water's
    /// edge and a pond's shore, false for chunk borders and a river's
    /// lowest line, where the ground is smooth.
    pub fn is_breakline(self) -> bool {
        match self {
            Self::RiverEdge | Self::PondShore => true,
            Self::ChunkBoundary | Self::RiverThalweg => false,
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
    pub at: (Cm, Cm),
    /// How far the mesh is from the ground there.
    pub error: CmHeight,
}

/// A straight piece of a feature line that a chunk's mesh leaves out:
/// it crossed a piece that takes precedence where the crossing couldn't be
/// made a vertex, it reached the chunk's border away from its edge
/// contracts' vertices, or the triangulation refused it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DroppedConstraint {
    /// One end, x and z.
    pub from: (Cm, Cm),
    /// The other end, x and z.
    pub to: (Cm, Cm),
    /// What the piece was.
    pub kind: ConstraintKind,
    /// The feature it followed.
    pub feature: FeatureId,
}

/// The canonical mesh of one chunk, at any chunk level: a triangulation of
/// the chunk's square that keeps every edge in `constraints`, with every
/// border edge shared exactly with the same level's neighbour across it.
///
/// Two meshes of the same chunk from the same ground under the same
/// [`NumericProfile`] are equal and hash the same (see
/// [`content_hash`](Self::content_hash)), whatever order they were built
/// in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TinChunk {
    /// The chunk.
    pub key: ChunkKey,
    /// The numeric rules it was built under.
    pub profile: NumericProfile,
    /// The [`content_hash`](super::TinTolerance::content_hash) of the
    /// tolerance it was built within, or `None` for a mesh triangulated
    /// from given vertices (see [`triangulate`](super::triangulate)).
    /// Part of the mesh's hash: the same chunk built within different
    /// tolerances hashes differently.
    pub tolerance: Option<u64>,
    /// Vertices, sorted by z, then x.
    pub vertices: Vec<TinVertex>,
    /// Triangles, each starting at its smallest index, sorted.
    pub triangles: Vec<TinTriangle>,
    /// The edges the mesh keeps, each from its smaller index, sorted.
    pub constraints: Vec<TinConstraint>,
    /// Where the mesh misses its tolerance, if anywhere.
    pub unmet: Vec<UnmetTolerance>,
    /// Feature line pieces the mesh had to leave out, sorted. Empty
    /// unless feature lines cross in ways the mesh can't keep.
    pub dropped: Vec<DroppedConstraint>,
}
