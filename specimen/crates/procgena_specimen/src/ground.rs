//! Ground shapes for scattering things over land: a height for each point
//! in X and Z, with Y up.
//!
//! Generators that spread copies over ground, such as sticks or trees, take
//! a [`Ground`]. Use [`FlatGround`], any closure `Fn(f32, f32) -> f32`, or a
//! [`HeightGrid`] sampled from a function or from another generator's
//! [`Specimen`].

use glam::{Vec2, Vec3};

use crate::{Specimen, TagFilter};

/// The shape of the ground, as a height for each point in X and Z, with Y
/// up.
///
/// Any closure `Fn(f32, f32) -> f32` taking X and Z can be used as ground.
/// [`FlatGround`] and [`HeightGrid`] are ready-made grounds.
pub trait Ground {
    /// Returns the ground's height in metres at `x`, `z`, or NaN where there
    /// is no ground; nothing is placed there.
    fn height(&self, x: f32, z: f32) -> f32;
}

/// Flat ground at `y = 0`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FlatGround;

impl Ground for FlatGround {
    fn height(&self, _x: f32, _z: f32) -> f32 {
        0.0
    }
}

impl<F: Fn(f32, f32) -> f32> Ground for F {
    fn height(&self, x: f32, z: f32) -> f32 {
        self(x, z)
    }
}

/// Ground heights sampled on a regular grid in X and Z, blended smoothly
/// between samples.
///
/// Sampling once and reusing the grid is useful when the real ground is
/// slow to query, such as a mesh. Outside the grid there is no ground, and
/// neither is there where any of the four surrounding samples has none.
#[derive(Debug, Clone, PartialEq)]
pub struct HeightGrid {
    origin: Vec2,
    cell: f32,
    columns: u32,
    rows: u32,
    heights: Vec<f32>,
}

impl HeightGrid {
    /// Samples `height` every `cell` metres over the rectangle from `min`
    /// to `max` (each as `[x, z]`). `height` takes X and Z and may return
    /// NaN where there is no ground.
    ///
    /// # Panics
    ///
    /// Panics if `cell` is not positive or `max` is not above `min`.
    pub fn from_fn(
        min: [f32; 2],
        max: [f32; 2],
        cell: f32,
        height: impl Fn(f32, f32) -> f32,
    ) -> Self {
        assert!(cell > 0.0 && max[0] > min[0] && max[1] > min[1]);
        let columns = ((max[0] - min[0]) / cell).ceil() as u32 + 1;
        let rows = ((max[1] - min[1]) / cell).ceil() as u32 + 1;
        let heights = (0..rows)
            .flat_map(|r| (0..columns).map(move |c| (c, r)))
            .map(|(c, r)| height(min[0] + c as f32 * cell, min[1] + r as f32 * cell))
            .collect();
        Self {
            origin: Vec2::from(min),
            cell,
            columns,
            rows,
            heights,
        }
    }

    /// Samples the top surface of a shared [`Specimen`], such as terrain from
    /// another procgena generator, every `cell` metres. Plant fields use
    /// ground meshes the same way.
    ///
    /// Only the parts of level `level` that `filter` matches are used.
    /// Where several surfaces overlap, the highest one is the ground. Points
    /// that no surface covers, seen from above, have no ground. Returns
    /// `None` when the filter matches nothing.
    ///
    /// # Panics
    ///
    /// Panics if `cell` is not positive or `level` is out of range.
    pub fn from_specimen(
        specimen: &Specimen,
        level: usize,
        filter: &TagFilter,
        cell: f32,
    ) -> Option<Self> {
        assert!(cell > 0.0);
        let part = specimen.merged(level, filter)?;
        let mesh = &part.mesh;
        if mesh.positions.is_empty() {
            return None;
        }
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for p in &mesh.positions {
            lo = lo.min(Vec2::new(p[0], p[2]));
            hi = hi.max(Vec2::new(p[0], p[2]));
        }
        let columns = ((hi.x - lo.x) / cell).ceil() as u32 + 1;
        let rows = ((hi.y - lo.y) / cell).ceil() as u32 + 1;
        let mut heights = vec![f32::NAN; (columns * rows) as usize];

        // Rasterise each triangle onto the grid, keeping the highest.
        for t in mesh.indices.chunks(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec3::from(mesh.positions[i as usize]));
            let (tmin, tmax) = (
                Vec2::new(a.x.min(b.x).min(c.x), a.z.min(b.z).min(c.z)),
                Vec2::new(a.x.max(b.x).max(c.x), a.z.max(b.z).max(c.z)),
            );
            let c0 = ((tmin.x - lo.x) / cell).ceil().max(0.0) as u32;
            let c1 = (((tmax.x - lo.x) / cell).floor() as u32).min(columns - 1);
            let r0 = ((tmin.y - lo.y) / cell).ceil().max(0.0) as u32;
            let r1 = (((tmax.y - lo.y) / cell).floor() as u32).min(rows - 1);
            for r in r0..=r1 {
                for col in c0..=c1 {
                    let p = Vec2::new(lo.x + col as f32 * cell, lo.y + r as f32 * cell);
                    if let Some(h) = height_in_triangle(p, a, b, c) {
                        let slot = &mut heights[(r * columns + col) as usize];
                        if slot.is_nan() || h > *slot {
                            *slot = h;
                        }
                    }
                }
            }
        }
        Some(Self {
            origin: lo,
            cell,
            columns,
            rows,
            heights,
        })
    }
}

