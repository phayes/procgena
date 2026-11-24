//! A plant Scatter's ground and density, sampled once over an area, so a
//! renderer can grow grass there without asking the ground again.

use glam::{DVec2, Vec2};
use procgena_core::scatter::lattice::to_metres;
use procgena_core::scatter::{Disc, ScatterInput};

use crate::scatter::PlantScatterOptions;

/// Most texels along either side of a [`PlantBake`]: 4096 by 4096 heights
/// and densities is 128 MB.
pub const MAX_BAKE_TEXELS: u32 = 4096;

/// A plant Scatter's ground and density over one area, sampled on a grid of
/// texels.
///
/// Make one with [`PlantScatterOptions::bake`]. Texels are laid out row by
/// row, the first row along the area's −Z edge and each row from −X to +X,
/// the layout of a plant heightmap and density mask.
#[derive(Debug, Clone, PartialEq)]
pub struct PlantBake {
    /// Texels along X and along Z.
    pub texels: [u32; 2],
    /// The area's width (X) and depth (Z), in metres.
    pub size: Vec2,
    /// The ground's height at each texel's centre, in metres. NaN where
    /// there is no ground: nothing grows there.
    pub heights: Vec<f32>,
    /// How much of the Scatter's density grows at each texel, from 0 (none)
    /// to 1 (all of it).
    pub density: Vec<f32>,
}

impl PlantBake {
    /// The position of texel `(x, z)`'s centre, in metres from the area's
    /// centre.
    #[must_use]
    pub fn texel_centre(&self, x: u32, z: u32) -> Vec2 {
        let cell = self.size / Vec2::new(self.texels[0] as f32, self.texels[1] as f32);
        (Vec2::new(x as f32, z as f32) + 0.5) * cell - self.size * 0.5
    }
}

impl PlantScatterOptions {
    /// Samples the ground and density for this Scatter over `input`'s area,
    /// one texel every `cell_size` metres.
    ///
    /// The grid has at least one and at most [`MAX_BAKE_TEXELS`] texels
    /// along each side, so a cell may come out a little smaller than asked,
    /// or larger for an area over 4096 cells across. At each texel the
    /// density is the input's mask at the texel's centre, and 0 where:
    ///
    /// - the ground has no height (a hole, or past its edge);
    /// - the ground is steeper than [`max_slope`](Self::max_slope);
    /// - any footprint in the input's `exclude` touches the texel, so no
    ///   plant grows inside a trunk or a boulder. A texel only partly
    ///   covered is cleared too, which leaves a bare rim up to one texel
    ///   wide around each footprint.
    #[must_use]
    pub fn bake(&self, input: &ScatterInput<'_>, cell_size: f32) -> PlantBake {
        // The grid: whole texels over the area.
        let size = input.area.size.max(Vec2::ZERO);
        let count =
            |length: f32| ((length / cell_size.max(1e-3)).ceil() as u32).clamp(1, MAX_BAKE_TEXELS);
        let texels = [count(size.x), count(size.y)];
        let mut bake = PlantBake {
            texels,
            size,
            heights: Vec::with_capacity((texels[0] * texels[1]) as usize),
            density: Vec::with_capacity((texels[0] * texels[1]) as usize),
        };

        // Heights, holes, the slope limit and the mask, at each centre.
        let steepest = self.max_slope.clamp(0.0, 90.0);
        let max_rise = steepest.to_radians().tan();
        for z in 0..texels[1] {
            for x in 0..texels[0] {
                let local = bake.texel_centre(x, z);
                let ground = input.sample(local);
                let rise = ground.gradient.length();
                // An unknown slope (NaN) counts as not too steep.
                let too_steep = steepest < 90.0 && rise > max_rise;
                let grows = ground.height.is_finite() && !too_steep;
                bake.heights.push(if ground.height.is_finite() {
                    ground.height
                } else {
                    f32::NAN
                });
                bake.density
                    .push(if grows { input.density(local) } else { 0.0 });
            }
        }

        // Clear every texel a footprint touches.
        if let Some(exclude) = input.exclude {
            for disc in exclude.discs() {
                clear_disc(&mut bake, input.area.centre, disc);
            }
        }
        bake
    }
}

/// Sets the density to 0 on every texel of `bake` that `disc` overlaps.
/// `centre` is the World position of the bake's middle.
fn clear_disc(bake: &mut PlantBake, centre: DVec2, disc: &Disc) {
    let radius = to_metres(disc.radius);
    if radius <= 0.0 {
        return;
    }
    // The disc's centre from the bake's −X, −Z corner.
    let middle = disc.centre.to_world() - centre + bake.size.as_dvec2() * 0.5;
    let [columns, rows] = bake.texels.map(f64::from);
    let cell = bake.size.as_dvec2() / DVec2::new(columns, rows);

    // The texels the disc's bounds reach, clipped to the grid.
    let first = ((middle - radius) / cell).floor();
    let last = ((middle + radius) / cell).floor();
    if last.x < 0.0 || last.y < 0.0 || first.x >= columns || first.y >= rows {
        return;
    }
    let clip = |v: f64, n: f64| v.clamp(0.0, n - 1.0) as u32;
    let (x0, x1) = (clip(first.x, columns), clip(last.x, columns));
    let (z0, z1) = (clip(first.y, rows), clip(last.y, rows));

    // A texel is touched when its nearest point to the disc's centre lies
    // inside the disc.
    for z in z0..=z1 {
        for x in x0..=x1 {
            let low = DVec2::new(f64::from(x), f64::from(z)) * cell;
            let nearest = middle.clamp(low, low + cell);
            if nearest.distance_squared(middle) < radius * radius {
                bake.density[(z * bake.texels[0] + x) as usize] = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests;
