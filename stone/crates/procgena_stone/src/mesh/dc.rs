//! Dual contouring: fits a vertex within each cell crossed by the surface.
//!
//! This can preserve sharper corners and creases than marching cubes. Nearby
//! surface parts can be joined at a vertex or edge when the grid cannot separate
//! them, creating non-manifold geometry that some mesh tools cannot accept.

use glam::{Mat3, Vec3};

use super::tables::MC_EDGES;
use super::{Crossings, StoneMesh, SurfacePoint, cell_case, offset, surface_normal};
use crate::field::StoneField;
use crate::grid::SampleGrid;
use crate::parallel;

/// Strength of the pull towards the average crossing position when fitting a vertex.
///
/// A positive value keeps the fit solvable even on flat surfaces. Increasing it
/// favours rounder corners; decreasing it allows sharper corners.
pub(crate) const REGULARIZATION: f32 = 0.05;

/// Builds a triangle mesh from a field and its matching grid and crossings.
///
/// Each crossed edge must have four neighbouring cells; boundary crossings
/// cause a panic. Vertices follow cell order, with x changing fastest. Each
/// crossed edge produces a four-corner face split into two triangles.
pub(super) fn mesh(field: &StoneField, grid: &SampleGrid, crossings: &Crossings) -> StoneMesh {
    // Collect cells with both inside and outside corners, with x changing fastest.
    let r = grid.resolution();
    let mut cells = Vec::new();
    for z in 0..r {
        for y in 0..r {
            for x in 0..r {
                let case = cell_case(grid, [x, y, z]);
                if case != 0 && case != u8::MAX {
                    cells.push([x, y, z]);
                }
            }
        }
    }
    let cell_keys: Vec<usize> = cells.iter().map(|&c| grid.index(c)).collect();
    let cell_id = |c: [u32; 3]| {
        cell_keys
            .binary_search(&grid.index(c))
            .expect("every cell around a crossed edge is crossed") as u32
    };

    // Fit a surface vertex within each collected cell.
    let points = parallel::map(&cells, parallel::PARALLEL, |&c| {
        cell_vertex(field, grid, crossings, c)
    });

    // Connect the four surrounding cell vertices to form an outward-facing face.
    let mut indices = Vec::with_capacity(crossings.keys.len() * 6);
    for &key in &crossings.keys {
        let n = grid.samples_per_axis() as u64;
        let axis = (key % 3) as usize;
        let i = key / 3;
        let s = [(i % n) as u32, (i / n % n) as u32, (i / (n * n)) as u32];

        // Start with corners ordered to face the positive axis direction.
        // Reverse them below if that direction points into the rock.
        let (b, c) = ((axis + 1) % 3, (axis + 2) % 3);
        let back = |s: [u32; 3], a: usize| {
            let mut t = s;
            t[a] = t[a]
                .checked_sub(1)
                .expect("crossed edges never lie on the grid boundary");
            t
        };
        let mut quad = [
            cell_id(back(back(s, b), c)),
            cell_id(back(s, c)),
            cell_id(s),
            cell_id(back(s, b)),
        ];
        if !grid.inside(s) {
            quad.reverse();
        }

        // Turn the four-corner face into two triangles along its shorter diagonal.
        let p = |k: usize| points[quad[k] as usize].position;
        let [q0, q1, q2, q3] = quad;
        if p(0).distance_squared(p(2)) <= p(1).distance_squared(p(3)) {
            indices.extend_from_slice(&[q0, q1, q2, q0, q2, q3]);
        } else {
            indices.extend_from_slice(&[q1, q2, q3, q1, q3, q0]);
        }
    }

    StoneMesh {
        positions: points.iter().map(|p| p.position.to_array()).collect(),
        normals: points.iter().map(|p| p.normal.to_array()).collect(),
        indices,
    }
}

/// Fits a surface vertex inside cell `c` and estimates its outward normal.
///
/// `field`, `grid`, and `crossings` must describe the same rock, and `c` must
/// have at least one crossing. The fitted vertex follows the surface directions
/// at the crossings, with a small pull towards their average position.
fn cell_vertex(
    field: &StoneField,
    grid: &SampleGrid,
    crossings: &Crossings,
    c: [u32; 3],
) -> SurfacePoint {
    // Express edge crossings relative to this cell: its minimum corner is
    // zero, and one unit is the cell width.
    let r = grid.resolution() as f32;
    let origin = grid.position(c);
    let edge_points: Vec<SurfacePoint> = MC_EDGES
        .iter()
        .filter_map(|&(o, axis)| crossings.id(grid, offset(c, o), axis))
        .map(|id| {
            let p = crossings.points[id as usize];
            SurfacePoint {
                position: (p.position - origin) * r,
                normal: p.normal,
            }
        })
        .collect();
    let mass = edge_points.iter().map(|p| p.position).sum::<Vec3>() / edge_points.len() as f32;

    // Fit the planes through the crossings, using their average as the origin.
    // The diagonal term pulls the solution towards that average.
    let mut a = Mat3::from_diagonal(Vec3::splat(REGULARIZATION));
    let mut rhs = Vec3::ZERO;
    for p in &edge_points {
        let n = p.normal;
        a += Mat3::from_cols(n * n.x, n * n.y, n * n.z);
        rhs += n * n.dot(p.position - mass);
    }
    let local = (mass + a.inverse() * rhs).clamp(Vec3::ZERO, Vec3::ONE);
    let position = origin + local / r;

    // Use the averaged crossing normals if the field gradient cannot be normalized.
    let average = edge_points
        .iter()
        .map(|p| p.normal)
        .sum::<Vec3>()
        .try_normalize()
        .unwrap_or(edge_points[0].normal);
    SurfacePoint {
        position,
        normal: surface_normal(field, position, 0.5 / r, average),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regularization_keeps_the_system_solvable() {
        // A flat surface alone does not fix a vertex position along the plane.
        // The pull towards the average must make that fit solvable.
        let n = Vec3::Z;
        let a = Mat3::from_diagonal(Vec3::splat(REGULARIZATION))
            + Mat3::from_cols(n * n.x, n * n.y, n * n.z);
        assert!(a.determinant() > 0.0);
        assert!((a.inverse() * Vec3::Z).is_finite());
    }
}
