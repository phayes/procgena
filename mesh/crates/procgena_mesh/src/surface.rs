//! Plain triangle meshes: positions, normals and indices.

use std::fmt;

use glam::Vec3;

/// A borrowed triangle mesh, checked when it is created.
///
/// Positions and normals are in metres and in the mesh's final
/// orientation. Triangles are counter-clockwise when seen from outside, and
/// normals point outward. Generators that grow on a surface, such as
/// clumps, grow on the side the normals point to.
#[derive(Debug, Clone, Copy)]
pub struct Surface<'a> {
    positions: &'a [[f32; 3]],
    normals: &'a [[f32; 3]],
    indices: &'a [u32],
}

impl<'a> Surface<'a> {
    /// Wraps mesh buffers as a surface.
    ///
    /// `normals` must have one entry per position, `indices` must hold whole
    /// triangles, and every index must name a position. Returns an error
    /// otherwise, or if the mesh has no triangles.
    pub fn new(
        positions: &'a [[f32; 3]],
        normals: &'a [[f32; 3]],
        indices: &'a [u32],
    ) -> Result<Self, SurfaceError> {
        // Check buffer shapes.
        if normals.len() != positions.len() {
            return Err(SurfaceError::NormalCount {
                positions: positions.len(),
                normals: normals.len(),
            });
        }
        if !indices.len().is_multiple_of(3) {
            return Err(SurfaceError::PartialTriangle {
                indices: indices.len(),
            });
        }
        if indices.is_empty() {
            return Err(SurfaceError::Empty);
        }

        // Check that every index names a vertex.
        if let Some(&index) = indices.iter().find(|&&i| i as usize >= positions.len()) {
            return Err(SurfaceError::IndexOutOfRange {
                index,
                vertices: positions.len(),
            });
        }

        Ok(Self {
            positions,
            normals,
            indices,
        })
    }

    /// Returns the vertex positions.
    pub fn positions(&self) -> &'a [[f32; 3]] {
        self.positions
    }

    /// Returns the vertex normals, one per position.
    pub fn normals(&self) -> &'a [[f32; 3]] {
        self.normals
    }

    /// Returns the triangle indices, three per triangle.
    pub fn indices(&self) -> &'a [u32] {
        self.indices
    }

    /// Returns the number of triangles.
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Returns the vertex indices of triangle `t`.
    ///
    /// # Panics
    ///
    /// Panics if `t` is not less than [`triangle_count`](Self::triangle_count).
    pub fn triangle(&self, t: usize) -> [usize; 3] {
        let i = &self.indices[t * 3..t * 3 + 3];
        [i[0] as usize, i[1] as usize, i[2] as usize]
    }

    /// Returns the corner positions of triangle `t`.
    ///
    /// # Panics
    ///
    /// Panics if `t` is not less than [`triangle_count`](Self::triangle_count).
    pub fn triangle_positions(&self, t: usize) -> [Vec3; 3] {
        self.triangle(t).map(|v| self.position(v))
    }

    /// Returns the position of vertex `v`.
    ///
    /// # Panics
    ///
    /// Panics if `v` is not less than the number of positions.
    pub fn position(&self, v: usize) -> Vec3 {
        Vec3::from(self.positions[v])
    }

    /// Returns the normal of vertex `v`, normalized. A zero normal stays zero.
    ///
    /// # Panics
    ///
    /// Panics if `v` is not less than the number of positions.
    pub fn normal(&self, v: usize) -> Vec3 {
        Vec3::from(self.normals[v]).normalize_or_zero()
    }

    /// Returns whether the mesh is closed: every edge, ignoring direction, is
    /// shared by exactly two triangles. A closed mesh encloses one or more
    /// solids.
    ///
    /// Vertices at exactly the same position count as one vertex, so a mesh
    /// that repeats positions at texture seams or between joined parts (as
    /// procedural tree bark does) is still recognized as closed.
    pub fn is_closed(&self) -> bool {
        self.is_closed_welded(&self.welded().0)
    }

    /// Same as [`is_closed`](Self::is_closed), for a caller that already has
    /// `welded`, the per-vertex position ids from [`welded`](Self::welded).
    ///
    /// # Panics
    ///
    /// Panics if `welded` has fewer entries than there are positions.
    pub fn is_closed_welded(&self, welded: &[u32]) -> bool {
        // Sort the welded edges so copies sit together, then check that each
        // appears exactly twice.
        let mut edges: Vec<u64> = Vec::with_capacity(self.indices.len());
        for &tri in self.indices.as_chunks::<3>().0 {
            let [a, b, c] = tri.map(|i| welded[i as usize]);
            for (x, y) in [(a, b), (b, c), (c, a)] {
                edges.push((u64::from(x.min(y)) << 32) | u64::from(x.max(y)));
            }
        }
        edges.sort_unstable();
        edges.chunk_by(|x, y| x == y).all(|run| run.len() == 2)
    }

    /// Gives every distinct position one id: returns, per vertex, the id of
    /// its position, and the number of distinct positions. Ids count up from
    /// 0 in order of first appearance. Positions are compared bit for bit,
    /// except that -0.0 equals 0.0.
    pub fn welded(&self) -> (Vec<u32>, usize) {
        let mut ids = std::collections::HashMap::<[u32; 3], u32>::new();
        let welded = self
            .positions
            .iter()
            .map(|p| {
                // Adding 0.0 turns -0.0 into 0.0, so the two compare equal.
                let key = p.map(|c| (c + 0.0).to_bits());
                let next = ids.len() as u32;
                *ids.entry(key).or_insert(next)
            })
            .collect();
        (welded, ids.len())
    }

    /// Returns the smallest box that holds every vertex, as `(min, max)`.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        self.positions.iter().fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(lo, hi), &p| (lo.min(Vec3::from(p)), hi.max(Vec3::from(p))),
        )
    }
}

