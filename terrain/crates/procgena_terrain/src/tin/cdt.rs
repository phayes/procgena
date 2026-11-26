//! Triangulating one chunk against its four edge contracts.

use spade::handles::FixedVertexHandle;
use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};

use crate::chunk::ChunkKey;

use super::contract::{ChunkEdgeKey, EdgeContract};
use super::mesh::{
    ConstraintKind, TinChunk, TinConstraint, TinTriangle, TinVertex, UnmetTolerance,
};
use super::numeric::NumericProfile;

/// A constrained Delaunay triangulation of one finest-level chunk, with its
/// border locked to the chunk's edge contracts. Positions are kept in
/// millimetres from the chunk's smallest corner, which `f64` holds
/// exactly.
pub(crate) struct ChunkCdt {
    key: ChunkKey,
    /// The chunk's smallest corner, in millimetres.
    origin: (i64, i64),
    cdt: ConstrainedDelaunayTriangulation<Point2<f64>>,
    /// The vertices, in the order they were inserted (spade's handle
    /// indices).
    vertices: Vec<TinVertex>,
    /// The border's vertex indices, round the chunk from the smallest
    /// corner towards +x first.
    border: Vec<u32>,
}

impl ChunkCdt {
    /// The chunk `key` with only its border: the vertices of `contracts`
    /// (its edges, in [`ChunkEdgeKey::of_chunk`] order), joined by
    /// constraint edges.
    ///
    /// # Panics
    ///
    /// If `contracts` are not the chunk's four edges, or don't meet at its
    /// corners.
    pub(crate) fn new(key: ChunkKey, contracts: [&EdgeContract; 4]) -> Self {
        let edges = ChunkEdgeKey::of_chunk(key);
        for (contract, edge) in contracts.iter().zip(edges) {
            assert_eq!(
                contract.key, edge,
                "contracts must be the chunk's edges, in order"
            );
        }
        let origin = edges[0].start();

        // The border round the chunk: the low-z and high-x edges forward,
        // the high-z and low-x edges backward, each without its last
        // corner.
        let mut ring: Vec<TinVertex> = Vec::new();
        let mut end: Option<TinVertex> = None;
        for (i, contract) in contracts.iter().enumerate() {
            let vertices = &contract.vertices;
            let forward: Vec<TinVertex> = if i < 2 {
                vertices.clone()
            } else {
                vertices.iter().rev().copied().collect()
            };
            if let Some(end) = end {
                assert_eq!(
                    end, forward[0],
                    "contracts must meet at the chunk's corners"
                );
            }
            end = forward.last().copied();
            ring.extend(&forward[..forward.len() - 1]);
        }
        assert_eq!(
            Some(ring[0]),
            end,
            "contracts must meet at the chunk's corners"
        );

        // Insert the border in canonical order (by z, then x), then lock
        // each border edge.
        let mut chunk = Self {
            key,
            origin: (origin.0.0, origin.1.0),
            cdt: ConstrainedDelaunayTriangulation::new(),
            vertices: Vec::with_capacity(ring.len()),
            border: Vec::with_capacity(ring.len()),
        };
        let mut sorted = ring.clone();
        sorted.sort_by_key(|v| (v.z, v.x));
        for v in sorted {
            chunk.insert(v);
        }
        chunk.border = ring
            .iter()
            .map(|v| chunk.index_of(*v).expect("border vertex inserted"))
            .collect();
        for k in 0..chunk.border.len() {
            let (a, b) = (chunk.border[k], chunk.border[(k + 1) % chunk.border.len()]);
            chunk.cdt.add_constraint(handle(a), handle(b));
        }
        chunk
    }

    /// Adds `vertex` inside the chunk, keeping the border, and returns its
    /// index.
    ///
    /// # Panics
    ///
    /// If `vertex` is already in the mesh, or spade can't insert it.
    pub(crate) fn insert(&mut self, vertex: TinVertex) -> u32 {
        let handle = self
            .cdt
            .insert(self.local(vertex))
            .expect("chunk-local millimetres are always valid positions");
        let index = handle.index() as u32;
        assert_eq!(
            index as usize,
            self.vertices.len(),
            "vertex {vertex:?} is already in the mesh"
        );
        self.vertices.push(vertex);
        index
    }

    /// The vertices, by index.
    pub(crate) fn vertices(&self) -> &[TinVertex] {
        &self.vertices
    }

