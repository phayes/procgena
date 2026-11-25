//! Describe a generated cluster as a tagged [`Specimen`].

use procgena_core::chain::{DistanceChain, ViewScale};
use procgena_specimen::{Level, Part, Specimen, SurfaceMesh, tags};

use crate::{Fruit, FruitMesh};

impl Fruit {
    /// The cluster as a tagged [`Specimen`] with two levels: its game-level
    /// mesh and its lower mesh ([`Fruit::meshes`]). Each level has one open
    /// part tagged `fruit` or `flower`, left out when the level has no
    /// triangles. The lower level starts where the default
    /// [`DistanceChain`] switches to it for this cluster, seen through a
    /// [`ViewScale::DEFAULT`] camera at High quality.
    pub fn specimen(&self) -> Specimen {
        let tag = if self.options().item.is_fruit() {
            tags::FRUIT
        } else {
            tags::FLOWER
        };
        let factor = DistanceChain::factor(self.size(), ViewScale::DEFAULT);
        let lower_start = factor / DistanceChain::DEFAULT.lower_below;
        let meshes = self.meshes();
        let levels = [(0.0, &meshes.game), (lower_start, &meshes.lower)]
            .into_iter()
            .map(|(start, mesh): (f32, &FruitMesh)| Level {
                start,
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
