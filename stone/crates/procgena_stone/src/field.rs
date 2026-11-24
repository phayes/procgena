//! Sample a rock’s shape without building a mesh.
//!
//! A signed distance field describes a surface with numbers: negative inside,
//! zero on the surface, and positive outside. These fields use that sign
//! convention, but their values are not always exact distances to the surface.

use glam::Vec3;

use crate::noise::Roughness;
use crate::options::{CarveOptions, OptionsError, StoneOptions};
use procgena_core::hash::pcg3d_unit;

/// Fixed hash input that gives cut directions a separate random sequence from cut sizes.
const CARVE_DIRECTION_SALT: u32 = 0x6361_7276;
/// Fixed hash input that gives cut sizes a separate random sequence from cut directions.
const CARVE_RADIUS_SALT: u32 = 0x6361_7277;
/// Outward shift of the field’s surface level, measured in carve-space units.
/// This slightly expands the shape after carving and roughening.
pub(crate) const SURFACE_OFFSET: f32 = 0.025;
/// Distance from the origin to each face of the unit-width stone-space cube.
pub(crate) const HALF_EXTENT: f32 = 0.5;

/// A spherical cut removed from the starting shape.
///
/// The centre and radius use carve-space units, where the containing cube
/// spans -1 to 1 on each axis before stretching.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarveSphere {
    /// Centre of the cutting sphere in carve space.
    pub center: Vec3,
    /// Radius of the cutting sphere in carve-space units.
    pub radius: f32,
}

/// A rock’s basic carved shape before stretching, roughness, and cube clipping.
///
/// Sample positions and distances use *carve space*: coordinates where the
/// containing cube spans -1 to 1 on each axis before stretching. The starting
/// sphere has radius [`base_radius`](Self::base_radius). The cuts returned by
/// [`spheres`](Self::spheres) remove material, with rounded transitions set by
/// [`smoothness`](Self::smoothness).
#[derive(Debug, Clone, PartialEq)]
pub struct CarveSet {
    base_radius: f32,
    smoothness: f32,
    spheres: Vec<CarveSphere>,
}

impl CarveSet {
    /// Creates a repeatable set of cuts from `seed` and the supplied carving settings.
    ///
    /// The caller must supply valid settings; this constructor does not check them.
    /// Use [`StoneOptions::validate`] to check the containing options first.
    /// Invalid settings may produce unusable field values or excessive allocations.
    pub fn new(seed: u32, options: &CarveOptions) -> Self {
        let spheres = (0..options.steps)
            .map(|i| {
                // Normalizing a point sampled from a cube favors diagonal directions.
                // Place the cut at the sampled distance in the opposite direction.
                let d = Vec3::from(pcg3d_unit([i, seed, CARVE_DIRECTION_SALT]));
                let direction = (d * 2.0 - Vec3::ONE).try_normalize().unwrap_or(Vec3::Y);
                let direction = bend_toward_axis(direction, options.blockiness);
                let jitter = pcg3d_unit([i, seed, CARVE_RADIUS_SALT])[0];
                let r = options.radius_min + options.radius_jitter * jitter;
                CarveSphere {
                    center: -direction * r,
                    radius: options.depth * r,
                }
            })
            .collect();
        Self {
            base_radius: options.base_radius,
            smoothness: options.smoothness,
            spheres,
        }
    }

    /// Radius of the starting sphere, in carve-space units.
    pub fn base_radius(&self) -> f32 {
        self.base_radius
    }

    /// Width of the rounded transitions between cuts, in carve-space units.
    pub fn smoothness(&self) -> f32 {
        self.smoothness
    }

    /// Cutting spheres in application order. Their centres and radii use carve-space units.
    pub fn spheres(&self) -> &[CarveSphere] {
        &self.spheres
    }

    /// Samples the carved shape at finite carve-space position `q`.
    ///
    /// The result is negative inside, zero on the surface, and positive outside.
    /// It uses carve-space units but is not necessarily an exact distance to the
    /// surface. Cuts only remove material, so the value cannot be less than the
    /// starting sphere’s field value.
    pub fn distance(&self, q: Vec3) -> f32 {
        self.distance_and_gradient(q).0
    }

