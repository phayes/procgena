//! Conversion to [`procgena_mesh::Object`], the shared format procgena
//! generators use to hand meshes to each other, so that, for example, moss
//! from `procgena_clump` can grow on a log.

use procgena_mesh::{Level, Object, Part, SurfaceMesh, TagSet, tags};

use crate::detail::{DeadwoodLod, LodLevel};
use crate::limb::SizeClass;
use crate::mesh::{DeadwoodMesh, MeshBuffers, PartRange};
use crate::options::OptionsError;
use crate::piece::Piece;

impl SizeClass {
    /// Returns the tag for this size class: `trunk`, `limb` or `branch`.
    pub fn tag(self) -> procgena_mesh::Tag {
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
    pub fn object_parts(&self) -> Vec<Part> {
        self.parts
            .iter()
            .map(|range| to_part(&self.surfaces, range))
            .collect()
    }
}

impl Piece {
    /// Builds an [`Object`] with one level per level of `lod`, for handing
    /// the piece to other procgena generators.
    ///
    /// # Errors
    ///
    /// Returns the first problem with `lod`; see [`DeadwoodLod::validate`].
    ///
    /// # Panics
    ///
    /// Panics if a mesh needs more vertices than fit in a 32-bit index.
    pub fn object(&self, lod: &DeadwoodLod) -> Result<Object, OptionsError> {
        Ok(object_from_meshes(&self.lod_meshes(lod)?))
    }
}

/// Builds an [`Object`] from meshes made by [`Piece::lod_meshes`]. Each
/// level of the object starts at its [`LodLevel::distance`].
pub fn object_from_meshes(meshes: &[(LodLevel, DeadwoodMesh)]) -> Object {
    Object {
        levels: meshes
            .iter()
            .map(|(level, mesh)| Level {
                start: level.distance,
                parts: mesh.object_parts(),
            })
            .collect(),
    }
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
