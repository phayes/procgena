//! A coarse grid over a region, for planning rivers and roads.

use glam::DVec2;

use crate::region::Boundary;

/// The preferred cell size, in metres.
const CELL: f64 = 25.0;

/// The most cells a grid may have; larger regions get larger cells.
const MAX_CELLS: f64 = 400_000.0;

/// Cell centres over a polygon's bounding box, with heights and distances to
/// the border.
#[derive(Debug, Clone)]
pub(crate) struct Grid {
    /// Centre of cell (0, 0).
    origin: DVec2,
    /// Cell size, in metres.
    pub cell: f64,
    /// Cells along x.
    pub width: usize,
    /// Cells along z.
    pub depth: usize,
    /// Whether each cell's centre is inside the region.
    pub inside: Vec<bool>,
    /// Distance from each cell's centre to the region's border, in metres.
    pub clearance: Vec<f64>,
    /// Ground height at each cell's centre, in metres.
    pub heights: Vec<f32>,
}

/// The 8 neighbouring cell offsets, in a fixed order.
const NEIGHBOURS: [(isize, isize); 16] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
    // Knight moves, so paths can run at about 27° and 63° as well as at
    // multiples of 45°, without stair-steps.
    (-2, -1),
    (-1, -2),
    (1, -2),
    (2, -1),
    (-2, 1),
    (-1, 2),
    (1, 2),
    (2, 1),
];

impl Grid {
    /// Builds the grid over `polygon`, with heights from `height`.
    pub(crate) fn new(polygon: &[DVec2], height: impl Fn(DVec2) -> f32) -> Self {
        Self::with_cell(polygon, height, CELL)
    }

    /// Like [`Grid::new`], with cells of `cell` metres where the region is
    /// small enough (larger regions get larger cells).
    pub(crate) fn with_cell(polygon: &[DVec2], height: impl Fn(DVec2) -> f32, cell: f64) -> Self {
        // Bounding box and cell size.
        let (min, max) = polygon
            .iter()
            .fold((DVec2::INFINITY, DVec2::NEG_INFINITY), |(lo, hi), &p| {
                (lo.min(p), hi.max(p))
            });
        let size = max - min;
        let cell = cell.max((size.x * size.y / MAX_CELLS).sqrt());
        let width = (size.x / cell).ceil() as usize + 1;
        let depth = (size.y / cell).ceil() as usize + 1;
        let origin = min + DVec2::splat(cell / 2.0);

        // Per-cell data.
        let boundary = Boundary::Polygon(polygon.to_vec());
        let mut grid = Self {
            origin,
            cell,
            width,
            depth,
            inside: Vec::with_capacity(width * depth),
            clearance: Vec::with_capacity(width * depth),
            heights: Vec::with_capacity(width * depth),
        };
        for i in 0..width * depth {
            let p = grid.centre(i);
            let inside = boundary.contains(p);
            grid.inside.push(inside);
            grid.clearance.push(if inside {
                boundary.distance_to_border(p)
            } else {
                0.0
            });
            grid.heights.push(if inside { height(p) } else { f32::NAN });
        }
        grid
    }

    /// The same cells with heights from `height` instead.
    pub(crate) fn with_heights(&self, height: impl Fn(DVec2) -> f32) -> Self {
        let mut grid = self.clone();
        for i in 0..grid.heights.len() {
            if grid.inside[i] {
                grid.heights[i] = height(grid.centre(i));
            }
        }
        grid
    }

    /// The centre of cell `i`.
    pub(crate) fn centre(&self, i: usize) -> DVec2 {
        let (x, z) = (i % self.width, i / self.width);
        self.origin + DVec2::new(x as f64, z as f64) * self.cell
    }

    /// The cell containing `p`, if it is inside the region.
    pub(crate) fn cell_of(&self, p: DVec2) -> Option<usize> {
        let q = ((p - self.origin) / self.cell).round();
        if q.x < 0.0 || q.y < 0.0 || q.x >= self.width as f64 || q.y >= self.depth as f64 {
            return None;
        }
        let i = q.y as usize * self.width + q.x as usize;
        self.inside[i].then_some(i)
    }

