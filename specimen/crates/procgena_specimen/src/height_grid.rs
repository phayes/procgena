//! Ground heights read from a specimen's top surface.

use glam::{Vec2, Vec3};
use procgena_core::HeightGrid;

use crate::{Specimen, TagFilter};

impl Specimen {
    /// Samples the specimen's top surface every `cell` metres, as ground for
    /// scattering things on, such as terrain from another procgena
    /// generator. Plant fields use ground meshes the same way.
    ///
    /// Only the parts of level `level` that `filter` matches are used.
    /// Where several surfaces overlap, the highest one is the ground. Points
    /// that no surface covers, seen from above, have no ground. Returns
    /// `None` when the filter matches nothing. The grid's positions are the
    /// specimen's own coordinates.
    ///
    /// # Panics
    ///
    /// Panics if `cell` is not positive or `level` is out of range.
    pub fn height_grid(&self, level: usize, filter: &TagFilter, cell: f32) -> Option<HeightGrid> {
        assert!(cell > 0.0);
        let part = self.merged(level, filter)?;
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
        Some(HeightGrid::from_samples(
            lo.as_dvec2(),
            f64::from(cell),
            columns,
            rows,
            heights,
        ))
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
    use glam::DVec2;
    use procgena_core::Ground;

    use super::*;
    use crate::{Part, SurfaceMesh, tags};

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
        let grid = specimen
            .height_grid(0, &TagFilter::any_of([tags::GROUND]), 0.1)
            .unwrap();
        assert!((grid.height(DVec2::new(1.0, 0.5)) - 0.2).abs() < 1e-3);
        assert!(grid.height(DVec2::new(3.0, 0.0)).is_nan());
        assert!(
            specimen
                .height_grid(0, &TagFilter::any_of([tags::BARK]), 0.1)
                .is_none()
        );

        // The slope of the tilted square: 0.8 m over 4 m along X.
        let slope = grid.gradient(DVec2::new(0.5, 0.5));
        assert!((slope - glam::Vec2::new(0.2, 0.0)).length() < 1e-3);
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
