//! Positions rocks on flat ground and suggests how far to bury them.
//!
//! Placement lifts the rock to ground level (`y = 0`). Settling optionally
//! turns it onto a resting side first. The suggested embed depth is separate:
//! lower the placed rock by that amount to sink it into the ground.

use glam::{Mat3, Vec3};

use crate::field::StoneField;
use crate::grid::SampleGrid;
use crate::mesh::{StoneMesh, crossing_edges, crossing_position};
use crate::options::PlacementOptions;
use crate::parallel;

/// Cells per axis used to estimate placement, independently of mesh detail.
pub(crate) const ANALYSIS_RESOLUTION: u32 = 64;
/// Number of directions sampled around the rock to find a resting side.
pub(crate) const DIRECTION_COUNT: u32 = 512;
/// Maximum number of moves towards a lower, more stable resting position.
pub(crate) const MAX_DESCENT_STEPS: u32 = 64;
/// Minimum dot product for directions close enough to compare as neighbours.
pub(crate) const NEIGHBOUR_COS: f32 = 0.95;
/// Number of searches with smaller tilts to improve the resting position.
pub(crate) const REFINE_ROUNDS: u32 = 3;
/// Number of nearby directions tested per search, including the current direction.
pub(crate) const REFINE_CANDIDATES: u32 = 48;
/// Initial range of nearby tilts, approximately in radians.
pub(crate) const REFINE_SPREAD: f32 = 0.157;
/// Angular spacing, in turns, used to distribute candidate directions.
pub(crate) const GOLDEN_FRACTION: f32 = 0.381_966_02;
/// Weights for averaging neighbouring horizontal slices when estimating width.
pub(crate) const PROFILE_KERNEL: [u32; 7] = [1, 6, 15, 20, 15, 6, 1];
/// Percentage of the widest slice's area that a lower slice needs to count as
/// nearly as wide, and so be chosen as the ground line instead.
pub(crate) const NEARLY_WIDEST_PERCENT: u32 = 90;

/// A rock's ground placement and suggested burial depth.
///
/// Positions in a placed mesh use *placed space*: stone-space units (see
/// [`StoneField`]) with the estimated lowest point at `y = 0` and its centre
/// of mass (its balance point, assuming uniform density) directly above the
/// origin. Multiply by [`crate::StoneOptions::scale`]
/// for metres; [`crate::Stone`] does this for you.
///
/// Settling approximates the resting side after a drop from the generated
/// orientation. All mesh detail levels share the same placement; their lowest
/// vertices may sit slightly above or below ground because surface estimates
/// vary with detail. Placement does not apply the suggested burial depth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StonePlacement {
    rotation: Mat3,
    translation: Vec3,
    embed_depth: f32,
}

impl StonePlacement {
    /// Estimates ground placement and burial depth for `field` with `options`.
    ///
    /// Pass a finite `max_embed_fraction` between 0 and 1, inclusive; this
    /// function does not validate it. See [`crate::StoneOptions::validate`].
    /// A rock with no detected interior or surface gets no rotation or offset
    /// and an embed depth of zero. Very small features may go undetected.
    pub fn new(field: &StoneField, options: &PlacementOptions) -> Self {
        let grid = SampleGrid::sample(field, ANALYSIS_RESOLUTION);
        Self::from_grid(&grid, options)
    }

    /// Estimates placement from `grid`; pass valid placement settings as for [`Self::new`].
    pub(crate) fn from_grid(grid: &SampleGrid, options: &PlacementOptions) -> Self {
        // Estimate the balance point and collect surface points relative to it.
        let Some(com) = center_of_mass(grid) else {
            return Self::EMPTY;
        };
        let offsets: Vec<Vec3> = crossing_edges(grid)
            .iter()
            .map(|&(s, axis)| crossing_position(grid, s, axis) - com)
            .collect();
        if offsets.is_empty() {
            return Self::EMPTY;
        }

        // Turn the chosen resting side down and lift its lowest surface point to y = 0.
        let (down, height) = if options.settle {
            settle_direction(&offsets)
        } else {
            (Vec3::NEG_Y, support_height(&offsets, Vec3::NEG_Y))
        };
        let rotation = rotation_to_down(down);
        let translation = Vec3::new(0.0, height, 0.0) - rotation * com;

        // Limit the burial suggestion to the allowed lower portion of the rock.
        let top = height + support_height(&offsets, -down);
        let limit = options.max_embed_fraction * top;
        let minimum = options.min_embed_fraction * top;
        let embed_depth = widest_height(grid, rotation, translation, limit).max(minimum);
        Self {
            rotation,
            translation,
            embed_depth,
        }
    }

    /// Leaves positions unchanged and suggests no burial for an undetected rock.
    const EMPTY: Self = Self {
        rotation: Mat3::IDENTITY,
        translation: Vec3::ZERO,
        embed_depth: 0.0,
    };