    /// The inside neighbours of cell `i` in 16 directions (the 8 adjacent
    /// cells and the 8 a knight's move away), with the distance to each.
    pub(crate) fn neighbours(&self, i: usize) -> impl Iterator<Item = (usize, f64)> + '_ {
        self.moves(i).map(|(j, length, _)| (j, length))
    }

    /// Like [`Grid::neighbours`], with each move's direction as an index
    /// below [`DIRECTIONS`] (see [`direction`]).
    pub(crate) fn moves(&self, i: usize) -> impl Iterator<Item = (usize, f64, usize)> + '_ {
        let (x, z) = ((i % self.width) as isize, (i / self.width) as isize);
        NEIGHBOURS
            .iter()
            .enumerate()
            .filter_map(move |(k, &(dx, dz))| {
                let (nx, nz) = (x + dx, z + dz);
                if nx < 0 || nz < 0 || nx >= self.width as isize || nz >= self.depth as isize {
                    return None;
                }
                let j = nz as usize * self.width + nx as usize;
                let step = ((dx * dx + dz * dz) as f64).sqrt();
                self.inside[j].then_some((j, step * self.cell, k))
            })
    }
}

/// How many move directions [`Grid::moves`] uses.
pub(crate) const DIRECTIONS: usize = NEIGHBOURS.len();

/// The unit vector of move direction `k`, which must be below
/// [`DIRECTIONS`].
pub(crate) fn direction(k: usize) -> DVec2 {
    let (dx, dz) = NEIGHBOURS[k];
    DVec2::new(dx as f64, dz as f64).normalize()
}

impl Grid {
    /// The (up to 8) cells touching cell `i`, inside the region or not.
    pub(crate) fn adjacent(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        let (x, z) = ((i % self.width) as isize, (i / self.width) as isize);
        NEIGHBOURS[..8].iter().filter_map(move |&(dx, dz)| {
            let (nx, nz) = (x + dx, z + dz);
            let fits = nx >= 0 && nz >= 0 && nx < self.width as isize && nz < self.depth as isize;
            fits.then(|| nz as usize * self.width + nx as usize)
        })
    }

    /// The cells a move from `a` to `b` passes through: `b` itself, plus
    /// for a diagonal or knight's move the two cells it passes between.
    /// Another path through both of those could cross this move without
    /// sharing a cell.
    pub(crate) fn passes(&self, a: usize, b: usize) -> impl Iterator<Item = usize> {
        let (ax, az) = ((a % self.width) as isize, (a / self.width) as isize);
        let (bx, bz) = ((b % self.width) as isize, (b / self.width) as isize);
        let (dx, dz) = (bx - ax, bz - az);
        let between = match (dx.abs(), dz.abs()) {
            (1, 1) => Some([(bx, az), (ax, bz)]),
            (2, 1) => Some([(ax + dx / 2, az), (ax + dx / 2, bz)]),
            (1, 2) => Some([(ax, az + dz / 2), (bx, az + dz / 2)]),
            _ => None,
        };
        let width = self.width;
        std::iter::once(b).chain(
            between
                .into_iter()
                .flatten()
                .map(move |(x, z)| z as usize * width + x as usize),
        )
    }
}

/// The area of a simple polygon, in square metres.
pub(crate) fn polygon_area(polygon: &[DVec2]) -> f64 {
    let n = polygon.len();
    (0..n)
        .map(|i| polygon[i].perp_dot(polygon[(i + 1) % n]))
        .sum::<f64>()
        .abs()
        / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_covers_the_polygon() {
        let square = [
            DVec2::ZERO,
            DVec2::new(500.0, 0.0),
            DVec2::new(500.0, 500.0),
            DVec2::new(0.0, 500.0),
        ];
        let grid = Grid::new(&square, |p| p.x as f32);
        let i = grid.cell_of(DVec2::new(250.0, 250.0)).unwrap();
        assert!((grid.clearance[i] - 250.0).abs() < grid.cell);
        assert!((grid.heights[i] - 250.0).abs() < grid.cell as f32);
        assert_eq!(grid.neighbours(i).count(), 16);
        assert!(grid.cell_of(DVec2::new(-50.0, 0.0)).is_none());
        assert_eq!(polygon_area(&square), 250_000.0);
    }

    #[test]
    fn knight_moves_pass_between_two_cells() {
        let square = [
            DVec2::ZERO,
            DVec2::new(500.0, 0.0),
            DVec2::new(500.0, 500.0),
            DVec2::new(0.0, 500.0),
        ];
        let grid = Grid::new(&square, |_| 0.0);
        let at = |x: usize, z: usize| z * grid.width + x;
        let passed: Vec<usize> = grid.passes(at(5, 5), at(7, 6)).collect();
        assert_eq!(passed, [at(7, 6), at(6, 5), at(6, 6)]);
        let diagonal: Vec<usize> = grid.passes(at(5, 5), at(6, 6)).collect();
        assert_eq!(diagonal, [at(6, 6), at(6, 5), at(5, 6)]);
        let straight: Vec<usize> = grid.passes(at(5, 5), at(6, 5)).collect();
        assert_eq!(straight, [at(6, 5)]);
    }
}
