//! Ground for previewing an Ecotope: a mesh of any [`Ground`], a gently
//! rolling height function to build a [`HeightGrid`](procgena_core::HeightGrid)
//! from, and a plain grey patch of it under the Ecotope.

use bevy::asset::RenderAssetUsages;
use bevy::math::DVec2;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use procgena_core::Ground;
use procgena_core::scatter::ScatterArea;

/// How [`EcotopeParams`](crate::EcotopeParams) draws the ground under its
/// Ecotope: a plain grey mesh of the ground over the area and a margin
/// around it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroundPatch {
    /// Ground drawn beyond the area on each side, in metres.
    pub margin: f64,
    /// The largest spacing of the mesh's vertices, in metres.
    pub cell: f64,
}

impl GroundPatch {
    /// The rectangle drawn around `area`: its smallest and largest World x
    /// and z.
    pub(crate) fn rectangle(&self, area: ScatterArea) -> (DVec2, DVec2) {
        let half = area.size.as_dvec2() * 0.5 + DVec2::splat(self.margin);
        (area.centre - half, area.centre + half)
    }
}

/// Marks the entity drawing the ground under an
/// [`EcotopeParams`](crate::EcotopeParams) Ecotope with a [`GroundPatch`],
/// a child of the Ecotope's entity.
#[derive(Component, Debug, Clone, Copy, Default)]
#[require(Transform, Visibility)]
pub struct EcotopeGroundPatch;

/// Rolling ground for previews: long swells about 2 m high with a shallower
/// ripple across them. Returns the height in metres at World position `p`.
pub fn rolling_hills(p: DVec2) -> f32 {
    let (x, z) = (p.x as f32, p.y as f32);
    1.2 * (0.09 * x).sin() + 0.8 * (0.07 * z + 0.03 * x).cos() + 0.25 * (0.31 * x + 0.2 * z).sin()
}

/// A mesh of `ground` over the rectangle from `min` to `max` (World X and
/// Z), with a vertex every `cell` metres or closer, so it fits the rectangle
/// exactly. Vertex positions are World positions; where the ground has no
/// height (NaN) the mesh lies at height 0.
///
/// # Panics
///
/// Panics unless `cell` is positive and `max` lies beyond `min` on both
/// axes.
pub fn ground_mesh(ground: &dyn Ground, min: DVec2, max: DVec2, cell: f64) -> Mesh {
    assert!(cell > 0.0 && max.x > min.x && max.y > min.y);
    let columns = ((max.x - min.x) / cell).ceil().max(1.0) as u32;
    let rows = ((max.y - min.y) / cell).ceil().max(1.0) as u32;

    // One vertex per grid corner, at the ground's height.
    let mut positions = Vec::with_capacity(((columns + 1) * (rows + 1)) as usize);
    let mut uvs = Vec::with_capacity(positions.capacity());
    for row in 0..=rows {
        for column in 0..=columns {
            let t = DVec2::new(
                f64::from(column) / f64::from(columns),
                f64::from(row) / f64::from(rows),
            );
            let p = min + (max - min) * t;
            let height = ground.height(p);
            let height = if height.is_nan() { 0.0 } else { height };
            positions.push([p.x as f32, height, p.y as f32]);
            uvs.push([t.x as f32, t.y as f32]);
        }
    }

    // Two triangles per cell, facing up.
    let mut indices = Vec::with_capacity((columns * rows * 6) as usize);
    let at = |column: u32, row: u32| row * (columns + 1) + column;
    for row in 0..rows {
        for column in 0..columns {
            let (a, b) = (at(column, row), at(column + 1, row));
            let (c, d) = (at(column, row + 1), at(column + 1, row + 1));
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices));
    mesh.compute_smooth_normals();
    mesh
}

#[cfg(test)]
mod tests {
    use procgena_core::{FlatGround, HeightGrid};

    use super::*;

    #[test]
    fn the_mesh_covers_the_rectangle_at_the_grounds_height() {
        let grid = HeightGrid::from_fn(DVec2::splat(-6.0), DVec2::splat(6.0), 0.5, rolling_hills);
        let mesh = ground_mesh(&grid, DVec2::new(-5.0, -4.0), DVec2::new(5.0, 4.0), 0.5);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("the mesh has positions");
        };
        assert_eq!(positions.len(), 21 * 17);
        assert_eq!(positions[0][0], -5.0);
        assert_eq!(positions.last().unwrap()[2], 4.0);
        for p in positions {
            let height = grid.height(DVec2::new(f64::from(p[0]), f64::from(p[2])));
            assert!((p[1] - height).abs() < 1e-4, "{p:?}");
        }
        assert_eq!(mesh.indices().unwrap().len(), 20 * 16 * 6);

        // Flat ground gives a flat mesh.
        let flat = ground_mesh(&FlatGround, DVec2::splat(-1.0), DVec2::splat(1.0), 0.3);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            flat.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("the mesh has positions");
        };
        assert!(positions.iter().all(|p| p[1] == 0.0));
    }
}