    /// Returns the suggested burial depth in unscaled stone-space units.
    ///
    /// Lower the placed rock by this amount to put the ground near its widest
    /// horizontal slice within the configured height limit. Multiply by
    /// [`crate::StoneOptions::scale`] for metres. For example, a depth of
    /// `0.2` at scale `3.0` suggests lowering the rock by 0.6 m.
    /// This is an estimate, not a guarantee that all overhangs will be buried.
    pub fn embed_depth(&self) -> f32 {
        self.embed_depth
    }

    /// Returns a conservative radius enclosing the placed rock at any mesh
    /// detail level, centred on the placed-space origin. Multiply by
    /// [`crate::StoneOptions::scale`] to convert to metres.
    pub fn bounding_radius(&self) -> f32 {
        self.translation.length() + 0.75f32.sqrt()
    }

    /// Rotates and offsets an unplaced mesh from [`crate::mesh_field`] onto
    /// the ground, including rotating its normals. Apply once, before scaling
    /// to metres. This does not sink the mesh by its embed depth.
    pub fn apply(&self, mesh: &mut StoneMesh) {
        for p in &mut mesh.positions {
            *p = (self.rotation * Vec3::from(*p) + self.translation).to_array();
        }
        for n in &mut mesh.normals {
            *n = (self.rotation * Vec3::from(*n)).to_array();
        }
    }

    /// Converts an unscaled, placed position back to coordinates for sampling
    /// the original [`StoneField`]. Undo any world transform and divide by
    /// [`crate::StoneOptions::scale`] before passing a world-space position.
    pub fn field_position(&self, placed: Vec3) -> Vec3 {
        self.rotation.transpose() * (placed - self.translation)
    }

    /// Returns the rotation applied before the ground-placement offset.
    #[cfg(test)]
    pub(crate) fn rotation(&self) -> Mat3 {
        self.rotation
    }

    /// Returns the unscaled offset applied after rotating the rock.
    #[cfg(test)]
    pub(crate) fn translation(&self) -> Vec3 {
        self.translation
    }
}

/// Estimates the centre of mass: the balance point of a uniformly dense rock.
/// Averages interior sample positions; returns `None` if there are none.
fn center_of_mass(grid: &SampleGrid) -> Option<Vec3> {
    let n = grid.samples_per_axis();
    let (mut sum, mut count) = ([0u32; 3], 0u32);
    for z in 0..n {
        for y in 0..n {
            for x in 0..n {
                if grid.inside([x, y, z]) {
                    sum = [sum[0] + x, sum[1] + y, sum[2] + z];
                    count += 1;
                }
            }
        }
    }
    if count == 0 {
        return None;
    }
    let mean = Vec3::new(sum[0] as f32, sum[1] as f32, sum[2] as f32) / count as f32;
    Some(mean / grid.resolution() as f32 - Vec3::splat(0.5))
}

/// Returns the support height: the balance point's distance above flat ground
/// for unit direction `down`. Pass surface offsets from the balance point.
/// An empty slice returns negative infinity.
fn support_height(offsets: &[Vec3], down: Vec3) -> f32 {
    offsets
        .iter()
        .fold(f32::NEG_INFINITY, |h, &p| h.max(p.dot(down)))
}

/// Returns unit direction `i` from the candidates distributed around the rock.
/// Pass `i < DIRECTION_COUNT`; the last direction is nearest to -y.
pub(crate) fn sphere_direction(i: u32) -> Vec3 {
    let y = 1.0 - (2 * i + 1) as f32 / DIRECTION_COUNT as f32;
    let r = (1.0 - y * y).max(0.0).sqrt();
    let angle = (i as f32 * GOLDEN_FRACTION).fract() * std::f32::consts::TAU;
    Vec3::new(r * angle.cos(), y, r * angle.sin())
}

/// Returns a nearby tilt of unit `down`, within roughly `spread` radians.
/// Pass `j < REFINE_CANDIDATES`; zero returns `down` unchanged.
pub(crate) fn refine_direction(down: Vec3, spread: f32, j: u32) -> Vec3 {
    if j == 0 {
        return down;
    }
    let (u, v) = tangent_basis(down);
    let radius = spread * (j as f32 / (REFINE_CANDIDATES - 1) as f32).sqrt();
    let angle = (j as f32 * GOLDEN_FRACTION).fract() * std::f32::consts::TAU;
    (down + radius * (angle.cos() * u + angle.sin() * v)).normalize()
}

/// Returns two perpendicular unit directions spanning the plane across unit `d`.
fn tangent_basis(d: Vec3) -> (Vec3, Vec3) {
    let helper = if d.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
    let u = helper.cross(d).normalize();
    (u, d.cross(u))
}