    /// Returns the carved field value and its analytic gradient at `q` in carve space.
    ///
    /// A gradient describes how the value changes along each axis. At a sphere’s
    /// centre its distance gradient is undefined; that sphere contributes a zero
    /// vector instead. Later cuts may still contribute a nonzero gradient.
    pub(crate) fn distance_and_gradient(&self, q: Vec3) -> (f32, Vec3) {
        let k = self.smoothness;
        let mut d = q.length() - self.base_radius;
        let mut g = q.normalize_or_zero();
        for sphere in &self.spheres {
            // Remove this sphere with a rounded transition. Blend the current
            // gradient with the negated cutting-sphere gradient using `h`.
            let offset = q - sphere.center;
            let b = offset.length() - sphere.radius;
            let gb = offset.normalize_or_zero();
            let h = (0.5 + 0.5 * (-b - d) / k).clamp(0.0, 1.0);
            d = d + (-b - d) * h + k * h * (1.0 - h);
            g = g * (1.0 - h) - gb * h;
        }
        (d, g)
    }
}

/// Turns a cut’s direction toward the nearest positive or negative x, y, or z axis.
///
/// Supply a direction of length 1 and an `amount` from 0 through 1. Zero keeps
/// the direction unchanged, 1 aligns it with the axis, and intermediate values
/// turn it partway while keeping its length at 1. Equally close axes are chosen
/// in x, y, z order.
fn bend_toward_axis(direction: Vec3, amount: f32) -> Vec3 {
    if amount <= 0.0 {
        return direction;
    }

    // The largest absolute component identifies the nearest axis; its sign
    // chooses the positive or negative direction along that axis.
    let a = direction.abs();
    let axis = if a.x >= a.y && a.x >= a.z {
        Vec3::X * direction.x.signum()
    } else if a.y >= a.z {
        Vec3::Y * direction.y.signum()
    } else {
        Vec3::Z * direction.z.signum()
    };

    // Blend toward the chosen axis, then restore length 1. The directions
    // cannot cancel out because they point the same way along that axis.
    direction.lerp(axis, amount).normalize()
}

/// A rock’s shape after carving, stretching, surface roughness, and cube clipping.
///
/// Supply positions in *stone space*: the containing cube spans -0.5 to 0.5
/// on each axis. [`StoneOptions::scale`] is not applied. To sample a position
/// relative to the rock’s centre in metres, divide it by that scale first.
#[derive(Debug, Clone, PartialEq)]
pub struct StoneField {
    carve: CarveSet,
    carve_scale: Vec3,
    roughness: Roughness,
    displacement: f32,
}

impl StoneField {
    /// Creates a field from validated shape and surface settings.
    ///
    /// All settings are checked, including [`StoneOptions::scale`] and
    /// [`StoneOptions::mesh`], although those two settings do not affect samples.
    /// Valid settings can still describe an empty rock.
    ///
    /// # Errors
    ///
    /// Returns the first error reported by [`StoneOptions::validate`].
    pub fn new(options: &StoneOptions) -> Result<Self, OptionsError> {
        options.validate()?;
        // Larger coordinate factors compress the shape along shorter stretch
        // axes. The longest axis keeps the usual factor of two.
        let stretch = Vec3::from(options.stretch);
        let carve_scale = 2.0 * stretch.max_element() / stretch;
        Ok(Self {
            carve: CarveSet::new(options.default_seed, &options.carve),
            carve_scale,
            roughness: Roughness {
                seed: options.default_seed,
                spread: options.surface.displacement_spread,
                octaves: options.surface.octaves,
            },
            displacement: options.surface.displacement_scale,
        })
    }

    /// Returns the basic cuts before stretching, roughness, and cube clipping.
    /// Samples of this [`CarveSet`] use carve-space coordinates.
    pub fn carve(&self) -> &CarveSet {
        &self.carve
    }

    /// Returns the seed, frequency, and layer count of the surface roughness.
    #[cfg(any(test, feature = "gpu"))]
    pub(crate) fn roughness(&self) -> Roughness {
        self.roughness
    }

