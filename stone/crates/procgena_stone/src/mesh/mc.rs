//! Marching cubes: approximates the surface with triangles in each grid cell.
//!
//! Vertices sit on grid edges where the samples change from inside to outside.
//! Neighbouring cells share these vertices.

use super::tables::{MC_EDGES, TRI_TABLE};
use super::{Crossings, StoneMesh, cell_case, offset};
use crate::grid::SampleGrid;

/// Builds triangles from `grid` using the matching `crossings` as vertices.
///
/// The crossings must have been computed from this grid. Missing crossings
/// referenced by the triangle table cause a panic. Vertices follow crossing ID
/// order; triangles follow cell order, with x changing fastest.
pub(super) fn mesh(grid: &SampleGrid, crossings: &Crossings) -> StoneMesh {
    let r = grid.resolution();
    let mut indices = Vec::new();
    for z in 0..r {
        for y in 0..r {
            for x in 0..r {
                let cell = [x, y, z];
                let case = cell_case(grid, cell);
                if case == 0 || case == u8::MAX {
                    continue;
                }
                for tri in TRI_TABLE[case as usize].as_chunks::<3>().0 {
                    if tri[0] < 0 {
                        break;
                    }
                    for &edge in tri {
                        let (o, axis) = MC_EDGES[edge as usize];
                        let id = crossings
                            .id(grid, offset(cell, o), axis)
                            .expect("the table only names edges the surface crosses");
                        indices.push(id);
                    }
                }
            }
        }
    }

    StoneMesh {
        positions: crossings
            .points
            .iter()
            .map(|p| p.position.to_array())
            .collect(),
        normals: crossings
            .points
            .iter()
            .map(|p| p.normal.to_array())
            .collect(),
        indices,
    }
}
