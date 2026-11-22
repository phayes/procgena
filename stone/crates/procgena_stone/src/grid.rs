//! Samples a rock on an evenly spaced grid to locate its surface.

use glam::Vec3;

use crate::field::StoneField;
use crate::parallel;

/// Maps a sample index to a coordinate in the unit-width stone cube.
///
/// Pass `resolution > 0` and `i` from 0 through `resolution`: the endpoints map
/// to -0.5 and 0.5. These bounds are not checked.
pub(crate) fn grid_coordinate(i: u32, resolution: u32) -> f32 {
    i as f32 / resolution as f32 - 0.5
}

/// Distance-field samples covering -0.5 to 0.5 on each axis.
///
/// A grid with `resolution` cells per axis has `resolution + 1` samples per
/// axis. Negative samples are inside the rock; zero and positive samples are
/// not. Samples are stored with x changing fastest, followed by y and z.
pub(crate) struct SampleGrid {
    resolution: u32,
    values: Vec<f32>,
}

impl SampleGrid {
    /// Samples `field` with the given number of cells per axis.
    ///
    /// The caller must supply a positive resolution small enough for the sample
    /// buffer to fit in memory. Uses multiple threads when `rayon` is enabled.
    pub(crate) fn sample(field: &StoneField, resolution: u32) -> Self {
        Self::sample_with(field, resolution, parallel::PARALLEL)
    }

    /// Samples `field`, optionally using multiple threads when `rayon` is enabled.
    ///
    /// The same resolution and memory requirements as [`Self::sample`] apply.
    fn sample_with(field: &StoneField, resolution: u32, parallel: bool) -> Self {
        let n = resolution as usize + 1;
        let mut values = vec![0.0; n * n * n];
        parallel::for_each_chunk_mut(&mut values, n * n, parallel, |z, plane| {
            let pz = grid_coordinate(z as u32, resolution);
            for (y, row) in plane.chunks_mut(n).enumerate() {
                let py = grid_coordinate(y as u32, resolution);
                for (x, value) in row.iter_mut().enumerate() {
                    let px = grid_coordinate(x as u32, resolution);
                    *value = field.distance(Vec3::new(px, py, pz));
                }
            }
        });
        Self { resolution, values }
    }

    /// Returns the number of cells along each axis.
    pub(crate) fn resolution(&self) -> u32 {
        self.resolution
    }

    /// Returns the number of samples along each axis, including both endpoints.
    pub(crate) fn samples_per_axis(&self) -> u32 {
        self.resolution + 1
    }

    /// Returns the flat buffer index for sample coordinates `[x, y, z]`.
    ///
    /// Each coordinate must be at most `resolution`. Coordinates are not checked.
    pub(crate) fn index(&self, s: [u32; 3]) -> usize {
        let n = self.samples_per_axis() as usize;
        s[0] as usize + n * (s[1] as usize + n * s[2] as usize)
    }

    /// Returns whether sample `s` has a negative value, meaning it is inside the rock.
    ///
    /// Each coordinate must be at most `resolution`. An invalid coordinate may
    /// read the wrong sample or panic if the resulting buffer index is out of bounds.
    pub(crate) fn inside(&self, s: [u32; 3]) -> bool {
        self.values[self.index(s)] < 0.0
    }

    /// Returns the distance-field value at sample coordinates `[x, y, z]`.
    ///
    /// Each coordinate must be at most `resolution`. An invalid coordinate may
    /// read the wrong sample or panic if the resulting buffer index is out of bounds.
    pub(crate) fn value(&self, s: [u32; 3]) -> f32 {
        self.values[self.index(s)]
    }

    /// Returns a sample's position in the unit-width stone cube.
    ///
    /// Each coordinate of `s` should be at most `resolution`; larger values produce
    /// positions outside the cube without a bounds check.
    pub(crate) fn position(&self, s: [u32; 3]) -> Vec3 {
        let r = self.resolution;
        Vec3::new(
            grid_coordinate(s[0], r),
            grid_coordinate(s[1], r),
            grid_coordinate(s[2], r),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoneOptions;

    fn field() -> StoneField {
        StoneField::new(&StoneOptions::default()).unwrap()
    }

    #[test]
    fn coordinates_reach_the_cube_faces_exactly() {
        for r in [8, 24, 100, 511, 512] {
            assert_eq!(grid_coordinate(0, r), -0.5);
            assert_eq!(grid_coordinate(r, r), 0.5);
        }
    }

    #[test]
    fn boundary_samples_are_outside() {
        // Even when the starting rock extends beyond the cube, clipping must
        // keep every boundary sample from being classified as inside.
        let mut options = StoneOptions::default();
        options.carve.base_radius = 1.5;
        options.carve.steps = 0;
        let field = StoneField::new(&options).unwrap();
        let grid = SampleGrid::sample(&field, 12);
        let n = grid.samples_per_axis();
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    let on_face = [x, y, z].iter().any(|&c| c == 0 || c == n - 1);
                    if on_face {
                        assert!(!grid.inside([x, y, z]), "sample {x},{y},{z}");
                    }
                }
            }
        }
        assert!(grid.inside([6, 6, 6]));
    }

    #[test]
    fn parallel_and_serial_sampling_match() {
        let field = field();
        let serial = SampleGrid::sample_with(&field, 20, false);
        let parallel = SampleGrid::sample_with(&field, 20, true);
        assert_eq!(serial.values, parallel.values);
        assert_eq!(serial.values.len(), 21 * 21 * 21);
    }
}
