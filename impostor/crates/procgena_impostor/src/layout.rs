//! Where each view of an impostor sits in its atlas: the directions an
//! impostor is baked for, laid out as an octahedron folded flat.
//!
//! The atlas is `frames` × `frames` cells. The cell in the middle looks
//! straight down on the object. Which other directions the cells cover
//! depends on the impostor's [`ViewCoverage`]:
//!
//! - [`ViewCoverage::SPHERE`] covers every direction. The upper half of the
//!   sphere fills a diamond in the middle of the atlas, and the lower half
//!   the four corners, which look at the object from straight below.
//! - Any other coverage spreads the views it covers, from straight above
//!   down to its lowest elevation, over the whole atlas. The corners look
//!   from the lowest elevation. At [`ViewCoverage::UPPER_HEMISPHERE`] the
//!   corners look from the horizon.
//!
//! [`view_direction`] and [`view_cell`] convert between cells and
//! directions, and [`view_basis`] gives the axes a view's image is drawn
//! along.
//!
//! Atlases baked with the [`Elevations`](crate::BakeLayout::Elevations)
//! layout instead hold one strip of views around the object's +Y axis, from
//! below to above: see [`elevation_direction`], [`elevation_index`] and
//! [`elevation_basis`].

use std::cmp::Ordering;
use std::f32::consts::{FRAC_PI_2, PI};
use std::hash::{Hash, Hasher};

use bevy::math::{Mat3, UVec2, Vec2, Vec3};

/// The viewer elevations an impostor is baked for, in the object's own
/// coordinates: from straight above (+90°) down to a lowest elevation.
///
/// A viewer below the lowest elevation sees the view baked at the lowest
/// elevation, so the picture is wrong by the angle between them. A wider
/// range spreads the same number of views over more directions, so the
/// views are further apart and switch more visibly as the camera moves.
///
/// The default is [`SPHERE`](Self::SPHERE), every direction.
#[derive(Clone, Copy, Debug)]
pub struct ViewCoverage {
    lowest_elevation_deg: f32,
}

impl ViewCoverage {
    /// Every direction, from straight above to straight below.
    pub const SPHERE: Self = Self::from_lowest_elevation(-90.0);

    /// From straight above down to the horizon, for objects seen only from
    /// level or above, such as low things on flat ground.
    pub const UPPER_HEMISPHERE: Self = Self::from_lowest_elevation(0.0);

    /// Views from straight above down to `degrees` of elevation, from -90
    /// (straight below) to 90 (straight above only).
    ///
    /// # Panics
    ///
    /// Panics if `degrees` is outside -90..=90 or is NaN.
    pub const fn from_lowest_elevation(degrees: f32) -> Self {
        assert!(
            degrees >= -90.0 && degrees <= 90.0,
            "the lowest elevation must be from -90 to 90 degrees"
        );
        // -0 and 0 are the same coverage, so they must compare and hash
        // the same.
        let degrees = if degrees == 0.0 { 0.0 } else { degrees };
        Self {
            lowest_elevation_deg: degrees,
        }
    }

    /// The lowest viewer elevation covered, in degrees.
    pub const fn lowest_elevation_deg(self) -> f32 {
        self.lowest_elevation_deg
    }

    /// Whether every direction is covered.
    pub fn is_sphere(self) -> bool {
        self.lowest_elevation_deg <= -90.0
    }

    /// The largest angle from +Y covered, in radians.
    fn polar_max(self) -> f32 {
        (90.0 - self.lowest_elevation_deg).to_radians()
    }
}

impl Default for ViewCoverage {
    fn default() -> Self {
        Self::SPHERE
    }
}

impl PartialEq for ViewCoverage {
    fn eq(&self, other: &Self) -> bool {
        self.lowest_elevation_deg.to_bits() == other.lowest_elevation_deg.to_bits()
    }
}

impl Eq for ViewCoverage {}

