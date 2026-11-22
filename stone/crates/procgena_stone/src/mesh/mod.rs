//! Builds triangle meshes of rock surfaces, with shared vertices and surface normals.

pub(crate) mod dc;
mod mc;
pub(crate) mod tables;

use glam::Vec3;

use crate::field::StoneField;
use crate::grid::SampleGrid;
use crate::options::{Mesher, OptionsError, check_resolution};
use crate::parallel;

/// Positive unit directions for axis indices 0 (x), 1 (y), and 2 (z).
const AXES: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::Z];

/// A rock surface described by vertices, normals, and triangle indices.
///
/// Generated meshes share vertices between neighbouring triangles and order
/// triangle corners counter-clockwise as viewed from outside. Normals describe
/// the outward surface direction for lighting.
///
/// When editing the buffers, keep one normal per position, three indices per
/// triangle, and all indices within the positions buffer.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StoneMesh {
    /// Vertex coordinates `[x, y, z]`. [`crate::Stone`] supplies metres;
    /// [`mesh_field`] supplies coordinates in the unit-width stone cube.
    pub positions: Vec<[f32; 3]>,
    /// Outward unit directions `[x, y, z]`, one per vertex, for surface lighting.
    pub normals: Vec<[f32; 3]>,
    /// Indices into `positions` and `normals`, grouped in threes to form triangles.
    pub indices: Vec<u32>,
}

impl StoneMesh {
    /// Returns the number of entries in the positions buffer, including unused vertices.
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Returns the number of complete index triples.
    ///
    /// Any trailing one or two indices in a manually edited buffer are ignored.
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Returns `(min, max)`, the corners of the axis-aligned box enclosing all vertices.
    ///
    /// The result uses the same units as `positions` and includes unused vertices.
    /// Returns `None` when there are no positions. Positions should be finite.
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let mut points = self.positions.iter().map(|&p| Vec3::from(p));
        let first = points.next()?;
        let (min, max) = points.fold((first, first), |(min, max), p| (min.min(p), max.max(p)));
        Some((min.to_array(), max.to_array()))
    }

    /// Multiplies vertex positions by `factor` without changing normals or indices.
    ///
    /// Use a finite, positive factor that keeps the resulting positions finite.
    /// Zero collapses the mesh; a negative factor reverses its orientation.
    /// Use [`crate::StoneOptions::scale`] to turn a [`mesh_field`] result into metres.
    pub fn scale(&mut self, factor: f32) {
        for p in &mut self.positions {
            *p = (Vec3::from(*p) * factor).to_array();
        }
    }
}

/// Builds a triangle mesh of `field` using the chosen mesher and resolution.
///
/// Positions lie in the stone cube from -0.5 to 0.5 on each axis. For world
/// units, multiply positions by [`crate::StoneOptions::scale`], or use
/// [`crate::Stone`] to apply that scale automatically.
///
/// Higher resolutions capture smaller features but require more time and memory.
/// Features missed by the sampling grid do not appear in the mesh; if no samples
/// are inside the rock, the mesh is empty. Repeating the same inputs produces
/// the same mesh. See [`Mesher`] for the differences between the two methods.
///
/// # Errors
///
/// Returns [`OptionsError::IntOutOfRange`] with the field name `"resolution"` if
/// `resolution` is outside [`crate::MIN_RESOLUTION`] through [`crate::MAX_RESOLUTION`].
pub fn mesh_field(
    field: &StoneField,
    mesher: Mesher,
    resolution: u32,
) -> Result<StoneMesh, OptionsError> {
    check_resolution("resolution", resolution)?;
    let grid = SampleGrid::sample(field, resolution);
    let crossings = Crossings::find(field, &grid);
    Ok(match mesher {
        Mesher::MarchingCubes => mc::mesh(&grid, &crossings),
        Mesher::DualContouring => dc::mesh(field, &grid, &crossings),
    })
}

/// An estimated surface position in stone space and its outward unit normal.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SurfacePoint {
    position: Vec3,
    normal: Vec3,
}

/// Surface estimates on grid edges whose endpoints differ in inside/outside status.
///
/// An edge starts at its lower-coordinate sample and runs along x, y, or z.
/// Its position in these buffers is its crossing ID.
struct Crossings {
    /// Sorted edge identifiers: three times the flat sample index, plus the axis
    /// index (0 for x, 1 for y, 2 for z).
    keys: Vec<u64>,
    /// Estimated positions and normals, paired with `keys` in the same order.
    points: Vec<SurfacePoint>,
}

