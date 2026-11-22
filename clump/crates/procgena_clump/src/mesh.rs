//! The triangle mesh produced for a clump.

/// A clump's triangle mesh, drawn on top of the input surface.
///
/// All attribute buffers have one entry per position. Triangles are
/// counter-clockwise seen from outside, and normals point outward. The mesh
/// has no texture coordinates; use triplanar mapping.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClumpMesh {
    /// Vertex positions in world units.
    pub positions: Vec<[f32; 3]>,
    /// Outward unit normals.
    pub normals: Vec<[f32; 3]>,
    /// How fully covered each vertex is, from 0 at the clump's edge to 1 at
    /// full thickness. Use it to blend the clump's material into the surface
    /// underneath.
    pub fade: Vec<f32>,
    /// Triangle indices, three per triangle.
    pub indices: Vec<u32>,
}

impl ClumpMesh {
    /// Returns the number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Returns the number of triangles.
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Returns true if the clump covers nothing.
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

/// Measurements taken while building a clump, for tuning settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BuildStats {
    /// Top triangles left out of the mesh because they stayed turned over,
    /// inside out, even after the clump around them was lowered as far as it
    /// goes. These are tiny slivers, and each leaves a pinhole that a closed
    /// clump walls off. The mesh itself never has a turned-over top triangle.
    pub folded: usize,
    /// Vertices kept thinner than the settings ask for, because another part
    /// of the surface faces them across a narrow gap, or because the input
    /// under them is thin.
    pub thinned: usize,
    /// Pairs of triangles that still pass through each other, when
    /// [`MeshOptions::strict`](crate::MeshOptions::strict) is on. Always 0
    /// when it is off, because the check doesn't run.
    pub crossings: usize,
}