impl Hash for ViewCoverage {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.lowest_elevation_deg.to_bits().hash(state);
    }
}

impl PartialOrd for ViewCoverage {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ViewCoverage {
    fn cmp(&self, other: &Self) -> Ordering {
        self.lowest_elevation_deg
            .total_cmp(&other.lowest_elevation_deg)
    }
}

/// The direction from the object towards the viewer of atlas cell `cell`
/// (column, row from the top), in the object's own coordinates, for an
/// atlas of `frames` cells along each side baked for `coverage`.
pub fn view_direction(cell: UVec2, frames: u32, coverage: ViewCoverage) -> Vec3 {
    let f = frames as f32;
    let uv = (cell.as_vec2() + 0.5) / f * 2.0 - 1.0;
    if coverage.is_sphere() {
        return octahedron_direction(uv);
    }
    unwarp(hemi_octahedron_direction(uv), coverage)
}

/// The atlas cell whose view is nearest to looking from `direction` (from
/// the object towards the viewer), for an atlas of `frames` cells along each
/// side baked for `coverage`. Directions below the lowest covered elevation
/// use the views at that elevation.
pub fn view_cell(direction: Vec3, frames: u32, coverage: ViewCoverage) -> UVec2 {
    let d = direction.try_normalize().unwrap_or(Vec3::Y);
    let uv = if coverage.is_sphere() {
        octahedron_uv(d)
    } else {
        hemi_octahedron_uv(warp(d, coverage))
    };
    let cell = |t: f32| (((t + 1.0) / 2.0 * frames as f32).floor() as u32).min(frames - 1);
    UVec2::new(cell(uv.x), cell(uv.y))
}

/// The axes of a view's image, in the object's coordinates: right and up
/// across the image, and back towards the viewer (`direction` itself). The
/// same axes a Bevy camera at `direction` looking at the origin has, with
/// up towards +Y, or towards -Z when looking straight down, or towards +Z
/// when looking straight up.
pub fn view_basis(direction: Vec3) -> Mat3 {
    let back = direction.normalize();
    let up_hint = view_up_hint(back);
    let right = up_hint.cross(back).normalize();
    let up = back.cross(right);
    Mat3::from_cols(right, up, back)
}

/// The direction [`view_basis`] keeps up towards for a view from `back`.
fn view_up_hint(back: Vec3) -> Vec3 {
    if back.y > 0.99 {
        Vec3::NEG_Z
    } else if back.y < -0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    }
}

/// The unit direction at `uv` (each from -1 to 1) of an octahedron folded
/// flat: the upper half in the diamond `|u| + |v| <= 1`, the lower half in
/// the corners.
fn octahedron_direction(uv: Vec2) -> Vec3 {
    let y = 1.0 - uv.x.abs() - uv.y.abs();
    let (x, z) = if y >= 0.0 {
        (uv.x, uv.y)
    } else {
        (
            (1.0 - uv.y.abs()) * sign_not_zero(uv.x),
            (1.0 - uv.x.abs()) * sign_not_zero(uv.y),
        )
    };
    Vec3::new(x, y, z).normalize()
}

/// Where unit direction `d` lies on [`octahedron_direction`]'s square.
fn octahedron_uv(d: Vec3) -> Vec2 {
    let p = d / (d.x.abs() + d.y.abs() + d.z.abs());
    if p.y >= 0.0 {
        Vec2::new(p.x, p.z)
    } else {
        Vec2::new(
            (1.0 - p.z.abs()) * sign_not_zero(p.x),
            (1.0 - p.x.abs()) * sign_not_zero(p.z),
        )
    }
}

/// The unit direction at `uv` (each from -1 to 1) of the upper half of an
/// octahedron, turned by 45° so that it fills the whole square.
fn hemi_octahedron_direction(uv: Vec2) -> Vec3 {
    let (x, z) = ((uv.x + uv.y) / 2.0, (uv.x - uv.y) / 2.0);
    Vec3::new(x, 1.0 - x.abs() - z.abs(), z).normalize()
}