impl Crossings {
    /// Finds edges with one endpoint inside the rock and one outside.
    ///
    /// `grid` must contain samples of `field`. Positions are interpolated between
    /// edge endpoints, and normals are estimated from the field.
    fn find(field: &StoneField, grid: &SampleGrid) -> Self {
        let edges = crossing_edges(grid);

        // Interpolate surface positions and estimate a normal at each position.
        let normal_step = 0.5 / grid.resolution() as f32;
        let points = parallel::map(&edges, parallel::PARALLEL, |&(s, axis)| {
            let position = crossing_position(grid, s, axis);
            // If the gradient cannot provide a normal, use the edge direction
            // from the inside endpoint towards the outside endpoint.
            let outward = if grid.inside(s) {
                AXES[axis]
            } else {
                -AXES[axis]
            };
            SurfacePoint {
                position,
                normal: surface_normal(field, position, normal_step, outward),
            }
        });

        let keys = edges
            .iter()
            .map(|&(s, axis)| edge_key(grid, s, axis))
            .collect();
        Self { keys, points }
    }

    /// Returns the crossing ID for an edge, or `None` if it has no recorded crossing.
    ///
    /// Pass the grid used to build this collection, a valid lower endpoint `s`,
    /// and an axis from 0 through 2.
    fn id(&self, grid: &SampleGrid, s: [u32; 3], axis: usize) -> Option<u32> {
        self.keys
            .binary_search(&edge_key(grid, s, axis))
            .ok()
            .map(|i| i as u32)
    }
}

/// Returns grid edges crossed by the rock surface as `(lower endpoint, axis)`
/// pairs, where axis is 0 for x, 1 for y, or 2 for z.
///
/// Each edge has one inside and one outside endpoint. Results are ordered by
/// lower endpoint, with x changing fastest, then by axis.
pub(crate) fn crossing_edges(grid: &SampleGrid) -> Vec<([u32; 3], usize)> {
    let n = grid.samples_per_axis();
    let mut edges = Vec::new();
    for z in 0..n {
        for y in 0..n {
            for x in 0..n {
                let s = [x, y, z];
                for axis in 0..3 {
                    if s[axis] + 1 < n && grid.inside(s) != grid.inside(step(s, axis)) {
                        edges.push((s, axis));
                    }
                }
            }
        }
    }
    edges
}

/// Estimates the unscaled surface position between an edge's two samples.
///
/// Pass a lower endpoint `s` and `axis` from [`crossing_edges`] for this `grid`.
/// The result is in the original stone space, before ground placement.
pub(crate) fn crossing_position(grid: &SampleGrid, s: [u32; 3], axis: usize) -> Vec3 {
    let a = grid.value(s);
    let b = grid.value(step(s, axis));
    let t = a / (a - b);
    grid.position(s) + AXES[axis] * (t / grid.resolution() as f32)
}

/// Encodes an edge as a sortable identifier.
///
/// `s` must be its lower endpoint in `grid`, and `axis` must be 0, 1, or 2.
fn edge_key(grid: &SampleGrid, s: [u32; 3], axis: usize) -> u64 {
    grid.index(s) as u64 * 3 + axis as u64
}

/// Returns `s` advanced by one sample along `axis` (0 = x, 1 = y, 2 = z).
///
/// The coordinate must not overflow `u32`. Panics for an axis greater than 2.
fn step(s: [u32; 3], axis: usize) -> [u32; 3] {
    let mut t = s;
    t[axis] += 1;
    t
}

/// Adds a corner or edge offset to grid coordinates, component by component.
///
/// The caller must ensure each sum fits in `u32`.
fn offset(c: [u32; 3], o: [u32; 3]) -> [u32; 3] {
    [c[0] + o[0], c[1] + o[1], c[2] + o[2]]
}

/// Encodes which corners of cell `c` are outside the rock.
///
/// Bit `i` corresponds to corner `i` in [`tables::MC_CORNERS`]. Each coordinate
/// of `c` must be less than the grid resolution so all eight corners exist.
fn cell_case(grid: &SampleGrid, c: [u32; 3]) -> u8 {
    let mut case = 0;
    for (i, &corner) in tables::MC_CORNERS.iter().enumerate() {
        if !grid.inside(offset(c, corner)) {
            case |= 1 << i;
        }
    }
    case
}

/// Estimates an outward unit normal at stone-space position `p`.
///
/// Use a positive, finite sampling `step` and a unit-length `fallback`. Returns
/// `fallback` if the field gradient cannot be normalized, including flat regions.
fn surface_normal(field: &StoneField, p: Vec3, step: f32, fallback: Vec3) -> Vec3 {
    field.gradient(p, step).try_normalize().unwrap_or(fallback)
}

#[cfg(test)]
pub(crate) mod tests;
