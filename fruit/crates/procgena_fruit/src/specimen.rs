//! Describe a generated cluster as a tagged [`Specimen`].

use procgena_specimen::{Level, Part, Specimen, SurfaceMesh, tags};

use crate::{Fruit, FruitMesh};

impl Fruit {
    /// The cluster as a tagged [`Specimen`] with two levels: its game-level
    /// mesh and its lower mesh ([`Fruit::meshes`]). Each level has one open
    /// part tagged `fruit` or `flower`, left out when the level has no
    /// triangles.
    pub fn specimen(&self) -> Specimen {
        let tag = if self.options().item.is_fruit() {
            tags::FRUIT
        } else {
            tags::FLOWER
        };
        let meshes = self.meshes();
        let levels = [&meshes.game, &meshes.lower]
            .into_iter()
            .map(|mesh: &FruitMesh| Level {
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
