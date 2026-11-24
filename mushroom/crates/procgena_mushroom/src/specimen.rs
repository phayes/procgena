//! Conversion to [`procgena_specimen::Specimen`], the shared format procgena
//! generators use to hand meshes to each other, so that, for example, moss
//! can grow on a mushroom.

use procgena_core::Quality;
use procgena_specimen::{Level, Part, Specimen, SurfaceMesh, Tag, TagSet, tags};

use crate::detail::{MushroomLod, MushroomLodLevel};
use crate::mesh::{MeshBuffers, MushroomMesh, PartKind, PartRange};
use crate::mushroom::Mushroom;
use crate::options::OptionsError;

impl PartKind {
    /// Returns the tag that names this kind of part. Parts also carry
    /// [`tags::MUSHROOM`].
    pub fn tag(self) -> Tag {
        match self {
            PartKind::Cap => tags::CAP,
            PartKind::Stem => tags::STEM,
            PartKind::Annulus => tags::ANNULUS,
        }
    }
}

impl MushroomMesh {
    /// Returns each part of the mesh as a standalone, tagged [`Part`], with
    /// its closed pieces recorded.
    ///
    /// Camera-facing shapes ([`MushroomMesh::billboard`]) are left out,
    /// because they turn with the camera and so have no fixed surface to
    /// grow on.
    pub fn specimen_parts(&self) -> Vec<Part> {
        self.parts
            .iter()
            .map(|range| to_part(&self.surfaces, range))
            .collect()
    }
}

impl Mushroom {
    /// Builds a [`Specimen`] with one detail level for each level of `lod`.
    /// Every member of a cluster is included, and parts of the same kind
    /// are merged, so the specimen has one cap part, one stem part, and so on.
    ///
    /// Each level starts at its distance in `lod` scaled to this mushroom's
    /// [`size`](crate::MushroomOptions::size) at
    /// [`Quality::High`], the quality every [`MushroomLod`] is written for.
    ///
    /// # Errors
    ///
    /// Returns an error if `lod` is invalid; see [`crate::validate_lod`].
    pub fn specimen(&self, lod: &MushroomLod) -> Result<Specimen, OptionsError> {
        let factor = lod.factor(self.options().size, Quality::High);
        Ok(specimen_from_meshes(&self.lod_meshes(lod)?, factor))
    }
}

/// Builds a [`Specimen`] from meshes you already made with
/// [`Mushroom::lod_meshes`], without meshing again. Each level starts at its
/// distance times `factor`, such as the [`Lod::factor`](procgena_core::Lod::factor)
/// for the mushroom's size.
pub fn specimen_from_meshes(meshes: &[(MushroomLodLevel, MushroomMesh)], factor: f32) -> Specimen {
    Specimen {
        levels: meshes
            .iter()
            .map(|(level, mesh)| Level {
                start: level.distance * factor,
                parts: mesh.specimen_parts(),
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