/// Where unit direction `d`, at or above the horizon, lies on
/// [`hemi_octahedron_direction`]'s square.
fn hemi_octahedron_uv(d: Vec3) -> Vec2 {
    let p = d / (d.x.abs() + d.y.abs() + d.z.abs());
    Vec2::new(p.x + p.z, p.x - p.z)
}

/// Moves unit direction `d` towards +Y so that `coverage`'s lowest
/// elevation lands on the horizon, scaling its angle from +Y. Directions
/// below the lowest elevation land on the horizon too.
fn warp(d: Vec3, coverage: ViewCoverage) -> Vec3 {
    if coverage.lowest_elevation_deg == 0.0 {
        // No scaling: only fold what lies below the horizon onto it.
        return Vec3::new(d.x, d.y.max(0.0), d.z)
            .try_normalize()
            .unwrap_or(Vec3::Y);
    }
    let polar_max = coverage.polar_max();
    let polar = d.y.clamp(-1.0, 1.0).acos().min(polar_max);
    with_polar(d, polar * FRAC_PI_2 / polar_max)
}

/// The inverse of [`warp`] for directions at or above the horizon.
fn unwarp(d: Vec3, coverage: ViewCoverage) -> Vec3 {
    if coverage.lowest_elevation_deg == 0.0 {
        return d;
    }
    let polar = d.y.clamp(-1.0, 1.0).acos();
    with_polar(d, polar * coverage.polar_max() / FRAC_PI_2)
}

/// The unit direction with `d`'s azimuth about +Y and angle `polar` from +Y.
fn with_polar(d: Vec3, polar: f32) -> Vec3 {
    let around = Vec2::new(d.x, d.z).try_normalize().unwrap_or(Vec2::X);
    let (sin, cos) = polar.sin_cos();
    Vec3::new(around.x * sin, cos, around.y * sin)
}

/// 1 for zero and above, -1 below.
fn sign_not_zero(t: f32) -> f32 {
    if t >= 0.0 { 1.0 } else { -1.0 }
}

/// The direction from the object towards the viewer of view `index` of
/// `count` in the [`Elevations`](crate::BakeLayout::Elevations) layout, in
/// the object's own coordinates.
///
/// The views look at the object's +Y axis from one side (from +Z), at
/// elevations evenly spaced from just above -Y (looking up from below) to
/// just below +Y (looking down from above), view 0 first. View `index` sits
/// in atlas cell `(index % frames, index / frames)`.
pub fn elevation_direction(index: u32, count: u32) -> Vec3 {
    let elevation = elevation_of(index, count);
    Vec3::new(0.0, elevation.sin(), elevation.cos())
}

/// The elevation view of `count` nearest to looking from `direction`, from
/// the object towards the viewer, whatever its azimuth about +Y.
pub fn elevation_index(direction: Vec3, count: u32) -> u32 {
    let d = direction.try_normalize().unwrap_or(Vec3::Z);
    let t = (d.y.clamp(-1.0, 1.0).asin() / PI + 0.5) * count as f32;
    (t.floor().max(0.0) as u32).min(count - 1)
}

/// The axes of an [`elevation_direction`] view's image, in the object's
/// coordinates: right (always +X), up (towards +Y on screen) and back
/// towards the viewer (`direction` itself). Unlike [`view_basis`], up never
/// flips, so +Y always points up the image.
pub fn elevation_basis(direction: Vec3) -> Mat3 {
    let back = direction.normalize();
    let right = Vec3::X;
    let up = back.cross(right);
    Mat3::from_cols(right, up, back)
}

/// The elevation of view `index` of `count`, in radians from -π/2 to π/2.
fn elevation_of(index: u32, count: u32) -> f32 {
    -FRAC_PI_2 + PI * (index as f32 + 0.5) / count as f32
}

#[cfg(test)]
mod tests {
    use bevy::prelude::{Quat, Transform};