/// An owned triangle mesh that can be borrowed as a [`Surface`].
///
/// It holds the geometry of each [`Part`](crate::Part), and generated meshes
/// such as [`SurfaceMesh::ground`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SurfaceMesh {
    /// Vertex positions in metres.
    pub positions: Vec<[f32; 3]>,
    /// Outward vertex normals, one per position.
    pub normals: Vec<[f32; 3]>,
    /// Counter-clockwise triangle indices, three per triangle.
    pub indices: Vec<u32>,
}

impl SurfaceMesh {
    /// Creates a flat, square ground patch at height 0 facing +Y.
    ///
    /// `size` is the side length in metres and `divisions` the number of
    /// cells along each side. The patch is centred on the origin.
    ///
    /// # Panics
    ///
    /// Panics if `divisions` is zero.
    pub fn ground(size: f32, divisions: u32) -> Self {
        assert!(divisions > 0, "a ground patch needs at least one division");
        let n = divisions + 1;
        let mut mesh = Self::default();

        // Lay out vertices row by row along +Z.
        for j in 0..n {
            for i in 0..n {
                let x = (i as f32 / divisions as f32 - 0.5) * size;
                let z = (j as f32 / divisions as f32 - 0.5) * size;
                mesh.positions.push([x, 0.0, z]);
                mesh.normals.push([0.0, 1.0, 0.0]);
            }
        }

        // Two counter-clockwise triangles per cell, seen from above.
        for j in 0..divisions {
            for i in 0..divisions {
                let a = j * n + i;
                let b = a + 1;
                let c = a + n;
                let d = c + 1;
                mesh.indices.extend_from_slice(&[a, c, b, b, c, d]);
            }
        }
        mesh
    }

    /// Borrows the mesh as a [`Surface`], checking it as [`Surface::new`] does.
    pub fn surface(&self) -> Result<Surface<'_>, SurfaceError> {
        Surface::new(&self.positions, &self.normals, &self.indices)
    }
}

/// Why mesh buffers can't be used as a [`Surface`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SurfaceError {
    /// The mesh has no triangles.
    Empty,
    /// The number of normals differs from the number of positions.
    NormalCount { positions: usize, normals: usize },
    /// The index count is not a multiple of three.
    PartialTriangle { indices: usize },
    /// A triangle refers to a vertex that doesn't exist.
    IndexOutOfRange { index: u32, vertices: usize },
}

impl fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "the surface has no triangles"),
            Self::NormalCount { positions, normals } => {
                write!(
                    f,
                    "the surface has {positions} positions but {normals} normals"
                )
            }
            Self::PartialTriangle { indices } => {
                write!(
                    f,
                    "the surface has {indices} indices, which is not a multiple of 3"
                )
            }
            Self::IndexOutOfRange { index, vertices } => {
                write!(f, "index {index} is out of range for {vertices} vertices")
            }
        }
    }
}

impl std::error::Error for SurfaceError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ground_is_a_valid_upward_grid() {
        let ground = SurfaceMesh::ground(2.0, 4);
        let surface = ground.surface().unwrap();
        assert_eq!(surface.positions().len(), 25);
        assert_eq!(surface.triangle_count(), 32);

        // Every triangle faces +Y.
        for t in 0..surface.triangle_count() {
            let [a, b, c] = surface.triangle_positions(t);
            assert!((b - a).cross(c - a).y > 0.0);
        }
        let (lo, hi) = surface.bounds();
        assert_eq!(lo, Vec3::new(-1.0, 0.0, -1.0));
        assert_eq!(hi, Vec3::new(1.0, 0.0, 1.0));
    }

    #[test]
    fn closedness_counts_repeated_positions_as_one_vertex() {
        let normals = [[0.0, 1.0, 0.0]; 7];

        // An open patch is not closed.
        assert!(!SurfaceMesh::ground(1.0, 2).surface().unwrap().is_closed());

        // A tetrahedron with shared corners is closed.
        let corners = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        let faces = [0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3];
        let shared = Surface::new(&corners, &normals[..4], &faces).unwrap();
        assert!(shared.is_closed());

        // The same tetrahedron with its last face on its own copies of the
        // corners, one written as -0.0, as at a texture seam, is still closed.
        let mut seamed = corners.to_vec();
        seamed.extend([[1.0, -0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
        let faces_seamed = [0, 2, 1, 0, 1, 3, 0, 3, 2, 4, 5, 6];
        let seamed = Surface::new(&seamed, &normals, &faces_seamed).unwrap();
        assert!(seamed.is_closed());

        // Removing a face opens it.
        let open = Surface::new(&corners, &normals[..4], &faces[..9]).unwrap();
        assert!(!open.is_closed());
    }

    #[test]
    fn bad_buffers_are_rejected() {
        let p = [[0.0; 3]; 3];
        let n = [[0.0, 1.0, 0.0]; 3];
        assert_eq!(Surface::new(&p, &n, &[]).unwrap_err(), SurfaceError::Empty);
        assert_eq!(
            Surface::new(&p, &n[..2], &[0, 1, 2]).unwrap_err(),
            SurfaceError::NormalCount {
                positions: 3,
                normals: 2
            }
        );
        assert_eq!(
            Surface::new(&p, &n, &[0, 1]).unwrap_err(),
            SurfaceError::PartialTriangle { indices: 2 }
        );
        assert_eq!(
            Surface::new(&p, &n, &[0, 1, 3]).unwrap_err(),
            SurfaceError::IndexOutOfRange {
                index: 3,
                vertices: 3
            }
        );
        assert!(Surface::new(&p, &n, &[0, 1, 2]).is_ok());
    }
}