impl Ground for HeightGrid {
    fn height(&self, x: f32, z: f32) -> f32 {
        let g = (Vec2::new(x, z) - self.origin) / self.cell;
        if g.x < 0.0 || g.y < 0.0 {
            return f32::NAN;
        }
        // Past the last sample there is no ground; exactly on it, blend from
        // the cell before so the far edge still counts.
        let (last_c, last_r) = ((self.columns - 1) as f32, (self.rows - 1) as f32);
        if g.x > last_c || g.y > last_r {
            return f32::NAN;
        }
        let c = (g.x.floor() as u32).min(self.columns.saturating_sub(2));
        let r = (g.y.floor() as u32).min(self.rows.saturating_sub(2));
        if c + 1 >= self.columns || r + 1 >= self.rows {
            return f32::NAN;
        }
        let (fx, fz) = (g.x - c as f32, g.y - r as f32);
        let at = |c: u32, r: u32| self.heights[(r * self.columns + c) as usize];
        let top = at(c, r) + (at(c + 1, r) - at(c, r)) * fx;
        let bottom = at(c, r + 1) + (at(c + 1, r + 1) - at(c, r + 1)) * fx;
        top + (bottom - top) * fz
    }
}

/// Returns the height of triangle `a`, `b`, `c` above point `p` (in X and
/// Z), or `None` if `p` is outside it.
fn height_in_triangle(p: Vec2, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let (a2, b2, c2) = (
        Vec2::new(a.x, a.z),
        Vec2::new(b.x, b.z),
        Vec2::new(c.x, c.z),
    );
    let area = (b2 - a2).perp_dot(c2 - a2);
    if area.abs() < 1e-12 {
        return None;
    }
    let w1 = (p - a2).perp_dot(c2 - a2) / area;
    let w2 = (b2 - a2).perp_dot(p - a2) / area;
    let w0 = 1.0 - w1 - w2;
    let inside = -1e-5;
    (w0 >= inside && w1 >= inside && w2 >= inside).then_some(w0 * a.y + w1 * b.y + w2 * c.y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Part, SurfaceMesh, tags};

    #[test]
    fn height_grid_blends_and_has_edges() {
        let g = HeightGrid::from_fn([-1.0, -1.0], [1.0, 1.0], 0.5, |x, _| x);
        assert!((g.height(0.3, 0.2) - 0.3).abs() < 1e-5);
        assert!(g.height(5.0, 0.0).is_nan());
        // The far edge and far corner were sampled, so they have ground.
        assert!((g.height(1.0, 0.0) - 1.0).abs() < 1e-5);
        assert!((g.height(1.0, 1.0) - 1.0).abs() < 1e-5);
        assert!(g.height(1.01, 0.0).is_nan());
    }

    #[test]
    fn height_grid_from_a_specimen_follows_its_top() {
        // A tilted square, tagged as ground, under a small level square
        // that is not.
        let tilted = SurfaceMesh {
            positions: vec![
                [-2.0, -0.4, -2.0],
                [2.0, 0.4, -2.0],
                [2.0, 0.4, 2.0],
                [-2.0, -0.4, 2.0],
            ],
            normals: vec![[0.0, 1.0, 0.0]; 4],
            indices: vec![0, 2, 1, 0, 3, 2],
        };
        let specimen = Specimen::single(vec![
            Part::new([tags::GROUND], tilted),
            Part::new([tags::ROCK], SurfaceMesh::ground(1.0, 1)),
        ]);
        let grid = HeightGrid::from_specimen(&specimen, 0, &TagFilter::any_of([tags::GROUND]), 0.1)
            .unwrap();
        assert!((grid.height(1.0, 0.5) - 0.2).abs() < 1e-3);
        assert!(grid.height(3.0, 0.0).is_nan());
        assert!(
            HeightGrid::from_specimen(&specimen, 0, &TagFilter::any_of([tags::BARK]), 0.1)
                .is_none()
        );
    }

    #[test]
    fn closures_and_flat_ground_are_grounds() {
        let slope = |x: f32, _z: f32| 0.5 * x;
        assert_eq!(slope.height(2.0, 7.0), 1.0);
        assert_eq!(FlatGround.height(3.0, -4.0), 0.0);
    }

    #[test]
    fn triangle_height_is_interpolated() {
        let (a, b, c) = (
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(0.0, 2.0, 1.0),
        );
        let h = height_in_triangle(Vec2::new(0.25, 0.25), a, b, c).unwrap();
        assert!((h - 0.75).abs() < 1e-5);
        assert!(height_in_triangle(Vec2::new(2.0, 2.0), a, b, c).is_none());
    }
}
