//! Conversion to [`procgena_mesh::Object`], the shared format procgena
//! generators use to hand meshes to each other, so that, for example, moss
//! can grow on a mushroom.

use procgena_mesh::{Level, Object, Part, SurfaceMesh, Tag, TagSet};

use crate::detail::{LodLevel, MushroomLod};
use crate::mesh::{MeshBuffers, MushroomMesh, PartKind, PartRange};
use crate::mushroom::Mushroom;
use crate::options::OptionsError;

/// Labels on the parts of a mushroom [`Object`], which other generators use
/// to pick the parts they want.
///
/// Every part has [`MUSHROOM`](tags::MUSHROOM) plus one of the others.
pub mod tags {
    use procgena_mesh::Tag;

    /// Any part of a mushroom.
    pub const MUSHROOM: Tag = Tag::new("mushroom");
    /// The cap.
    pub const CAP: Tag = Tag::new("cap");
    /// The stem.
    pub const STEM: Tag = Tag::new("stem");
    /// The ring around the stem.
    pub const RING: Tag = Tag::new("ring");
    /// Warts on the cap.
    pub const WART: Tag = Tag::new("wart");
}

impl PartKind {
    /// Returns the tag that names this kind of part. Parts also carry
    /// [`tags::MUSHROOM`].
    pub fn tag(self) -> Tag {
        match self {
            PartKind::Cap => tags::CAP,
            PartKind::Stem => tags::STEM,
            PartKind::Ring => tags::RING,
            PartKind::Warts => tags::WART,
        }
    }
}

impl MushroomMesh {
    /// Returns each part of the mesh as a standalone, tagged [`Part`], with
    /// its closed pieces recorded.
    ///
    /// A flat stem ([`crate::StemRibbon`]) is left out, because it turns
    /// with the camera and so has no fixed surface to grow on.
    pub fn object_parts(&self) -> Vec<Part> {
        self.parts
            .iter()
            .map(|range| to_part(&self.surfaces, range))
            .collect()
    }
}

impl Mushroom {
    /// Builds an [`Object`] with one detail level for each level of `lod`.
    ///
    /// # Errors
    ///
    /// Returns an error if `lod` is invalid; see [`MushroomLod::validate`].
    pub fn object(&self, lod: &MushroomLod) -> Result<Object, OptionsError> {
        Ok(object_from_meshes(&self.lod_meshes(lod)?))
    }
}

/// Builds an [`Object`] from meshes you already made with
/// [`Mushroom::lod_meshes`], without meshing again.
pub fn object_from_meshes(meshes: &[(LodLevel, MushroomMesh)]) -> Object {
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

/// Copies the triangles of `range` out of `buffers` into a standalone
/// [`Part`] with the part's tags.
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
    let mut part = Part::new(TagSet::from([tags::MUSHROOM, range.kind.tag()]), mesh);
    part.closed = range.closed;
    part.shells = range.shells.clone();
    part
}