    /// Returns the roughness depth; zero or less disables roughness.
    #[cfg(any(test, feature = "gpu"))]
    pub(crate) fn displacement(&self) -> f32 {
        self.displacement
    }

    /// Returns the per-axis factors for converting stone-space positions to carve space.
    ///
    /// Multiply a stone-space position by this vector before sampling [`Self::carve`].
    /// The factor is 2 on the longest stretch axis and larger on shorter axes.
    pub fn carve_scale(&self) -> Vec3 {
        self.carve_scale
    }

    /// Samples the rock at finite stone-space position `p`.
    ///
    /// Returns a negative value inside, zero on the surface, and a positive value
    /// outside. Points on or beyond the containing cube’s boundary have values
    /// of at least zero.
    ///
    /// The magnitude is not an exact distance to the surface, especially with
    /// unequal stretch or surface roughness. It is not in metres and should
    /// not be treated as a guaranteed safe step size for ray tracing.
    pub fn distance(&self, p: Vec3) -> f32 {
        // Sample the carved shape in carve space. Transform its gradient back
        // to stone space to orient the roughness patterns.
        let (rock, rock_gradient) = self.carve.distance_and_gradient(p * self.carve_scale);
        let roughness = if self.displacement > 0.0 {
            let normal = rock_gradient * self.carve_scale;
            self.displacement * self.roughness.triplanar(p, normal).length()
        } else {
            0.0
        };
        let stone = 0.5 * (rock + roughness - SURFACE_OFFSET);

        // Keep only the part of the rock inside the unit-width cube.
        let cube = p.abs().max_element() - HALF_EXTENT;
        stone.max(cube)
    }