    use super::*;

    /// The coverages the layout tests run for: the whole sphere, the trees'
    /// -45°, the upper hemisphere, a cap above the horizon, and nearly the
    /// whole sphere.
    fn coverages() -> [ViewCoverage; 5] {
        [-90.0, -45.0, 0.0, 30.0, -80.0].map(ViewCoverage::from_lowest_elevation)
    }

    /// Elevation of unit direction `d`, in degrees.
    fn elevation(d: Vec3) -> f32 {
        d.y.clamp(-1.0, 1.0).asin().to_degrees()
    }

    #[test]
    fn every_cell_is_found_again_from_its_own_direction() {
        for coverage in coverages() {
            for frames in [4, 6, 8, 10, 12] {
                for y in 0..frames {
                    for x in 0..frames {
                        let cell = UVec2::new(x, y);
                        let direction = view_direction(cell, frames, coverage);
                        assert!(direction.is_normalized(), "{coverage:?} {cell}");
                        assert!(
                            elevation(direction) >= coverage.lowest_elevation_deg() - 1e-3,
                            "{coverage:?} {cell} is below the coverage"
                        );
                        assert_eq!(view_cell(direction, frames, coverage), cell, "{coverage:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_whole_atlas_is_spent_on_the_covered_views() {
        for coverage in coverages() {
            let frames = 10;
            // The lowest views of the atlas lie within one view's spacing
            // of the lowest covered elevation.
            let spacing = (90.0 - coverage.lowest_elevation_deg()) / (frames as f32 / 2.0);
            let lowest = (0..frames * frames)
                .map(|i| view_direction(UVec2::new(i % frames, i / frames), frames, coverage))
                .map(elevation)
                .fold(f32::INFINITY, f32::min);
            assert!(
                lowest - coverage.lowest_elevation_deg() < spacing,
                "{coverage:?}: lowest view at {lowest}°"
            );
        }
    }

    #[test]
    fn the_middle_looks_down_and_views_below_the_coverage_fold_up() {
        for coverage in coverages() {
            let middle = view_cell(Vec3::Y, 8, coverage);
            assert!(view_direction(middle, 8, coverage).y > 0.9);
        }

        // Below the lowest elevation, the views at the lowest elevation
        // are used, with the same azimuth.
        let at = |degrees: f32| {
            let r = degrees.to_radians();
            Vec3::new(r.cos() * 0.6, r.sin(), r.cos() * 0.8)
        };
        let hemisphere = ViewCoverage::UPPER_HEMISPHERE;
        assert_eq!(
            view_cell(at(-30.0), 8, hemisphere),
            view_cell(at(0.0), 8, hemisphere)
        );
        let slope = ViewCoverage::from_lowest_elevation(-45.0);
        assert_eq!(
            view_cell(at(-70.0), 10, slope),
            view_cell(at(-45.0), 10, slope)
        );
        assert_ne!(
            view_cell(at(-40.0), 10, slope),
            view_cell(at(0.0), 10, slope)
        );

        // The whole sphere has views from straight below.
        let sphere = ViewCoverage::SPHERE;
        let bottom = view_cell(Vec3::NEG_Y, 8, sphere);
        assert!(view_direction(bottom, 8, sphere).y < -0.9);
    }

    /// The upper hemisphere's layout as it was before view coverage, so
    /// impostors baked for it are laid out as before.
    #[test]
    fn the_upper_hemisphere_keeps_the_original_layout() {
        fn original_direction(cell: UVec2, frames: u32) -> Vec3 {
            let f = frames as f32;
            let u = (cell.x as f32 + 0.5) / f * 2.0 - 1.0;
            let v = (cell.y as f32 + 0.5) / f * 2.0 - 1.0;
            let (x, z) = ((u + v) / 2.0, (u - v) / 2.0);
            Vec3::new(x, 1.0 - x.abs() - z.abs(), z).normalize()
        }
        fn original_cell(direction: Vec3, frames: u32) -> UVec2 {
            let mut d = direction;
            d.y = d.y.max(0.0);
            let d = d.try_normalize().unwrap_or(Vec3::Y);
            let p = d / (d.x.abs() + d.y.abs() + d.z.abs());
            let (u, v) = (p.x + p.z, p.x - p.z);
            let cell = |t: f32| (((t + 1.0) / 2.0 * frames as f32).floor() as u32).min(frames - 1);
            UVec2::new(cell(u), cell(v))
        }
        let hemisphere = ViewCoverage::UPPER_HEMISPHERE;
        for frames in [4, 8] {
            for i in 0..frames * frames {
                let cell = UVec2::new(i % frames, i / frames);
                assert_eq!(
                    view_direction(cell, frames, hemisphere),
                    original_direction(cell, frames)
                );
            }
            for i in 0..200 {
                let t = i as f32 * 0.731;
                let d = Vec3::new(t.sin(), (t * 1.7).cos(), (t * 0.3).cos() - 0.5);
                assert_eq!(view_cell(d, frames, hemisphere), original_cell(d, frames));
            }
        }
    }

    #[test]
    fn the_basis_matches_a_bevy_camera_looking_at_the_object() {
        for direction in [
            Vec3::new(0.3, 0.8, -0.5),
            Vec3::new(0.3, -0.8, -0.5),
            Vec3::Y,
            Vec3::NEG_Y,
            Vec3::X,
        ] {
            let back = direction.normalize();
            let up = if back.y > 0.99 {
                Vec3::NEG_Z
            } else if back.y < -0.99 {
                Vec3::Z
            } else {
                Vec3::Y
            };
            let camera = Transform::from_translation(direction).looking_at(Vec3::ZERO, up);
            let basis = view_basis(direction);
            let expected = Mat3::from_quat(camera.rotation);
            assert!(basis.abs_diff_eq(expected, 1e-5), "{basis} vs {expected}");
        }
    }

    #[test]
    fn coverages_compare_by_their_lowest_elevation() {
        let zero = ViewCoverage::from_lowest_elevation(0.0);
        assert_eq!(ViewCoverage::from_lowest_elevation(-0.0), zero);
        assert_eq!(zero, ViewCoverage::UPPER_HEMISPHERE);
        assert_ne!(ViewCoverage::from_lowest_elevation(-45.0), zero);
        assert!(ViewCoverage::SPHERE < zero);
        assert_eq!(ViewCoverage::default(), ViewCoverage::SPHERE);
        assert!(ViewCoverage::SPHERE.is_sphere() && !zero.is_sphere());
    }

    #[test]
    #[should_panic(expected = "lowest elevation")]
    fn coverage_below_straight_down_is_refused() {
        ViewCoverage::from_lowest_elevation(-91.0);
    }

    #[test]
    fn elevation_views_run_from_below_to_above_and_keep_their_up() {
        let count = 16;
        for index in 0..count {
            let direction = elevation_direction(index, count);
            assert!(direction.is_normalized());
            assert_eq!(elevation_index(direction, count), index);
            // Any azimuth about +Y finds the same view.
            let turned = Quat::from_rotation_y(1.0) * direction;
            assert_eq!(elevation_index(turned, count), index);
            // +Y points up the image, even nearly along +Y or -Y.
            let basis = elevation_basis(direction);
            assert!(basis.y_axis.dot(Vec3::Y) > 0.0, "view {index}");
            assert!(
                basis
                    .x_axis
                    .cross(basis.y_axis)
                    .abs_diff_eq(basis.z_axis, 1e-5)
            );
        }
        assert!(elevation_direction(0, count).y < -0.99);
        assert!(elevation_direction(count - 1, count).y > 0.99);
        assert_eq!(elevation_index(Vec3::NEG_Y, count), 0);
        assert_eq!(elevation_index(Vec3::Y, count), count - 1);
    }
}
