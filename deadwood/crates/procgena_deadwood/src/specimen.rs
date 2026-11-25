//! Conversion to [`procgena_specimen::Specimen`], the shared format procgena
//! generators use to hand meshes to each other, so that, for example, moss
//! from `procgena_clump` can grow on a log.

use procgena_core::chain::{DistanceChain, Step, ViewScale};
use procgena_specimen::{Level, Part, Specimen, SurfaceMesh, TagSet, tags};

use crate::limb::SizeClass;
use crate::mesh::{DeadwoodMesh, MeshBuffers, PartRange};
use crate::piece::Piece;

impl SizeClass {
    /// Returns the tag for this size class: `trunk`, `limb` or `branch`.
    pub fn tag(self) -> procgena_specimen::Tag {
        match self {
            SizeClass::Trunk => tags::TRUNK,
            SizeClass::Limb => tags::LIMB,
            SizeClass::Branch => tags::BRANCH,
        }
    }
}

impl DeadwoodMesh {
    /// Returns one [`Part`] per size class, in the same order as
    /// [`DeadwoodMesh::parts`].
    ///
    /// Each part is closed (watertight, with no holes in its surface) and
    /// lists one shell per stem. Its vertex numbers start from 0. Every part
    /// is tagged `deadwood`, `wood` and its size class (`trunk`, `limb` or
    /// `branch`), plus `bark` while any bark remains on it. Strips in
    /// [`DeadwoodMesh::ribbons`] are left out, since nothing can grow on
    /// them.
    pub fn specimen_parts(&self) -> Vec<Part> {
        self.parts
            .iter()
            .map(|range| to_part(&self.surfaces, range))
            .collect()
    }
}

impl Piece {
    /// Builds a [`Specimen`] for handing the piece to other procgena
    /// generators. It has two levels: the game-level mesh, then the lower
    /// mesh (see [`Piece::lod_meshes`]).
    ///
    /// # Panics
    ///
    /// Panics if a mesh needs more vertices than fit in a 32-bit index.
    pub fn specimen(&self) -> Specimen {
        specimen_from_meshes(&self.lod_meshes())
    }
}

/// Builds a [`Specimen`] from meshes made by [`Piece::lod_meshes`]. The
/// lower level starts where the default
/// [`DistanceChain`](procgena_core::chain::DistanceChain) switches to it for
/// this piece (see [`lower_start`]).
pub fn specimen_from_meshes(meshes: &[(Step, DeadwoodMesh); 2]) -> Specimen {
    let (_, radius) = meshes[0].1.bounding_sphere();
    let starts = [0.0, lower_start(2.0 * radius)];
    Specimen {
        levels: meshes
            .iter()
            .zip(starts)
            .map(|((_, mesh), start)| Level {
                start,
                parts: mesh.specimen_parts(),
            })
            .collect(),
    }
}

/// The camera distance in metres where the default [`DistanceChain`]
/// switches from the game-level mesh to the lower mesh, for a piece whose
/// game-level mesh has a bounding sphere `diameter` metres across (see
/// [`DeadwoodMesh::bounding_sphere`]), seen through a
/// [`ViewScale::DEFAULT`] camera at High quality.
pub fn lower_start(diameter: f32) -> f32 {
    DistanceChain::factor(diameter, ViewScale::DEFAULT) / DistanceChain::DEFAULT.lower_below
}

/// Copies the vertices and triangles of `range` into a closed, tagged part,
/// renumbering vertices to start from 0.
fn to_part(buffers: &MeshBuffers, range: &PartRange) -> Part {
    let (v0, v1) = (range.vertices.start as usize, range.vertices.end as usize);
    let mesh = SurfaceMesh {
        positions: buffers.positions[v0..v1].to_vec(),
        normals: buffers.normals[v0..v1].to_vec(),
        indices: buffers.indices[range.indices.start as usize..range.indices.end as usize]
            .iter()
            .map(|&i| i - range.vertices.start)
            .collect(),
    };
    let mut set = TagSet::from([tags::DEADWOOD, tags::WOOD, range.size.tag()]);
    if range.bark {
        set.insert(tags::BARK);
    }
    let mut part = Part::new(set, mesh);
    part.closed = true;
    part.shells = range.shells.clone();
    part
}