    /// Estimates how the field changes along each axis at stone-space position `p`.
    ///
    /// Pass a finite position and a finite, positive `step` in stone-space units.
    /// About half a sampling-grid cell is a useful starting point. Smaller steps
    /// capture finer changes but may lose precision; larger steps smooth detail.
    /// These inputs are not validated.
    ///
    /// Near a regular surface the result points outward. Its length is not
    /// necessarily one, and it may be zero; normalize a nonzero result to use it
    /// as a surface normal.
    pub fn gradient(&self, p: Vec3, step: f32) -> Vec3 {
        let axis = |e: Vec3| self.distance(p + e * step) - self.distance(p - e * step);
        Vec3::new(axis(Vec3::X), axis(Vec3::Y), axis(Vec3::Z)) / (2.0 * step)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEEDS: [u32; 8] = [0, 1, 2, 3, 880, 12345, 0xdead_beef, u32::MAX];

    fn field(seed: u32) -> StoneField {
        let options = StoneOptions {
            default_seed: seed,
            ..StoneOptions::default()
        };
        StoneField::new(&options).unwrap()
    }

    /// Generates unit directions roughly evenly spread over a sphere with a Fibonacci spiral.
    fn directions(count: usize) -> impl Iterator<Item = Vec3> {
        let golden = std::f32::consts::PI * (3.0 - 5f32.sqrt());
        (0..count).map(move |i| {
            let y = 1.0 - 2.0 * (i as f32 + 0.5) / count as f32;
            let r = (1.0 - y * y).sqrt();
            let a = golden * i as f32;
            Vec3::new(r * a.cos(), y, r * a.sin())
        })
    }

    /// Estimates the surface radius along unit direction `dir` for the test field.
    ///
    /// Assumes radius 0.5 is outside the rock and that the inward scan finds an
    /// interior sample before reaching the origin. Very thin features can be missed.
    fn surface_radius(field: &StoneField, dir: Vec3) -> f32 {
        // Scan inward from radius 0.5 to bracket a surface crossing, then
        // repeatedly halve that interval to refine its location.
        let mut outside = 0.5;
        let mut inside = outside;
        while field.distance(dir * inside) >= 0.0 {
            outside = inside;
            inside -= 0.002;
            assert!(inside > 0.0, "no surface along {dir}");
        }
        for _ in 0..30 {
            let mid = 0.5 * (inside + outside);
            if field.distance(dir * mid) < 0.0 {
                inside = mid;
            } else {
                outside = mid;
            }
        }
        0.5 * (inside + outside)
    }

    #[test]
    fn carve_set_follows_options() {
        let options = CarveOptions::default();
        let carve = CarveSet::new(880, &options);
        assert_eq!(carve.spheres().len(), options.steps as usize);
        for sphere in carve.spheres() {
            let r = sphere.center.length();
            assert!((options.radius_min..options.radius_min + options.radius_jitter).contains(&r));
            assert!((sphere.radius - options.depth * r).abs() < 1e-5);
        }
        assert_ne!(carve, CarveSet::new(881, &options));
    }

    #[test]
    fn bend_toward_axis_moves_monotonically_to_the_dominant_axis() {
        for dir in directions(64) {
            assert_eq!(bend_toward_axis(dir, 0.0), dir);

            // At full blockiness, the direction lies exactly along a positive
            // or negative x, y, or z axis.
            let axis = bend_toward_axis(dir, 1.0);
            assert_eq!(axis.abs().max_element(), 1.0, "{dir} -> {axis}");
            assert_eq!(axis.abs().element_sum(), 1.0, "{dir} -> {axis}");
            assert!(axis.dot(dir) > 0.0, "{dir} -> {axis}");

            // Increasing the amount turns the direction closer to that axis
            // while keeping its length at 1.
            let mut previous = dir.dot(axis);
            for amount in [0.25, 0.5, 0.75] {
                let bent = bend_toward_axis(dir, amount);
                assert!((bent.length() - 1.0).abs() < 1e-5);
                let closeness = bent.dot(axis);
                assert!(closeness >= previous - 1e-6, "{dir} at {amount}");
                previous = closeness;
            }
        }
    }

    #[test]
    fn full_blockiness_puts_every_cut_on_an_axis() {
        let options = CarveOptions {
            blockiness: 1.0,
            ..CarveOptions::default()
        };
        for seed in SEEDS {
            let carve = CarveSet::new(seed, &options);
            let random = CarveSet::new(seed, &CarveOptions::default());
            for (sphere, unbent) in carve.spheres().iter().zip(random.spheres()) {
                let off_axis =
                    sphere.center.abs().element_sum() - sphere.center.abs().max_element();
                assert_eq!(off_axis, 0.0, "seed {seed}: centre {}", sphere.center);
                // Turning a cut leaves its radius and its centre’s distance
                // from the rock’s centre unchanged.
                assert_eq!(sphere.radius, unbent.radius);
                assert!((sphere.center.length() - unbent.center.length()).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn full_blockiness_gives_flat_axis_aligned_sides() {
        // With roughness disabled, check that each side is nearly flat near
        // its centre: moving sideways should barely change the surface’s
        // position along the axis that points towards that side.
        let mut options = StoneOptions::default();
        options.carve.blockiness = 1.0;
        options.surface.displacement_scale = 0.0;
        for seed in SEEDS {
            options.default_seed = seed;
            let field = StoneField::new(&options).unwrap();
            for axis in [
                Vec3::X,
                Vec3::NEG_X,
                Vec3::Y,
                Vec3::NEG_Y,
                Vec3::Z,
                Vec3::NEG_Z,
            ] {
                let centre = surface_radius(&field, axis);
                let side = axis.any_orthogonal_vector().normalize() * 0.06;
                let shifted = surface_radius(&field, (axis * centre + side).normalize());
                let along_axis = shifted * (axis * centre + side).normalize().dot(axis);
                assert!(
                    (along_axis - centre).abs() < 0.01,
                    "seed {seed} along {axis}: side at {centre}, {along_axis} off centre"
                );
            }
        }
    }

    #[test]
    fn carve_gradient_matches_finite_differences() {
        let carve = CarveSet::new(5, &CarveOptions::default());
        for dir in directions(64) {
            let q = dir * 0.83;
            let (_, g) = carve.distance_and_gradient(q);
            let e = 1e-3;
            let fd = Vec3::new(
                carve.distance(q + Vec3::X * e) - carve.distance(q - Vec3::X * e),
                carve.distance(q + Vec3::Y * e) - carve.distance(q - Vec3::Y * e),
                carve.distance(q + Vec3::Z * e) - carve.distance(q - Vec3::Z * e),
            ) / (2.0 * e);
            assert!(
                (g - fd).length() < 1e-2,
                "at {q}: exact {g}, estimated {fd}"
            );
        }
    }

    #[test]
    fn inside_at_centre_and_outside_on_cube_boundary() {
        for seed in SEEDS {
            let field = field(seed);
            assert!(field.distance(Vec3::ZERO) < 0.0, "seed {seed}");
            for i in 0..=20 {
                for j in 0..=20 {
                    let a = i as f32 / 20.0 - 0.5;
                    let b = j as f32 / 20.0 - 0.5;
                    for p in [
                        Vec3::new(0.5, a, b),
                        Vec3::new(-0.5, a, b),
                        Vec3::new(a, 0.5, b),
                        Vec3::new(a, -0.5, b),
                        Vec3::new(a, b, 0.5),
                        Vec3::new(a, b, -0.5),
                    ] {
                        assert!(field.distance(p) > 0.0, "seed {seed} at {p}");
                    }
                }
            }
        }
    }

    #[test]
    fn same_seed_same_field_different_seed_different_field() {
        let points: Vec<Vec3> = directions(50).map(|d| d * 0.42).collect();
        let sample = |f: &StoneField| points.iter().map(|&p| f.distance(p)).collect::<Vec<_>>();
        assert_eq!(sample(&field(880)), sample(&field(880)));
        assert_ne!(sample(&field(880)), sample(&field(881)));
    }

    #[test]
    fn surface_radius_stays_between_derived_bounds_at_defaults() {
        let options = StoneOptions::default();
        let carve = &options.carve;

        // Cuts and roughness cannot expand the starting sphere. Allow for the
        // outward surface offset, then convert its radius to stone space.
        let outer = 0.5 * (carve.base_radius + SURFACE_OFFSET);

        // At the defaults, each cut stays at least (1 - depth) * radius_min
        // from the origin in carve space. Allow smoothness / 4 for rounding
        // and displacement_scale * sqrt(3) for roughness, then include the
        // surface offset and convert the remaining radius to stone space.
        let cut = (1.0 - carve.depth) * carve.radius_min - carve.smoothness / 4.0;
        let roughness = options.surface.displacement_scale * 3f32.sqrt();
        let inner = 0.5 * (cut - roughness + SURFACE_OFFSET);
        assert!(inner > 0.1, "inner bound {inner} is too weak to be useful");

        for seed in SEEDS {
            let field = field(seed);
            for dir in directions(200) {
                let r = surface_radius(&field, dir);
                assert!(
                    r <= outer + 1e-4,
                    "seed {seed} along {dir}: radius {r} > {outer}"
                );
                assert!(r >= inner, "seed {seed} along {dir}: radius {r} < {inner}");
            }
        }
    }

    #[test]
    fn stretch_flattens_the_short_axis() {
        let options = StoneOptions {
            stretch: [1.0, 0.5, 1.0],
            ..StoneOptions::default()
        };
        let field = StoneField::new(&options).unwrap();
        assert_eq!(field.carve_scale(), Vec3::new(2.0, 4.0, 2.0));
        let height = surface_radius(&field, Vec3::Y) + surface_radius(&field, -Vec3::Y);
        let width = surface_radius(&field, Vec3::X) + surface_radius(&field, -Vec3::X);
        assert!(height < 0.7 * width, "height {height}, width {width}");
    }

    #[test]
    fn gradient_points_outward_near_the_surface() {
        for seed in SEEDS {
            let field = field(seed);
            for dir in directions(100) {
                let p = dir * surface_radius(&field, dir);
                let g = field.gradient(p, 0.005);
                assert!(g.dot(dir) > 0.0, "seed {seed} at {p}: gradient {g}");
            }
        }
    }

    #[test]
    fn invalid_options_are_rejected() {
        let mut options = StoneOptions::default();
        options.carve.smoothness = 0.0;
        assert!(StoneField::new(&options).is_err());
    }
}