/// Estimates a resting side from nonempty surface offsets about the balance
/// point. Returns a unit down direction and the balance point's ground height.
/// Starts near the generated orientation and favours tilts that lower it.
fn settle_direction(offsets: &[Vec3]) -> (Vec3, f32) {
    // Compare balance-point heights for the candidate resting sides.
    let directions: Vec<u32> = (0..DIRECTION_COUNT).collect();
    let heights = parallel::map(&directions, parallel::PARALLEL, |&i| {
        support_height(offsets, sphere_direction(i))
    });

    // Start near upright and move to lower neighbouring orientations.
    let mut current = DIRECTION_COUNT - 1;
    for _ in 0..MAX_DESCENT_STEPS {
        let here = sphere_direction(current);
        let mut best = current;
        for i in 0..DIRECTION_COUNT {
            if sphere_direction(i).dot(here) >= NEIGHBOUR_COS
                && heights[i as usize] < heights[best as usize]
            {
                best = i;
            }
        }
        if best == current {
            break;
        }
        current = best;
    }

    // Try progressively smaller tilts around the selected orientation.
    let mut down = sphere_direction(current);
    let mut height = heights[current as usize];
    let mut spread = REFINE_SPREAD;
    for _ in 0..REFINE_ROUNDS {
        let candidates: Vec<u32> = (0..REFINE_CANDIDATES).collect();
        let tried = parallel::map(&candidates, parallel::PARALLEL, |&j| {
            let d = refine_direction(down, spread, j);
            (d, support_height(offsets, d))
        });
        for &(d, h) in &tried[1..] {
            if h < height {
                (down, height) = (d, h);
            }
        }
        spread *= 0.5;
    }
    (down, height)
}

/// Returns a rotation that points the chosen unit `down` direction towards the ground.
///
/// Upward directions include a half-turn about x before the final tilt.
pub(crate) fn rotation_to_down(down: Vec3) -> Mat3 {
    if down.y > 0.0 {
        let half_turn = Mat3::from_diagonal(Vec3::new(1.0, -1.0, -1.0));
        return smallest_rotation_to_down(half_turn * down) * half_turn;
    }
    smallest_rotation_to_down(down)
}

/// Returns the shortest turn from unit `down` to -y. Requires `down.y <= 0`.
fn smallest_rotation_to_down(down: Vec3) -> Mat3 {
    let c = -down.y;
    // Build the rotation from the turn's axis and angle.
    let v = down.cross(Vec3::NEG_Y);
    let k = Mat3::from_cols(
        Vec3::new(0.0, v.z, -v.y),
        Vec3::new(-v.z, 0.0, v.x),
        Vec3::new(v.y, -v.x, 0.0),
    );
    Mat3::IDENTITY + k + k * k * (1.0 / (1.0 + c))
}

/// Returns the number of horizontal slices needed for a rotated sampling cube.
/// Each slice is one grid cell tall at the supplied `resolution`.
pub(crate) const fn profile_bins(resolution: u32) -> u32 {
    (resolution * 7).div_ceil(4) + 1
}

/// Returns the height of the lowest horizontal slice that is nearly as wide as
/// the widest one, after applying `rotation` and `translation`, considering
/// only slice centres at or below unscaled `limit`.
///
/// "Nearly as wide" means at least [`NEARLY_WIDEST_PERCENT`] of the widest
/// slice's area, so straight-sided rocks get a slice near the ground. Returns
/// `limit` if no centre qualifies. Interior sample counts, averaged across
/// nearby slices, approximate area.
fn widest_height(grid: &SampleGrid, rotation: Mat3, translation: Vec3, limit: f32) -> f32 {
    // Group interior samples by their height above ground after placement.
    let resolution = grid.resolution();
    let bins = profile_bins(resolution) as usize;
    let mut counts = vec![0u32; bins];
    let n = grid.samples_per_axis();
    for z in 0..n {
        for y in 0..n {
            for x in 0..n {
                if grid.inside([x, y, z]) {
                    let height = (rotation * grid.position([x, y, z])).y + translation.y;
                    counts[profile_bin(height, resolution)] += 1;
                }
            }
        }
    }

    // Average neighbouring slice areas for every slice centred within the limit.
    // Slices past either end of the profile are left out of the average, so
    // the lowest slices are not made to look narrower than they are.
    let kernel_total: u32 = PROFILE_KERNEL.iter().sum();
    let areas: Vec<u32> = (0..bins)
        .take_while(|&bin| (bin as f32 + 0.5) / resolution as f32 <= limit)
        .map(|bin| {
            let (mut area, mut weight) = (0, 0);
            for (k, &w) in PROFILE_KERNEL.iter().enumerate() {
                let at = bin as i64 + k as i64 - (PROFILE_KERNEL.len() / 2) as i64;
                if (0..bins as i64).contains(&at) {
                    area += w * counts[at as usize];
                    weight += w;
                }
            }
            area * kernel_total / weight
        })
        .collect();

    // Choose the lowest slice that is nearly as wide as the widest one.
    let widest = areas.iter().copied().max().unwrap_or(0);
    areas
        .iter()
        .position(|&area| area * 100 >= widest * NEARLY_WIDEST_PERCENT)
        .map_or(limit, |bin| (bin as f32 + 0.5) / resolution as f32)
}

/// Maps unscaled placed `height` to a horizontal slice at `resolution` cells
/// per axis. Heights outside the profile use the nearest end slice.
pub(crate) fn profile_bin(height: f32, resolution: u32) -> usize {
    let bins = profile_bins(resolution) as i64;
    ((height * resolution as f32).floor() as i64).clamp(0, bins - 1) as usize
}

#[cfg(test)]
mod tests;