    /// Each triangle's vertex indices, anticlockwise in (x, z) (spade's
    /// winding, which faces down), in spade's order.
    pub(crate) fn faces(&self) -> impl Iterator<Item = [u32; 3]> + '_ {
        self.cdt
            .inner_faces()
            .map(|face| face.vertices().map(|v| v.fix().index() as u32))
    }

    /// The triangles around vertex `index`, anticlockwise in (x, z), as
    /// [`faces`](Self::faces) gives them.
    pub(crate) fn faces_around(&self, index: u32) -> Vec<[u32; 3]> {
        self.cdt
            .vertex(handle(index))
            .out_edges()
            .filter_map(|edge| edge.face().as_inner())
            .map(|face| face.vertices().map(|v| v.fix().index() as u32))
            .collect()
    }

    /// Whether the triangle `face` (anticlockwise in (x, z)) is in the
    /// mesh.
    pub(crate) fn has_face(&self, face: [u32; 3]) -> bool {
        self.cdt
            .get_edge_from_neighbors(handle(face[0]), handle(face[1]))
            .and_then(|edge| edge.face().as_inner())
            .is_some_and(|f| {
                f.vertices()
                    .iter()
                    .any(|v| v.fix().index() as u32 == face[2])
            })
    }

    /// Each triangle's vertex indices, facing up, in spade's order.
    pub(crate) fn triangles(&self) -> impl Iterator<Item = [u32; 3]> + '_ {
        self.cdt.inner_faces().map(|face| {
            let [a, b, c] = face.vertices().map(|v| v.fix().index() as u32);
            // Spade winds faces anticlockwise in (x, z), which faces down.
            [a, c, b]
        })
    }

    /// The mesh as a canonical [`TinChunk`], with `unmet` recorded.
    ///
    /// # Panics
    ///
    /// If a border edge has been lost (which spade's constraints prevent).
    pub(crate) fn to_chunk(&self, unmet: Vec<UnmetTolerance>) -> TinChunk {
        let n = self.border.len();
        let constraints = (0..n)
            .map(|k| {
                let (a, b) = (self.border[k], self.border[(k + 1) % n]);
                assert!(
                    self.cdt.exists_constraint(handle(a), handle(b)),
                    "border edge {a}–{b} of chunk {:?} is lost",
                    self.key
                );
                TinConstraint {
                    v: [a, b],
                    kind: ConstraintKind::ChunkBoundary,
                    feature: None,
                }
            })
            .collect();
        TinChunk {
            key: self.key,
            profile: NumericProfile::CURRENT,
            vertices: self.vertices.clone(),
            triangles: self.triangles().map(TinTriangle).collect(),
            constraints,
            unmet,
        }
        .normalized()
    }

    /// The index of the vertex at `vertex`'s position, if any.
    fn index_of(&self, vertex: TinVertex) -> Option<u32> {
        self.vertices
            .iter()
            .position(|v| v.xz() == vertex.xz())
            .map(|i| i as u32)
    }

    /// `vertex`'s position in millimetres from the chunk's smallest corner.
    pub(crate) fn local(&self, vertex: TinVertex) -> Point2<f64> {
        Point2::new(
            (vertex.x.0 - self.origin.0) as f64,
            (vertex.z.0 - self.origin.1) as f64,
        )
    }
}

/// Spade's handle for vertex index `i`.
fn handle(i: u32) -> FixedVertexHandle {
    FixedVertexHandle::from_index(i as usize)
}

/// The canonical mesh of finest-level chunk `key` with its border locked to
/// `contracts` (its four edges, in [`ChunkEdgeKey::of_chunk`] order) and
/// `interior` vertices added in canonical order (by z, then x).
///
/// # Panics
///
/// If `contracts` are not the chunk's edges or don't meet at its corners,
/// or an interior vertex is not strictly inside the chunk or repeats
/// another vertex's position.
pub fn triangulate(
    key: ChunkKey,
    contracts: [&EdgeContract; 4],
    interior: &[TinVertex],
) -> TinChunk {
    let mut chunk = ChunkCdt::new(key, contracts);
    let size = ChunkKey::size_at(0) * super::numeric::MM_PER_M;
    let mut sorted = interior.to_vec();
    sorted.sort_by_key(|v| (v.z, v.x));
    for v in sorted {
        let p = chunk.local(v);
        assert!(
            p.x > 0.0 && p.x < size && p.y > 0.0 && p.y < size,
            "interior vertex {v:?} is not inside chunk {key:?}"
        );
        chunk.insert(v);
    }
    chunk.to_chunk(Vec::new())
}
