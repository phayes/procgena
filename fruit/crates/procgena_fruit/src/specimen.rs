//! Describe a generated cluster as a tagged [`Specimen`].

use procgena_core::LodLevel;
use procgena_specimen::{Level, Part, Specimen, SurfaceMesh, tags};

use crate::{ClusterDetail, Fruit, FruitMesh};

impl Fruit {
    /// The cluster as a tagged [`Specimen`] with one level per entry of
    /// `lods`, such as the result of [`Fruit::lod_meshes`], ordered by
    /// increasing distance. Each level has one open part tagged `fruit` or
    /// `flower`, left out when the level has no triangles.
    ///
    /// # Panics
    ///
    /// Panics if `lods` is empty.
    pub fn specimen(&self, lods: &[(LodLevel<ClusterDetail>, FruitMesh)]) -> Specimen {
        assert!(
            !lods.is_empty(),
            "a fruit specimen needs at least one level"
        );
        let tag = if self.options().item.is_fruit() {
            tags::FRUIT
        } else {
            tags::FLOWER
        };
        let levels = lods
            .iter()
            .enumerate()
            .map(|(i, (lod, mesh))| Level {
                start: if i == 0 { 0.0 } else { lod.distance },
                parts: (!mesh.indices.is_empty())
                    .then(|| {
                        let surface = SurfaceMesh {
                            positions: mesh.positions.clone(),
                            normals: mesh.normals.clone(),
                            indices: mesh.indices.clone(),
                        };
                        Part::new([tag.clone()], surface)
                    })
                    .into_iter()
                    .collect(),
            })
            .collect();
        Specimen { levels }
    }
}
